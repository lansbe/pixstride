//! One isolated native process per image: bounded pipes, deadline, no image cache.
#[cfg(feature = "native-codecs")]
use crate::Buffer;
use crate::{
    pipeline::Processed,
    request::{OutputFormat, Request},
    watermark, Error,
};
#[cfg(feature = "native-codecs")]
use std::io::{Read, Write};

pub(crate) mod capabilities;
#[cfg(feature = "native-codecs")]
mod composite;
#[cfg(feature = "native-codecs")]
mod encode;
#[cfg(feature = "native-codecs")]
mod engine;
#[cfg(feature = "native-codecs")]
mod geometry;
#[cfg(feature = "native-codecs")]
mod metadata;
#[cfg(feature = "native-codecs")]
mod palette;
#[cfg(feature = "native-codecs")]
mod scale;
#[cfg(feature = "native-codecs")]
mod svg;

pub fn sniff(data: &[u8]) -> Option<OutputFormat> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(OutputFormat::Png)
    } else if data.starts_with(&[0xff, 0xd8]) {
        Some(OutputFormat::Jpeg)
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some(OutputFormat::Gif)
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        Some(OutputFormat::Webp)
    } else if data.starts_with(b"BM") {
        Some(OutputFormat::Bmp)
    } else if data.starts_with(b"II*\0") || data.starts_with(b"MM\0*") {
        Some(OutputFormat::Tiff)
    } else if data.starts_with(b"\0\0\x01\0") {
        Some(OutputFormat::Ico)
    } else if data.starts_with(b"\xff\x0a") || data.starts_with(b"\0\0\0\x0cJXL \r\n\x87\n") {
        Some(OutputFormat::Jxl)
    } else if data.get(4..8) == Some(b"ftyp") {
        match data.get(8..12) {
            Some(b"avif" | b"avis") => Some(OutputFormat::Avif),
            Some(b"heic" | b"heix" | b"mif1" | b"msf1" | b"hevc") => Some(OutputFormat::Heic),
            _ => None,
        }
    } else if crate::svg::looks_like(data) {
        Some(OutputFormat::Svg)
    } else {
        None
    }
}

pub fn needs(data: &[u8]) -> bool {
    matches!(
        sniff(data),
        Some(OutputFormat::Avif | OutputFormat::Heic | OutputFormat::Jxl)
    ) || (sniff(data) == Some(OutputFormat::Webp) && data.get(12..16) != Some(b"VP8L"))
}

#[cfg(not(feature = "native-codecs"))]
pub fn process(_: &[u8], _: &Request, _: &watermark::Config) -> Result<Processed, Error> {
    Err("this operation requires a build with native-codecs")
}

