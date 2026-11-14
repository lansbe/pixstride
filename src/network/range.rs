//! Bounded single byte ranges over an admitted raw image; no second body copy.
use std::{ops::Range, time::SystemTime};
pub(super) fn permitted(value: &str, etag: Option<&str>, modified: Option<SystemTime>) -> bool {
    if value.is_empty() {
        return true;
    }
    if value.starts_with('"') {
        return etag == Some(value);
    }
    !value.starts_with("W/")
        && httpdate::parse_http_date(value).is_ok_and(|t| {
            modified.is_some_and(|m| {
                m.duration_since(SystemTime::UNIX_EPOCH)
                    .ok()
                    .zip(t.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .is_some_and(|(m, t)| m.as_secs() <= t.as_secs())
            })
        })
}
pub(super) fn select(value: &str, length: usize) -> Result<Option<Range<usize>>, ()> {
    let Some(value) = value.strip_prefix("bytes=") else {
        return Ok(None);
    };
    // A server may ignore multiple ranges. Avoid multipart amplification.
    if value.contains(',') {
        return Ok(None);
    }
    if value.len() > 128 || length == 0 {
        return Err(());
    }
    let (start, end) = value.split_once('-').ok_or(())?;
    let number = |s: &str| {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(());
        }
        s.parse::<usize>().map_err(|_| ())
    };
    let range = if start.is_empty() {
        let suffix = number(end)?;
        if suffix == 0 {
            return Err(());
        }
        length.saturating_sub(suffix)..length
    } else {
        let start = number(start)?;
        if start >= length {
            return Err(());
        }
        let end = if end.is_empty() {
            length
        } else {
            number(end)?.min(length - 1) + 1
        };
        if end <= start {
            return Err(());
        }
        start..end
    };
    Ok(Some(range))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_are_bounded_and_unsatisfiable_ranges_fail() {
        for (s, expected) in [
            ("bytes=0-3", 0..4),
            ("bytes=7-", 7..10),
            ("bytes=-3", 7..10),
            ("bytes=-100", 0..10),
            ("bytes=3-100", 3..10),
        ] {
            assert_eq!(select(s, 10), Ok(Some(expected)));
        }
        for s in [
            "bytes=-0",
            "bytes=10-",
            "bytes=5-4",
            "bytes=+1-2",
            "bytes=999999999999999999999999-",
        ] {
            assert!(select(s, 10).is_err());
        }
        assert_eq!(select("bytes=0-1,4-5", 10), Ok(None));
    }
    #[test]
    fn if_range_requires_a_strong_current_validator() {
        assert!(permitted("\"a\"", Some("\"a\""), None));
        assert!(!permitted("W/\"a\"", Some("W/\"a\""), None));
        assert!(!permitted("\"stale\"", Some("\"current\""), None));
        assert!(permitted(
            "Thu, 01 Jan 1970 00:00:00 GMT",
            None,
            Some(SystemTime::UNIX_EPOCH)
        ));
        assert!(!permitted("Thu, 01 Jan 1970 00:00:00 GMT", None, None));
    }
}
