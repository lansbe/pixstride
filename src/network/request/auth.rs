//! Immutable signing configuration; key rotation never enables unsigned URLs.
use super::*;

impl Config {
    #[cfg(test)]
    pub fn signed(key: Vec<u8>, salt: Vec<u8>) -> Result<Self, Error> {
        Self::rotating(vec![(key, salt)])
    }

    pub fn rotating(pairs: Vec<(Vec<u8>, Vec<u8>)>) -> Result<Self, Error> {
        if pairs.is_empty()
            || pairs.len() > 8
            || pairs.iter().any(|(key, salt)| {
                !(32..=4096).contains(&key.len()) || !(16..=4096).contains(&salt.len())
            })
        {
            return Err("invalid signing configuration");
        }
        Ok(Self {
            auth: Auth::Signed { pairs },
            presets: Default::default(),
            defaults: Vec::new(),
            encoding: Default::default(),
            preferred: Vec::new(),
            native_defaults: false,
            signature_size: 32,
            trusted: Vec::new(),
            policy: Default::default(),
        })
    }

    /// The caller must restrict access to trusted local clients.
    pub fn unsafe_local() -> Self {
        Self {
            auth: Auth::UnsafeLocal,
            presets: Default::default(),
            defaults: Vec::new(),
            encoding: Default::default(),
            preferred: Vec::new(),
            native_defaults: false,
            signature_size: 32,
            trusted: Vec::new(),
            policy: Default::default(),
        }
    }

    pub fn with_presets(mut self, value: &str) -> Result<Self, Error> {
        self.presets = presets::Presets::parse(value)?;
        let probe = Self {
            auth: Auth::UnsafeLocal,
            presets: self.presets.clone(),
            defaults: self.defaults.clone(),
            encoding: self.encoding.clone(),
            preferred: self.preferred.clone(),
            native_defaults: self.native_defaults,
            signature_size: 32,
            trusted: Vec::new(),
            policy: Default::default(),
        };
        for name in self.presets.names() {
            super::parse(
                &format!("/unsafe/pr:{name}/plain/local:///preset-validation.png@png"),
                &probe,
            )?;
        }
        Ok(self)
    }

    pub(crate) fn requires_local_host(&self) -> bool {
        matches!(self.auth, Auth::UnsafeLocal)
    }

    pub fn from_env() -> Result<Self, Error> {
        let key = crate::config::secret("PIXSTRIDE_KEY")?;
        let salt = crate::config::secret("PIXSTRIDE_SALT")?;
        let config = match (key, salt) {
            (Some(keys), Some(salts)) => {
                let keys = keys.split(',').collect::<Vec<_>>();
                let salts = salts.split(',').collect::<Vec<_>>();
                if keys.len() != salts.len() { return Err("signing key/salt count mismatch"); }
                let pairs = keys.into_iter().zip(salts).map(|(key, salt)| {
                    Ok((signature::decode_hex(key.trim())?, signature::decode_hex(salt.trim())?))
                }).collect::<Result<Vec<_>, Error>>()?;
                Self::rotating(pairs)?
            }
            (None, None) if std::env::var("PIXSTRIDE_ALLOW_UNSAFE").as_deref() == Ok("1") => Self::unsafe_local(),
            _ => return Err("configure PIXSTRIDE_KEY and PIXSTRIDE_SALT; unsigned mode requires explicit PIXSTRIDE_ALLOW_UNSAFE=1"),
        };
        config
            .with_environment()?
            .with_signature_environment()?
            .with_presets(&crate::config::secret("PIXSTRIDE_PRESETS")?.unwrap_or_default())
    }

    pub(crate) fn verify(&self, value: &str, path: &str) -> Result<(), Error> {
        if self.trusted.iter().any(|s| s == value) {
            return Ok(());
        }
        let valid = match &self.auth {
            Auth::UnsafeLocal => value == "unsafe",
            Auth::Signed { pairs } => pairs.iter().fold(false, |valid, (key, salt)| {
                signature::verify_imgproxy_size(value, path, key, salt, self.signature_size) | valid
            }),
        };
        if valid {
            Ok(())
        } else {
            Err("invalid signature")
        }
    }
}

impl Config {
    fn with_signature_environment(mut self) -> Result<Self, Error> {
        if let Ok(value) = std::env::var("PIXSTRIDE_SIGNATURE_SIZE") {
            self.signature_size = value.parse().map_err(|_| "invalid signature size")?;
            if !(1..=32).contains(&self.signature_size) {
                return Err("signature size must be 1..32");
            }
        }
        if let Some(value) = crate::config::secret("PIXSTRIDE_TRUSTED_SIGNATURES")? {
            self.trusted = value.split(',').map(|s| s.trim().to_owned()).collect();
            if self.trusted.len() > 16
                || self.trusted.iter().any(|s| {
                    s.is_empty()
                        || s.len() > 128
                        || !s
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                })
            {
                return Err("invalid trusted signatures");
            }
        }
        Ok(self)
    }

    pub fn sign(&self, path: &str) -> Result<String, Error> {
        if !path.starts_with('/') || path.len() > MAX_TARGET || path.contains(['\r', '\n']) {
            return Err("invalid signing path");
        }
        let Auth::Signed { pairs } = &self.auth else {
            return Err("signing key required");
        };
        let (key, salt) = &pairs[0];
        Ok(format!(
            "/{}/{}",
            signature::sign_imgproxy_size(path, key, salt, self.signature_size),
            path.trim_start_matches('/')
        ))
    }
}
