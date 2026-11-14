//! SHA-256/HMAC and unpadded URL-safe base64 for imgproxy URL signatures.
//!
//! This module uses only Rust's standard library. Signature checking compares
//! every byte of a full 32-byte HMAC before deciding; signature length is public.
//! It is experimental code and has not had an independent cryptographic audit.

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub(crate) struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    used: usize,
    length: u64,
}

impl Sha256 {
    pub(crate) fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            block: [0; 64],
            used: 0,
            length: 0,
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (word, bytes) in w[..16].iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_be_bytes(bytes.try_into().expect("four-byte SHA-256 word"));
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let big1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(big1)
                .wrapping_add(choose)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let big0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = big0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (state, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *state = state.wrapping_add(value);
        }
    }

    pub(crate) fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        if self.used != 0 {
            let n = (64 - self.used).min(data.len());
            self.block[self.used..self.used + n].copy_from_slice(&data[..n]);
            self.used += n;
            data = &data[n..];
            if self.used == 64 {
                let block = self.block;
                self.compress(&block);
                self.used = 0;
            } else {
                return;
            }
        }
        while data.len() >= 64 {
            let block: &[u8; 64] = data[..64].try_into().expect("complete SHA-256 block");
            self.compress(block);
            data = &data[64..];
        }
        self.block[..data.len()].copy_from_slice(data);
        self.used = data.len();
    }

    pub(crate) fn finish(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        self.block[self.used] = 0x80;
        self.used += 1;
        if self.used > 56 {
            self.block[self.used..].fill(0);
            let block = self.block;
            self.compress(&block);
            self.used = 0;
        }
        self.block[self.used..56].fill(0);
        self.block[56..64].copy_from_slice(&bits.to_be_bytes());
        let block = self.block;
        self.compress(&block);
        let mut result = [0u8; 32];
        for (out, word) in result.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            out.copy_from_slice(&word.to_be_bytes());
        }
        result
    }
}

/// HMAC-SHA-256 over several slices without concatenating the input.
pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > block.len() {
        let mut hash = Sha256::new();
        hash.update(key);
        block[..32].copy_from_slice(&hash.finish());
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36u8; 64];
    let mut outer_pad = [0x5cu8; 64];
    for i in 0..64 {
        inner_pad[i] ^= block[i];
        outer_pad[i] ^= block[i];
    }
    let mut inner = Sha256::new();
    inner.update(&inner_pad);
    for part in parts {
        inner.update(part);
    }
    let inner_hash = inner.finish();
    let mut outer = Sha256::new();
    outer.update(&outer_pad);
    outer.update(&inner_hash);
    outer.finish()
}

/// Decode URL-safe base64. URL payloads may contain trailing '='; signatures may not.
pub fn decode_base64url(input: &str, allow_padding: bool) -> Result<Vec<u8>, &'static str> {
    let raw = if allow_padding {
        input.trim_end_matches('=')
    } else {
        input
    };
    if raw.len() % 4 == 1 || input.len() - raw.len() > 2 {
        return Err("invalid base64url");
    }
    let mut output = Vec::with_capacity(raw.len().saturating_mul(3) / 4);
    let mut bits = 0u32;
    let mut available = 0u8;
    for b in raw.bytes() {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err("invalid base64url"),
        };
        bits = (bits << 6) | u32::from(value);
        available += 6;
        if available >= 8 {
            available -= 8;
            output.push((bits >> available) as u8);
            bits &= (1u32 << available) - 1;
        }
    }
    // Reject non-canonical unused bits; paths and signatures have one encoding.
    if bits != 0 {
        return Err("invalid base64url");
    }
    Ok(output)
}

#[inline(never)]
fn constant_time_eq(expected: &[u8], decoded: &[u8]) -> bool {
    debug_assert_eq!(decoded.len(), expected.len());
    let mut diff = 0u8;
    for i in 0..expected.len() {
        diff |= std::hint::black_box(expected[i]) ^ std::hint::black_box(decoded[i]);
    }
    std::hint::black_box(diff) == 0
}

/// Verify the OSS default: HMAC-SHA-256(key, salt || '/' || normalized path).
#[cfg(test)]
pub fn verify_imgproxy(signature: &str, normalized_path: &str, key: &[u8], salt: &[u8]) -> bool {
    verify_imgproxy_size(signature, normalized_path, key, salt, 32)
}

pub fn verify_imgproxy_size(
    signature: &str,
    normalized_path: &str,
    key: &[u8],
    salt: &[u8],
    size: usize,
) -> bool {
    let Ok(decoded) = decode_base64url(signature, false) else {
        return false;
    };
    if !(1..=32).contains(&size) || decoded.len() != size {
        return false;
    }
    let slash: &[u8] = if normalized_path.starts_with('/') {
        b""
    } else {
        b"/"
    };
    let expected = hmac_sha256(key, &[salt, slash, normalized_path.as_bytes()]);
    constant_time_eq(&expected[..size], &decoded)
}

/// Decode a bounded hex-encoded key or salt without exposing its contents.
pub fn decode_hex(input: &str) -> Result<Vec<u8>, &'static str> {
    if input.is_empty() || !input.len().is_multiple_of(2) || input.len() > 8192 {
        return Err("invalid signing configuration");
    }
    fn digit(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(input.len() / 2);
    for pair in input.as_bytes().as_chunks::<2>().0 {
        out.push(
            (digit(pair[0]).ok_or("invalid signing configuration")? << 4)
                | digit(pair[1]).ok_or("invalid signing configuration")?,
        );
    }
    Ok(out)
}

/// Sign exactly the slash-prefixed processing path, using unpadded base64url.
#[cfg(test)]
pub fn sign_imgproxy(path: &str, key: &[u8], salt: &[u8]) -> String {
    sign_imgproxy_size(path, key, salt, 32)
}

pub fn sign_imgproxy_size(path: &str, key: &[u8], salt: &[u8], size: usize) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let digest = hmac_sha256(key, &[salt, path.as_bytes()]);
    let mut result = String::with_capacity(43);
    for chunk in digest[..size.min(32)].chunks(3) {
        let value = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | chunk.get(2).copied().unwrap_or(0) as u32;
        for shift in (0..=3).rev().take(chunk.len() + 1) {
            result.push(ALPHABET[((value >> (shift * 6)) & 63) as usize] as char);
        }
    }
    result
}

#[cfg(test)]
#[path = "signature_regressions.rs"]
mod security_regressions;
