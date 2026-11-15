# Changelog

Notable changes are recorded here for users, following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html). During 0.x development, incompatible changes use a new minor version and are called out explicitly.

## [Unreleased]

### Added

- Retain the qualified unreleased candidate's six complete native Linux runs: default routing has lower CPU, HTTP p50/p95 and peak cgroup memory in all 25 cases in each of three AMD64 and three ARM64 runs. See [the measurements and limits](BENCHMARKS.md#unreleased-candidate-confirmation), including larger encoded files and decoded-pixel differences. Published 0.3.0 results remain separate.
- Publish qualified repeated candidate snapshots separately from release evidence, retaining every raw report and checking archive hashes, derived metrics and generated documentation. Add aggregate process-memory diagnostics in separate post-measurement containers.
- Add repeated candidate qualification that rejects default CPU/p50/p95/memory losses, duplicate samples, mismatched source/corpus/options and decoded-pixel regressions; require extra retained runs near parity before confirming 25/25.
- Reusable 25-case image-processing benchmarks for native Linux AMD64 and ARM64, comparing immutable Pixstride default/native and imgproxy images with one shared public corpus, balanced repeated HTTP measurements, child-inclusive CPU, peak cgroup memory, output sizes and decoded-pixel differences. Preserve raw samples and generate checked documentation tables from the measured evidence.

### Changed

- Keep glibc mmap/trim thresholds fixed at 4 MiB in workers and the container so large per-image allocations can be released instead of raising adaptive heap retention.
- Materialize bounded cropped JPEG input from a sequential full decode, preserving validation of unused rows while releasing decoder caches. Stream bounded single-pass JPEG blur/sharpen output through thin strips; materialize independent rasters for repeated byte-target encodes.
- Stream eligible single-pass TIFF input; keep random access for animation, row rewinds and repeated encoding.
- Release native source graphs and thread-private regions after materializing eligible PNG output, before allocating filter/compression buffers, then release owned raster pixels before compression. Keep final raster ownership independent and retain byte limits and encoded bytes.
- Require Adler32 and ancillary CRC validation in the optimized PNG input decoder, including late chunks and cropped requests; override the dependency's permissive defaults.
- Move encoded inputs and eligible decoded PNG rasters into native graph ownership without redundant copies. Retain shared immutable source bytes until native finalization, use exclusive pixel ownership, and correct the vendored source wrapper's native reference counting when cloned.
- Release libvips thread-private regions and error state after every native image, after dropping all image references. Bound glibc worker arenas to two. On Linux, request pipe capacity up to 1 MiB when kernel quotas permit, retaining the same framed protocol and absolute deadline.
- Feed native RGB/RGBA8 rasters directly to lossless PNG encoding and encode ordinary RGB BMP output without expanding to RGBA first. Validate and free the owned GLib allocation once; retain byte-identical output for the same pixels/options.
- Build unreleased containers with pinned Ubuntu 26.04/glibc and the same Rust 1.100.0 toolchain, retaining non-root execution, codec checks and resource restrictions. Published 0.3.0 images keep Alpine/musl.
- Use libdeflate level 1 for lossless compression to reduce CPU/latency at the cost of larger encoded output. Keep decoded samples, quality settings and file ceilings unchanged. Avoid redundant native full-frame extraction, identity crops/resizes and color-space operations; allow streaming JPEG blur/sharpen while retaining random access for row-rewinding operations and byte-target retries.
- Use source-precision premultiplied samples for native Lanczos alpha resizing, matching imgproxy’s integer convolution pipeline. Preserve HDR/floating types and explicit quality/lossless settings; low alpha/color samples and encoded bytes can change relative to the previous floating intermediate.
- Reuse bounded isolated native workers instead of starting a process and initializing libvips for every image. Use nonblocking owned pipes with one absolute read/write deadline, avoiding two per-image helper threads. Keep length-delimited pipes, per-image deadlines and renewed CPU budgets, replace failed workers, recycle after 256 successful images and reap idle workers on shutdown. Native image/operation caches remain disabled; no response cache is added. Performance qualification of this unreleased implementation is recorded separately from the published 0.3.0 baseline.
- Default unreleased source builds to automatic routing: retain validated opaque PNG identities and use reusable native workers for other eligible transforms. Keep explicit `builtin` legacy routing and forced `native` processing. Native selection can change JPEG sampling/resize pixels, encoded bytes and ETags; change the signed cachebuster or invalidate transformed-image caches when upgrading.
- Accelerate PNG adaptive filter scoring and residual generation with baseline SSE2 on x86_64, retaining scalar-equivalent scores, filters and tie ordering. ARM64 retains its NEON implementation.
- Use crc32fast 1.5.5 and bounded libdeflate-sys 1.26.1 compression, replacing the in-tree DEFLATE encoder. Decode and checksum validation remain active. Lossless encoded bytes can change. Eligible native 8-bit sRGB PNG output uses the optimized encoder while HDR, metadata/profile retention, palettes and interlace keep the native saver.
- Render eligible fontless SVG-to-PNG requests at target size with resvg 0.50.0 inside the isolated worker, without initializing libvips. Deny resource resolvers and retain XML/source/pixel/deadline/address-space admission; unsupported text/images and complex options retain the native renderer.
- Decode eligible plain eight-bit PNGs with png 0.18.1 before native geometry, retaining CRC/trailer validation, source budgets, straight alpha and invisible RGB. Metadata, profiles, HDR, palettes, animation and interlace retain the native loader. Reduce eligible JPEGs at load by 2, 4 or 8 and use sequential access for single-pass requests. Keep geometry based on original dimensions, validate original source budgets before shrinking, and preserve random access for repeated quality attempts and operations that rewind rows. Reduced JPEG sampling can change pixels and encoded bytes.


