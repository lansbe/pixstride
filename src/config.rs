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
            let address = format!("127.0.0.1:{port}")
                .parse()
                .map_err(|_| "invalid health address")?;
            let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(2))
                .map_err(|_| "health connection failed")?;
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .map_err(|_| "health timeout failed")?;
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .map_err(|_| "health timeout failed")?;
            write!(
                socket,
                "GET /health HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"
            )
            .map_err(|_| "health write failed")?;
            let mut bytes = [0; 128];
            let n = socket.read(&mut bytes).map_err(|_| "health read failed")?;
            if bytes[..n].starts_with(b"HTTP/1.1 200 ") {
                Ok(())
            } else {
                Err("unhealthy")
            }
        }
        "process" if args.len() == 5 => {
            let file = crate::os::InputFile::open(args[2].as_bytes())?;
            let mut bytes = file.read_all()?;
            let request = request::parse(&args[4], &request::Config::unsafe_local())?;
            let out = pipeline::process(
                bytes.as_mut_slice(),
                &request,
                &watermark::Config::from_env()?,
            )?;
            std::fs::write(&args[3], out.body.as_slice()).map_err(|_| "cannot write output")
        }
        "sign" if args.len() == 3 => {
            let key = secret("PIXSTRIDE_KEY")?.ok_or("signing key required")?;
            let salt = secret("PIXSTRIDE_SALT")?.ok_or("signing salt required")?;
            let key = crate::signature::decode_hex(&key)?;
            let salt = crate::signature::decode_hex(&salt)?;
            request::Config::signed(key.clone(), salt.clone())?;
            if !args[2].starts_with('/') || args[2].contains(['\r', '\n']) {
                return Err("path must start with /");
            }
            println!(
                "/{}/{}",
                crate::signature::sign_imgproxy(&args[2], &key, &salt),
                args[2].trim_start_matches('/')
            );
            Ok(())
        }
        "--version" => {
            println!("pixstride {}", env!("CARGO_PKG_VERSION"));
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
