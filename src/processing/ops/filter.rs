use super::*;
impl Filter {
    pub(crate) fn method(self) -> &'static [u8] {
        match self {
            Self::Lanczos3 => b"lanczos3",
        }
    }
}
impl Default for TransformOptions {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            mode: ResizeMode::Fit,
            gravity: Gravity::Center,
            enlarge: false,
            rotation: Rotation::R0,
            flip_horizontal: false,
            flip_vertical: false,
            padding: Padding::default(),
            background: None,
            watermark: None,
            method: Filter::Lanczos3,
            crop: None,
        }
    }
}
pub(crate) fn checked_len(width: usize, height: usize) -> Result<usize, Error> {
    if width == 0 || height == 0 {
        return Err("zero image dimension");
    }
    let pixels = width
        .checked_mul(height)
        .ok_or("image dimensions overflow")?;
    if pixels > MAX_PIXELS {
        return Err("image exceeds pixel limit");
    }
    pixels.checked_mul(4).ok_or("image size overflow")
}
pub(crate) fn validate(image: &Image) -> Result<(), Error> {
    if image.pixels.len() != checked_len(image.width, image.height)? {
        return Err("invalid RGBA buffer length");
    }
    Ok(())
}
pub(crate) fn validate_gravity(gravity: Gravity) -> Result<(), Error> {
    if let Gravity::Focus { x, y } = gravity {
        if !x.is_finite()
            || !y.is_finite()
            || !(0.0..=1.0).contains(&x)
            || !(0.0..=1.0).contains(&y)
        {
            return Err("invalid focal point");
        }
    }
    Ok(())
}
pub(crate) fn alignments(gravity: Gravity) -> Result<(Alignment, Alignment), Error> {
    validate_gravity(gravity)?;
    use Alignment::{Center as C, End as E, Focus as F, Start as S};
    Ok(match gravity {
        Gravity::Center => (C, C),
        Gravity::North => (C, S),
        Gravity::South => (C, E),
        Gravity::East => (E, C),
        Gravity::West => (S, C),
        Gravity::NorthEast => (E, S),
        Gravity::NorthWest => (S, S),
        Gravity::SouthEast => (E, E),
        Gravity::SouthWest => (S, E),
        Gravity::Focus { x, y } => (F(x), F(y)),
    })
}
// Signed positions also support watermarks larger than their canvas.
pub(crate) fn raw_axis_position(base: usize, child: usize, align: Alignment) -> i128 {
    let delta = base as i128 - child as i128;
    match align {
        Alignment::Start => 0,
        Alignment::Center => {
            // imgproxy's center uses round-to-even((base - child + 1) / 2).
            let numerator = delta + 1;
            let half = numerator.div_euclid(2);

            if numerator.rem_euclid(2) == 1 && half.rem_euclid(2) == 1 {
                half + 1
            } else {
                half
            }
        }
        Alignment::End => delta,
        Alignment::Focus(value) => {
            let scaled = base as f64 * value;
            let whole = scaled as i128;
            let fraction = scaled - whole as f64;
            let point = if fraction > 0.5 || (fraction == 0.5 && whole % 2 != 0) {
                whole + 1
            } else {
                whole
            };
            point - (child / 2) as i128
        }
    }
}
pub(crate) fn axis_position(base: usize, child: usize, align: Alignment) -> i128 {
    let delta = base as i128 - child as i128;
    raw_axis_position(base, child, align).clamp(delta.min(0), delta.max(0))
}
pub(crate) fn watermark_position(base: usize, child: usize, align: Alignment, offset: i32) -> i128 {
    let offset = i128::from(offset);
    let adjusted = match align {
        Alignment::End => raw_axis_position(base, child, align) - offset,
        _ => raw_axis_position(base, child, align) + offset,
    };
    // Allow partial overlap but retain at least one pixel, matching the
    // watermark-placement contract for an oversized watermark or offset.
    adjusted.clamp(1 - child as i128, base as i128 - 1)
}
pub(crate) fn mirror_alignment(alignment: Alignment) -> Alignment {
    match alignment {
        Alignment::Start => Alignment::End,
        Alignment::Center => Alignment::Center,
        Alignment::End => Alignment::Start,
        Alignment::Focus(value) => Alignment::Focus(1.0 - value),
    }
}
// The requested source crop is expressed in final-image coordinates. Undo
// the later flips and rotation to select the corresponding source region.
pub(crate) fn source_crop_alignment(
    gravity: Gravity,
    rotation: Rotation,
    horizontal: bool,
    vertical: bool,
) -> Result<(Alignment, Alignment), Error> {
    let (mut x, mut y) = alignments(gravity)?;
    if horizontal {
        x = mirror_alignment(x);
    }
    if vertical {
        y = mirror_alignment(y);
    }
    Ok(match rotation {
        Rotation::R0 => (x, y),
        Rotation::R90 => (y, mirror_alignment(x)),
        Rotation::R180 => (mirror_alignment(x), mirror_alignment(y)),
        Rotation::R270 => (mirror_alignment(y), x),
    })
}
/// Crop inside the image, clipping requested dimensions to those available.
pub(crate) fn crop(image: Image, crop: Crop) -> Result<Image, Error> {
    validate(&image)?;
    let alignment = alignments(crop.gravity)?;
    crop_aligned(image, crop.width, crop.height, alignment)
}
pub(crate) fn crop_aligned(
    image: Image,
    width: usize,
    height: usize,
    alignment: (Alignment, Alignment),
) -> Result<Image, Error> {
    let width = if width == 0 {
        image.width
    } else {
        width.min(image.width)
    };
    let height = if height == 0 {
        image.height
    } else {
        height.min(image.height)
    };
    if width == image.width && height == image.height {
        return Ok(image);
    }
    let left = axis_position(image.width, width, alignment.0);
    let top = axis_position(image.height, height, alignment.1);
    let (left, top) = (left as usize, top as usize);
    let mut pixels = Buffer::new(checked_len(width, height)?)?;
    let input = image.pixels.as_slice();
    let output = pixels.as_mut_slice();
    let row_bytes = width * 4;
    for row in 0..height {
        let src = ((top + row) * image.width + left) * 4;
        let dst = row * row_bytes;
        output[dst..dst + row_bytes].copy_from_slice(&input[src..src + row_bytes]);
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}
pub(crate) fn ratio_round(
    value: usize,
    numerator: usize,
    denominator: usize,
) -> Result<usize, Error> {
    let value = (value as u128) * (numerator as u128);
    let result = (value + (denominator as u128 / 2)) / denominator as u128;
    usize::try_from(result.max(1)).map_err(|_| "image dimensions overflow")
}
pub(crate) fn resize_plan(
    source_w: usize,
    source_h: usize,
    mut width: usize,
    mut height: usize,
    mode: ResizeMode,
    enlarge: bool,
) -> Result<ResizePlan, Error> {
    checked_len(source_w, source_h)?;
    if width == 0 && height == 0 {
        return Ok(ResizePlan {
            width: source_w,
            height: source_h,
            target_w: source_w,
            target_h: source_h,
            fill: false,
        });
    }
    if width == 0 {
        width = if mode == ResizeMode::Force {
            source_w
        } else {
            ratio_round(source_w, height, source_h)?
        };
    }
    if height == 0 {
        height = if mode == ResizeMode::Force {
            source_h
        } else {
            ratio_round(source_h, width, source_w)?
        };
    }
    checked_len(width, height)?;
    let mode = if mode == ResizeMode::Auto {
        if (source_w >= source_h) == (width >= height) {
            ResizeMode::Fill
        } else {
            ResizeMode::Fit
        }
    } else {
        mode
    };
    let (out_w, out_h) = match mode {
        ResizeMode::Force => {
            if enlarge || (width <= source_w && height <= source_h) {
                (width, height)
            } else if (width as u128) * (source_h as u128) >= (height as u128) * (source_w as u128)
            {
                (source_w, ratio_round(height, source_w, width)?)
            } else {
                (ratio_round(width, source_h, height)?, source_h)
            }
        }
        ResizeMode::Fit | ResizeMode::Fill => {
            let width_limited =
                (width as u128) * (source_h as u128) <= (height as u128) * (source_w as u128);
            let use_width = if mode == ResizeMode::Fit {
                width_limited
            } else {
                !width_limited
            };
            let (num, den) = if use_width {
                (width, source_w)
            } else {
                (height, source_h)
            };
            if !enlarge && num >= den {
                (source_w, source_h)
            } else {
                (
                    ratio_round(source_w, num, den)?,
                    ratio_round(source_h, num, den)?,
                )
            }
        }
        ResizeMode::Auto => unreachable!(),
    };
    checked_len(out_w, out_h)?;
    Ok(ResizePlan {
        width: out_w,
        height: out_h,
        target_w: width,
        target_h: height,
        fill: mode == ResizeMode::Fill,
    })
}
/// Dimensions of the resize result with no explicit crop, rotation, padding,
/// or other operations. Uses the same plan as `apply`, including fill cropping.
pub(crate) fn output_resize_dimensions(
    source_w: usize,
    source_h: usize,
    options: &TransformOptions,
) -> Result<(usize, usize), Error> {
    let plan = resize_plan(
        source_w,
        source_h,
        options.width,
        options.height,
        options.mode,
        options.enlarge,
    )?;
    if plan.fill {
        Ok((
            plan.width.min(plan.target_w),
            plan.height.min(plan.target_h),
        ))
    } else {
        Ok((plan.width, plan.height))
    }
}
/// Rotate a complete RGBA image clockwise.
pub(crate) fn rotate(image: Image, rotation: Rotation) -> Result<Image, Error> {
    validate(&image)?;
    if rotation == Rotation::R0 {
        return Ok(image);
    }
    let (width, height) = match rotation {
        Rotation::R90 | Rotation::R270 => (image.height, image.width),
        Rotation::R0 | Rotation::R180 => (image.width, image.height),
    };
    let mut pixels = Buffer::new(checked_len(width, height)?)?;
    let input = image.pixels.as_slice();
    let output = pixels.as_mut_slice();
    for y in 0..image.height {
        for x in 0..image.width {
            let (dx, dy) = match rotation {
                Rotation::R0 => (x, y),
                Rotation::R90 => (image.height - 1 - y, x),
                Rotation::R180 => (image.width - 1 - x, image.height - 1 - y),
                Rotation::R270 => (y, image.width - 1 - x),
            };
            let src = (y * image.width + x) * 4;
            let dst = (dy * width + dx) * 4;
            output[dst..dst + 4].copy_from_slice(&input[src..src + 4]);
        }
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}
/// Flip in place, preserving every RGBA byte, including transparent RGB.
pub(crate) fn flip(mut image: Image, horizontal: bool, vertical: bool) -> Result<Image, Error> {
    validate(&image)?;
    let pixels = image.pixels.as_mut_slice();
    if vertical {
        for y in 0..image.height / 2 {
            let mirror_y = image.height - 1 - y;
            for x in 0..image.width * 4 {
                pixels.swap(y * image.width * 4 + x, mirror_y * image.width * 4 + x);
            }
        }
    }
    if horizontal {
        for y in 0..image.height {
            for x in 0..image.width / 2 {
                let a = (y * image.width + x) * 4;
                let b = (y * image.width + image.width - 1 - x) * 4;
                for channel in 0..4 {
                    pixels.swap(a + channel, b + channel);
                }
            }
        }
    }
    Ok(image)
}
pub(crate) fn pad(image: Image, padding: Padding) -> Result<Image, Error> {
    if padding == Padding::default() {
        return Ok(image);
    }
    let width = image
        .width
        .checked_add(padding.left)
        .and_then(|n| n.checked_add(padding.right))
        .ok_or("image dimensions overflow")?;
    let height = image
        .height
        .checked_add(padding.top)
        .and_then(|n| n.checked_add(padding.bottom))
        .ok_or("image dimensions overflow")?;
    let mut pixels = Buffer::new(checked_len(width, height)?)?;
    let row_bytes = image.width * 4;
    for row in 0..image.height {
        let src = row * row_bytes;
        let dst = ((padding.top + row) * width + padding.left) * 4;
        pixels.as_mut_slice()[dst..dst + row_bytes]
            .copy_from_slice(&image.pixels.as_slice()[src..src + row_bytes]);
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}
/// Integer straight-alpha source-over with nearest rounding. The effective
/// source alpha has already been scaled by the watermark's global opacity.
pub(crate) fn source_over(src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
    let sa = u64::from(src[3]);
    let da = u64::from(dst[3]);
    if sa == 0 {
        return dst;
    }
    if sa == 255 || da == 0 {
        return src;
    }
    let alpha = sa * 255 + da * (255 - sa);
    if alpha == 0 {
        return [0, 0, 0, 0];
    }
    let mut out = [0u8; 4];
    for channel in 0..3 {
        let value = u64::from(src[channel]) * sa * 255 + u64::from(dst[channel]) * da * (255 - sa);
        out[channel] = ((value + alpha / 2) / alpha) as u8;
    }
    out[3] = ((alpha + 127) / 255) as u8;
    out
}
pub(crate) fn background(mut image: Image, color: [u8; 4]) -> Image {
    for pixel in image.pixels.as_mut_slice().as_chunks_mut::<4>().0 {
        let out = source_over([pixel[0], pixel[1], pixel[2], pixel[3]], color);
        pixel.copy_from_slice(&out);
    }
    image
}
