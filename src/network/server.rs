//! Local compatibility endpoint. No image cache; one complete source read per request.
use super::handler::handle;
use crate::{fetch, local::LocalRoot, request, Error};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc,
};

pub(crate) struct Active(Arc<AtomicBool>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
pub(crate) struct Admission(Arc<AtomicUsize>);
impl Admission {
    fn try_acquire(count: &Arc<AtomicUsize>, limit: usize) -> Option<Self> {
        count
            .try_update(Ordering::Acquire, Ordering::Relaxed, |n| {
                if n < limit {
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
type Assignment = (Box<dyn super::socket::Socket>, Active, Admission);
type Worker = (Arc<AtomicBool>, mpsc::SyncSender<Assignment>);

fn finish_with_hook<F: FnOnce()>(
    active: Active,
    socket: Box<dyn super::socket::Socket>,
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
fn finish(active: Active, socket: Box<dyn super::socket::Socket>, admission: Admission) {
    finish_with_hook(active, socket, admission, || {});
}

// At most the configured client count owns a guard, including completed responses
// whose sockets have not closed. On failure return the socket for bounded 503.
fn dispatch(
    socket: Box<dyn super::socket::Socket>,
    workers: &[Worker],
    count: &Arc<AtomicUsize>,
) -> Option<Box<dyn super::socket::Socket>> {
    let admission = match Admission::try_acquire(count, workers.len()) {
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

pub fn serve(ip: std::net::IpAddr, port: u16, path: Option<&std::path::Path>) -> Result<(), Error> {
    let root = Arc::new(
        path.map(LocalRoot::open)
            .transpose()
            .map_err(|_| "invalid local source root")?,
    );
    let settings = Arc::new(super::settings::Settings::from_env()?);
    let (concurrency, max_clients) = super::engine::policy()?;
    let _telemetry = super::telemetry::initialize()?;
    let config = Arc::new(request::Config::from_env()?);
    let watermark = Arc::new(crate::watermark::Config::from_env()?);
    fetch::initialize()?;
    let pool = Arc::new(fetch::Pool::new().map_err(|_| "HTTPS transport unavailable")?);
    let listener =
        super::socket::Listener::bind(ip, port).map_err(|_| "cannot bind server port")?;
    let engine = Arc::new(super::engine::Engine::new(concurrency));
    let admission = Arc::new(AtomicUsize::new(0));
    super::lifecycle::initialize()?;
    let metrics_listener = super::metrics_listener::Listener::start(Arc::clone(&settings))?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "cannot configure listener")?;
    let mut handles = Vec::new();
    let mut workers = Vec::new();
    for index in 0..max_clients {
        let busy = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel::<Assignment>(1);
        let root = Arc::clone(&root);
        let config = Arc::clone(&config);
        let watermark = Arc::clone(&watermark);
        let pool = Arc::clone(&pool);
        let engine = Arc::clone(&engine);
        let settings = Arc::clone(&settings);
        let worker = std::thread::Builder::new()
            .name(format!("image-worker-{index}"))
            .spawn(move || {
                while let Ok((socket, active, admission)) = receiver.recv() {
                    let mut socket = super::connection::Connection::new(socket);
                    for index in 0..32 {
                        super::protocol::allow_keep_alive(settings.keep_alive > 0 && index < 31);
                        handle(
                            &mut socket,
                            port,
                            &root,
                            &config,
                            &watermark,
                            &pool,
                            &engine,
                            &settings,
                        );
                        if !super::protocol::keeping_alive()
                            || !socket.next(std::time::Duration::from_secs(settings.keep_alive))
                        {
                            break;
                        }
                    }
                    super::protocol::allow_keep_alive(false);
                    finish(active, Box::new(socket), admission);
                }
            })
            .map_err(|_| "cannot start image worker")?;
        handles.push(worker);
        workers.push((busy, sender));
    }
    eprintln!("pixstride listening on {ip}:{port}, no image cache");
    while super::lifecycle::wait(&listener)? {
        let socket = match listener.accept() {
            Ok(s) => s,
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
    metrics_listener.finish();
    Ok(())
}
