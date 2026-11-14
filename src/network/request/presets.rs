//! Bounded, ordered preset expansion after signature verification.
use super::Error;
use std::collections::BTreeMap;

#[derive(Default, Clone)]
pub(super) struct Presets(BTreeMap<String, Vec<String>>);

impl Presets {
    pub fn parse(value: &str) -> Result<Self, Error> {
        if value.len() > 8192 {
            return Err("preset configuration exceeds limit");
        }
        let mut presets = BTreeMap::new();
        for line in value
            .split([';', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
        {
            let (name, body) = line.split_once('=').ok_or("invalid preset definition")?;
            let name = name.trim();
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            {
                return Err("invalid preset name");
            }
            let options = body
                .trim()
                .split('/')
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if options
                .iter()
                .any(|o| !o.contains(':') || o.contains(['=', '\r']))
                || presets.insert(name.into(), options).is_some()
                || presets.len() > 64
            {
                return Err("invalid or duplicate preset");
            }
        }
        let result = Self(presets);
        for name in result.0.keys() {
            result.expand(&[format!("pr:{name}")], false)?;
        }
        Ok(result)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }
    pub fn expand(&self, options: &[String], default: bool) -> Result<Vec<String>, Error> {
        let mut result = Vec::new();
        if default && self.0.contains_key("default") {
            self.visit("pr:default", &mut Vec::new(), &mut result)?;
        }
        for option in options {
            self.visit(option, &mut Vec::new(), &mut result)?;
        }
        Ok(result)
    }

    fn visit(
        &self,
        option: &str,
        stack: &mut Vec<String>,
        result: &mut Vec<String>,
    ) -> Result<(), Error> {
        let (name, args) = option.split_once(':').ok_or("invalid preset option")?;
        if name == "preset" || name == "pr" {
            for name in args.split(':') {
                if stack.len() >= 8 || stack.iter().any(|s| s == name) {
                    return Err("preset cycle or depth limit");
                }
                let options = self.0.get(name).ok_or("unknown preset")?;
                stack.push(name.into());
                for option in options {
                    self.visit(option, stack, result)?;
                }
                stack.pop();
            }
        } else {
            if result.len() >= 256 {
                return Err("expanded option count exceeds limit");
            }
            result.push(option.into());
            if result.iter().map(String::len).sum::<usize>() > 16384 {
                return Err("expanded options exceed limit");
            }
        }
        Ok(())
    }
}
