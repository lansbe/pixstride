//! Local compatibility endpoint. No image cache; one complete source read per request.
use crate::{fetch, local::LocalRoot, pipeline, protocol, request, Error};
use std::{
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MAX_SOURCE: usize = 64 * 1024 * 1024;
const MAX_ADMITTED: usize = 4;
pub(crate) enum Source {
    Local(crate::Buffer),
    Remote(Vec<u8>),
}
impl Source {
    fn bytes_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Local(b) => b.as_mut_slice(),
            Self::Remote(b) => b,
        }
    }
}
// A short-lived stack value avoids a per-request heap allocation.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Route {
    Health,
    Favicon,
    Image(std::result::Result<request::Request, request::Error>),
}
pub(crate) struct Active(Arc<AtomicBool>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
pub(crate) struct Admission(Arc<AtomicUsize>);
impl Admission {
    fn try_acquire(count: &Arc<AtomicUsize>) -> Option<Self> {
        count
            .try_update(Ordering::Acquire, Ordering::Relaxed, |n| {
                if n < MAX_ADMITTED {
                    Some(n + 1)
                } else {
                    None
                }
            })
            .ok()
            .map(|_| Self(Arc::clone(count)))
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}
type Assignment = (TcpStream, Active, Admission);
type Worker = (Arc<AtomicBool>, mpsc::SyncSender<Assignment>);

fn finish_with_hook<F: FnOnce()>(
    active: Active,
    socket: TcpStream,
    admission: Admission,
    after_release: F,
) {
    // The response write is finished before releasing the slot. Releasing it
    // before close lets an EOF-waiting client reuse this worker on its next
    // request; the distinct admission guard is held until after socket close.
    drop(active);
    after_release();
    drop(socket);
    drop(admission);
}
fn finish(active: Active, socket: TcpStream, admission: Admission) {
    finish_with_hook(active, socket, admission, || {});
}

// At most MAX_ADMITTED requests own a guard, including completed responses
// whose sockets have not closed. On failure return the socket for bounded 503.
fn dispatch(socket: TcpStream, workers: &[Worker], count: &Arc<AtomicUsize>) -> Option<TcpStream> {
    let admission = match Admission::try_acquire(count) {
        Some(admission) => admission,
        None => return Some(socket),
    };
    let mut pending = Some((socket, admission));
    for (busy, sender) in workers {
        if busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            let (socket, admission) = pending.take().unwrap();
            let item = (socket, Active(Arc::clone(busy)), admission);
            if let Err(error) = sender.send(item) {
                let (socket, active, admission) = error.0;
                drop(active);
                pending = Some((socket, admission));
                continue;
            }
            break;
        }
    }
    pending.map(|(socket, admission)| {
        drop(admission);
        socket
    })
}

fn handle(
    socket: &mut TcpStream,
    port: u16,
    root: &Option<LocalRoot>,
    config: &request::Config,
    watermark: &crate::watermark::Config,
    pool: &fetch::Pool,
    engine: &Mutex<()>,
) {
    let _ = socket.set_nodelay(true);
    let _ = socket.set_write_timeout(Some(Duration::from_secs(15)));
    let route = match protocol::request_with(socket, port, config.requires_local_host(), |target| {
        match target {
            "/health" => Route::Health,
            "/favicon.ico" => Route::Favicon,
            _ => Route::Image(request::parse(target, config)),
        }
    }) {
        Ok(r) => r,
        Err(_) => {
            let _ = protocol::text(socket, 400, "Invalid HTTP request");
            return;
        }
    };
    let request = match route {
        Route::Health => {
            let _ = protocol::text(socket, 200, "ok\n");
            return;
        }
        Route::Favicon => {
            let _ = protocol::text(socket, 204, "");
            return;
        }
        Route::Image(Ok(r)) => r,
        Route::Image(Err(e)) => {
            let _ = protocol::text(socket, 400, e);
            return;
        }
    };
    if request
        .watermark
        .as_ref()
        .is_some_and(|spec| spec.opacity > 0.0)
        && !watermark.is_configured()
    {
        let _ = protocol::text(socket, 422, "watermark image is not configured");
        return;
    }
    let started = Instant::now();
    let (mut source, transfers, upstream, new_connections) =
        if request.source.starts_with("local:///") {
            match root
                .as_ref()
                .ok_or_else(|| "local sources are disabled".to_string())
                .and_then(|root| root.read(&request.source))
            {
                Ok(b) => (Source::Local(b), 0, 0, 0),
                Err(e) => {
                    let _ = protocol::text(socket, 422, &e);
                    return;
                }
            }
        } else {
            match pool.fetch(&request.source, MAX_SOURCE) {
                Ok(d) => (
                    Source::Remote(d.body),
                    d.transfers,
                    d.upstream_bytes,
                    d.new_connections,
                ),
                Err(_) => {
                    let _ = protocol::text(
                        socket,
                        502,
                        "Unable to retrieve the complete public HTTP(S) source",
                    );
                    return;
                }
            }
        };
    let fetch_ms = started.elapsed().as_secs_f64() * 1000.0;
    let queued = Instant::now();
    let lock = match engine.lock() {
        Ok(l) => l,
        Err(_) => {
            let _ = protocol::text(socket, 503, "Image worker unavailable");
            return;
        }
    };
    let queue_ms = queued.elapsed().as_secs_f64() * 1000.0;
    let processing = Instant::now();
    let output = pipeline::process(source.bytes_mut(), &request, watermark);
    let process_ms = processing.elapsed().as_secs_f64() * 1000.0;
    drop(lock);
    drop(source);
    match output {
        Ok(out) => {
            let _=protocol::response(socket,200,out.mime,format_args!("Server-Timing: fetch;dur={fetch_ms:.3}, queue;dur={queue_ms:.3}, engine;dur={process_ms:.3}\r\nX-Image-Engine: pixstride\r\nX-Upstream-Transfers: {transfers}\r\nX-Upstream-Bytes: {upstream}\r\nX-Upstream-New-Connections: {new_connections}\r\nX-Image-Width: {}\r\nX-Image-Height: {}\r\n",out.width,out.height),out.body.as_slice());
        }
        Err(e) => {
            let _ = protocol::text(socket, 422, e);
        }
    }
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod http_tests;

pub fn serve(ip: std::net::IpAddr, port: u16, path: Option<&std::path::Path>) -> Result<(), Error> {
    let root = Arc::new(
        path.map(LocalRoot::open)
            .transpose()
            .map_err(|_| "invalid local source root")?,
    );
    let config = Arc::new(request::Config::from_env()?);
    let watermark = Arc::new(crate::watermark::Config::from_env()?);
    fetch::initialize()?;
    let pool = Arc::new(fetch::Pool::new().map_err(|_| "HTTPS transport unavailable")?);
    let listener = TcpListener::bind((ip, port)).map_err(|_| "cannot bind server port")?;
    let engine = Arc::new(Mutex::new(()));
    let admission = Arc::new(AtomicUsize::new(0));
    super::lifecycle::initialize()?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "cannot configure listener")?;
    let mut handles = Vec::new();
    let mut workers = Vec::new();
    for index in 0..MAX_ADMITTED {
        let busy = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel::<Assignment>(1);
        let root = Arc::clone(&root);
        let config = Arc::clone(&config);
        let watermark = Arc::clone(&watermark);
        let pool = Arc::clone(&pool);
        let engine = Arc::clone(&engine);
        let worker = std::thread::Builder::new()
            .name(format!("image-worker-{index}"))
            .spawn(move || {
                while let Ok((mut socket, active, admission)) = receiver.recv() {
                    handle(
                        &mut socket,
                        port,
                        &root,
                        &config,
                        &watermark,
                        &pool,
                        &engine,
                    );
                    finish(active, socket, admission);
                }
            })
            .map_err(|_| "cannot start image worker")?;
        handles.push(worker);
        workers.push((busy, sender));
    }
    eprintln!("pixstride listening on {ip}:{port}, no image cache");
    while super::lifecycle::wait(&listener)? {
        let socket = match listener.accept() {
            Ok((s, _)) => s,
            Err(_) => continue,
        };
        if let Some(socket) = dispatch(socket, &workers, &admission) {
            crate::overload::reject(socket);
        }
    }
    drop(workers);
    for worker in handles {
        let _ = worker.join();
    }
    Ok(())
}