### Fixed

- Enforce exact encoded-byte limits for accelerated SVG-to-PNG output, including targets too small for a lossless result.
- Keep random source access when computed JPEG/TIFF geometry can discard source rows, including implicit vertical fill/auto crops and fill-down. Reject truncated JPEG tails even when the selected crop uses only earlier rows.
- Exercise the same host-driven default-routing container checks in ordinary and tagged CI, replacing Alpine-only BusyBox clients after the Ubuntu migration.

## [0.3.0] - 2026-11-14

### Changed

- Upgrade the native PNG palette fallback to color_quant 2.0.0 and use its separate RGB/alpha palette APIs. Corrected palette lookup and training can change quantized PNG bytes, pixels and ETags. Change the signed cachebuster or invalidate transformed-image caches when deploying; native imagequant output and GIF's separately locked quantizer are unaffected.
- Require Rust 1.100 and use Rust 1.100.0 for the pinned Alpine builder and native CI. This includes the Rust 1.99.1 FFI/linker regression fixes; the runtime Alpine release and image-processing contract are unchanged.

### Security

- Enforce `PIXSTRIDE_ALLOWED_SOURCES` on initial HTTP sources and every redirect destination after removing literal dot path segments. Previously allowed sources can now fail when they redirect outside the configured source patterns; update those patterns only for intended destinations.
- Add `Authorization` to `Vary` on Bearer-protected image responses so shared caches cannot reuse an authorized response for requests with missing or different credentials. Configured public cache TTLs remain available to authorized requests.
- Close keep-alive connections after a failed or truncated response write, preventing later pipelined requests from being processed after an incomplete response.
- Require `PIXSTRIDE_SIGNATURE_SIZE` to be 16–32 bytes (default 32), preventing practical exhaustive forgery of short HMAC signatures. Configurations using 1–15 bytes now fail server startup and the `sign` command. Before upgrading, configure every signer and server to use the same supported size and regenerate signed URLs; existing short signatures are no longer accepted.

## [0.2.0] - 2026-11-14

### Added

