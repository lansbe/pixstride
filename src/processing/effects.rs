//! Bounded RGBA8 effects implemented in Rust, without external codecs.
//!
//! Blur is a separable, normalized Gaussian in premultiplied alpha; sharpen
//! is an RGB unsharp mask of amount 1 with original alpha; pixelation is a
//! mirrored-edge, alpha-weighted box average; trim checks a median-filtered
//! RGB image after flattening alpha over magenta. These are deliberately
//! specified operations, not claims of byte-identical libvips output.
use crate::{Buffer, Error, Image, MAX_PIXELS};

const MAX_SIGMA: f64 = 100.0;
const MAX_RADIUS: usize = 300;
const MAX_BLOCK: usize = 1024;
const MAX_FILTER_TAPS: usize = 120_000_000;
const MAX_TRIM_PIXELS: usize = 5_000_000;

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trim {
    pub threshold: u8,
    /// RGB comparison color. Alpha is ignored; None selects the top-left
    /// pixel after flattening transparent pixels over magenta.
    pub color: Option<[u8; 4]>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Effects {
    pub blur_sigma: f64,
    pub sharpen_sigma: f64,
    /// Zero and one mean no pixelation.
    pub pixelate: usize,
    pub trim: Option<Trim>,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            blur_sigma: 0.0,
            sharpen_sigma: 0.0,
            pixelate: 1,
            trim: None,
        }
    }
}

fn validate(image: &Image) -> Result<usize, Error> {
    if image.width == 0 || image.height == 0 {
        return Err("zero image dimension");
    }
    let count = image
        .width
        .checked_mul(image.height)
        .ok_or("image dimensions overflow")?;
    if count > MAX_PIXELS {
        return Err("image exceeds pixel limit");
    }
    if image.pixels.len() != count.checked_mul(4).ok_or("image size overflow")? {
        return Err("invalid RGBA buffer length");
    }
    Ok(count)
}

fn validate_sigma(sigma: f64) -> Result<(), Error> {
    if !sigma.is_finite() || !(0.0..=MAX_SIGMA).contains(&sigma) {
        return Err("invalid filter sigma");
    }
    Ok(())
}

// exp(-x), x >= 0: Taylor series after reduction to x/16, then four
// squarings. At the Gaussian cutoff x <= 4.5, 13 terms are ample; larger
// exponents are negligible at 8-bit precision and return zero.
fn exp_negative(x: f64) -> f64 {
    if x > 32.0 {
        return 0.0;
    }
    let z = -x / 16.0;
    let mut sum = 1.0;
    let mut term = 1.0;
    for n in 1..=24 {
        term *= z / n as f64;
        sum += term;
    }
    for _ in 0..4 {
        sum *= sum;
    }
    sum
}

fn round_u8(value: f64) -> u8 {
    if value <= 0.0 {
        0
    } else if value >= 255.0 {
        255
    } else {
        (value + 0.5) as u8
    }
}

fn clamped_position(index: usize, offset: isize, limit: usize) -> usize {
    (index as isize + offset).clamp(0, limit as isize - 1) as usize
}

fn store_f32(bytes: &mut [u8], offset: usize, value: f32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
}

