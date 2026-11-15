//! Bounded, recyclable native processes; pixels and operations are never cached.
use super::{parse_output, timeout, worker};
use crate::{pipeline::Processed, Error};
use std::{
    io::Write,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{mpsc, Condvar, Mutex, OnceLock},
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
            .env("PIXSTRIDE_PROCESS_TIMEOUT", timeout.to_string());
        let mut child = command.spawn().map_err(|_| "cannot start native worker")?;
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("native worker pipe unavailable");
        };
        Ok(Self {
            child,
            input,
            output,
            requests: 0,
            timeout,
        })
    }

    fn exchange(&mut self, options: &[u8], source: &[u8], mark: &[u8]) -> Result<Processed, Error> {
        let Self {
            child,
            input,
            output,
            timeout,
            ..
        } = self;
        std::thread::scope(|scope| {
            let (sent, received) = mpsc::sync_channel(1);
            let (write_sent, write_received) = mpsc::sync_channel(1);
            let deadline = Instant::now() + Duration::from_secs(*timeout);
            let writer = std::thread::Builder::new()
                .name("native-input".into())
                .spawn_scoped(scope, move || {
                    let result = (|| -> std::io::Result<()> {
                        for bytes in [options, source, mark] {
                            input.write_all(&(bytes.len() as u64).to_le_bytes())?;
                            input.write_all(bytes)?;
                        }
                        input.flush()
                    })();
                    let _ = write_sent.send(result);
                })
                .map_err(|_| "cannot start native input thread")?;
            let reader = match std::thread::Builder::new()
                .name("native-output".into())
                .spawn_scoped(scope, move || {
                    let result = worker::frame(output, worker::OUTPUT_LIMIT)
                        .and_then(|bytes| bytes.ok_or("native worker closed output"));
                    let _ = sent.send(result);
                }) {
                Ok(reader) => reader,
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = writer.join();
                    return Err("cannot start native output thread");
                }
            };
            let written = write_received
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| "native processing deadline exceeded")
                .and_then(|result| result.map_err(|_| "native input rejected"));
            let result = written.and_then(|()| {
                received
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .map_err(|_| "native processing deadline exceeded")
                    .and_then(|result| result)
            });
            let mut crashed = false;
            if result.is_err() {
                use std::os::unix::process::ExitStatusExt;
                let status = match child.try_wait() {
                    Ok(Some(status)) => Some(status),
                    _ => {
                        let _ = child.kill();
                        child.wait().ok()
                    }
                };
                crashed = status.is_some_and(|status| {
                    status.code() == Some(101)
                        || status.signal().is_some_and(|signal| {
                            [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGABRT]
                                .contains(&signal)
                        })
                });
            }
            writer.join().map_err(|_| "native input worker failed")?;
            reader.join().map_err(|_| "native output worker failed")?;
            if crashed {
                return Err("native worker crashed");
            }
            let bytes = result?;
            parse_output(&bytes)
        })
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
