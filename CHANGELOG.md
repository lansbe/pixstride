# Changelog

Notable changes are recorded here for users, following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html). During 0.x development, incompatible changes use a new minor version and are called out explicitly.

## [Unreleased]

### Added

- Bounded URL rewriting/source patterns, preset-only policy, Client Hints, enforced formats, configurable HTTP routes, authenticated separate Prometheus listener and processed fallback images.
- SVG sanitization with bounded embedded PNG/JPEG resources; HEIF response MIME matching OSS and JPEG XL animation input.
- OTLP metrics/logs, optional W3C parent sampling and PEM TLS identity; direct Datadog/New Relic OTLP, four error-notice APIs and CloudWatch SigV4 with local protocol tests on all CI platforms.

- Native JPEG progressive/PNG interlaced and bounded palette settings, encoder effort/preset controls, processing defaults, preferred formats, truncated HMAC and explicit trusted signatures.

- Optional isolated libvips engine with native WebP/AVIF/HEIC/JPEG XL, broader static SVG, 16-bit colour/alpha, selected copyright metadata and bounded GIF/WebP animation.
- OSS URL presets, key rotation, expiration, DPR/zoom/minimum dimensions, extension, smart/fractional crops, watermark tiling/URL/Base64, per-format quality and bounded output-size search.
- HEAD, Bearer authorization, opt-in modern format negotiation, ETag/Last-Modified/TTL, UTF-8 disposition, CORS, Prometheus and optional bounded OTLP/HTTP traces.
- Optional S3/GCS/Azure/Swift readers with per-request credential loading and Keystone v3 token exchange, nested confined local paths, inbound IPv6/Unix sockets and native concurrency with explicit memory reservations.
- Pinned imgproxy 4.0.21 compatibility matrix, synthetic differential/native/HTTP/mutation/soak checks and macOS ARM64/Intel CI alongside Linux containers.
- A protected `develop` integration line, separate prerelease channels, automatic merged-branch deletion and a safe local cleanup command.
- Generated Markdown sections, documentation consistency checks and release preparation that synchronizes Cargo versions and the changelog.
- Automatic GitHub release notes from the reviewed changelog after both container architectures pass validation.

### Changed

- External crates and image libraries are permitted; feature-specific dependencies, resource limits and migration differences are documented in `COMPATIBILITY.md`. Native and cloud capabilities are opt-in in Cargo; the container includes them.
- Building now requires fetching locked Cargo dependencies before offline checks. Python tooling requires Python 3.11+.
- Release version tags are immutable. Prereleases never update the stable Docker or GitHub `latest` release, and older releases cannot roll a channel backwards.

### Fixed

- Read startup entropy through the OS random device on glibc, musl and macOS; qualify JPEG XL animation input with the installed encoder API and allow configured watermark/fallback cloud sources.

- Match effective DPR, minimum-size priority, separate extension gravities, zoomed canvases and result limits including padding against 50 synthetic OSS comparisons; support RGB ICC retention with watermark composition and quoted local SVG references.
- Install Go explicitly in macOS CI for the pinned OSS reference build.

- Explicitly return accepted sockets to blocking mode on nonblocking listeners, preventing intermittent macOS HTTP 400 responses before request data arrives.
- Read a complete bounded health status line instead of assuming the first TCP read contains it.
- Use the platform C character type and GLib allocation cleanup in the locally patched libvips binding, including unsigned C char on Linux ARM64.

### Security

- Bound native child lifetime, pipes, frame totals and encoding attempts; remove secrets from child environments and reject external/active SVG resources.
- Preserve host/public-address checks in cloud SDK transport, use conditional object reads and reject symlinks throughout nested local paths.

## [0.1.1] - 2026-11-08

### Security

- Bound cumulative DEFLATE, progressive JPEG, TIFF, SVG and filtered-resize work. These limits can reject otherwise valid, expensive inputs.
- Reuse immutable DEFLATE tables and bound GIF palette probing without caching image bodies.
- Restrict unsigned Host headers, preserve short overload deadlines and enforce watermark limits during the actual file read.
- Clarify libcurl callback ownership and validate Linux C/Rust ABI values for both container architectures.
- Scan and smoke-test the exact platform manifests promoted to release tags, preserving provenance and SBOM descriptors.

## [0.1.0] - 2026-11-08

### Added

- Initial public Rust image transformation server, custom codecs, signed URLs, explicit source-host allowlists and bounded local-file access.
- Linux AMD64/ARM64 Docker images, MIT licensing and deployment documentation.
- Historical CPU, loopback HTTP and RAM comparisons with their quality and environment limitations. This is not full imgproxy compatibility.

[Unreleased]: https://github.com/lansbe/pixstride/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/lansbe/pixstride/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/lansbe/pixstride/releases/tag/v0.1.0
