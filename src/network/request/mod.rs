//! Bounded subset of imgproxy OSS URL syntax. Unsupported options fail closed.
//!
//! This parser does not fetch a source or establish its trust. The caller must
//! restrict local files to its configured root, and validate every HTTP(S)
//! destination and redirect before opening a socket.

use crate::effects::{Effects, Trim};
use crate::ops::{Crop, Gravity, Padding, ResizeMode, Rotation, TransformOptions};
use crate::signature;
pub type Error = &'static str;
const MAX_TARGET: usize = 16 * 1024;
const MAX_SOURCE: usize = 12_000;
const MAX_DIMENSION: usize = 100_000;
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Png,
    Bmp,
    Jpeg,
    Webp,
    Tiff,
    Ico,
    Gif,
    Avif,
    Heic,
    Jxl,
    Svg,
}
/// OSS watermark parameters; the image itself comes from server configuration.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug)]
pub struct WatermarkSpec {
    pub opacity: f64,
    pub gravity: Gravity,
    pub offset_x: f64,
    pub offset_y: f64,
    /// Zero retains the source watermark's native size.
    pub scale: f64,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Request {
    pub source: String,
    pub extra: extended::Options,
    pub options: TransformOptions,
    /// None delegates format selection to the caller; unsupported selections must fail.
    pub format: Option<OutputFormat>,
    /// OSS accepts zero as a request for its configured default quality.
    pub quality: Option<u8>,
    pub auto_rotate: bool,
    pub strip_metadata: bool,
    pub strip_color_profile: bool,
    pub effects: Effects,
    pub watermark: Option<WatermarkSpec>,
}
pub(crate) enum Auth {
    Signed { pairs: Vec<(Vec<u8>, Vec<u8>)> },
    UnsafeLocal,
}
/// Secrets are private and deliberately do not implement Debug or Display.
pub struct Config {
    auth: Auth,
    presets: presets::Presets,
    defaults: Vec<String>,
    encoding: defaults::Encoding,
    preferred: Vec<OutputFormat>,
    native_defaults: bool,
    signature_size: usize,
    trusted: Vec<String>,
}
mod watermark_spec;
pub(crate) use watermark_spec::*;
mod parse;
pub(crate) use parse::*;

mod auth;
mod defaults;
mod extended;
mod presets;

#[cfg(test)]
mod compatibility_tests;
