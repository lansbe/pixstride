use super::*;
impl Default for WatermarkSpec {
    fn default() -> Self {
        Self {
            opacity: 0.0,
            gravity: Gravity::Center,
            offset_x: 0.0,
            offset_y: 0.0,
            scale: 0.0,
        }
    }
}
impl Config {
    pub fn signed(key: Vec<u8>, salt: Vec<u8>) -> Result<Self, Error> {
        if key.len() < 32 || salt.len() < 16 || key.len() > 4096 || salt.len() > 4096 {
            return Err("invalid signing configuration");
        }
        Ok(Self {
            auth: Auth::Signed { key, salt },
        })
    }

    /// The caller must bind the resulting service to loopback only.
    /// Requests must explicitly use the `/unsafe/` signature segment.
    pub fn unsafe_local() -> Self {
        Self {
            auth: Auth::UnsafeLocal,
        }
    }

    /// No configured key and salt selects explicit `/unsafe/` local requests.
    /// A partially configured or invalid pair is always an error, never a downgrade.
    pub fn from_env() -> Result<Self, Error> {
        let key = crate::config::secret("PIXSTRIDE_KEY")?;
        let salt = crate::config::secret("PIXSTRIDE_SALT")?;
        match (key,salt) {
            (Some(key),Some(salt))=>Self::signed(signature::decode_hex(&key)?,signature::decode_hex(&salt)?),
            (None,None) if std::env::var("PIXSTRIDE_ALLOW_UNSAFE").as_deref()==Ok("1")=>Ok(Self::unsafe_local()),
            _=>Err("configure PIXSTRIDE_KEY and PIXSTRIDE_SALT; unsigned mode requires explicit PIXSTRIDE_ALLOW_UNSAFE=1"),
        }
    }