- Optional S3 client-side KMS decryption for authenticated V2 AES-GCM and V3 committing HKDF-SHA512 envelopes, bounded in-place payload authentication and MIME/Unicode context handling. Real TLS S3/KMS tests cover independent envelopes, SigV4 and tamper/downgrade rejection.
- CloudWatch SDK/profile credentials under the existing one-second export deadline; shared source identity transport for S3 roles and KMS. Optional cloud root CAs retain certificate/hostname verification.
- OSS origin/result debug headers from both engines and bounded gzip/deflate HTTP source decoding with separate wire-byte accounting.
- Optional staged startup configuration from AWS Secrets Manager, paginated decrypted SSM parameters, Google Secret Manager with CRC32C and local environment files. SDK OAuth/ADC and credential files are covered by synthetic protocol checks; native workers never load these sources.
- SDK metadata credentials behind a separate explicit opt-in; S3 roles/external IDs/path style and per-bucket regions, S3/GCS object versions, per-provider bucket deny lists and Swift v1/v2/v3 authentication with public regional catalog discovery and independent user/project domains.

- IPv4/IPv6 source fetching, explicit loopback/private/link-local and port policies, bounded download/redirect settings and configurable User-Agent/TLS verification. Real loopback IPv4/IPv6 checks cover redirects and per-request cookie isolation.
- Opt-in origin Cache-Control/Expires passthrough, subordinate to signed URL expiration, fallback policy and private cookie caching. Send an explicit restrictive Content-Security-Policy on responses.
- Bounded single byte ranges for admitted raw images, strong/date If-Range, conditional-response precedence, suffix/open-ended ranges and 206/416 responses without a second body allocation.

- Optional per-source cookie passthrough with RFC6265 rules, a pinned public suffix list, bounded per-request storage and no cookie state in pooled transports. Cookie-dependent image responses always disable shared caching.

- Opt-in HTTP keep-alive and pipelining with a 32-request connection limit, fixed pending-input buffer, idle expiry and signal-aware shutdown; verify reuse, request framing and shutdown on all CI platforms.

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

- External crates and image libraries are permitted; feature-specific dependencies, resource limits and migration differences are documented in the [0.2.0 compatibility guide](https://github.com/lansbe/pixstride/blob/v0.2.0/COMPATIBILITY.md). Native and cloud capabilities are opt-in in Cargo; the container includes them.
- Building now requires fetching locked Cargo dependencies before offline checks. Python tooling requires Python 3.11+.
- Release version tags are immutable. Prereleases never update the stable Docker or GitHub `latest` release, and older releases cannot roll a channel backwards.

### Fixed

- Read exactly the bounded stat size for SDK object bodies, avoiding short-read rejection when the server returns fewer bytes than the prior maximum-sized request.
- Keep credential/configuration/certificate reads nonblocking and limited to regular targets while supporting rotating mounted symlinks; image-source confinement remains unchanged.
- Wait for listener readiness in the nonblocking acceptance regression, avoiding a scheduling race after TCP connect.

- Match OSS HTTP error statuses for Bearer authorization, invalid signatures, processing URLs, expiration and source pixel limits; add seven negative differential cases.

- Read startup entropy through the OS random device on glibc, musl and macOS; qualify JPEG XL animation input with the installed encoder API and allow configured watermark/fallback cloud sources.

- Match effective DPR, minimum-size priority, separate extension gravities, zoomed canvases and result limits including padding against 50 synthetic OSS comparisons; support RGB ICC retention with watermark composition and quoted local SVG references.
- Install Go explicitly in macOS CI for the pinned OSS reference build.

- Explicitly return accepted sockets to blocking mode on nonblocking listeners, preventing intermittent macOS HTTP 400 responses before request data arrives.
- Read a complete bounded health status line instead of assuming the first TCP read contains it.
- Use the platform C character type and GLib allocation cleanup in the locally patched libvips binding, including unsigned C char on Linux ARM64.

### Security

- Enforce allowed-source patterns on every HTTP redirect and on normalized initial HTTP paths, preventing literal `.` and `..` segments from escaping an allowed path prefix. Relative redirects that stay within the allowed prefix remain supported.

- Reject active foreign XML namespaces in SVG passthrough and remove them during sanitization; include an XHTML object regression alongside scripts/styles/external references.

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

[Unreleased]: https://github.com/lansbe/pixstride/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/lansbe/pixstride/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/lansbe/pixstride/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/lansbe/pixstride/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/lansbe/pixstride/releases/tag/v0.1.0
