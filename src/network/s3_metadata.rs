//! Decode S3's MIME/double-encoded encryption-context metadata before KMS use.
use crate::Error;
use std::collections::BTreeMap;
pub(super) fn context(value: &str) -> Result<BTreeMap<String, String>, Error> {
    if value.len() > 16384 {
        return Err("S3 encryption context exceeds limit");
    }
    let value = mime(value)?;
    serde_json::from_str(&value).map_err(|_| "invalid KMS encryption context")
}
fn mime(value: &str) -> Result<String, Error> {
    if !value.starts_with("=?") {
        return Ok(value.into());
    }
    let mut rest = value;
    let mut output = String::new();
    let mut encoded = false;
    while let Some((prefix, word)) = rest.split_once("=?") {
        if !encoded || !prefix.bytes().all(|b| b.is_ascii_whitespace()) {
            output.push_str(prefix);
        }
        let (charset, word) = word.split_once('?').ok_or("invalid encoded S3 context")?;
        let (encoding, word) = word.split_once('?').ok_or("invalid encoded S3 context")?;
        let (body, tail) = word.split_once("?=").ok_or("invalid encoded S3 context")?;
        let data = if encoding.eq_ignore_ascii_case("b") {
            crate::signature::decode_base64url(&body.replace('+', "-").replace('/', "_"), true)?
        } else if encoding.eq_ignore_ascii_case("q") {
            quoted(body.as_bytes())?
        } else {
            return Err("unsupported encoded S3 context");
        };
        if charset.eq_ignore_ascii_case("iso-8859-1") {
            output.extend(data.into_iter().map(char::from));
        } else if charset.eq_ignore_ascii_case("utf-8") || charset.eq_ignore_ascii_case("us-ascii")
        {
            output.push_str(std::str::from_utf8(&data).map_err(|_| "invalid encoded S3 context")?);
        } else {
            return Err("unsupported S3 context charset");
        }
        if output.len() > 32768 {
            return Err("decoded S3 context exceeds limit");
        }
        rest = tail;
        encoded = true;
    }
    output.push_str(rest);
    // S3 may UTF-8 encode each Latin-1 representation of the original UTF-8
    // bytes before applying MIME encoding. Repair only a valid round trip.
    if !output.is_ascii() && output.chars().all(|c| u32::from(c) <= 255) {
        let bytes = output.chars().map(|c| c as u8).collect::<Vec<_>>();
        if let Ok(repaired) = String::from_utf8(bytes) {
            output = repaired;
        }
    }
    Ok(output)
}
fn quoted(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut output = Vec::with_capacity(data.len());
    let mut at = 0;
    while at < data.len() {
        if data[at] == b'=' {
            let pair = data
                .get(at + 1..at + 3)
                .ok_or("invalid quoted S3 context")?;
            let value = u8::from_str_radix(
                std::str::from_utf8(pair).map_err(|_| "invalid quoted S3 context")?,
                16,
            )
            .map_err(|_| "invalid quoted S3 context")?;
            output.push(value);
            at += 3;
        } else {
            output.push(if data[at] == b'_' { b' ' } else { data[at] });
            at += 1;
        }
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aws_double_encoding_and_quoted_words_preserve_the_kms_context() {
        let original = "{\"note\":\"café\"}";
        let double = original
            .as_bytes()
            .iter()
            .copied()
            .map(char::from)
            .collect::<String>();
        let encoded = format!(
            "=?UTF-8?B?{}?=",
            openssl::base64::encode_block(double.as_bytes())
        );
        assert_eq!(context(&encoded).unwrap()["note"], "café");
        assert_eq!(
            context("=?US-ASCII?Q?=7B=22note=22=3A=22a_b=22=7D?=").unwrap()["note"],
            "a b"
        );
        assert!(context("=?unknown?B?e30=?=").is_err());
    }
}
