//! Typed native encoder settings and environment-derived processing defaults.
use super::*;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Encoding {
    pub jpeg_progressive: bool,
    pub png_interlaced: bool,
    pub png_quantize: bool,
    pub png_colors: u16,
    pub webp_effort: u8,
    pub webp_preset: String,
    pub avif_effort: u8,
    pub jxl_effort: u8,
    pub linear: bool,
    pub rasterize_svg: bool,
    pub sanitize_svg: bool,
}
impl Default for Encoding {
    fn default() -> Self {
        Self {
            jpeg_progressive: false,
            png_interlaced: false,
            png_quantize: false,
            png_colors: 256,
            webp_effort: 4,
            webp_preset: "default".into(),
            avif_effort: 1,
            jxl_effort: 4,
            linear: false,
            rasterize_svg: false,
            sanitize_svg: false,
        }
    }
}

fn flag(value: &str) -> Result<bool, Error> {
    match value {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err("invalid processing flag"),
    }
}
fn integer(value: &str, min: u16, max: u16) -> Result<u16, Error> {
    let n = value
        .parse::<u16>()
        .map_err(|_| "invalid processing number")?;
    if !(min..=max).contains(&n) {
        return Err("processing number exceeds range");
    }
    Ok(n)
}
impl Config {
    pub fn with_environment(mut self) -> Result<Self, Error> {
        for (name, option) in [
            ("PIXSTRIDE_QUALITY", "q"),
            ("PIXSTRIDE_FORMAT_QUALITY", "fq"),
            ("PIXSTRIDE_STRIP_METADATA", "sm"),
            ("PIXSTRIDE_KEEP_COPYRIGHT", "kcr"),
            ("PIXSTRIDE_STRIP_COLOR_PROFILE", "scp"),
            ("PIXSTRIDE_AUTO_ROTATE", "ar"),
            ("PIXSTRIDE_PRESERVE_HDR", "ph"),
            ("PIXSTRIDE_ENFORCE_THUMBNAIL", "eth"),
            ("PIXSTRIDE_MAX_SRC_RESOLUTION", "msr"),
            ("PIXSTRIDE_MAX_SRC_FILE_SIZE", "msfs"),
            ("PIXSTRIDE_MAX_ANIMATION_FRAMES", "maf"),
            ("PIXSTRIDE_MAX_ANIMATION_FRAME_RESOLUTION", "mafr"),
            ("PIXSTRIDE_MAX_RESULT_DIMENSION", "mrd"),
            ("PIXSTRIDE_SKIP_PROCESSING_FORMATS", "skp"),
        ] {
            if let Ok(value) = std::env::var(name) {
                let value = value.replace(['=', ','], ":");
                self.defaults.push(format!("{option}:{value}"));
            }
        }
        let mut configured = false;
        for name in [
            "PIXSTRIDE_JPEG_PROGRESSIVE",
            "PIXSTRIDE_PNG_INTERLACED",
            "PIXSTRIDE_PNG_QUANTIZE",
            "PIXSTRIDE_PNG_QUANTIZATION_COLORS",
            "PIXSTRIDE_WEBP_EFFORT",
            "PIXSTRIDE_WEBP_PRESET",
            "PIXSTRIDE_AVIF_SPEED",
            "PIXSTRIDE_JXL_EFFORT",
            "PIXSTRIDE_USE_LINEAR_COLORSPACE",
            "PIXSTRIDE_ALWAYS_RASTERIZE_SVG",
            "PIXSTRIDE_SANITIZE_SVG",
        ] {
            if let Ok(value) = std::env::var(name) {
                self.encoding.set(name, &value)?;
                configured = true;
            }
        }
        if configured && !cfg!(feature = "native-codecs") {
            return Err("encoder settings require native-codecs");
        }
        if let Ok(value) = std::env::var("PIXSTRIDE_PREFERRED_FORMATS") {
            self.preferred = value
                .split(',')
                .map(|v| format(v.trim()))
                .collect::<Result<_, _>>()?;
            if self.preferred.is_empty() || self.preferred.len() > 11 {
                return Err("invalid preferred formats");
            }
        }
        self.native_defaults =
            configured || !self.defaults.is_empty() || !self.preferred.is_empty();
        self.policy = policy::Policy::from_env()?;
        // Validate every default now, including when no image request follows.
        let probe = Self {
            auth: Auth::UnsafeLocal,
            presets: Default::default(),
            defaults: self.defaults.clone(),
            encoding: self.encoding.clone(),
            preferred: self.preferred.clone(),
            native_defaults: self.native_defaults,
            signature_size: 32,
            trusted: Vec::new(),
            policy: Default::default(),
        };
        super::parse("/unsafe/plain/local:///defaults.png@png", &probe)?;
        Ok(self)
    }
}
impl Encoding {
    fn set(&mut self, name: &str, value: &str) -> Result<(), Error> {
        match name {
            "PIXSTRIDE_SANITIZE_SVG" => self.sanitize_svg = flag(value)?,
            "PIXSTRIDE_ALWAYS_RASTERIZE_SVG" => self.rasterize_svg = flag(value)?,
            "PIXSTRIDE_JPEG_PROGRESSIVE" => self.jpeg_progressive = flag(value)?,
            "PIXSTRIDE_PNG_INTERLACED" => self.png_interlaced = flag(value)?,
            "PIXSTRIDE_PNG_QUANTIZE" => self.png_quantize = flag(value)?,
            "PIXSTRIDE_USE_LINEAR_COLORSPACE" => self.linear = flag(value)?,
            "PIXSTRIDE_PNG_QUANTIZATION_COLORS" => self.png_colors = integer(value, 2, 256)?,
            "PIXSTRIDE_WEBP_EFFORT" => self.webp_effort = integer(value, 1, 6)? as u8,
            "PIXSTRIDE_AVIF_SPEED" => self.avif_effort = 9 - integer(value, 0, 9)? as u8,
            "PIXSTRIDE_JXL_EFFORT" => self.jxl_effort = integer(value, 1, 9)? as u8,
            "PIXSTRIDE_WEBP_PRESET" => {
                let value = value.to_ascii_lowercase();
                if !["default", "picture", "photo", "drawing", "icon", "text"]
                    .contains(&value.as_str())
                {
                    return Err("invalid WebP preset");
                }
                self.webp_preset = value;
            }
            _ => return Err("unknown encoder setting"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoder_ranges_and_flags_are_validated_without_environment_mutation() {
        let mut e = Encoding::default();
        e.set("PIXSTRIDE_PNG_QUANTIZATION_COLORS", "16").unwrap();
        assert_eq!(e.png_colors, 16);
        assert!(e.set("PIXSTRIDE_WEBP_EFFORT", "7").is_err());
        assert!(e.set("PIXSTRIDE_PNG_QUANTIZATION_COLORS", "1").is_err());
        assert!(e.set("PIXSTRIDE_JPEG_PROGRESSIVE", "yes").is_err());
        assert!(e.set("PIXSTRIDE_WEBP_PRESET", "other").is_err());
    }
}
