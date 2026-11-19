//! Validate rendered SVG opacity before avoiding unnecessary alpha processing.
use crate::{
    request::{OutputFormat, Request},
    Error,
};
use libvips::{
    enums::BandFormat,
    voption::{Setter, VOption},
    VipsImage,
};

pub(super) fn prepare(
    image: VipsImage,
    r: &Request,
    format: OutputFormat,
) -> Result<VipsImage, Error> {
    if format != OutputFormat::Svg
        || r.extra.preserve_hdr
        || r.extra.encoding.linear
        || image.get_bands() != 4
        || image.get_format().ok() != Some(BandFormat::Uchar)
        || !super::shared::fits_memory(&image)
        || !image.hasalpha()
    {
        return Ok(image);
    }
    // A lazy minimum would render the SVG again during geometry. Keep one
    // bounded raster for both consumers, including genuinely transparent SVGs.
    let image = super::shared::materialize(image)?;
    let alpha = image
        .extract_band(3)
        .and_then(|band| band.min())
        .map_err(|_| "native SVG opacity validation failed")?;
    if alpha != 255.0 {
        return Ok(image);
    }
    image
        .extract_band_with_opts(0, VOption::new().set("n", 3))
        .map_err(|_| "native SVG RGB extraction failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{self, Config};
    use libvips::{
        enums::{BandFormat, Interpretation},
        voption::{Setter, VOption},
    };

    fn request() -> Request {
        request::parse(
            "/unsafe/plain/local:///fixture.svg@bmp",
            &Config::unsafe_local(),
        )
        .unwrap()
    }

    fn image(alpha: [u8; 2]) -> VipsImage {
        super::super::engine::initialize().unwrap();
        VipsImage::new_from_owned_u8(vec![255, 0, 0, alpha[0], 11, 22, 33, alpha[1]], 2, 1, 4)
            .unwrap()
            .copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
            .unwrap()
    }

    #[test]
    fn opaque_svg_samples_drop_only_alpha_and_keep_native_metadata() {
        let mut input = image([255; 2]);
        for (name, value) in [("palette", 1), ("bits-per-sample", 8), ("orientation", 6)] {
            input.set_int(name, value).unwrap();
        }
        let output = prepare(input, &request(), OutputFormat::Svg).unwrap();
        assert_eq!(output.get_bands(), 3);
        assert_eq!(
            &*super::super::raster::export(&output).unwrap(),
            &[255, 0, 0, 11, 22, 33]
        );
        for (name, value) in [("palette", 1), ("bits-per-sample", 8), ("orientation", 6)] {
            assert_eq!(output.get_int(name).unwrap(), value);
        }
    }

    #[test]
    fn partial_antialiased_and_invisible_samples_keep_exact_rgba() {
        for alpha in [0, 128, 224, 254] {
            let output = prepare(image([255, alpha]), &request(), OutputFormat::Svg).unwrap();
            assert_eq!(output.get_bands(), 4);
            assert_eq!(
                &*super::super::raster::export(&output).unwrap(),
                &[255, 0, 0, 255, 11, 22, 33, alpha]
            );
        }
    }

    #[test]
    fn other_sources_linear_hdr_and_over_budget_rasters_keep_their_route() {
        let r = request();
        assert_eq!(
            prepare(image([255; 2]), &r, OutputFormat::Png)
                .unwrap()
                .get_bands(),
            4
        );
        let mut hdr = request();
        hdr.extra.preserve_hdr = true;
        assert_eq!(
            prepare(image([255; 2]), &hdr, OutputFormat::Svg)
                .unwrap()
                .get_bands(),
            4
        );
        let mut linear = request();
        linear.extra.encoding.linear = true;
        assert_eq!(
            prepare(image([255; 2]), &linear, OutputFormat::Svg)
                .unwrap()
                .get_bands(),
            4
        );
        let float = image([255; 2]).cast(BandFormat::Float).unwrap();
        assert_eq!(
            prepare(float, &r, OutputFormat::Svg)
                .unwrap()
                .get_format()
                .unwrap(),
            BandFormat::Float
        );
        // Header-only lazy black: the guard must not allocate or scan this >64MiB raster.
        let large = VipsImage::black_with_opts(4097, 4096, VOption::new().set("bands", 4))
            .unwrap()
            .copy_with_opts(VOption::new().set("interpretation", Interpretation::Srgb as i32))
            .unwrap();
        assert_eq!(
            prepare(large, &r, OutputFormat::Svg).unwrap().get_bands(),
            4
        );
    }

    #[test]
    fn real_svg_pipeline_uses_rgb_bmp_only_for_verified_opaque_pixels() {
        let opaque = br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1"><rect width="2" height="1" fill="red"/></svg>"#;
        let output = super::super::engine::process(opaque.to_vec(), &request(), &[]).unwrap();
        assert_eq!(output.origin, [2, 1]);
        assert_eq!(output.dimensions, [2, 1]);
        assert_eq!(
            u16::from_le_bytes(output.body[28..30].try_into().unwrap()),
            24
        );
        let (_, _, bands, pixels) = crate::image::BmpView::parse(&output.body)
            .unwrap()
            .decode_native()
            .unwrap();
        assert_eq!(bands, 3);
        assert_eq!(pixels, [255, 0, 0, 255, 0, 0]);
        let partial = br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1"><rect width="2" height="1" fill="red" opacity="0.5"/></svg>"#;
        let output = super::super::engine::process(partial.to_vec(), &request(), &[]).unwrap();
        assert_eq!(
            u16::from_le_bytes(output.body[28..30].try_into().unwrap()),
            32
        );
        let decoded = crate::image::bmp(&output.body).unwrap();
        assert_eq!(decoded.pixels.as_slice(), [255, 0, 0, 128, 255, 0, 0, 128]);
    }
}
