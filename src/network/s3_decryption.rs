//! S3 Encryption Client V2/V3 KMS envelopes; authenticate before image admission.
use crate::Error;
use openssl::{cipher::Cipher, cipher_ctx::CipherCtx, md::Md, pkey::Id, pkey_ctx::PkeyCtx};
pub(super) fn enabled() -> bool {
    matches!(
        std::env::var("PIXSTRIDE_S3_USE_DECRYPTION_CLIENT").as_deref(),
        Ok("1" | "true")
    )
}
fn decode(value: &str, limit: usize) -> Result<Vec<u8>, Error> {
    if value.len() > limit * 2 {
        return Err("encrypted metadata exceeds limit");
    }
    let data =
        crate::signature::decode_base64url(&value.replace('+', "-").replace('/', "_"), true)?;
    if data.len() > limit {
        return Err("encrypted metadata exceeds limit");
    }
    Ok(data)
}
pub(super) async fn decrypt(
    mut body: Vec<u8>,
    metadata: Option<opendal::UserMetadata<'_>>,
    max: usize,
    region: &str,
) -> Result<Vec<u8>, Error> {
    let metadata = metadata.ok_or("encrypted S3 metadata required")?;
    let get = |name: &str| {
        metadata
            .get(name)
            .filter(|v| !v.is_empty() && v.len() <= 16384)
            .ok_or("incomplete S3 encryption metadata")
    };
    let v3 = metadata.get("x-amz-3").is_some();
    if metadata.get("x-amz-key").is_some() || v3 && metadata.get("x-amz-key-v2").is_some() {
        return Err("ambiguous S3 encryption metadata");
    }
    let (algorithm, wrap, encrypted, nonce, context, commit) = if v3 {
        (
            get("x-amz-c")?,
            get("x-amz-w")?,
            get("x-amz-3")?,
            decode(get("x-amz-i")?, 28)?,
            get("x-amz-t")?,
            Some(decode(get("x-amz-d")?, 28)?),
        )
    } else {
        if get("x-amz-tag-len")? != "128" {
            return Err("unsupported S3 authentication tag");
        }
        (
            get("x-amz-cek-alg")?,
            get("x-amz-wrap-alg")?,
            get("x-amz-key-v2")?,
            decode(get("x-amz-iv")?, 12)?,
            get("x-amz-matdesc")?,
            None,
        )
    };
    if v3
        && (algorithm != "115"
            || !matches!(wrap, "kms+context" | "12")
            || nonce.len() != 28
            || commit.as_ref().is_none_or(|c| c.len() != 28))
        || !v3 && (algorithm != "AES/GCM/NoPadding" || wrap != "kms+context" || nonce.len() != 12)
    {
        return Err("unsupported S3 encryption algorithm");
    }
    let context = super::s3_metadata::context(context)?;
    if context.len() > 64 || context.get("aws:x-amz-cek-alg").map(String::as_str) != Some(algorithm)
    {
        return Err("S3 encryption context does not match algorithm");
    }
    let encrypted = decode(encrypted, 6144)?;
    if encrypted.is_empty() {
        return Err("empty encrypted data key");
    }
    let encrypted = openssl::base64::encode_block(&encrypted);
    let result = crate::config_sources::kms(
        serde_json::json!({"CiphertextBlob":encrypted,"EncryptionContext":context}),
        region,
    )
    .await?;
    let mut key = decode(
        result
            .get("Plaintext")
            .and_then(serde_json::Value::as_str)
            .ok_or("KMS returned no data key")?,
        32,
    )?;
    if key.len() != 32 {
        return Err("KMS returned an invalid data key");
    }
    let iv = if let Some(commit) = commit {
        let expected = derive(&key, &nonce, b"\0sCOMMITKEY", 28)?;
        if !openssl::memcmp::eq(&expected, &commit) {
            return Err("S3 key commitment mismatch");
        }
        key = derive(&key, &nonce, b"\0sDERIVEKEY", 32)?;
        vec![1; 12]
    } else {
        nonce
    };
    authenticate(&mut body, &key, &iv)?;
    if body.len() > max {
        return Err("decrypted source exceeds limit");
    }
    Ok(body)
}
fn derive(key: &[u8], salt: &[u8], info: &[u8], size: usize) -> Result<Vec<u8>, Error> {
    let result = (|| -> Result<Vec<u8>, openssl::error::ErrorStack> {
        let mut ctx = PkeyCtx::new_id(Id::HKDF)?;
        ctx.derive_init()?;
        ctx.set_hkdf_md(Md::sha512())?;
        ctx.set_hkdf_key(key)?;
        ctx.set_hkdf_salt(salt)?;
        ctx.add_hkdf_info(info)?;
        let mut out = vec![0; size];
        ctx.derive(Some(&mut out))?;
        Ok(out)
    })();
    result.map_err(|_| "S3 key derivation failed")
}
fn authenticate(body: &mut Vec<u8>, key: &[u8], iv: &[u8]) -> Result<(), Error> {
    if body.len() < 16 || body.len() > crate::MAX_FILE + 16 || key.len() != 32 || iv.len() != 12 {
        return Err("invalid encrypted S3 content");
    }
    let length = body.len() - 16;
    let tag: [u8; 16] = body[length..]
        .try_into()
        .map_err(|_| "invalid encrypted tag")?;
    let result = (|| -> Result<(), openssl::error::ErrorStack> {
        let mut ctx = CipherCtx::new()?;
        ctx.decrypt_init(Some(Cipher::aes_256_gcm()), Some(key), Some(iv))?;
        ctx.set_tag(&tag)?;
        let written = ctx.cipher_update_inplace(body, length)?;
        let mut final_bytes = [0; 16];
        let end = ctx.cipher_final(&mut final_bytes)?;
        body.truncate(written);
        body.extend_from_slice(&final_bytes[..end]);
        Ok(())
    })();
    result.map_err(|_| "S3 content authentication failed")
}
#[cfg(test)]
mod tests {
    use super::*;
    fn hex(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|v| u8::from_str_radix(std::str::from_utf8(v).unwrap(), 16).unwrap())
            .collect()
    }
    #[test]
    fn nist_aes256_gcm_authenticates_before_exposing_plaintext() {
        let encrypted = hex("cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919");
        let mut valid = encrypted.clone();
        authenticate(&mut valid, &[0; 32], &[0; 12]).unwrap();
        assert_eq!(valid, vec![0; 16]);
        let mut invalid = encrypted;
        invalid[0] ^= 1;
        assert!(authenticate(&mut invalid, &[0; 32], &[0; 12]).is_err());
    }
}