    pub(crate) fn verify(&self, value: &str, normalized_path: &str) -> Result<(), Error> {
        match &self.auth {
            Auth::UnsafeLocal if value == "unsafe" => Ok(()),
            Auth::Signed { key, salt }
                if signature::verify_imgproxy(value, normalized_path, key, salt) =>
            {
                Ok(())
            }
            _ => Err("invalid signature"),
        }
    }
}
pub(crate) fn format(value: &str) -> Result<OutputFormat, Error> {
    match value {
        "png" => Ok(OutputFormat::Png),
        "bmp" => Ok(OutputFormat::Bmp),
        "jpeg" | "jpg" => Ok(OutputFormat::Jpeg),
        "webp" => Ok(OutputFormat::Webp),
        "tiff" | "tif" => Ok(OutputFormat::Tiff),
        "ico" => Ok(OutputFormat::Ico),
        "gif" => Ok(OutputFormat::Gif),
        _ => Err("unsupported output format"),
    }
}
pub(crate) fn decimal(value: &str) -> Result<usize, Error> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("invalid dimension");
    }
    let n = value.parse::<usize>().map_err(|_| "invalid dimension")?;
    if n > MAX_DIMENSION {
        return Err("dimension exceeds limit");
    }
    Ok(n)
}
pub(crate) fn boolean(value: &str) -> Result<bool, Error> {
    match value {
        "1" | "t" | "T" | "true" | "TRUE" | "True" => Ok(true),
        "0" | "f" | "F" | "false" | "FALSE" | "False" => Ok(false),
        _ => Err("invalid boolean"),
    }
}
pub(crate) fn sigma(value: &str) -> Result<f64, Error> {
    let n = value
        .parse::<f64>()
        .map_err(|_| "invalid effect strength")?;
    if !n.is_finite() || !(0.0..=100.0).contains(&n) {
        return Err("invalid effect strength");
    }
    Ok(n)
}
pub(crate) fn color(values: &[&str]) -> Result<Option<[u8; 4]>, Error> {
    fn digit(b: u8) -> Result<u8, Error> {
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            b'A'..=b'F' => Ok(b - b'A' + 10),
            _ => Err("invalid background color"),
        }
    }
    match values {
        [""] => Ok(None),
        [hex] if hex.len() == 3 => {
            let h = hex.as_bytes();
            Ok(Some([
                digit(h[0])? * 17,
                digit(h[1])? * 17,
                digit(h[2])? * 17,
                255,
            ]))
        }
        [hex] if hex.len() == 6 => {
            let h = hex.as_bytes();
            Ok(Some([
                (digit(h[0])? << 4) | digit(h[1])?,
                (digit(h[2])? << 4) | digit(h[3])?,
                (digit(h[4])? << 4) | digit(h[5])?,
                255,
            ]))
        }
        [r, g, b] => {
            let parse = |v: &str| v.parse::<u8>().map_err(|_| "invalid background color");
            Ok(Some([parse(r)?, parse(g)?, parse(b)?, 255]))
        }
        _ => Err("invalid background color"),
    }
}
pub(crate) fn gravity(values: &[&str]) -> Result<Gravity, Error> {
    if values.is_empty() || values.len() > 3 {
        return Err("invalid gravity");
    }
    if values[0] == "fp" {
        if values.len() != 3 {
            return Err("focus point requires x and y");
        }
        let x = values[1]
            .parse::<f64>()
            .map_err(|_| "invalid focus point")?;
        let y = values[2]
            .parse::<f64>()
            .map_err(|_| "invalid focus point")?;
        if !x.is_finite()
            || !y.is_finite()
            || !(0.0..=1.0).contains(&x)
            || !(0.0..=1.0).contains(&y)
        {
            return Err("invalid focus point");
        }
        return Ok(Gravity::Focus { x, y });
    }
    // The current operations type has no offset. Do not accept nonzero offsets
    // and silently drop them. Empty and numeric zero offsets are equivalent.
    for value in &values[1..] {
        if !value.is_empty()
            && value
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v == 0.0)
                .is_none()
        {
            return Err("gravity offset not supported");
        }
    }
    match values[0] {
        "ce" => Ok(Gravity::Center),
        "no" => Ok(Gravity::North),
        "so" => Ok(Gravity::South),
        "ea" => Ok(Gravity::East),
        "we" => Ok(Gravity::West),
        "noea" => Ok(Gravity::NorthEast),
        "nowe" => Ok(Gravity::NorthWest),
        "soea" => Ok(Gravity::SouthEast),
        "sowe" => Ok(Gravity::SouthWest),
        _ => Err("unsupported gravity"),
    }
}
pub(crate) fn normalize_path(path: &str) -> String {
    let (options, plain) = if let Some(rest) = path.strip_prefix("plain/") {
        ("", Some(rest))
    } else if let Some((options, rest)) = path.split_once("/plain/") {
        (options, Some(rest))
    } else {
        (path, None)
    };
    let options = options.replace("%3A", ":");
    if let Some(plain) = plain {
        // Match imgproxy's restoration of a single slash after the URL scheme.
        let mut restored = plain.to_owned();
        if let Some((scheme, rest)) = plain.split_once(":/") {
            if !scheme.is_empty()
                && !scheme.chars().any(char::is_whitespace)
                && !rest.is_empty()
                && !rest.starts_with('/')
            {
                restored = if scheme == "local" {
                    format!("{scheme}:///{rest}")
                } else {
                    format!("{scheme}://{rest}")
                };
            }
        }
        let normalized = format!("{options}/plain/{restored}");
        normalized
            .strip_prefix('/')
            .unwrap_or(&normalized)
            .to_owned()
    } else {
        options.strip_prefix('/').unwrap_or(&options).to_owned()
    }
}
pub(crate) fn percent_decode(value: &str) -> Result<String, Error> {
    let bytes = value.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err("invalid source URL encoding");
            }
            let a = (bytes[i + 1] as char)
                .to_digit(16)
                .ok_or("invalid source URL encoding")?;
            let b = (bytes[i + 2] as char)
                .to_digit(16)
                .ok_or("invalid source URL encoding")?;
            result.push(((a << 4) | b) as u8);
            i += 3;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(result).map_err(|_| "invalid source URL encoding")
}
pub(crate) fn validate_source(source: &str) -> Result<(), Error> {
    if source.is_empty()
        || source.len() > MAX_SOURCE
        || source.bytes().any(|b| b <= 32 || b >= 127 || b == b'\\')
    {
        return Err("invalid source URL");
    }
    if let Some(name) = source.strip_prefix("local:///") {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '?', '#']) {
            return Err("invalid local source");
        }
        return Ok(());
    }
    let rest = source
        .strip_prefix("http://")
        .or_else(|| source.strip_prefix("https://"))
        .ok_or("unsupported source scheme")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') {
        return Err("invalid source URL");
    }
    Ok(())
}
pub(crate) fn source_and_extension(
    parts: &[&str],
) -> Result<(String, Option<OutputFormat>), Error> {
    if parts.is_empty() {
        return Err("missing source URL");
    }
    let (source, extension) = if parts[0] == "plain" {
        if parts.len() < 2 {
            return Err("missing source URL");
        }
        let encoded = parts[1..].join("/");
        let mut split = encoded.split('@');
        let raw = split.next().unwrap_or("");
        let suffix = split.next();
        if split.next().is_some() || raw.is_empty() {
            return Err("invalid plain source URL");
        }
        (
            percent_decode(raw)?,
            suffix.filter(|s| !s.is_empty()).map(format).transpose()?,
        )
    } else {
        let encoded = parts.join("");
        let mut split = encoded.split('.');
        let raw = split.next().unwrap_or("");
        let suffix = split.next();
        if split.next().is_some() || raw.is_empty() {
            return Err("invalid encoded source URL");
        }
        let decoded =
            signature::decode_base64url(raw, true).map_err(|_| "invalid encoded source URL")?;
        let source = String::from_utf8(decoded).map_err(|_| "invalid encoded source URL")?;
        (
            source,
            suffix.filter(|s| !s.is_empty()).map(format).transpose()?,
        )
    };
    validate_source(&source)?;
    Ok((source, extension))
}
