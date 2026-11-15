//! Bounded, recyclable native processes; pixels and operations are never cached.
use super::{parse_output, pipe, timeout, worker};
use crate::{pipeline::Processed, Error};
use std::{
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};

const MAX_WORKERS: usize = 4;
const MAX_REQUESTS: usize = 256;

struct Worker {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    requests: usize,
    timeout: u64,
}

impl Worker {
    fn spawn(timeout: u64) -> Result<Self, Error> {
        let mut command =
            Command::new(std::env::current_exe().map_err(|_| "cannot locate native worker")?);
        command
            .arg("native-worker-pool")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env_clear();
        for name in [
            "PATH",
            "LANG",
            "LC_ALL",
            "LD_LIBRARY_PATH",
            "DYLD_LIBRARY_PATH",
            "VIPS_PLUGIN_PATH",
            "FONTCONFIG_PATH",
            "FONTCONFIG_FILE",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .env("VIPS_CONCURRENCY", "1")
            .env("VIPS_DISC_THRESHOLD", "1073741824")
            .env("MALLOC_ARENA_MAX", "2")
            .env("PIXSTRIDE_PROCESS_TIMEOUT", timeout.to_string());
        let mut child = command.spawn().map_err(|_| "cannot start native worker")?;
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("native worker pipe unavailable");
        };
        let worker = Self {
            child,
            input,
            output,
            requests: 0,
            timeout,
        };
        pipe::nonblocking(&worker.input)?;
        pipe::nonblocking(&worker.output)?;
        Ok(worker)
    }

    fn exchange(&mut self, options: &[u8], source: &[u8], mark: &[u8]) -> Result<Processed, Error> {
        let deadline = Instant::now() + Duration::from_secs(self.timeout);
        let result = (|| {
            for bytes in [options, source, mark] {
                pipe::write_all(
                    &mut self.input,
                    &(bytes.len() as u64).to_le_bytes(),
                    deadline,
                )?;
                pipe::write_all(&mut self.input, bytes, deadline)?;
            }
            let bytes = pipe::frame(&mut self.output, worker::OUTPUT_LIMIT, deadline)?;
            parse_output(&bytes)
        })();
        if result.is_err() {
            use std::os::unix::process::ExitStatusExt;
            let status = match self.child.try_wait() {
                Ok(Some(status)) => Some(status),
                _ => {
                    let _ = self.child.kill();
                    self.child.wait().ok()
                }
            };
            if status.is_some_and(|status| {
                status.code() == Some(101)
                    || status.signal().is_some_and(|signal| {
                        [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGABRT].contains(&signal)
                    })
            }) {
                return Err("native worker crashed");
            }
        }
        result
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct State {
    idle: Vec<Worker>,
    total: usize,
    closed: bool,
}

struct Pool {
    state: Mutex<State>,
    ready: Condvar,
}

static POOL: OnceLock<Pool> = OnceLock::new();

struct Lease<'a> {
    pool: &'a Pool,
    worker: Option<Worker>,
    healthy: bool,
}

impl Pool {
    fn acquire(&self) -> Result<Lease<'_>, Error> {
        let state = self.state.lock().map_err(|_| "native pool unavailable")?;
        let (mut state, deadline) = self
            .ready
            .wait_timeout_while(state, Duration::from_secs(15), |s| {
                !s.closed && s.idle.is_empty() && s.total >= MAX_WORKERS
            })
            .map_err(|_| "native pool unavailable")?;
        if state.closed
            || (deadline.timed_out() && state.idle.is_empty() && state.total >= MAX_WORKERS)
        {
            return Err("native queue deadline exceeded");
        }
        let worker = state.idle.pop();
        if worker.is_none() {
            state.total += 1;
        }
        Ok(Lease {
            pool: self,
            worker,
            healthy: false,
        })
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.pool.state.lock() {
            if self.healthy
                && !state.closed
                && self
                    .worker
                    .as_ref()
                    .is_some_and(|w| w.requests < MAX_REQUESTS)
            {
                state.idle.push(self.worker.take().unwrap());
            } else {
                state.total -= 1;
            }
            self.pool.ready.notify_one();
        }
    }
}

pub(super) fn process(options: &[u8], source: &[u8], mark: &[u8]) -> Result<Processed, Error> {
    let timeout = timeout()?;
    let pool = POOL.get_or_init(|| Pool {
        state: Mutex::new(State {
            idle: Vec::new(),
            total: 0,
            closed: false,
        }),
        ready: Condvar::new(),
    });
    let mut lease = pool.acquire()?;
    if lease
        .worker
        .as_mut()
        .is_some_and(|w| w.timeout != timeout || w.child.try_wait().map_or(true, |s| s.is_some()))
    {
        lease.worker = None;
    }
    if lease.worker.is_none() {
        lease.worker = Some(Worker::spawn(timeout)?);
    }
    let worker = lease.worker.as_mut().unwrap();
    let result = worker.exchange(options, source, mark);
    worker.requests += 1;
    lease.healthy = result.is_ok();
    result
}

pub(super) fn shutdown() {
    let idle = POOL.get().and_then(|pool| {
        let mut state = pool.state.lock().ok()?;
        state.closed = true;
        let idle = std::mem::take(&mut state.idle);
        state.total -= idle.len();
        pool.ready.notify_all();
        Some(idle)
    });
    drop(idle);
}
