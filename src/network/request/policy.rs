//! Restrict public URL options and rewrite sources before transport admission.
use super::*;
use regex::{Captures, Regex, RegexBuilder};
#[derive(Clone)]
pub(super) struct Policy {
    pub separator: String,
    pub only_presets: bool,
    pub allow_security: bool,
    pub filename_suffix: bool,
    allowed_options: Vec<String>,
    allowed_sources: Vec<Regex>,
    replacements: Vec<(Regex, String)>,
    base: String,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            separator: ":".into(),
            only_presets: false,
            allow_security: true,
            filename_suffix: false,
            allowed_options: Vec::new(),
            allowed_sources: Vec::new(),
            replacements: Vec::new(),
            base: String::new(),
        }
    }
}
fn pattern(value: &str) -> Result<Regex, Error> {
    if value.len() > 1024 || value.matches('*').count() > 32 {
        return Err("source pattern exceeds limit");
    }
    let pattern = format!(
        "^{}",
        value
            .split('*')
            .map(regex::escape)
            .collect::<Vec<_>>()
            .join("([^/]*)")
    );
    RegexBuilder::new(&pattern)
        .size_limit(2 * 1024 * 1024)
        .dfa_size_limit(256 * 1024)
        .build()
        .map_err(|_| "invalid source pattern")
}
fn flag(name: &str, default: bool) -> Result<bool, Error> {
    match std::env::var(name).as_deref() {
        Ok("1" | "true") => Ok(true),
        Ok("0" | "false") => Ok(false),
        Err(std::env::VarError::NotPresent) => Ok(default),
        _ => Err("invalid parser policy flag"),
    }
}
fn append(out: &mut String, value: &str) -> Result<(), Error> {
    if value.len() > MAX_SOURCE.saturating_sub(out.len()) {
        return Err("rewritten source exceeds limit");
    }
    out.push_str(value);
    Ok(())
}
fn expand(captures: &Captures<'_>, replacement: &str) -> Result<String, Error> {
    let mut result = String::new();
    let mut rest = replacement;
    while let Some((prefix, tail)) = rest.split_once('$') {
        append(&mut result, prefix)?;
        if let Some(tail) = tail.strip_prefix('$') {
            append(&mut result, "$")?;
            rest = tail;
            continue;
        }
        let (name, tail) = if let Some(tail) = tail.strip_prefix('{') {
            if let Some((name, tail)) = tail.split_once('}') {
                (name, tail)
            } else {
                append(&mut result, "$")?;
                rest = tail;
                continue;
            }
        } else {
            let len = tail
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                .count();
            (&tail[..len], &tail[len..])
        };
        if name.is_empty() {
            append(&mut result, "$")?;
        } else if let Some(value) = name
            .parse::<usize>()
            .ok()
            .and_then(|n| captures.get(n))
            .or_else(|| captures.name(name))
        {
            append(&mut result, value.as_str())?;
        }
        rest = tail;
    }
    append(&mut result, rest)?;
    Ok(result)
}
impl Policy {
    pub fn from_env() -> Result<Self, Error> {
        let mut p = Self {
            only_presets: flag("PIXSTRIDE_ONLY_PRESETS", false)?,
            allow_security: flag("PIXSTRIDE_ALLOW_SECURITY_OPTIONS", true)?,
            filename_suffix: flag("PIXSTRIDE_BASE64_URL_INCLUDES_FILENAME", false)?,
            ..Default::default()
        };
        if let Ok(value) = std::env::var("PIXSTRIDE_ARGUMENTS_SEPARATOR") {
            if value.is_empty()
                || value.len() > 8
                || value
                    .bytes()
                    .any(|b| b <= 32 || b >= 127 || b"/=?#".contains(&b))
            {
                return Err("invalid arguments separator");
            }
            p.separator = value;
        }
        if let Some(value) = crate::config::secret("PIXSTRIDE_ALLOWED_PROCESSING_OPTIONS")? {
            p.allowed_options = value.split(',').map(|s| s.trim().to_owned()).collect();
            if p.allowed_options.len() > 128 {
                return Err("too many allowed options");
            }
        }
        if let Some(value) = crate::config::secret("PIXSTRIDE_ALLOWED_SOURCES")? {
            p.allowed_sources = value
                .split(',')
                .map(|s| pattern(s.trim()))
                .collect::<Result<_, _>>()?;
            if p.allowed_sources.len() > 32 {
                return Err("too many source patterns");
            }
        }
        if let Some(value) = crate::config::secret("PIXSTRIDE_URL_REPLACEMENTS")? {
            for rule in value.split(';') {
                let (from, to) = rule.split_once('=').ok_or("invalid URL replacement")?;
                if to.len() > 1024 || p.replacements.len() >= 16 {
                    return Err("URL replacements exceed limit");
                }
                p.replacements.push((pattern(from)?, to.into()));
            }
        }
        p.base = crate::config::secret("PIXSTRIDE_BASE_URL")?.unwrap_or_default();
        if p.base.len() > 1024 {
            return Err("base URL exceeds limit");
        }
        Ok(p)
    }
    pub fn options(&self, requested: &[String]) -> Result<(), Error> {
        for option in requested {
            let name = option.split(':').next().unwrap_or("");
            if !self.allowed_options.is_empty() && !self.allowed_options.iter().any(|s| s == name) {
                return Err("processing option is disabled");
            }
            if !self.allow_security
                && [
                    "msr",
                    "max_src_resolution",
                    "msfs",
                    "max_src_file_size",
                    "maf",
                    "max_animation_frames",
                    "mafr",
                    "max_animation_frame_resolution",
                    "mrd",
                    "max_result_dimension",
                ]
                .contains(&name)
            {
                return Err("URL security options are disabled");
            }
        }
        Ok(())
    }
    pub fn source(&self, mut source: String) -> Result<String, Error> {
        if source.len() > MAX_SOURCE {
            return Err("source exceeds limit");
        }
        for (pattern, to) in &self.replacements {
            if let Some(captures) = pattern.captures(&source) {
                let mut result = expand(&captures, to)?;
                append(&mut result, &source[captures.get(0).unwrap().end()..])?;
                source = result;
            }
        }
        if !self.base.is_empty() && !source.starts_with(&self.base) {
            let mut result = self.base.clone();
            append(&mut result, &source)?;
            source = result;
        }
        validate_source(&source)?;
        if !self.allowed_sources.is_empty()
            && !self.allowed_sources.iter().any(|p| p.is_match(&source))
        {
            return Err("source does not match allowed patterns");
        }
        Ok(source)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wildcard_replacements_are_bounded_and_transport_policy_still_follows() {
        let p = Policy {
            replacements: vec![(pattern("alias://*/").unwrap(), "https://$1/".into())],
            allowed_sources: vec![pattern("https://example.test/public/").unwrap()],
            ..Default::default()
        };
        assert_eq!(
            p.source("alias://example.test/public/a.png".into())
                .unwrap(),
            "https://example.test/public/a.png"
        );
        assert!(p
            .source("alias://example.test/private/a.png".into())
            .is_err());
        assert!(p.source("alias://other.test/public/a.png".into()).is_err());
        let p = Policy {
            replacements: vec![(pattern("*").unwrap(), "${1}${1}".repeat(100))],
            ..Default::default()
        };
        assert!(p.source("a".repeat(1000)).is_err());
    }
}
