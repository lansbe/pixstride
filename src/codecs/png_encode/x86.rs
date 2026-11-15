//! SSE2 PNG residuals; x86_64 guarantees SSE2. Same scoring and tie order.
use super::filtered;
use core::arch::x86_64::*;

unsafe fn select(mask: __m128i, a: __m128i, b: __m128i) -> __m128i {
    _mm_or_si128(_mm_and_si128(mask, a), _mm_andnot_si128(mask, b))
}

unsafe fn paeth8(a: __m128i, b: __m128i, c: __m128i) -> __m128i {
    let zero = _mm_setzero_si128();
    let abs = |x| _mm_max_epi16(x, _mm_sub_epi16(zero, x));
    let da = abs(_mm_sub_epi16(b, c));
    let db = abs(_mm_sub_epi16(a, c));
    let dc = abs(_mm_sub_epi16(_mm_add_epi16(a, b), _mm_add_epi16(c, c)));
    let choose_a = _mm_or_si128(_mm_cmpgt_epi16(da, db), _mm_cmpgt_epi16(da, dc));
    // choose_a contains lanes where a loses. Invert by swapping selection arguments.
    select(choose_a, select(_mm_cmpgt_epi16(db, dc), c, b), a)
}

// SAFETY: callers ensure i >= channels and i + 16 <= both slice lengths.
unsafe fn residuals(current: &[u8], previous: &[u8], i: usize, channels: usize) -> [__m128i; 5] {
    let x = _mm_loadu_si128(current.as_ptr().add(i).cast());
    let a = _mm_loadu_si128(current.as_ptr().add(i - channels).cast());
    let b = _mm_loadu_si128(previous.as_ptr().add(i).cast());
    let c = _mm_loadu_si128(previous.as_ptr().add(i - channels).cast());
    let zero = _mm_setzero_si128();
    let prediction = _mm_packus_epi16(
        paeth8(
            _mm_unpacklo_epi8(a, zero),
            _mm_unpacklo_epi8(b, zero),
            _mm_unpacklo_epi8(c, zero),
        ),
        paeth8(
            _mm_unpackhi_epi8(a, zero),
            _mm_unpackhi_epi8(b, zero),
            _mm_unpackhi_epi8(c, zero),
        ),
    );
    let average = _mm_sub_epi8(
        _mm_avg_epu8(a, b),
        _mm_and_si128(_mm_xor_si128(a, b), _mm_set1_epi8(1)),
    );
    [
        x,
        _mm_sub_epi8(x, a),
        _mm_sub_epi8(x, b),
        _mm_sub_epi8(x, average),
        _mm_sub_epi8(x, prediction),
    ]
}

pub(super) fn scores(current: &[u8], previous: &[u8], channels: usize) -> [u64; 5] {
    debug_assert_eq!(current.len(), previous.len());
    let mut scores = [0; 5];
    let mut i = 0;
    for at in 0..channels.min(current.len()) {
        for (kind, score) in scores.iter_mut().enumerate() {
            let v = u16::from(filtered(kind, current[at], 0, previous[at], 0));
            *score += u64::from(v.min(256 - v));
        }
        i += 1;
    }
    // SAFETY: each guarded block and its left neighbours lie in equally sized slices.
    unsafe {
        let zero = _mm_setzero_si128();
        let mut sums = [_mm_setzero_si128(); 5];
        while i + 16 <= current.len() {
            for (kind, value) in residuals(current, previous, i, channels)
                .into_iter()
                .enumerate()
            {
                let magnitude = _mm_min_epu8(value, _mm_sub_epi8(zero, value));
                sums[kind] = _mm_add_epi64(sums[kind], _mm_sad_epu8(magnitude, zero));
            }
            i += 16;
        }
        for kind in 0..5 {
            let mut lanes = [0u64; 2];
            _mm_storeu_si128(lanes.as_mut_ptr().cast(), sums[kind]);
            scores[kind] += lanes[0] + lanes[1];
        }
    }
    for at in i..current.len() {
        for (kind, score) in scores.iter_mut().enumerate() {
            let v = u16::from(filtered(
                kind,
                current[at],
                current[at - channels],
                previous[at],
                previous[at - channels],
            ));
            *score += u64::from(v.min(256 - v));
        }
    }
    scores
}

pub(super) fn append(
    raw: &mut Vec<u8>,
    current: &[u8],
    previous: &[u8],
    channels: usize,
    kind: usize,
    mut i: usize,
) -> usize {
    // SAFETY: prefix pixels were handled by the caller; each guarded block and
    // its shifted neighbours are in bounds. Store into a live stack array.
    unsafe {
        while i + 16 <= current.len() {
            let mut bytes = [0; 16];
            _mm_storeu_si128(
                bytes.as_mut_ptr().cast(),
                residuals(current, previous, i, channels)[kind],
            );
            raw.extend_from_slice(&bytes);
            i += 16;
        }
    }
    i
}