fn load_f32(bytes: &[u8], offset: usize) -> f32 {
    f32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn gaussian(image: &Image, sigma: f64) -> Result<Image, Error> {
    let count = validate(image)?;
    validate_sigma(sigma)?;
    let scaled = sigma * 3.0;
    let radius = (scaled as usize) + usize::from((scaled as usize as f64) < scaled);
    if radius > MAX_RADIUS {
        return Err("filter radius exceeds limit");
    }
    let span = radius * 2 + 1;
    if count
        .checked_mul(span)
        .and_then(|x| x.checked_mul(2))
        .filter(|&x| x <= MAX_FILTER_TAPS)
        .is_none()
    {
        return Err("filter computation budget exceeded");
    }
    let mut weights = [0.0f64; 2 * MAX_RADIUS + 1];
    let mut total = 0.0;
    for (i, weight) in weights[..span].iter_mut().enumerate() {
        let distance = i as f64 - radius as f64;
        *weight = exp_negative(distance * distance / (2.0 * sigma * sigma));
        total += *weight;
    }
    for weight in &mut weights[..span] {
        *weight /= total;
    }
    let mut scratch = Buffer::new(count.checked_mul(16).ok_or("image size overflow")?)?;
    let temp = scratch.as_mut_slice();
    let source = image.pixels.as_slice();
    for y in 0..image.height {
        for x in 0..image.width {
            let mut sums = [0.0f64; 4];
            for (i, weight) in weights[..span].iter().enumerate() {
                let sx = clamped_position(x, i as isize - radius as isize, image.width);
                let index = (y * image.width + sx) * 4;
                let a = f64::from(source[index + 3]);
                for channel in 0..3 {
                    sums[channel] += f64::from(source[index + channel]) * a / 255.0 * weight;
                }
                sums[3] += a * weight;
            }
            let base = (y * image.width + x) * 16;
            for channel in 0..4 {
                store_f32(temp, base + channel * 4, sums[channel] as f32);
            }
        }
    }
    let mut pixels = Buffer::new(count * 4)?;
    let output = pixels.as_mut_slice();
    for y in 0..image.height {
        for x in 0..image.width {
            let mut sums = [0.0f64; 4];
            for (i, weight) in weights[..span].iter().enumerate() {
                let sy = clamped_position(y, i as isize - radius as isize, image.height);
                let base = (sy * image.width + x) * 16;
                for channel in 0..4 {
                    sums[channel] += f64::from(load_f32(temp, base + channel * 4)) * weight;
                }
            }
            let dst = (y * image.width + x) * 4;
            let alpha = sums[3].clamp(0.0, 255.0);
            if alpha > 0.0 {
                for channel in 0..3 {
                    output[dst + channel] = round_u8(sums[channel] * 255.0 / alpha);
                }
            }
            output[dst + 3] = round_u8(alpha);
        }
    }
    Ok(Image {
        width: image.width,
        height: image.height,
        pixels,
    })
}

/// Gaussian blur. At a negligible sigma (<1e-6), preserve input exactly.
pub fn blur(image: Image, sigma: f64) -> Result<Image, Error> {
    validate(&image)?;
    validate_sigma(sigma)?;
    if sigma < 1e-6 {
        return Ok(image);
    }
    gaussian(&image, sigma)
}

/// RGB unsharp mask, amount 1, alpha preserved. This is not vips_sharpen's
/// nonlinear CIELAB algorithm; visual/numeric parity is not asserted.
pub fn sharpen(mut image: Image, sigma: f64) -> Result<Image, Error> {
    validate(&image)?;
    validate_sigma(sigma)?;
    if sigma < 1e-6 {
        return Ok(image);
    }
    let softened = gaussian(&image, sigma)?;
    for (pixel, low) in image
        .pixels
        .as_mut_slice()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(softened.pixels.as_slice().chunks_exact(4))
    {
        if pixel[3] != 0 {
            for channel in 0..3 {
                let value = 2 * i16::from(pixel[channel]) - i16::from(low[channel]);
                pixel[channel] = value.clamp(0, 255) as u8;
            }
        }
    }
    Ok(image)
}

fn mirrored(index: usize, size: usize) -> usize {
    let period = size * 2;
    let index = index % period;
    if index < size {
        index
    } else {
        period - 1 - index
    }
}

/// Pixelation by mirrored-edge blocks. Uses premultiplied RGB during averaging.
pub fn pixelate(image: Image, block: usize) -> Result<Image, Error> {
    validate(&image)?;
    if block > MAX_BLOCK {
        return Err("pixelation block exceeds limit");
    }
    let block = block.min(image.width.max(image.height));
    if block <= 1 {
        return Ok(image);
    }
    let padded_w = image
        .width
        .checked_add(block - 1)
        .ok_or("image dimensions overflow")?
        / block
        * block;
    let padded_h = image
        .height
        .checked_add(block - 1)
        .ok_or("image dimensions overflow")?
        / block
        * block;
    if padded_w
        .checked_mul(padded_h)
        .filter(|&x| x <= MAX_FILTER_TAPS)
        .is_none()
    {
        return Err("filter computation budget exceeded");
    }
    let mut pixels = Buffer::new(image.pixels.len())?;
    let input = image.pixels.as_slice();
    let output = pixels.as_mut_slice();
    let samples = (block * block) as u64;
    for top in (0..image.height).step_by(block) {
        for left in (0..image.width).step_by(block) {
            let mut alpha_sum = 0u64;
            let mut color_sum = [0u64; 3];
            for y in top..top + block {
                for x in left..left + block {
                    let sx = mirrored(x, image.width);
                    let sy = mirrored(y, image.height);
                    let at = (sy * image.width + sx) * 4;
                    let alpha = u64::from(input[at + 3]);
                    alpha_sum += alpha;
                    for c in 0..3 {
                        color_sum[c] += u64::from(input[at + c]) * alpha;
                    }
                }
            }
            let mut average = [0u8; 4];
            average[3] = ((alpha_sum + samples / 2) / samples) as u8;
            for c in 0..3 {
                average[c] = (color_sum[c] + alpha_sum / 2)
                    .checked_div(alpha_sum)
                    .unwrap_or(0) as u8;
            }
            for y in top..(top + block).min(image.height) {
                for x in left..(left + block).min(image.width) {
                    let at = (y * image.width + x) * 4;
                    output[at..at + 4].copy_from_slice(&average);
                }
            }
        }
    }
    Ok(Image {
        width: image.width,
        height: image.height,
        pixels,
    })
}

fn trim_rgb(input: &[u8], index: usize) -> [u8; 3] {
    let alpha = u16::from(input[index + 3]);
    let bg = [255u16, 0, 255];
    let mut rgb = [0u8; 3];
    for c in 0..3 {
        rgb[c] = ((u16::from(input[index + c]) * alpha + bg[c] * (255 - alpha) + 127) / 255) as u8;
    }
    rgb
}

/// Remove uniform background edges based on a 3x3 median-filtered view.
/// The original unfiltered RGBA pixels inside the selected box are retained.
pub fn trim(image: Image, options: Trim) -> Result<Image, Error> {
    let count = validate(&image)?;
    if count > MAX_TRIM_PIXELS {
        return Err("trim computation budget exceeded");
    }
    if options.threshold == 255 {
        return Ok(image);
    }
    let input = image.pixels.as_slice();
    let background = match options.color {
        Some(c) => [c[0], c[1], c[2]],
        None => trim_rgb(input, 0),
    };
    let (mut left, mut top, mut right, mut bottom) = (image.width, image.height, 0usize, 0usize);
    let mut found = false;
    for y in 0..image.height {
        for x in 0..image.width {
            let mut channels = [[0u8; 9]; 3];
            let mut n = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let sx = clamped_position(x, dx, image.width);
                    let sy = clamped_position(y, dy, image.height);
                    let rgb = trim_rgb(input, (sy * image.width + sx) * 4);
                    for c in 0..3 {
                        channels[c][n] = rgb[c];
                    }
                    n += 1;
                }
            }
            let mut different = false;
            for c in 0..3 {
                channels[c].sort_unstable();
                different |= channels[c][4].abs_diff(background[c]) > options.threshold;
            }
            if different {
                found = true;
                left = left.min(x);
                right = right.max(x);
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }
    if !found {
        return Ok(image);
    }
    let width = right - left + 1;
    let height = bottom - top + 1;
    if width == image.width && height == image.height {
        return Ok(image);
    }
    let mut pixels = Buffer::new(width * height * 4)?;
    for y in 0..height {
        let src = ((top + y) * image.width + left) * 4;
        let dst = y * width * 4;
        pixels.as_mut_slice()[dst..dst + width * 4].copy_from_slice(&input[src..src + width * 4]);
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}

/// Apply filters in the order blur, sharpen, pixelate. Trimming is deliberately
/// separate so callers can perform it before crop and resizing.
pub fn apply_filters(mut image: Image, effects: &Effects) -> Result<Image, Error> {
    validate(&image)?;
    validate_sigma(effects.blur_sigma)?;
    validate_sigma(effects.sharpen_sigma)?;
    if effects.pixelate > MAX_BLOCK {
        return Err("pixelation block exceeds limit");
    }
    image = blur(image, effects.blur_sigma)?;
    image = sharpen(image, effects.sharpen_sigma)?;
    pixelate(image, effects.pixelate)
}
