//! Configured local watermark for the  compatibility pipeline.
//!
//! The configured asset is opened and decoded again for every request that
//! actually asks for a watermark. It is distinct from the request's source
//! image, and neither is cached here. The local file is bounded before reading.
use std::os::unix::ffi::OsStrExt;

use crate::{metadata, ops, os, pipeline, request::WatermarkSpec, Error, Image, MAX_PIXELS};

const MAX_WATERMARK_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIMENSION: usize = 100_000;
const MAX_SCALE: f64 = 1000.0;

/// Configuration is parsed once at service/CLI startup and can be borrowed by
/// concurrent requests. The path is administrator configuration, never a URL
/// supplied by the requesting client.
pub struct Config {
    path: Option<Vec<u8>>,
    base_opacity: f64,
}

impl Config {
    /// Validate the configured path as a bounded, non-symlink regular file.
    /// The file is opened again on use to detect replacement and ordinary
    /// changes; this startup check is not an atomic filesystem snapshot.
    pub fn from_path(path: Option<&[u8]>, base_opacity: f64) -> Result<Self, Error> {
        if !base_opacity.is_finite() || base_opacity <= 0.0 || base_opacity > 1.0 {
            return Err("invalid watermark base opacity");
        }
        let path = path.filter(|value| !value.is_empty());
        if let Some(value) = path {
            let file = os::InputFile::open(value)?;
            if file.len() == 0 {
                return Err("watermark file is empty");
            }
            if file.len() > MAX_WATERMARK_BYTES {
                return Err("watermark file exceeds limit");
            }
        }
        Ok(Self {
            path: path.map(<[u8]>::to_vec),
            base_opacity,
        })
    }

    /// Read supported imgproxy-style local configuration once at startup.
    /// Unset or empty watermark path disables the configured asset.
    pub fn from_env() -> Result<Self, Error> {
        let path = std::env::var_os("PIXSTRIDE_WATERMARK_PATH");
        let opacity = match std::env::var_os("PIXSTRIDE_WATERMARK_OPACITY") {
            None => 1.0,
            Some(value) => value
                .to_str()
                .ok_or("invalid watermark base opacity")?
                .parse::<f64>()
                .map_err(|_| "invalid watermark base opacity")?,
        };
        Self::from_path(path.as_deref().map(OsStrExt::as_bytes), opacity)
    }

    pub fn is_configured(&self) -> bool {
        self.path.is_some()
    }
}

fn validate_canvas(image: &Image) -> Result<(), Error> {
    if image.width == 0
        || image.height == 0
        || image.width > MAX_DIMENSION
        || image.height > MAX_DIMENSION
    {
        return Err("invalid watermark canvas dimensions");
    }
    let count = image
        .width
        .checked_mul(image.height)
        .ok_or("image dimensions overflow")?;
    if count > MAX_PIXELS
        || image.pixels.len() != count.checked_mul(4).ok_or("image size overflow")?
    {
        return Err("invalid RGBA image");
    }
    Ok(())
}

fn validate_spec(spec: &WatermarkSpec) -> Result<(), Error> {
    if !spec.opacity.is_finite() || !(0.0..=1.0).contains(&spec.opacity) {
        return Err("invalid watermark opacity");
    }
    if !spec.scale.is_finite() || !(0.0..=MAX_SCALE).contains(&spec.scale) {
        return Err("invalid watermark scale");
    }
    for value in [spec.offset_x, spec.offset_y] {
        if !value.is_finite() || value.abs() > MAX_DIMENSION as f64 {
            return Err("invalid watermark offset");
        }
    }
    if matches!(spec.gravity, ops::Gravity::Focus { .. }) {
        return Err("unsupported watermark gravity");
    }
    Ok(())
}

fn offset(value: f64, dimension: usize) -> Result<i32, Error> {
    let value = if value.abs() < 1.0 {
        value * dimension as f64
    } else {
        value
    };
    let rounded = value.round_ties_even();
    if rounded < i32::MIN as f64 || rounded > i32::MAX as f64 {
        return Err("watermark offset exceeds limit");
    }
    Ok(rounded as i32)
}

fn scaled_box(dimension: usize, scale: f64) -> Result<usize, Error> {
    let value = (dimension as f64 * scale).round_ties_even();
    if !value.is_finite() || value > MAX_DIMENSION as f64 {
        return Err("watermark dimensions exceed limit");
    }
    Ok((value as usize).max(1))
}

fn load(config: &Config) -> Result<Image, Error> {
    let path = config
        .path
        .as_deref()
        .ok_or("watermark image is not configured")?;
    let file = os::InputFile::open(path)?;
    if file.len() == 0 {
        return Err("watermark file is empty");
    }
    if file.len() > MAX_WATERMARK_BYTES {
        return Err("watermark file exceeds limit");
    }
    let bytes = file.read_all()?;
    let orientation = metadata::orientation(bytes.as_slice())?;
    let profile = metadata::extract_icc(bytes.as_slice())?;
    let mut image = pipeline::decode(bytes.as_slice())?;
    if let Some(profile) = profile {
        // Unsupported/LUT/HDR profiles fail instead of silently using the
        // wrong colors. A source profile is handled by the caller separately.
        metadata::convert_to_srgb(&mut image, &profile)?;
    }
    metadata::orient(image, orientation)
}

/// Apply the configured watermark after all geometry, filters, and decoration.
/// A disabled request does not access the configured file. Opacity is quantized
/// to the compositor's 8-bit level and may differ from libvips by one level.
pub fn apply(image: Image, config: &Config, spec: Option<&WatermarkSpec>) -> Result<Image, Error> {
    validate_canvas(&image)?;
    let Some(spec) = spec else {
        return Ok(image);
    };
    validate_spec(spec)?;
    if spec.opacity == 0.0 {
        return Ok(image);
    }
    let effective = spec.opacity * config.base_opacity;
    let alpha = (effective * 255.0 + 0.5) as u8;
    if alpha == 0 {
        return Err("watermark opacity below supported precision");
    }
    let target = if spec.scale > 0.0 {
        let width = scaled_box(image.width, spec.scale)?;
        let height = scaled_box(image.height, spec.scale)?;
        if width
            .checked_mul(height)
            .filter(|&pixels| pixels <= MAX_PIXELS)
            .is_none()
        {
            return Err("watermark dimensions exceed limit");
        }
        Some((width, height))
    } else {
        None
    };
    let offset_x = offset(spec.offset_x, image.width)?;
    let offset_y = offset(spec.offset_y, image.height)?;
    let mut mark = load(config)?;
    if let Some((width, height)) = target {
        let resize = ops::TransformOptions {
            width,
            height,
            enlarge: true,
            mode: ops::ResizeMode::Fit,
            ..Default::default()
        };
        mark = ops::apply_geometry(mark, &resize)?;
    }
    let mark = ops::Watermark {
        image: mark,
        gravity: spec.gravity,
        offset_x,
        offset_y,
        opacity: alpha,
    };
    let options = ops::TransformOptions {
        watermark: Some(mark),
        ..Default::default()
    };
    ops::apply_decorations(image, &options)
}
