use super::*;
/// Both backends promise at least 16-byte alignment and initialized bytes.
/// Logical lengths are checked before exposing the corresponding typed slice.
pub(crate) fn typed_mut<T>(buf: &mut Buffer) -> &mut [T] {
    let bytes = buf.as_mut_slice();
    debug_assert_eq!((bytes.as_ptr() as usize) % align_of::<T>(), 0);
    debug_assert_eq!(bytes.len() % size_of::<T>(), 0);
    // SAFETY: Buffer guarantees alignment for these types; T is instantiated
    // here only as padding-free Span, f32 or usize. The Buffer is exclusively
    // borrowed, its byte
    // length is an exact multiple of size_of::<T>(), and all bit patterns of
    // those types are valid. The lifetime cannot outlive the Buffer.
    unsafe {
        slice::from_raw_parts_mut(bytes.as_mut_ptr().cast::<T>(), bytes.len() / size_of::<T>())
    }
}
pub(crate) fn typed<T>(buf: &Buffer) -> &[T] {
    let bytes = buf.as_slice();
    debug_assert_eq!((bytes.as_ptr() as usize) % align_of::<T>(), 0);
    debug_assert_eq!(bytes.len() % size_of::<T>(), 0);
    // SAFETY: the same alignment, size and initialized-bit-pattern guarantees
    // as typed_mut apply. The immutable slice is tied to the Buffer's borrow.
    unsafe { slice::from_raw_parts(bytes.as_ptr().cast::<T>(), bytes.len() / size_of::<T>()) }
}
#[inline]
pub(crate) fn abs(x: f64) -> f64 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}
#[inline]
pub(crate) fn floor_i64(x: f64) -> i64 {
    let i = x as i64;
    if (i as f64) > x {
        i - 1
    } else {
        i
    }
}
#[inline]
pub(crate) fn ceil_i64(x: f64) -> i64 {
    -floor_i64(-x)
}
/// sin(pi*x), only called for |x| <= 3. Range reduction limits the polynomial
/// to [-pi/2, pi/2]. The Taylor polynomial includes the degree-15 term.
#[inline]
pub(crate) fn sin_pi(x: f64) -> f64 {
    let negative = x < 0.0;
    let a = abs(x);
    let whole = a as u32;
    let fraction = a - whole as f64;
    let reduced = if fraction > 0.5 {
        1.0 - fraction
    } else {
        fraction
    };
    let z = PI * reduced;
    let z2 = z * z;
    let value = z
        * (1.0
            + z2 * (-1.0 / 6.0
                + z2 * (1.0 / 120.0
                    + z2 * (-1.0 / 5040.0
                        + z2 * (1.0 / 362880.0
                            + z2 * (-1.0 / 39916800.0
                                + z2 * (1.0 / 6227020800.0 + z2 * (-1.0 / 1307674368000.0))))))));
    if negative ^ ((whole & 1) != 0) {
        -value
    } else {
        value
    }
}
#[inline]
pub(crate) fn kernel(filter: Filter, x: f64) -> f64 {
    let x = abs(x);
    match filter {
        Filter::Linear => {
            if x < 1.0 {
                1.0 - x
            } else {
                0.0
            }
        }
        Filter::Lanczos3 => {
            if x >= 3.0 {
                0.0
            } else if x < 1e-10 {
                1.0
            } else {
                3.0 * sin_pi(x) * sin_pi(x / 3.0) / (PI * PI * x * x)
            }
        }
    }
}
pub(crate) fn make_span(src: usize, dst: usize, index: usize, filter: Filter) -> Span {
    let ratio = src as f64 / dst as f64;
    let scale = if ratio > 1.0 { ratio } else { 1.0 };
    let center = (index as f64 + 0.5) * ratio - 0.5;
    let radius = if filter == Filter::Linear { 1.0 } else { 3.0 } * scale;
    let first_i = ceil_i64(center - radius);
    let last_i = floor_i64(center + radius);
    let first = if first_i < 0 {
        0
    } else {
        (first_i as usize).min(src - 1)
    };
    let last = if last_i < 0 {
        0
    } else {
        (last_i as usize).min(src - 1)
    };
    let count = last - first + 1;
    let mut sum = 0.0;
    let mut n = 0;
    while n < count {
        sum += kernel(filter, ((first + n) as f64 - center) / scale);
        n += 1;
    }
    // Valid dimensions and the chosen kernels always overlap at least one
    // positive sample. The guard also bounds behavior under rounding at edges.
    let inv_sum = if abs(sum) > 1e-20 { 1.0 / sum } else { 0.0 };
    Span {
        first,
        count,
        weight_offset: 0,
        center,
        scale,
        inv_sum,
        opaque_alpha: 0.0,
        fast_stride: 0,
        fast_eight: 0,
        _padding: [0; 2],
    }
}
#[inline]
pub(crate) fn span_weight(span: &Span, n: usize, filter: Filter) -> f32 {
    (kernel(filter, ((span.first + n) as f64 - span.center) / span.scale) * span.inv_sum) as f32
}
impl Plan {
    /// Compact cached coefficients for opaque exact x2/x4 reductions.
    /// Their centers and source indices are integer or half-integer and at
    /// most 100000, so equal relative positions are represented exactly.
    /// Count, scale and normalization must also agree bit-for-bit. Any
    /// shape exceeding the bounded scratch falls back to the ordinary plan.
    pub(crate) fn new_compact(
        src: usize,
        dst: usize,
        first_dst: usize,
        count: usize,
        filter: Filter,
        opaque: bool,
    ) -> Result<Option<Self>, Error> {
        const MAX_TAPS: usize = 25;
        const MAX_PATTERNS: usize = 64;
        if src > 100_000 || dst > 100_000 {
            return Ok(None);
        }
        let stride = if dst.checked_mul(2) == Some(src) {
            2
        } else if dst.checked_mul(4) == Some(src) {
            4
        } else {
            return Ok(None);
        };
        let mut spans = Buffer::new(
            count
                .checked_mul(size_of::<Span>())
                .ok_or("resize plan too large")?,
        )?;
        let entries = typed_mut::<Span>(&mut spans);
        let mut patterns = [None::<Span>; MAX_PATTERNS];
        let mut pattern_count = 0usize;
        let mut unique = [0.0f32; MAX_TAPS * MAX_PATTERNS];
        let mut used = 0usize;
        let mut previous_interior: Option<Span> = None;
        for (local, entry) in entries.iter_mut().enumerate() {
            // For an unclipped x2/x4 span, both source indices and its
            // half-integer center move by the exact integer stride. Thus
            // every relative tap, sum, f32 weight, and alpha stays bitwise
            // identical. Borders retain the original calculation.
            if let Some(previous) = previous_interior {
                let mut shifted = previous;
                shifted.first += stride;
                shifted.center += stride as f64;
                if shifted.first + shifted.count < src {
                    *entry = shifted;
                    previous_interior = Some(shifted);
                    continue;
                }
            }
            let mut span = make_span(src, dst, first_dst + local, filter);
            if span.count > MAX_TAPS {
                return Ok(None);
            }
            let relative = (span.first as f64 - span.center).to_bits();
            let mut found = None;
            for prior in patterns[..pattern_count].iter().flatten() {
                if prior.count == span.count
                    && prior.scale.to_bits() == span.scale.to_bits()
                    && prior.inv_sum.to_bits() == span.inv_sum.to_bits()
                    && (prior.first as f64 - prior.center).to_bits() == relative
                {
                    found = Some(*prior);
                    break;
                }
            }
            if let Some(prior) = found {
                span.weight_offset = prior.weight_offset;
                span.opaque_alpha = prior.opaque_alpha;
            } else {
                if pattern_count == MAX_PATTERNS || used + span.count > unique.len() {
                    return Ok(None);
                }
                span.weight_offset = used;
                let mut alpha = 0.0f32;
                for n in 0..span.count {
                    let weight = span_weight(&span, n, filter);
                    if opaque {
                        alpha += 255.0 * weight;
                        unique[used + n] = 255.0 * weight / 255.0;
                    } else {
                        unique[used + n] = weight;
                    }
                }
                used += span.count;
                span.opaque_alpha = alpha;
                patterns[pattern_count] = Some(span);
                pattern_count += 1;
            }
            *entry = span;
            previous_interior = if span.first > 0 && span.first + span.count < src {
                Some(span)
            } else {
                None
            };
        }
        let mut weights = Buffer::new(used * size_of::<f32>())?;
        typed_mut::<f32>(&mut weights).copy_from_slice(&unique[..used]);
        let cached = typed::<f32>(&weights);
        let mut block = 0usize;
        while opaque && block + 4 <= entries.len() {
            let first = entries[block];
            let mut eligible = first.first + first.count + 4 * stride - 1 <= src;
            for lane in 1..4 {
                let next = entries[block + lane];
                eligible &= next.first == first.first + lane * stride
                    && next.count == first.count
                    && next.opaque_alpha.to_bits() == first.opaque_alpha.to_bits();
                if eligible {
                    for n in 0..first.count {
                        if cached[first.weight_offset + n].to_bits()
                            != cached[next.weight_offset + n].to_bits()
                        {
                            eligible = false;
                            break;
                        }
                    }
                }
            }
            if eligible {
                entries[block].fast_stride = stride as u8;
            }
            block += 4;
        }
        Ok(Some(Self {
            spans,
            weights: Some(weights),
            filter,
            opaque_weights: opaque,
        }))
    }

