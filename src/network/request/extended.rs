//! OSS options shared by the HTTP facade and isolated native engine.
use super::*;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Options {
    pub native: bool,
    pub encoding: defaults::Encoding,
    pub preferred: Vec<OutputFormat>,
    pub base_opacity: f64,
    pub dpr: f64,
    pub zoom: [f64; 2],
    pub min: [usize; 2],
    pub extend: bool,
    pub extend_aspect: bool,
    pub extend_gravity: Gravity,
    pub extend_offsets: [f64; 2],
    pub aspect_gravity: Gravity,
    pub aspect_offsets: [f64; 2],
    pub offsets: [f64; 2],
    pub smart: bool,
    pub fill_down: bool,
    pub crop_size: Option<[f64; 2]>,
    pub crop_offsets: [f64; 2],
    pub crop_smart: bool,
    pub watermark_repeat: bool,
    pub trim_equal: [bool; 2],
    pub format_quality: Vec<(OutputFormat, u8)>,
    pub max_bytes: usize,
    pub preserve_hdr: bool,
    pub keep_copyright: bool,
    pub enforce_thumbnail: bool,
    pub skip: Vec<OutputFormat>,
    pub raw: bool,
    pub expires: Option<u64>,
    pub filename: Option<String>,
    pub attachment: bool,
    pub max_source_pixels: usize,
    pub max_source_bytes: usize,
    pub max_frames: usize,
    pub max_frame_pixels: usize,
    pub max_result_dimension: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            native: false,
            encoding: Default::default(),
            preferred: Vec::new(),
            base_opacity: 1.0,
            dpr: 1.0,
            zoom: [1.0; 2],
            min: [0; 2],
            extend: false,
            extend_aspect: false,
            extend_gravity: Gravity::Center,
            extend_offsets: [0.0; 2],
            aspect_gravity: Gravity::Center,
            aspect_offsets: [0.0; 2],
            offsets: [0.0; 2],
            smart: false,
            fill_down: false,
            crop_size: None,
            crop_offsets: [0.0; 2],
            crop_smart: false,
            watermark_repeat: false,
            trim_equal: [false; 2],
            format_quality: Vec::new(),
            max_bytes: 0,
            preserve_hdr: false,
            keep_copyright: false,
            enforce_thumbnail: false,
            skip: Vec::new(),
            raw: false,
            expires: None,
            filename: None,
            attachment: false,
            max_source_pixels: crate::MAX_PIXELS,
            max_source_bytes: crate::MAX_FILE,
            max_frames: 1,
            max_frame_pixels: crate::MAX_PIXELS,
            max_result_dimension: 100_000,
        }
    }
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Bmp => "bmp",
            Self::Gif => "gif",
            Self::Tiff => "tiff",
            Self::Ico => "ico",
            Self::Avif => "avif",
            Self::Heic => "heic",
            Self::Jxl => "jxl",
            Self::Svg => "svg",
        }
    }
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Bmp => "image/bmp",
            Self::Gif => "image/gif",
            Self::Tiff => "image/tiff",
            Self::Ico => "image/x-icon",
            Self::Avif => "image/avif",
            Self::Heic => "image/heic",
            Self::Jxl => "image/jxl",
            Self::Svg => "image/svg+xml",
        }
    }
}

pub(super) fn number(value: &str, zero: bool) -> Result<f64, Error> {
    let n = value.parse::<f64>().map_err(|_| "invalid finite number")?;
    if !n.is_finite() || n < 0.0 || (!zero && n == 0.0) || n > 100_000.0 {
        return Err("number exceeds supported range");
    }
    Ok(n)
}

pub(super) fn gravity_options(
    args: &[&str],
    offsets: &mut [f64; 2],
    smart: &mut bool,
) -> Result<Gravity, Error> {
    if args.is_empty() || args.len() > 3 {
        return Err("invalid gravity");
    }
    *smart = args[0] == "sm";
    if *smart {
        if args.len() != 1 {
            return Err("smart gravity does not accept offsets");
        }
        return Ok(Gravity::Center);
    }
    if args[0] == "fp" {
        *offsets = [0.0; 2];
        return gravity(args);
    }
    let result = gravity(&args[..1])?;
    for (index, value) in args.iter().skip(1).enumerate() {
        if value.is_empty() {
            continue;
        }
        let n = value.parse::<f64>().map_err(|_| "invalid gravity offset")?;
        if !n.is_finite() || n.abs() > 100_000.0 {
            return Err("gravity offset exceeds limit");
        }
        offsets[index] = n;
    }
    Ok(result)
}

pub(super) fn extend(args: &[&str], o: &mut Options, aspect: bool) -> Result<(), Error> {
    if args.is_empty() || args.len() > 4 {
        return Err("invalid extend arguments");
    }
    let enabled = boolean(args[0])?;
    if aspect {
        o.extend_aspect = enabled;
    } else {
        o.extend = enabled;
    }
    if args.len() > 1 {
        let mut offsets = [0.0; 2];
        let mut smart = false;
        let gravity = gravity_options(&args[1..], &mut offsets, &mut smart)?;
        if smart {
            return Err("smart extend gravity unsupported");
        }
        if aspect {
            o.aspect_gravity = gravity;
            o.aspect_offsets = offsets;
        } else {
            o.extend_gravity = gravity;
            o.extend_offsets = offsets;
        }
    }
    o.native = true;
    Ok(())
}

