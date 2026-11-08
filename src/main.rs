// Fixed transform coefficients preserve the established numerical contract.
#![allow(clippy::approx_constant)]
// Codec loops deliberately use bounded indices and explicit channel arguments.
#![allow(
    clippy::chunks_exact_to_as_chunks,
    clippy::needless_range_loop,
    clippy::too_many_arguments
)]
#[cfg(not(all(
    any(target_os = "linux", target_os = "macos"),
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
compile_error!("supported targets are Linux/macOS on x86-64/ARM64");
mod codecs;
use codecs::{
    deflate_encode, extra_encode, extra_formats, gif_encode, image, jpeg, jpeg_encode, metadata,
    png, png_encode, png_extended, svg, webp,
};
mod processing;
use processing::{effects, ops, pipeline, resize, watermark};
mod network;
use network::{fetch, local, overload, protocol, request, server};
mod security;
use security::signature;
mod buffer;
mod config;
mod os;
pub use buffer::Buffer;
pub type Error = &'static str;
pub type Result<T, E = String> = std::result::Result<T, E>;
pub const MAX_PIXELS: usize = 50_000_000;
pub const MAX_FILE: usize = 64 * 1024 * 1024;
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Buffer,
}
fn main() {
    if let Err(error) = config::run() {
        eprintln!("pixstride: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod security_regressions;