    /// Pair two previously checked groups only when every cached f32 weight
    /// and alpha is bit-identical. This is paid once per stripe, not per row.
    pub(crate) fn mark_eight(&mut self, step: usize) {
        let cached = typed::<f32>(self.weights.as_ref().unwrap());
        let entries = typed_mut::<Span>(&mut self.spans);
        let mut block = 0usize;
        while block + 8 <= entries.len() {
            let first = entries[block];
            let next = entries[block + 4];
            let mut eligible = usize::from(first.fast_stride) == step
                && usize::from(next.fast_stride) == step
                && first.first.checked_add(4 * step) == Some(next.first)
                && first.count == next.count
                && first.opaque_alpha.to_bits() == next.opaque_alpha.to_bits();
            if eligible {
                for n in 0..first.count {
                    if cached[first.weight_offset + n].to_bits()
                        != cached[next.weight_offset + n].to_bits()
                    {
                        eligible = false;
                        break;
                    }
                }
            }
            if eligible {
                entries[block].fast_eight = 1;
                block += 8;
            } else {
                block += 4;
            }
        }
    }

    pub(crate) fn new(
        src: usize,
        dst: usize,
        first_dst: usize,
        count: usize,
        filter: Filter,
        opaque: bool,
    ) -> Result<Self, Error> {
        let span_bytes = count
            .checked_mul(size_of::<Span>())
            .ok_or("resize plan too large")?;
        let mut spans = Buffer::new(span_bytes)?;
        let entries = typed_mut::<Span>(&mut spans);
        let mut total_weights = 0usize;
        for (local, entry) in entries.iter_mut().enumerate() {
            let mut span = make_span(src, dst, first_dst + local, filter);
            span.weight_offset = total_weights;
            total_weights = total_weights
                .checked_add(span.count)
                .ok_or("resize weights too large")?;
            *entry = span;
        }
        let weight_bytes = total_weights
            .checked_mul(size_of::<f32>())
            .ok_or("resize weights too large")?;
        let weights = if weight_bytes <= WEIGHT_CACHE_BYTES {
            let mut b = Buffer::new(weight_bytes)?;
            let out = typed_mut::<f32>(&mut b);
            for span in entries.iter_mut() {
                let mut alpha = 0.0f32;
                for n in 0..span.count {
                    let weight = span_weight(span, n, filter);
                    if opaque {
                        alpha += 255.0 * weight;
                        // Preserve the previous f32 multiply then divide, even
                        // when mathematically this is simply `weight`.
                        out[span.weight_offset + n] = 255.0 * weight / 255.0;
                    } else {
                        out[span.weight_offset + n] = weight;
                    }
                }
                span.opaque_alpha = alpha;
            }
            Some(b)
        } else {
            None
        };
        let opaque_weights = opaque && weights.is_some();
        let stride = if dst.checked_mul(2) == Some(src) {
            2
        } else if dst.checked_mul(4) == Some(src) {
            4
        } else {
            0
        };
        if opaque_weights && stride != 0 {
            // The ARM64 group path is enabled only when all four output
            // samples really have identical, precomputed f32 coefficients.
            // Clipped borders and any differing phase stay on the single-pixel
            // path. This preparation is performed once per request/stripe.
            let cached = typed::<f32>(weights.as_ref().unwrap());
            let mut block = 0usize;
            while block + 4 <= entries.len() {
                let first = entries[block];
                let mut eligible = first.first + first.count + 4 * stride - 1 <= src;
                for lane in 1..4 {
                    let next = entries[block + lane];
                    eligible &= next.first == first.first + lane * stride
                        && next.count == first.count
                        && next.opaque_alpha.to_bits() == first.opaque_alpha.to_bits();
                    if eligible {
                        for n in 0..first.count {
                            if cached[first.weight_offset + n].to_bits()
                                != cached[next.weight_offset + n].to_bits()
                            {
                                eligible = false;
                                break;
                            }
                        }
                    }
                }
                if eligible {
                    entries[block].fast_stride = stride as u8;
                }
                block += 4;
            }
        }
        Ok(Self {
            spans,
            weights,
            filter,
            opaque_weights,
        })
    }

    #[inline]
    pub(crate) fn weight(&self, span: &Span, n: usize) -> f32 {
        match &self.weights {
            Some(w) => typed::<f32>(w)[span.weight_offset + n],
            None => span_weight(span, n, self.filter),
        }
    }
}
#[inline]
pub(crate) fn byte(x: f32) -> u8 {
    if x <= 0.0 {
        0
    } else if x >= 255.0 {
        255
    } else {
        (x + 0.5) as u8
    }
}