pub(super) fn apply(name: &str, args: &[&str], o: &mut Options) -> Result<bool, Error> {
    let one = || {
        if args.len() == 1 {
            Ok(args[0])
        } else {
            Err("one argument required")
        }
    };
    match name {
        "dpr" => {
            o.dpr = number(one()?, false)?;
            o.native = true;
        }
        "zoom" | "z" => {
            if args.is_empty() || args.len() > 2 {
                return Err("invalid zoom");
            }
            o.zoom = [
                number(args[0], false)?,
                number(args.get(1).unwrap_or(&args[0]), false)?,
            ];
            o.native = true;
        }
        "min-width" | "mw" => {
            o.min[0] = decimal(one()?)?;
            o.native = true;
        }
        "min-height" | "mh" => {
            o.min[1] = decimal(one()?)?;
            o.native = true;
        }
        "extend" | "ex" => extend(args, o, false)?,
        "extend_aspect_ratio" | "extend_ar" | "exar" => extend(args, o, true)?,
        "preserve_hdr" | "ph" => {
            o.preserve_hdr = boolean(one()?)?;
            o.native |= o.preserve_hdr;
        }
        "keep_copyright" | "kcr" => {
            o.keep_copyright = boolean(one()?)?;
            o.native |= o.keep_copyright;
        }
        "enforce_thumbnail" | "eth" => {
            o.enforce_thumbnail = boolean(one()?)?;
            o.native |= o.enforce_thumbnail;
        }
        "max_bytes" | "mb" => {
            o.max_bytes = one()?.parse().map_err(|_| "invalid byte limit")?;
            if o.max_bytes > crate::MAX_FILE {
                return Err("byte limit exceeds output limit");
            }
            o.native = true;
        }
        "format_quality" | "fq" => {
            if args.is_empty() || !args.len().is_multiple_of(2) || args.len() > 22 {
                return Err("format quality needs format/quality pairs");
            }
            for pair in args.chunks_exact(2) {
                let q = pair[1].parse::<u8>().map_err(|_| "invalid quality")?;
                if q > 100 {
                    return Err("invalid quality");
                }
                let f = format(pair[0])?;
                o.format_quality.retain(|(old, _)| *old != f);
                o.format_quality.push((f, q));
            }
            o.native = true;
        }
        "skip_processing" | "skp" => {
            o.skip = args.iter().map(|s| format(s)).collect::<Result<_, _>>()?;
            o.native = true;
        }
        "raw" => {
            o.raw = boolean(one()?)?;
            o.native |= o.raw;
        }
        "cachebuster" | "cb" => {
            one()?;
        }
        "expires" | "exp" => {
            let n = one()?.parse::<i64>().map_err(|_| "invalid expiration")?;
            o.expires = if n > 0 { Some(n as u64) } else { None };
        }
        "filename" | "fn" => {
            if args.is_empty() || args.len() > 2 {
                return Err("invalid filename");
            }
            let value = if args.len() == 2 && boolean(args[1])? {
                String::from_utf8(signature::decode_base64url(args[0], true)?)
                    .map_err(|_| "invalid filename")?
            } else {
                percent_decode(args[0])?
            };
            if value.len() > 255
                || value
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '/' | '\\'))
            {
                return Err("invalid filename");
            }
            o.filename = Some(value);
        }
        "return_attachment" | "att" => o.attachment = boolean(one()?)?,
        "max_src_resolution" | "msr" => {
            let n = number(one()?, false)? * 1_000_000.0;
            if n > crate::MAX_PIXELS as f64 {
                return Err("source limit cannot exceed server limit");
            }
            o.max_source_pixels = n as usize;
            o.native = true;
        }
        "max_src_file_size" | "msfs" => {
            o.max_source_bytes = one()?.parse().map_err(|_| "invalid source byte limit")?;
            if o.max_source_bytes == 0 || o.max_source_bytes > crate::MAX_FILE {
                return Err("source byte limit exceeds server limit");
            }
        }
        "max_animation_frames" | "maf" => {
            o.max_frames = decimal(one()?)?;
            if !(1..=256).contains(&o.max_frames) {
                return Err("invalid animation limit");
            }
            o.native = true;
        }
        "max_animation_frame_resolution" | "mafr" => {
            let n = number(one()?, false)? * 1_000_000.0;
            if n > crate::MAX_PIXELS as f64 {
                return Err("frame limit exceeds server limit");
            }
            o.max_frame_pixels = n as usize;
            o.native = true;
        }
        "max_result_dimension" | "mrd" => {
            let n = decimal(one()?)?;
            o.max_result_dimension = if n == 0 { 100_000 } else { n };
            o.native = true;
        }
        _ => return Ok(false),
    }
    Ok(true)
}
