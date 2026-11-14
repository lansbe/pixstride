# Local rs-vips portability patch

This directory contains the MIT-licensed rs-vips 0.7.0 source from crates.io, with its original copyright notices and LICENSE. The original crate archive SHA-256 is `52bff86a769bb29a4a68a54ca68dfb8e713aa5e6c6a380d26d6878393007528c` and upstream commit is `66c148c302cf975f440bb7148879bf127866d32b`. Source: [rs-vips](https://github.com/mrdkprj/rs-vips), [published crate](https://crates.io/crates/rs-vips/0.7.0).

The single source change is `new_c_string_from_raw` in `src/utils.rs`: use the platform C char type (unsigned on Linux ARM64) and copy GLib-owned strings before releasing them with `g_free`. Never transfer a GLib allocation to `CString::from_raw`. The other source files and link script are preserved from the published archive; the local manifest retains the two upstream dependencies and disables publishing.

Upstream generated bindings/operations retain their upstream layout. The project's 500-line limit applies to first-party `src/`, not imported generated dependency code. Review the patch against the archive when updating it, retain license/provenance, run both Linux architectures and macOS checks, and remove the override when a released upstream version supplies the corrections. Cargo.lock records this local package and all registry dependencies; Docker includes the same source. No source is modified in Cargo's shared registry cache.
