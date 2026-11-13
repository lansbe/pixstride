//! Startup configuration and the small operational CLI.
use crate::{pipeline, request, watermark, Error};
use std::{
    io::{Read, Write},
    net::TcpStream,
    path::Path,
    time::Duration,
};

pub fn run() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "native-worker") {
        return crate::processing::native::worker();
    }
    match args.get(1).map(String::as_str).unwrap_or("serve") {
        "serve" if args.len() <= 2 => {
            let port = std::env::var("PIXSTRIDE_PORT")
                .unwrap_or_else(|_| "8080".into())
                .parse::<u16>()
                .map_err(|_| "invalid port")?;
            if port == 0 {
                return Err("port must be nonzero");
            }
            let bind = std::env::var("PIXSTRIDE_BIND").unwrap_or_else(|_| "127.0.0.1".into());
            let ip = bind
                .parse::<std::net::IpAddr>()
                .map_err(|_| "bind must be an IP address")?;
            let root = std::env::var_os("PIXSTRIDE_LOCAL_ROOT");
            crate::server::serve(ip, port, root.as_deref().map(Path::new))
        }
        "healthcheck" => {
            let port = std::env::var("PIXSTRIDE_PORT").unwrap_or_else(|_| "8080".into());
            let mut socket: Box<dyn crate::network::socket::Socket> = if let Some(path) =
                std::env::var_os("PIXSTRIDE_UNIX_SOCKET")
            {
                Box::new(
                    std::os::unix::net::UnixStream::connect(path)
                        .map_err(|_| "health connection failed")?,
                )
            } else {
                let bind = std::env::var("PIXSTRIDE_BIND").unwrap_or_else(|_| "127.0.0.1".into());
                let mut ip = bind
                    .parse::<std::net::IpAddr>()
                    .map_err(|_| "invalid health address")?;
                if ip.is_unspecified() {
                    ip = if ip.is_ipv6() {
                        std::net::Ipv6Addr::LOCALHOST.into()
                    } else {
                        std::net::Ipv4Addr::LOCALHOST.into()
                    };
                }
                let address =
                    std::net::SocketAddr::new(ip, port.parse().map_err(|_| "invalid health port")?);
                Box::new(
                    TcpStream::connect_timeout(&address, Duration::from_secs(2))
                        .map_err(|_| "health connection failed")?,
                )
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .map_err(|_| "health timeout failed")?;
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .map_err(|_| "health timeout failed")?;
            let secret = secret("PIXSTRIDE_SECRET")?;
            if secret
                .as_ref()
                .is_some_and(|s| s.bytes().any(|b| b < 32 || b == 127))
            {
                return Err("invalid health authorization");
            }
            let authorization = secret
                .map(|s| format!("Authorization: Bearer {s}\r\n"))
                .unwrap_or_default();
            write!(
                socket,
                "GET /health HTTP/1.1\r\nHost: localhost:{port}\r\n{authorization}\r\n"
            )
            .map_err(|_| "health write failed")?;
            let mut bytes = [0; 128];
            let mut used = 0;
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while used < bytes.len() && !bytes[..used].windows(2).any(|w| w == b"\r\n") {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err("health read deadline exceeded");
                }
                socket
                    .set_read_timeout(Some(remaining))
                    .map_err(|_| "health timeout failed")?;
                let n = socket
                    .read(&mut bytes[used..])
                    .map_err(|_| "health read failed")?;
                if n == 0 {
                    break;
                }
                used += n;
            }
            if bytes[..used].starts_with(b"HTTP/1.1 200 ")
                && bytes[..used].windows(2).any(|w| w == b"\r\n")
            {
                Ok(())
            } else {
                Err("unhealthy")
            }
        }
        "process" if args.len() == 5 => {
            crate::fetch::initialize()?;
            let file = crate::os::InputFile::open(args[2].as_bytes())?;
            let mut bytes = file.read_all()?;
            let config = request::Config::unsafe_local()
                .with_environment()?
                .with_presets(&secret("PIXSTRIDE_PRESETS")?.unwrap_or_default())?;
            let request = request::parse(&args[4], &config)?;
            let out = pipeline::process(
                bytes.as_mut_slice(),
                &request,
                &watermark::Config::from_env()?,
            )?;
            std::fs::write(&args[3], out.body.as_slice()).map_err(|_| "cannot write output")
        }
        "sign" if args.len() == 3 => {
            println!("{}", request::Config::from_env()?.sign(&args[2])?);
            Ok(())
        }
        "--version" => {
            println!("pixstride {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "capabilities" if args.len() == 2 => {
            println!("{}", crate::processing::native::capabilities::report()?);
            Ok(())
        }
        "--help" | "help" => {
            println!("pixstride [serve | healthcheck | sign PATH | process INPUT OUTPUT /unsafe/OPTIONS/plain/local:///image@FORMAT]\nConfiguration: PIXSTRIDE_BIND, PIXSTRIDE_PORT, PIXSTRIDE_KEY[_FILE], PIXSTRIDE_SALT[_FILE], PIXSTRIDE_ALLOWED_HOSTS, PIXSTRIDE_LOCAL_ROOT.\nUnsigned requests require PIXSTRIDE_ALLOW_UNSAFE=1.");
            Ok(())
        }
        _ => Err("invalid command; use --help"),
    }
}

pub fn secret(name: &str) -> Result<Option<String>, Error> {
    fn variable(name: &str) -> Result<Option<String>, Error> {
        match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(_) => Err("invalid secret configuration encoding"),
        }
    }
    let value = variable(name)?;
    let file = variable(&format!("{name}_FILE"))?;
    if value.is_some() && file.is_some() {
        return Err("configure secret value or file, not both");
    }
    match file {
        Some(path) => {
            let mut f = std::fs::File::open(path).map_err(|_| "cannot open secret file")?;
            let mut bytes = Vec::new();
            std::io::Read::by_ref(&mut f)
                .take(8193)
                .read_to_end(&mut bytes)
                .map_err(|_| "cannot read secret file")?;
            if bytes.len() > 8192 {
                return Err("secret file too large");
            }
            String::from_utf8(bytes)
                .map(|v| Some(v.trim().into()))
                .map_err(|_| "invalid secret file")
        }
        None => Ok(value),
    }
}
