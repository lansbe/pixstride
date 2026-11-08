use super::{Error, MAX_PIXELS};

// A separable filter can visit a source-height by output-width intermediate
// even when both input and output are small. Bound that intermediate and a
// combined estimate for source, two filter passes, and output. This is an
// admission proxy, not an exact tap count or a preemptive CPU deadline.
const MAX_INTERMEDIATE_PIXELS: usize = 16_000_000;
const MAX_FILTER_WORK: usize = 100_000_000;
const BUDGET_ERROR: Error = "resize computation budget exceeded";

pub(super) fn check_filtered_work(
    source_width: usize,
    source_height: usize,
    width: usize,
    height: usize,
) -> Result<(), Error> {
    if source_width == 0 || source_height == 0 || width == 0 || height == 0 {
        return Err("zero image dimension");
    }
    let source = source_width
        .checked_mul(source_height)
        .ok_or(BUDGET_ERROR)?;
    let output = width.checked_mul(height).ok_or(BUDGET_ERROR)?;
    if source > MAX_PIXELS || output > MAX_PIXELS {
        return Err("image exceeds pixel limit");
    }
    let intermediate = source_height.checked_mul(width).ok_or(BUDGET_ERROR)?;
    if intermediate > MAX_INTERMEDIATE_PIXELS {
        return Err(BUDGET_ERROR);
    }
    let work = intermediate
        .checked_mul(2)
        .and_then(|n| n.checked_add(source))
        .and_then(|n| n.checked_add(output))
        .ok_or(BUDGET_ERROR)?;
    if work > MAX_FILTER_WORK {
        return Err(BUDGET_ERROR);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::resize::{self, StreamRgbBmp, StreamRgbaTarget};
    use crate::{Buffer, Image};

    fn solid_image(width: usize, height: usize) -> Image {
        let mut pixels = Buffer::new(width * height * 4).unwrap();
        for pixel in pixels.as_mut_slice().chunks_exact_mut(4) {
            pixel.copy_from_slice(&[32, 64, 128, 255]);
        }
        Image {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn intermediate_and_total_work_have_checked_boundaries() {
        assert_eq!(check_filtered_work(1, 4000, 4000, 1), Ok(()));
        assert_eq!(check_filtered_work(1, 4001, 4001, 1), Err(BUDGET_ERROR));
        assert_eq!(check_filtered_work(10_000, 5000, 500, 90_000), Ok(()));
        assert_eq!(
            check_filtered_work(10_000, 5000, 500, 90_001),
            Err(BUDGET_ERROR)
        );
        assert_eq!(check_filtered_work(usize::MAX, 2, 1, 1), Err(BUDGET_ERROR));
        assert_eq!(check_filtered_work(1, 1, 0, 1), Err("zero image dimension"));
    }

    #[test]
    fn regular_resize_rejects_cross_axis_work_but_nearest_still_works() {
        assert!(matches!(
            resize::resize(solid_image(1, 4001), 4001, 1, b"lanczos3"),
            Err(BUDGET_ERROR)
        ));
        let nearest = resize::resize(solid_image(1, 4001), 4001, 1, b"nearest").unwrap();
        assert_eq!(nearest.pixels.len(), 4001 * 4);
        assert!(nearest
            .pixels
            .chunks_exact(4)
            .all(|p| p == [32, 64, 128, 255]));
    }

    #[test]
    fn streamed_resize_rejects_before_building_the_filter_plan() {
        let target = Buffer::new(0).unwrap();
        assert!(matches!(
            StreamRgbBmp::new(
                1,
                4001,
                4001,
                1,
                b"lanczos3",
                target,
                0,
                crate::png::opaque_rgb_to_rgba,
                crate::image::pack_bgr_row,
            ),
            Err(BUDGET_ERROR)
        ));
    }

    #[test]
    fn buffered_rgb_fallback_rejects_the_same_work() {
        let rows = vec![0; 4 * 4001];
        let stride = (4001 * 3 + 3) & !3;
        let mut output = vec![0; 54 + stride];
        assert_eq!(
            resize::opaque_rgb_to_bmp(
                &rows,
                1,
                4001,
                3,
                4001,
                1,
                b"lanczos3",
                &mut output,
                stride,
                crate::png::opaque_rgb_to_rgba,
                crate::image::pack_bgr_row,
            ),
            Err(BUDGET_ERROR)
        );
    }

    #[test]
    fn normal_and_streamed_filtered_results_remain_equal() {
        let expected = resize::resize(solid_image(4, 4), 2, 2, b"lanczos3").unwrap();
        let target = StreamRgbaTarget::new(2, 2).unwrap();
        let mut stream = StreamRgbBmp::new(
            4,
            4,
            2,
            2,
            b"lanczos3",
            target,
            6,
            crate::png::opaque_rgb_to_rgba,
            crate::image::pack_bgr_row,
        )
        .unwrap()
        .unwrap();
        let row = [32, 64, 128, 32, 64, 128, 32, 64, 128, 32, 64, 128];
        for y in 0..4 {
            stream.push(y, &row);
        }
        let streamed = stream.finish().unwrap().into_image();
        assert_eq!(streamed.pixels.as_slice(), expected.pixels.as_slice());
        assert!(streamed
            .pixels
            .chunks_exact(4)
            .all(|p| p == [32, 64, 128, 255]));
    }
}
