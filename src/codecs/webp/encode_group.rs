use super::*;
impl EncodeGroup {
    pub(crate) fn from_counts(f: &EncodeCounts) -> Result<Self, Error> {
        Ok(Self {
            green: EncodeTree::from_counts(&f.green)?,
            red: EncodeTree::from_counts(&f.red)?,
            blue: EncodeTree::from_counts(&f.blue)?,
            alpha: EncodeTree::from_counts(&f.alpha)?,
            distance: EncodeTree::from_counts(&f.distance)?,
        })
    }
    pub(crate) fn entries<'a>(&'a self, f: &'a EncodeCounts) -> [(&'a EncodeTree, &'a [u32]); 5] {
        [
            (&self.green, &f.green),
            (&self.red, &f.red),
            (&self.blue, &f.blue),
            (&self.alpha, &f.alpha),
            (&self.distance, &f.distance),
        ]
    }
}
impl EncodeZones {
    pub(crate) fn new(width: usize, height: usize) -> Self {
        let tile_width = width.div_ceil(256);
        let tile_height = height.div_ceil(256);
        Self {
            tile_width,
            tile_height,
            columns: tile_width.min(4),
            rows: tile_height.min(4),
        }
    }
    pub(crate) fn count(&self) -> usize {
        self.columns * self.rows
    }
    pub(crate) fn group(&self, tile_x: usize, tile_y: usize) -> usize {
        let y = if self.rows == self.tile_height {
            tile_y
        } else {
            tile_y * self.rows / self.tile_height
        };
        let x = if self.columns == self.tile_width {
            tile_x
        } else {
            tile_x * self.columns / self.tile_width
        };
        y * self.columns + x
    }
    pub(crate) fn at(&self, index: usize, width: usize) -> usize {
        if self.count() == 1 {
            return 0;
        }
        let y = index / width;
        self.group((index - y * width) >> 8, y >> 8)
    }
}
#[inline(always)]
pub(crate) fn rgba_word(p: &[u8]) -> u32 {
    (u32::from(p[3]) << 24) | (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2])
}
#[inline(always)]
pub(crate) fn subtract_channels(a: u32, b: u32) -> u32 {
    let mut out = 0u32;
    for shift in [0, 8, 16, 24] {
        out |= u32::from(((a >> shift) as u8).wrapping_sub((b >> shift) as u8)) << shift;
    }
    out
}
#[inline(always)]
pub(crate) fn residual_green(current: u32, predicted: u32) -> u32 {
    let residual = subtract_channels(current, predicted);
    let g = (residual >> 8) as u8;
    let r = ((residual >> 16) as u8).wrapping_sub(g);
    let b = (residual as u8).wrapping_sub(g);
    (residual & 0xff00_ff00) | (u32::from(r) << 16) | u32::from(b)
}
#[inline(always)]
pub(crate) fn read_rgba(pixels: &[u8], position: usize) -> u32 {
    rgba_word(&pixels[position * 4..position * 4 + 4])
}
#[cfg(any(
    test,
    not(all(
        target_arch = "aarch64",
        target_endian = "little",
        target_feature = "neon"
    ))
))]
#[inline(always)]
pub(crate) fn signed_magnitude(value: u8) -> u64 {
    i16::from(value as i8).unsigned_abs() as u64
}
#[cfg(any(
    test,
    not(all(
        target_arch = "aarch64",
        target_endian = "little",
        target_feature = "neon"
    ))
))]
#[inline(always)]
pub(crate) fn score_predictions_scalar(
    current: u32,
    predictions: &[u32; 8],
    scores: &mut [u64; 8],
) {
    for (i, &predicted) in predictions.iter().enumerate() {
        let residual = residual_green(current, predicted);
        for shift in [0, 8, 16, 24] {
            scores[i] += signed_magnitude((residual >> shift) as u8);
        }
    }
}
#[cfg(all(
    target_arch = "aarch64",
    target_endian = "little",
    target_feature = "neon"
))]
#[inline(always)]
pub(crate) fn score_predictions(current: u32, predictions: &[u32; 8], scores: &mut [u64; 8]) {
    use core::arch::aarch64::*;
    // Each u32 is little-endian B,G,R,A. Interleaved loads separate the eight
    // candidates into channels. All 32 bytes and the eight u16 stores are in
    // bounds; no pixel, result, or working state persists between requests.
    unsafe {
        let p = vld4_u8(predictions.as_ptr().cast::<u8>());
        let green = vsub_u8(vdup_n_u8((current >> 8) as u8), p.1);
        let blue = vsub_u8(vsub_u8(vdup_n_u8(current as u8), p.0), green);
        let red = vsub_u8(vsub_u8(vdup_n_u8((current >> 16) as u8), p.2), green);
        let alpha = vsub_u8(vdup_n_u8((current >> 24) as u8), p.3);
        // Two's-complement absolute value of -128 keeps 0x80, whose unsigned
        // interpretation is the desired magnitude 128.
        let magnitude = |v| vreinterpret_u8_s8(vabs_s8(vreinterpret_s8_u8(v)));
        let br = vaddl_u8(magnitude(blue), magnitude(red));
        let ga = vaddl_u8(magnitude(green), magnitude(alpha));
        let mut sums = [0u16; 8];
        vst1q_u16(sums.as_mut_ptr(), vaddq_u16(br, ga));
        for i in 0..8 {
            scores[i] += u64::from(sums[i]);
        }
    }
}
#[cfg(not(all(
    target_arch = "aarch64",
    target_endian = "little",
    target_feature = "neon"
)))]
#[inline(always)]
pub(crate) fn score_predictions(current: u32, predictions: &[u32; 8], scores: &mut [u64; 8]) {
    score_predictions_scalar(current, predictions, scores)
}
// Evaluate a small bounded set per 16x16 tile. The score is only a heuristic;
// it does not guarantee the smallest VP8L stream or lower CPU than libwebp.
pub(crate) fn choose_predictors(
    pixels: &[u8],
    width: usize,
    height: usize,
) -> Result<Vec<u8>, Error> {
    const MODES: [u32; 8] = [0, 1, 2, 4, 5, 7, 11, 12];
    let tile_width = width.div_ceil(16);
    let tile_height = height.div_ceil(16);
    let mut result = zeros::<u8>(
        tile_width
            .checked_mul(tile_height)
            .ok_or("WebP: tile overflow")?,
    )?;
    for tile_y in 0..tile_height {
        for tile_x in 0..tile_width {
            let mut scores = [0u64; MODES.len()];
            for y in (tile_y * 16)..((tile_y + 1) * 16).min(height) {
                for x in (tile_x * 16)..((tile_x + 1) * 16).min(width) {
                    if y == 0 || x == 0 {
                        continue;
                    } // borders are fixed by VP8L
                    let pos = y * width + x;
                    let current = read_rgba(pixels, pos);
                    let l = read_rgba(pixels, pos - 1);
                    let t = read_rgba(pixels, pos - width);
                    let tl = read_rgba(pixels, pos - width - 1);
                    let tr = if x + 1 < width {
                        read_rgba(pixels, pos - width + 1)
                    } else {
                        read_rgba(pixels, y * width)
                    };
                    // Specialize the fixed candidate set instead of dispatching
                    // the general decoder predictor eight times per pixel.
                    let predictions = [
                        0xff00_0000,
                        l,
                        t,
                        tl,
                        average(average(l, tr), t),
                        average(l, t),
                        select(l, t, tl),
                        clamp_predict(l, t, tl, false),
                    ];
                    score_predictions(current, &predictions, &mut scores);
                }
            }
            let best = scores
                .iter()
                .enumerate()
                .min_by_key(|&(i, &score)| (score, i))
                .map(|(i, _)| MODES[i] as u8)
                .unwrap_or(1);
            result[tile_y * tile_width + tile_x] = best;
        }
    }
    Ok(result)
}
pub(crate) fn source_predictor(
    pixels: &[u8],
    width: usize,
    x: usize,
    y: usize,
    mode: u32,
) -> Result<u32, Error> {
    let pos = y * width + x;
    if x == 0 && y == 0 {
        return Ok(0xff00_0000);
    }
    if y == 0 {
        return Ok(read_rgba(pixels, pos - 1));
    }
    if x == 0 {
        return Ok(read_rgba(pixels, pos - width));
    }
    Ok(match mode {
        0 => 0xff00_0000,
        1 => read_rgba(pixels, pos - 1),
        2 => read_rgba(pixels, pos - width),
        4 => read_rgba(pixels, pos - width - 1),
        5 => {
            let tr = if x + 1 < width {
                read_rgba(pixels, pos - width + 1)
            } else {
                read_rgba(pixels, y * width)
            };
            average(
                average(read_rgba(pixels, pos - 1), tr),
                read_rgba(pixels, pos - width),
            )
        }
        7 => average(read_rgba(pixels, pos - 1), read_rgba(pixels, pos - width)),
        11 => select(
            read_rgba(pixels, pos - 1),
            read_rgba(pixels, pos - width),
            read_rgba(pixels, pos - width - 1),
        ),
        12 => clamp_predict(
            read_rgba(pixels, pos - 1),
            read_rgba(pixels, pos - width),
            read_rgba(pixels, pos - width - 1),
            false,
        ),
        _ => {
            let tr = if x + 1 < width {
                read_rgba(pixels, pos - width + 1)
            } else {
                read_rgba(pixels, y * width)
            };
            predictor(
                mode,
                read_rgba(pixels, pos - 1),
                read_rgba(pixels, pos - width),
                tr,
                read_rgba(pixels, pos - width - 1),
            )?
        }
    })
}
/// Encode RGBA pixels as a lossless still-image VP8L stream, with a fixed
/// per-tile predictor, subtract-green, canonical prefix codes and bounded LZ77.
/// RGB values under fully transparent pixels are preserved. The match-finder
/// table contains positions, is request-local, and is not an image cache.
pub(crate) fn encode(image: &Image) -> Result<Buffer, Error> {
    encode_prepared(prepare(image)?)
}
/// Reuse owned RGBA storage for the native-endian residual words. Predictors
/// are chosen before any pixel is modified.
pub(crate) fn encode_owned(image: Image) -> Result<Buffer, Error> {
    encode_prepared(prepare_owned(image)?)
}
impl Residuals {
    pub(crate) fn words(&self) -> &[u32] {
        match self {
            Self::Allocated(words) => words,
            Self::InPlace(buffer) => {
                let bytes = buffer.as_slice();
                debug_assert_eq!(bytes.len() % 4, 0);
                debug_assert_eq!((bytes.as_ptr() as usize) % std::mem::align_of::<u32>(), 0);
                // SAFETY: Buffer storage is 16-byte aligned, initialized, and
                // its length was checked to be exactly 4 * width * height.
                // Every u32 bit pattern is valid; native-endian residuals were
                // written before this immutable borrow. The returned slice is
                // tied to `self`, so the Buffer outlives it and cannot mutate.
                unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<u32>(), bytes.len() / 4) }
            }
        }
    }
}
pub(crate) fn inspect(image: &Image) -> Result<(usize, bool, Vec<u8>), Error> {
    let count = checked_pixels(image.width, image.height)?;
    let len = count.checked_mul(4).ok_or("WebP: image size overflow")?;
    let pixels = image.pixels.as_slice();
    if pixels.len() != len {
        return Err("WebP: RGBA buffer size mismatch");
    }
    let has_alpha = pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 255);
    let modes = choose_predictors(pixels, image.width, image.height)?;
    Ok((count, has_alpha, modes))
}
pub(crate) fn prepare(image: &Image) -> Result<Prepared, Error> {
    let (count, has_alpha, modes) = inspect(image)?;
    let pixels = image.pixels.as_slice();
    let tile_width = image.width.div_ceil(16);
    let mut transformed = zeros::<u32>(count)?;
    for (i, target) in transformed.iter_mut().enumerate() {
        let x = i % image.width;
        let y = i / image.width;
        let mode = u32::from(modes[(y >> 4) * tile_width + (x >> 4)]);
        *target = residual_green(
            read_rgba(pixels, i),
            source_predictor(pixels, image.width, x, y, mode)?,
        );
    }
    Ok(Prepared {
        width: image.width,
        height: image.height,
        has_alpha,
        modes,
        transformed: Residuals::Allocated(transformed),
    })
}
pub(crate) fn residuals_in_place(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    modes: &[u8],
) -> Result<(), Error> {
    let tile_width = width.div_ceil(16);
    // Each prediction reads the left pixel and/or pixels of the preceding
    // row. For the last column, top-right means the first pixel of this row.
    // Reverse rows and columns leave all of these source pixels untouched;
    // a one-pixel-wide image takes the x == 0 border path instead.
    for y in (0..height).rev() {
        for x in (0..width).rev() {
            let i = y * width + x;
            let mode = u32::from(modes[(y >> 4) * tile_width + (x >> 4)]);
            let residual = residual_green(
                read_rgba(pixels, i),
                source_predictor(pixels, width, x, y, mode)?,
            );
            pixels[i * 4..i * 4 + 4].copy_from_slice(&residual.to_ne_bytes());
        }
    }
    Ok(())
}
pub(crate) fn prepare_owned(image: Image) -> Result<Prepared, Error> {
    let (_, has_alpha, modes) = inspect(&image)?;
    let Image {
        width,
        height,
        mut pixels,
    } = image;
    residuals_in_place(pixels.as_mut_slice(), width, height, &modes)?;
    Ok(Prepared {
        width,
        height,
        has_alpha,
        modes,
        transformed: Residuals::InPlace(pixels),
    })
}
