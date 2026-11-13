//! Inspect the actual installed libvips operations, including optional plugins.
use crate::Error;

pub fn report() -> Result<serde_json::Value, Error> {
    let mut report = serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"builtin":true,"native_codecs":cfg!(feature="native-codecs"),"cloud_sources":cfg!(feature="cloud-sources"),"telemetry":cfg!(feature="telemetry")});
    #[cfg(feature = "native-codecs")]
    {
        libvips::Vips::init("pixstride-capabilities")
            .map_err(|_| "cannot initialize native engine")?;
        report["libvips"] = serde_json::json!(
            libvips::Vips::version_string().map_err(|_| "cannot read libvips version")?
        );
        let available = |name: &str| {
            let name = std::ffi::CString::new(name).unwrap();
            // SAFETY: both names are live NUL-terminated strings; type_find only
            // queries the initialized GType registry and retains neither pointer.
            unsafe {
                libvips::bindings::vips_type_find(c"VipsOperation".as_ptr(), name.as_ptr()) != 0
            }
        };
        let mut formats = serde_json::Map::new();
        for (format, load, save) in [
            ("png", "pngload_buffer", "pngsave_buffer"),
            ("jpeg", "jpegload_buffer", "jpegsave_buffer"),
            ("webp", "webpload_buffer", "webpsave_buffer"),
            ("gif", "gifload_buffer", ""),
            ("tiff", "tiffload_buffer", "tiffsave_buffer"),
            ("avif", "heifload_buffer", "heifsave_buffer"),
            ("heic", "heifload_buffer", "heifsave_buffer"),
            ("jxl", "jxlload_buffer", "jxlsave_buffer"),
            ("svg", "svgload_buffer", ""),
        ] {
            formats.insert(format.into(),serde_json::json!({"load":available(load),"save":format=="gif" || (!save.is_empty()&&available(save)),"codec_probe_required":matches!(format,"avif"|"heic")}));
        }
        report["native_formats"] = formats.into();
    }
    // Keep one shape in both feature configurations without mutability warnings.
    report["builtin_formats"] = serde_json::json!([
        "png",
        "jpeg",
        "webp-lossless",
        "gif",
        "tiff",
        "bmp",
        "ico",
        "ppm",
        "svg-subset"
    ]);
    Ok(report)
}
