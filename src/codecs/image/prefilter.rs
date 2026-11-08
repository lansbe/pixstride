//! Exact x4 reduction using the retained box-then-Lanczos pipeline.
use super::*;
pub(crate) fn prehalve_parsed(
    parsed: crate::png::ParsedPng<'_>,
    sw: usize,
    sh: usize,
    width: usize,
    height: usize,
) -> Result<Buffer, Error> {
    let (bmp, stride) = bmp_output(width, height)?;
    let mut stream = crate::resize::StreamRgbBmp::new(
        sw / 2,
        sh / 2,
        width,
        height,
        b"lanczos3",
        bmp,
        stride,
        crate::png::opaque_rgb_to_rgba,
        pack_bgr_row,
    )?
    .ok_or("prehalve resize exceeds streaming budget")?;
    let mut previous = Buffer::new(sw * 3)?;
    let mut half = Buffer::new(sw / 2 * 3)?;
    // Decoder still consumes and validates every byte, including late CRC,
    // filter and Adler errors. The output remains private until its success.
    parsed.stream_rgb_rows(&mut |y, row| {
        if y % 2 == 0 {
            previous.as_mut_slice().copy_from_slice(row);
        } else {
            half_rgb_pair(previous.as_slice(), row, half.as_mut_slice());
            stream.push(y / 2, half.as_slice());
        }
    })?;
    stream.finish()
}
fn half_rgb_pair(top: &[u8], bottom: &[u8], output: &mut [u8]) {
    debug_assert_eq!(top.len(), bottom.len());
    debug_assert_eq!(top.len(), output.len() * 2);
    let mut x = 0;
    let pixels = top.len() / 3;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use core::arch::aarch64::*;
        while x + 16 <= pixels {
            // Read 16 whole RGB pixels from each row and write 8. Halving
            // rounds upward separately for each axis, as the scalar oracle.
            let a = vld3q_u8(top.as_ptr().add(x * 3));
            let b = vld3q_u8(bottom.as_ptr().add(x * 3));
            let r = vrshrn_n_u16::<1>(vpaddlq_u8(vrhaddq_u8(a.0, b.0)));
            let g = vrshrn_n_u16::<1>(vpaddlq_u8(vrhaddq_u8(a.1, b.1)));
            let b = vrshrn_n_u16::<1>(vpaddlq_u8(vrhaddq_u8(a.2, b.2)));
            vst3_u8(output.as_mut_ptr().add(x / 2 * 3), uint8x8x3_t(r, g, b));
            x += 16;
        }
    }
    while x < pixels {
        for c in 0..3 {
            let first = (u16::from(top[x * 3 + c]) + u16::from(bottom[x * 3 + c])).div_ceil(2);
            let second =
                (u16::from(top[(x + 1) * 3 + c]) + u16::from(bottom[(x + 1) * 3 + c])).div_ceil(2);
            output[x / 2 * 3 + c] = (first + second).div_ceil(2) as u8;
        }
        x += 2;
    }
}