#[cfg(feature = "native-codecs")]
pub fn process(
    source: &[u8],
    request: &Request,
    watermark: &watermark::Config,
) -> Result<Processed, Error> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    if source.len() > request.extra.max_source_bytes.min(crate::MAX_FILE) {
        return Err("source exceeds byte limit");
    }
    let mut document =
        serde_json::to_value(request).map_err(|_| "cannot serialize native request")?;
    // Decoding needs bytes and transforms, never the authenticated source URL.
    document["source"] = serde_json::json!("");
    document["extra"]["base_opacity"] = serde_json::json!(watermark.base_opacity());
    let options = serde_json::to_vec(&document).map_err(|_| "cannot serialize native request")?;
    if options.len() > 65536 {
        return Err("native request exceeds limit");
    }
    let mark = if request.watermark.as_ref().is_some_and(|s| s.opacity > 0.0) {
        watermark.encoded()?
    } else {
        Vec::new()
    };
    let timeout = std::env::var("PIXSTRIDE_PROCESS_TIMEOUT")
        .unwrap_or_else(|_| "10".into())
        .parse::<u64>()
        .map_err(|_| "invalid processing timeout")?;
    if !(1..=120).contains(&timeout) {
        return Err("processing timeout exceeds range");
    }
    let mut command =
        Command::new(std::env::current_exe().map_err(|_| "cannot locate native worker")?);
    command
        .arg("native-worker")
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
        "PIXSTRIDE_PROCESS_TIMEOUT",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    // No source URL, secrets or image bytes appear in the process arguments.
    command
        .env("VIPS_CONCURRENCY", "1")
        .env("VIPS_DISC_THRESHOLD", "1073741824");
    let mut child = command.spawn().map_err(|_| "cannot start native worker")?;
    let mut stdin = child.stdin.take().ok_or("native worker pipe unavailable")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("native worker pipe unavailable")?;
    let result = std::thread::scope(|scope| {
        let writer = std::thread::Builder::new()
            .name("native-input".into())
            .spawn_scoped(scope, move || -> std::io::Result<()> {
                for bytes in [&options[..], source, &mark[..]] {
                    stdin.write_all(&(bytes.len() as u64).to_le_bytes())?;
                    stdin.write_all(bytes)?;
                }
                Ok(())
            })
            .map_err(|_| "cannot start native input thread")?;
        let reader = std::thread::Builder::new()
            .name("native-output".into())
            .spawn_scoped(scope, move || {
                let mut bytes = Vec::new();
                stdout
                    .take((crate::MAX_FILE + 49) as u64)
                    .read_to_end(&mut bytes)
                    .map(|_| bytes)
            })
            .map_err(|_| "cannot start native output thread")?;
        let deadline = Instant::now() + Duration::from_secs(timeout);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err("native processing deadline exceeded");
                }
            }
        };
        let written = writer.join();
        let read = reader.join();
        let status = status?;
        use std::os::unix::process::ExitStatusExt;
        if status.code() == Some(101)
            || status.signal().is_some_and(|n| {
                [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGABRT].contains(&n)
            })
        {
            return Err("native worker crashed");
        }
        if !status.success() {
            return Err("native processing failed or exceeded its resource limit");
        }
        written
            .map_err(|_| "native input worker failed")?
            .map_err(|_| "native input rejected")?;
        let bytes = read
            .map_err(|_| "native output worker failed")?
            .map_err(|_| "native output failed")?;
        parse_output(&bytes)
    });
    // Reap even if a pipe or a join failed before a normal status was obtained.
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[cfg(feature = "native-codecs")]
fn parse_output(bytes: &[u8]) -> Result<Processed, Error> {
    if bytes.len() < 41 || &bytes[..4] != b"PXN2" {
        return Err("invalid native output");
    }
    let width = u64::from_le_bytes(bytes[4..12].try_into().unwrap()) as usize;
    let height = u64::from_le_bytes(bytes[12..20].try_into().unwrap()) as usize;
    crate::image::dimensions(width, height)?;
    let origin = [
        u64::from_le_bytes(bytes[20..28].try_into().unwrap()) as usize,
        u64::from_le_bytes(bytes[28..36].try_into().unwrap()) as usize,
    ];
    crate::image::dimensions(origin[0], origin[1])?;
    let format = match bytes[36] {
        0 => OutputFormat::Png,
        1 => OutputFormat::Jpeg,
        2 => OutputFormat::Webp,
        3 => OutputFormat::Gif,
        4 => OutputFormat::Bmp,
        5 => OutputFormat::Tiff,
        6 => OutputFormat::Ico,
        7 => OutputFormat::Avif,
        8 => OutputFormat::Heic,
        9 => OutputFormat::Jxl,
        10 => OutputFormat::Svg,
        _ => return Err("invalid native output format"),
    };
    if bytes.len() - 37 > crate::MAX_FILE {
        return Err("native output exceeds limit");
    }
    let mut body = Buffer::new(bytes.len() - 37)?;
    body.as_mut_slice().copy_from_slice(&bytes[37..]);
    Ok(Processed {
        body,
        mime: format.mime(),
        width,
        height,
        origin,
    })
}

