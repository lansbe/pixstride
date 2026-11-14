//! Configuration is staged before server/native threads exist, then installed once.
use crate::Error;
const SELECTORS: [&str; 4] = [
    "PIXSTRIDE_ENV_AWS_SECRET_ID",
    "PIXSTRIDE_ENV_AWS_SSM_PARAMETERS_PATH",
    "PIXSTRIDE_ENV_GCP_SECRET_ID",
    "PIXSTRIDE_ENV_LOCAL_FILE_PATH",
];
#[cfg(feature = "cloud-config")]
mod cloud;
#[cfg(feature = "cloud-config")]
mod transport;
pub fn load() -> Result<(), Error> {
    if !SELECTORS.iter().any(|k| std::env::var_os(k).is_some()) {
        return Ok(());
    }
    #[cfg(not(feature = "cloud-config"))]
    {
        Err("configuration sources require cloud-config")
    }
    #[cfg(feature = "cloud-config")]
    {
        let mut values = Values::default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "configuration runtime unavailable")?;
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(60), cloud::load(&mut values))
                .await
                .map_err(|_| "configuration deadline exceeded")?
        })?;
        // Drop resolver/runtime workers before mutating the process environment.
        drop(runtime);
        if let Some(path) = values.get("PIXSTRIDE_ENV_LOCAL_FILE_PATH") {
            values.document(&read(&path)?)?;
        }
        for (key, value) in values.loaded {
            std::env::set_var(key, value);
        }
        Ok(())
    }
}
#[cfg(feature = "cloud-config")]
fn read(path: &str) -> Result<Vec<u8>, Error> {
    crate::os::InputFile::open(path.as_bytes())?
        .read_limited(65536)
        .map(|b| b.as_slice().to_vec())
        .map_err(|_| "cannot read bounded configuration file")
}
#[cfg(feature = "cloud-config")]
#[derive(Default)]
struct Values {
    loaded: std::collections::BTreeMap<String, String>,
    entries: usize,
    bytes: usize,
}
#[cfg(feature = "cloud-config")]
impl Values {
    fn get(&self, name: &str) -> Option<String> {
        self.loaded
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    }
    fn secret(&self, name: &str) -> Result<Option<String>, Error> {
        let value = self.get(name);
        let file = self.get(&format!("{name}_FILE"));
        if value.is_some() && file.is_some() {
            return Err("configure credential value or file, not both");
        }
        match file {
            Some(path) => String::from_utf8(read(&path)?)
                .map(|v| Some(v.trim().into()))
                .map_err(|_| "invalid credential encoding"),
            None => Ok(value),
        }
    }
    fn insert(&mut self, key: String, value: String) -> Result<(), Error> {
        if !["PIXSTRIDE_", "AWS_", "AZURE_", "GOOGLE_"]
            .iter()
            .any(|p| key.starts_with(p))
            || !key
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            || value.contains('\0')
        {
            return Err("invalid configuration variable");
        }
        self.entries += 1;
        self.bytes += key.len() + value.len();
        if self.entries > 128 || self.bytes > 65536 {
            return Err("loaded configuration exceeds limit");
        }
        self.loaded.insert(key, value);
        Ok(())
    }
    fn document(&mut self, data: &[u8]) -> Result<(), Error> {
        if data.len() > 65536 {
            return Err("configuration document exceeds limit");
        }
        for pair in dotenvy::from_read_iter(data) {
            let (key, value) = pair.map_err(|_| "invalid environment document")?;
            self.insert(key, value)?;
        }
        Ok(())
    }
}
#[cfg(all(test, feature = "cloud-config"))]
mod tests {
    use super::*;
    #[test]
    fn bounded_documents_support_quotes_and_reject_system_overrides() {
        let mut values = Values::default();
        values
            .document(b"# synthetic\nPIXSTRIDE_SECRET='spaces and # signs'\nPIXSTRIDE_TTL=12\n")
            .unwrap();
        assert_eq!(
            values.get("PIXSTRIDE_SECRET").as_deref(),
            Some("spaces and # signs")
        );
        assert!(values.document(b"LD_PRELOAD=/untrusted\n").is_err());
        assert!(values.document(&vec![b'x'; 65537]).is_err());
    }
}
