use super::*;
/// Parse and authenticate an imgproxy-style request target. The outer HTTP
/// query is excluded from the signature as in imgproxy; an embedded plain URL
/// query must be escaped as `%3F` in the source path.
pub(crate) fn parse(target: &str, config: &Config) -> Result<Request, Error> {
    if target.len() > MAX_TARGET
        || !target.starts_with('/')
        || target.bytes().any(|b| b <= 32 || b >= 127)
    {
        return Err("invalid request target");
    }
    let path = target
        .split('?')
        .next()
        .unwrap_or("")
        .strip_prefix('/')
        .ok_or("invalid request target")?;
    let (sig, path) = path.split_once('/').ok_or("missing signature or source")?;
    if sig.is_empty() || path.is_empty() {
        return Err("missing signature or source");
    }
    let normalized = normalize_path(path);
    if normalized.is_empty() {
        return Err("missing source URL");
    }
    config.verify(sig, &normalized)?;

    let mut options = TransformOptions::default();
    let mut fmt = None;
    let mut quality = None;
    let mut auto_rotate = true;
    let mut strip_metadata = true;
    let mut strip_color_profile = true;
    let mut effects = Effects::default();
    let mut watermark: Option<WatermarkSpec> = None;
    let mut explicit_crop: Option<(usize, usize, Option<Gravity>)> = None;
    let mut gravity_seen = false;
    let mut parts = normalized.split('/').peekable();
    while parts.peek().is_some_and(|part| part.contains(':')) {
        let part = parts.next().expect("peeked option");
        let mut fields = part.split(':');
        let name = fields.next().unwrap_or("");
        let args: Vec<_> = fields.collect();
        match name {
            "resize" | "rs" => {
                if args.is_empty() || args.len() > 4 {
                    return Err("unsupported resize arguments");
                }
                if !args[0].is_empty() {
                    options.mode = match args[0] {
                        "fit" => ResizeMode::Fit,
                        "fill" => ResizeMode::Fill,
                        "force" => ResizeMode::Force,
                        "auto" => ResizeMode::Auto,
                        _ => return Err("unsupported resize type"),
                    };
                }
                if let Some(value) = args.get(1).filter(|v| !v.is_empty()) {
                    options.width = decimal(value)?;
                }
                if let Some(value) = args.get(2).filter(|v| !v.is_empty()) {
                    options.height = decimal(value)?;
                }
                if let Some(value) = args.get(3).filter(|v| !v.is_empty()) {
                    options.enlarge = boolean(value)?;
                }
            }
            "size" | "s" => {
                if args.is_empty() || args.len() > 3 {
                    return Err("unsupported size arguments");
                }
                if !args[0].is_empty() {
                    options.width = decimal(args[0])?;
                }
                if let Some(value) = args.get(1).filter(|v| !v.is_empty()) {
                    options.height = decimal(value)?;
                }
                if let Some(value) = args.get(2).filter(|v| !v.is_empty()) {
                    options.enlarge = boolean(value)?;
                }
            }
            "resizing_type" | "rt" => {
                if args.len() != 1 {
                    return Err("invalid resize type");
                }
                options.mode = match args[0] {
                    "fit" => ResizeMode::Fit,
                    "fill" => ResizeMode::Fill,
                    "force" => ResizeMode::Force,
                    "auto" => ResizeMode::Auto,
                    _ => return Err("unsupported resize type"),
                };
            }
            "width" | "w" => {
                if args.len() != 1 {
                    return Err("invalid width");
                }
                options.width = decimal(args[0])?;
            }
            "height" | "h" => {
                if args.len() != 1 {
                    return Err("invalid height");
                }
                options.height = decimal(args[0])?;
            }
            "enlarge" | "el" => {
                if args.len() != 1 {
                    return Err("invalid enlarge");
                }
                options.enlarge = boolean(args[0])?;
            }
            "gravity" | "g" => {
                // imgproxy retains previous offsets in some repeated forms; the
                // current geometry type cannot represent that retained state.
                if gravity_seen {
                    return Err("repeated gravity unsupported");
                }
                gravity_seen = true;
                options.gravity = gravity(&args)?;
            }
            "crop" | "c" => {
                if args.is_empty() || args.len() > 5 {
                    return Err("invalid crop");
                }
                // Fractional imgproxy crops cannot be represented by our pixel Crop yet.
                let width = decimal(args[0])?;
                let (_, prior_height, prior_gravity) = explicit_crop.unwrap_or((0, 0, None));
                let height = if args.len() > 1 {
                    decimal(args[1])?
                } else {
                    prior_height
                };
                let g = if args.len() > 2 {
                    Some(gravity(&args[2..])?)
                } else {
                    prior_gravity
                };
                explicit_crop = Some((width, height, g));
            }
            "rotate" | "rot" => {
                if args.len() != 1 {
                    return Err("invalid rotation");
                }
                let angle = args[0].parse::<i32>().map_err(|_| "invalid rotation")?;
                options.rotation = match angle.rem_euclid(360) {
                    0 => Rotation::R0,
                    90 => Rotation::R90,
                    180 => Rotation::R180,
                    270 => Rotation::R270,
                    _ => return Err("rotation must be a multiple of 90"),
                };
            }
            "flip" | "fl" => {
                if args.is_empty() || args.len() > 2 {
                    return Err("invalid flip");
                }
                if !args[0].is_empty() {
                    options.flip_horizontal = boolean(args[0])?;
                }
                if let Some(value) = args.get(1).filter(|v| !v.is_empty()) {
                    options.flip_vertical = boolean(value)?;
                }
            }
            "padding" | "pd" => {
                if args.is_empty() || args.len() > 4 {
                    return Err("invalid padding");
                }
                let top = if args[0].is_empty() {
                    options.padding.top
                } else {
                    decimal(args[0])?
                };
                let right = args
                    .get(1)
                    .filter(|v| !v.is_empty())
                    .map(|v| decimal(v))
                    .transpose()?
                    .unwrap_or(top);
                let bottom = args
                    .get(2)
                    .filter(|v| !v.is_empty())
                    .map(|v| decimal(v))
                    .transpose()?
                    .unwrap_or(top);
                let left = args
                    .get(3)
                    .filter(|v| !v.is_empty())
                    .map(|v| decimal(v))
                    .transpose()?
                    .unwrap_or(right);
                options.padding = Padding {
                    top,
                    right,
                    bottom,
                    left,
                };
            }
            "background" | "bg" => {
                options.background = color(&args)?;
            }
            "blur" | "bl" => {
                if args.len() != 1 {
                    return Err("invalid blur");
                }
                effects.blur_sigma = sigma(args[0])?;
            }
            "sharpen" | "sh" => {
                if args.len() != 1 {
                    return Err("invalid sharpen");
                }
                effects.sharpen_sigma = sigma(args[0])?;
            }
            "pixelate" | "pix" => {
                if args.len() != 1 {
                    return Err("invalid pixelate");
                }
                let n = decimal(args[0])?;
                if n > 1024 {
                    return Err("pixelate exceeds limit");
                }
                effects.pixelate = n.max(1);
            }
            "watermark" | "wm" => {
                // The OSS form has five meaningful arguments. Reject extras,
                // unsupported repeat gravity and non-finite values explicitly.
                if args.is_empty() || args.len() > 5 {
                    return Err("invalid watermark");
                }
                let opacity = args[0]
                    .parse::<f64>()
                    .map_err(|_| "invalid watermark opacity")?;
                if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
                    return Err("invalid watermark opacity");
                }
                let mark = watermark.get_or_insert_with(WatermarkSpec::default);
                mark.opacity = opacity;
                if let Some(value) = args.get(1).filter(|v| !v.is_empty()) {
                    let g = gravity(&[value])?;
                    if matches!(g, Gravity::Focus { .. }) {
                        return Err("unsupported watermark gravity");
                    }
                    mark.gravity = g;
                }
                for (index, field) in [(2, &mut mark.offset_x), (3, &mut mark.offset_y)] {
                    if let Some(value) = args.get(index).filter(|v| !v.is_empty()) {
                        let v = value
                            .parse::<f64>()
                            .map_err(|_| "invalid watermark offset")?;
                        if !v.is_finite() || v.abs() > MAX_DIMENSION as f64 {
                            return Err("invalid watermark offset");
                        }
                        *field = v;
                    }
                }
                if let Some(value) = args.get(4).filter(|v| !v.is_empty()) {
                    let scale = value
                        .parse::<f64>()
                        .map_err(|_| "invalid watermark scale")?;
                    if !scale.is_finite() || !(0.0..=1000.0).contains(&scale) {
                        return Err("invalid watermark scale");
                    }
                    mark.scale = scale;
                }
            }
            "trim" | "t" => {
                if args.is_empty() || args.len() > 4 {
                    return Err("invalid trim");
                }
                if args[0].is_empty() {
                    if args.len() > 1 {
                        return Err("unsupported trim arguments");
                    }
                    effects.trim = None;
                } else {
                    let threshold = args[0]
                        .parse::<f64>()
                        .map_err(|_| "invalid trim threshold")?;
                    if !threshold.is_finite()
                        || !(0.0..=255.0).contains(&threshold)
                        || threshold.fract() != 0.0
                    {
                        return Err("unsupported trim threshold");
                    }
                    // With no color the effects module samples the top-left
                    // pixel after alpha flattening, as in imgproxy's smart mode.
                    let color = args
                        .get(1)
                        .filter(|v| !v.is_empty())
                        .map(|raw| color(&[raw]))
                        .transpose()?
                        .flatten();
                    for value in args.get(2..).unwrap_or(&[]) {
                        if !value.is_empty() && boolean(value)? {
                            return Err("equal-axis trim unsupported");
                        }
                    }
                    effects.trim = Some(Trim {
                        threshold: threshold as u8,
                        color,
                    });
                }
            }
            "quality" | "q" => {
                if args.len() != 1 {
                    return Err("invalid quality");
                }
                let q = args[0].parse::<u8>().map_err(|_| "invalid quality")?;
                if q > 100 {
                    return Err("invalid quality");
                }
                quality = Some(q);
            }
            "format" | "f" | "ext" => {
                if args.len() != 1 {
                    return Err("invalid output format");
                }
                fmt = Some(format(args[0])?);
            }
            "auto_rotate" | "ar" => {
                if args.len() != 1 {
                    return Err("invalid auto rotation");
                }
                auto_rotate = boolean(args[0])?;
            }
            "strip_metadata" | "sm" => {
                if args.len() != 1 {
                    return Err("invalid metadata option");
                }
                strip_metadata = boolean(args[0])?;
            }
            "strip_color_profile" | "scp" => {
                if args.len() != 1 {
                    return Err("invalid color profile option");
                }
                strip_color_profile = boolean(args[0])?;
            }
            _ => return Err("unsupported processing option"),
        }
    }
    let remaining: Vec<_> = parts.collect();
    let (source, extension) = source_and_extension(&remaining)?;
    if let Some(extension) = extension {
        fmt = Some(extension);
    }
    if fmt == Some(OutputFormat::Webp) && quality.is_some_and(|q| q != 100) {
        return Err("lossy WebP quality is unsupported");
    }
    if let Some((width, height, crop_gravity)) = explicit_crop {
        options.crop = Some(Crop {
            width,
            height,
            gravity: crop_gravity.unwrap_or(options.gravity),
        });
    }
    Ok(Request {
        source,
        options,
        format: fmt,
        quality,
        auto_rotate,
        strip_metadata,
        strip_color_profile,
        effects,
        watermark,
    })
}
