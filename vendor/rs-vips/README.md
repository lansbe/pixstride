# Local rs-vips portability patch

This directory contains the MIT-licensed rs-vips 0.7.0 source from crates.io, with its original copyright notices and LICENSE. The original crate archive SHA-256 is `52bff86a769bb29a4a68a54ca68dfb8e713aa5e6c6a380d26d6878393007528c` and upstream commit is `66c148c302cf975f440bb7148879bf127866d32b`. Source: [rs-vips](https://github.com/mrdkprj/rs-vips), [published crate](https://crates.io/crates/rs-vips/0.7.0).

Local changes retain the upstream dependencies and link script; the manifest disables publishing:

- `new_c_string_from_raw` in `src/utils.rs` uses the platform C char type (unsigned on Linux ARM64) and copies GLib-owned strings before releasing them with `g_free`. Never transfer a GLib allocation to `CString::from_raw`.
- `VipsSource::new_from_shared_memory` keeps an immutable `Arc<Vec<u8>>` on the native source, avoiding an encoded-input copy. `VipsImage::new_from_owned_u8` validates dimensions/bands and consumes an exact pixel `Vec<u8>` without copying. Pixel ownership is exclusive because native operations can mutate pixels. Private `g_object_set_data_full` callbacks release each owner at native-object finalization; downstream lazy images retain those objects after Rust wrappers drop.
- `VipsSource::clone` acquires a native reference with `g_object_ref`, matching its destructor. The upstream derived clone copied the pointer without retaining it.

Lifetime regressions in `src/processing/native/shared.rs` exercise lazy evaluation after wrapper drops, source cloning, invisible RGB/alpha preservation, allocation release and invalid raster sizes.

Upstream generated bindings/operations retain their upstream layout. The project's 500-line limit applies to first-party `src/`, not imported generated dependency code. Review the patch against the archive when updating it, retain license/provenance, run both Linux architectures and macOS checks, and remove the override when a released upstream version supplies the corrections. Cargo.lock records this local package and all registry dependencies; Docker includes the same source. No source is modified in Cargo's shared registry cache.