#[cfg(feature = "native-codecs")]
fn frame(input: &mut impl Read, limit: usize) -> Result<Vec<u8>, Error> {
    let mut size = [0; 8];
    input
        .read_exact(&mut size)
        .map_err(|_| "truncated native frame")?;
    let size = u64::from_le_bytes(size);
    if size > limit as u64 {
        return Err("native frame exceeds limit");
    }
    let mut data = vec![0; size as usize];
    input
        .read_exact(&mut data)
        .map_err(|_| "truncated native frame")?;
    Ok(data)
}

pub fn worker() -> Result<(), Error> {
    #[cfg(feature = "native-codecs")]
    {
        limits()?;
        let mut input = std::io::stdin().lock();
        let request: Request = serde_json::from_slice(&frame(&mut input, 65536)?)
            .map_err(|_| "invalid native request")?;
        let source = frame(&mut input, crate::MAX_FILE)?;
        let mark = frame(&mut input, 16 * 1024 * 1024)?;
        let engine::NativeResult {
            body: bytes,
            dimensions: [width, height],
            origin,
            format,
        } = engine::process(&source, &request, &mark)?;
        let id = match format {
            OutputFormat::Png => 0,
            OutputFormat::Jpeg => 1,
            OutputFormat::Webp => 2,
            OutputFormat::Gif => 3,
            OutputFormat::Bmp => 4,
            OutputFormat::Tiff => 5,
            OutputFormat::Ico => 6,
            OutputFormat::Avif => 7,
            OutputFormat::Heic => 8,
            OutputFormat::Jxl => 9,
            OutputFormat::Svg => 10,
        };
        let mut out = std::io::stdout().lock();
        out.write_all(b"PXN2")
            .and_then(|_| out.write_all(&(width as u64).to_le_bytes()))
            .and_then(|_| out.write_all(&(height as u64).to_le_bytes()))
            .and_then(|_| out.write_all(&(origin[0] as u64).to_le_bytes()))
            .and_then(|_| out.write_all(&(origin[1] as u64).to_le_bytes()))
            .and_then(|_| out.write_all(&[id]))
            .and_then(|_| out.write_all(&bytes))
            .map_err(|_| "native output write failed")
    }
    #[cfg(not(feature = "native-codecs"))]
    {
        Err("native-codecs is not enabled")
    }
}

#[cfg(feature = "native-codecs")]
fn limits() -> Result<(), Error> {
    let timeout = std::env::var("PIXSTRIDE_PROCESS_TIMEOUT")
        .unwrap_or_else(|_| "10".into())
        .parse::<u64>()
        .map_err(|_| "invalid native timeout")?;
    if !(1..=120).contains(&timeout) {
        return Err("native timeout exceeds range");
    }
    let limits = [
        (libc::RLIMIT_CORE, 0),
        (libc::RLIMIT_NOFILE, 128),
        (libc::RLIMIT_CPU, timeout + 1),
    ];
    for (resource, value) in limits {
        let limit = libc::rlimit {
            rlim_cur: value as libc::rlim_t,
            rlim_max: value as libc::rlim_t,
        };
        // SAFETY: libc supplies the platform ABI and resource constants; the
        // initialized limit lives through this call in this dedicated child.
        if unsafe { libc::setrlimit(resource, &limit) } != 0 {
            return Err("cannot set native resource limit");
        }
    }
    #[cfg(target_os = "linux")]
    {
        let limit = libc::rlimit {
            rlim_cur: 1024 * 1024 * 1024,
            rlim_max: 1024 * 1024 * 1024,
        };
        // SAFETY: same ABI/lifetime guarantee; the bound applies to this child only.
        if unsafe { libc::setrlimit(libc::RLIMIT_AS, &limit) } != 0 {
            return Err("cannot set native address-space limit");
        }
    }
    Ok(())
}
