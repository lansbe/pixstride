use super::*;
pub(crate) fn watermark(mut image: Image, mark: &Watermark) -> Result<Image, Error> {
    validate(&mark.image)?;
    let (align_x, align_y) = alignments(mark.gravity)?;
    let left = watermark_position(image.width, mark.image.width, align_x, mark.offset_x);
    let top = watermark_position(image.height, mark.image.height, align_y, mark.offset_y);
    let x0 = left.max(0).min(image.width as i128) as usize;
    let y0 = top.max(0).min(image.height as i128) as usize;
    let x1 = (left + mark.image.width as i128)
        .max(0)
        .min(image.width as i128) as usize;
    let y1 = (top + mark.image.height as i128)
        .max(0)
        .min(image.height as i128) as usize;
    if x0 >= x1 || y0 >= y1 || mark.opacity == 0 {
        return Ok(image);
    }
    let source = mark.image.pixels.as_slice();
    let output = image.pixels.as_mut_slice();
    for y in y0..y1 {
        for x in x0..x1 {
            let sx = (x as i128 - left) as usize;
            let sy = (y as i128 - top) as usize;
            let s = (sy * mark.image.width + sx) * 4;
            let d = (y * image.width + x) * 4;
            let alpha = ((u16::from(source[s + 3]) * u16::from(mark.opacity) + 127) / 255) as u8;
            let result = source_over(
                [source[s], source[s + 1], source[s + 2], alpha],
                [output[d], output[d + 1], output[d + 2], output[d + 3]],
            );
            output[d..d + 4].copy_from_slice(&result);
        }
    }
    Ok(image)
}
/// Apply source crop, scaling, rotation, flips and final fill crop.
/// Filters may be inserted after this step and before decoration.
pub(crate) fn apply_geometry(mut image: Image, options: &TransformOptions) -> Result<Image, Error> {
    validate(&image)?;
    validate_gravity(options.gravity)?;
    let swap_dimensions = matches!(options.rotation, Rotation::R90 | Rotation::R270);
    if let Some(selection) = options.crop {
        let alignment = source_crop_alignment(
            selection.gravity,
            options.rotation,
            options.flip_horizontal,
            options.flip_vertical,
        )?;
        let (width, height) = if swap_dimensions {
            (selection.height, selection.width)
        } else {
            (selection.width, selection.height)
        };
        image = crop_aligned(image, width, height, alignment)?;
    }
    let (width, height) = if swap_dimensions {
        (options.height, options.width)
    } else {
        (options.width, options.height)
    };
    // Auto compares source and target orientation in final-image coordinates.
    // Comparing the pre-rotation boxes is not equivalent when either is square.
    let mode = if options.mode == ResizeMode::Auto {
        let (source_w, source_h) = if swap_dimensions {
            (image.height, image.width)
        } else {
            (image.width, image.height)
        };
        let target_w = if options.width == 0 {
            source_w
        } else {
            options.width
        };
        let target_h = if options.height == 0 {
            source_h
        } else {
            options.height
        };
        if (source_w >= source_h) == (target_w >= target_h) {
            ResizeMode::Fill
        } else {
            ResizeMode::Fit
        }
    } else {
        options.mode
    };
    let plan = resize_plan(
        image.width,
        image.height,
        width,
        height,
        mode,
        options.enlarge,
    )?;
    image = resize::resize(image, plan.width, plan.height, options.method.method())?;
    image = rotate(image, options.rotation)?;
    image = flip(image, options.flip_horizontal, options.flip_vertical)?;
    if plan.fill {
        let (width, height) = if swap_dimensions {
            (plan.target_h, plan.target_w)
        } else {
            (plan.target_w, plan.target_h)
        };
        image = crop(
            image,
            Crop {
                width,
                height,
                gravity: options.gravity,
            },
        )?;
    }
    Ok(image)
}
/// Apply padding, background and watermark after optional image filters.
pub(crate) fn apply_decorations(
    mut image: Image,
    options: &TransformOptions,
) -> Result<Image, Error> {
    validate(&image)?;
    image = pad(image, options.padding)?;
    if let Some(color) = options.background {
        image = background(image, color);
    }
    if let Some(mark) = &options.watermark {
        image = watermark(image, mark)?;
    }
    Ok(image)
}
