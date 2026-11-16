//! Reusable isolated native processes: bounded pipes, deadline, no image cache.
#[cfg(feature = "native-codecs")]
use crate::Buffer;
use crate::{
    pipeline::Processed,
    request::{OutputFormat, Request},
    watermark, Error,
};

pub(crate) mod capabilities;
#[cfg(feature = "native-codecs")]
mod composite;
#[cfg(feature = "native-codecs")]
mod encode;
#[cfg(feature = "native-codecs")]
mod engine;
#[cfg(feature = "native-codecs")]
mod fast_svg;
#[cfg(feature = "native-codecs")]
mod geometry;
#[cfg(feature = "native-codecs")]
mod jpeg_decode;
#[cfg(feature = "native-codecs")]
mod metadata;
#[cfg(feature = "native-codecs")]
mod palette;
#[cfg(feature = "native-codecs")]
mod pipe;
#[cfg(feature = "native-codecs")]
mod png_decode;
#[cfg(feature = "native-codecs")]
mod png_identity;
#[cfg(feature = "native-codecs")]
mod png_resize;
#[cfg(feature = "native-codecs")]
mod pool;
#[cfg(feature = "native-codecs")]
mod raster;
#[cfg(feature = "native-codecs")]
mod scale;
#[cfg(feature = "native-codecs")]
mod shared;
#[cfg(feature = "native-codecs")]
mod svg;
#[cfg(feature = "native-codecs")]
mod worker;

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
    pool::process(&options, source, &mark)
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

/// Ensures process-owned children are reaped before an operational command exits.
pub(crate) struct Shutdown;
impl Drop for Shutdown {
    fn drop(&mut self) {
        #[cfg(feature = "native-codecs")]
        pool::shutdown();
    }
}

#[cfg(feature = "native-codecs")]
fn timeout() -> Result<u64, Error> {
    let timeout = std::env::var("PIXSTRIDE_PROCESS_TIMEOUT")
        .unwrap_or_else(|_| "10".into())
        .parse::<u64>()
        .map_err(|_| "invalid processing timeout")?;
    if !(1..=120).contains(&timeout) {
        return Err("processing timeout exceeds range");
    }
    Ok(timeout)
}

pub fn worker() -> Result<(), Error> {
    run_worker(false)
}

pub fn pooled_worker() -> Result<(), Error> {
    run_worker(true)
}

fn run_worker(persistent: bool) -> Result<(), Error> {
    #[cfg(feature = "native-codecs")]
    {
        worker::run(persistent)
    }
    #[cfg(not(feature = "native-codecs"))]
    {
        let _ = persistent;
        Err("native-codecs is not enabled")
    }
}
