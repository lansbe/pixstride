# Changelog

Notable changes are recorded here for users, following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html). During 0.x development, incompatible changes use a new minor version and are called out explicitly.

## [Unreleased]

### Added

- Add collection protocol 2.3 to compare explicit BMP V5 alpha from both engines. Keep all prior protocol results and the 2.2 sampling, deadline, resource limits, quality thresholds and victory criteria.
- Add explicit collection protocol 2.2 with a shared 3,600-second case deadline after protocol 2.1 missed nine responses on one heavy x86_64 trial. Preserve complete 2.0/2.1 cohorts and their failures; workload definitions, samples, resource limits, numerical budgets and victory gates remain fixed.
- Version collection protocol 2.1 explicitly, allowing 1,800 seconds per workload to complete heavy trials without changing the 1,000 cases, sampling, service resources, pixel budgets or strict victory gates. Preserve schema-2/600-second evidence and reject mixed or relabelled protocol cohorts.
- Add explicit unreleased V2 candidate campaigns with one native Docker build per architecture, immutable image artifacts, source/tree/Dockerfile/lock provenance and separate complete-cohort archives. Keep published pins and historical evidence intact.
- Introduce benchmark v2 with 1,000 distinct workloads, 20 processing families, twelve static input representations, five output codecs and ten difficulty tiers ordered before measurement. Add a deterministic shared corpus, failure-preserving native shard collector, complete-cohort/replay/quality validation, separate v2 target pins and immutable result archives. Native preflight validates coverage; full three-repeat timings remain a separate campaign. Keep the 25-case v1 evidence and published 0.4.0 preflight intact, including its 33 engine failures and 141 pixel-budget failures per mode/architecture.

- Verify published benchmark image identities against raw OCI index/platform digests, release version/source annotations, native image configs and runtime capabilities, independently of the harness revision. Default new runs to the current release pins while retaining the immutable 0.3.0 baseline.
- Record/check published-image comparisons separately from pre-release source cohorts, retaining all six native reports and every failed comparison. Share the strict CPU/p50/p95/peak and decoded-pixel gates, generate both-mode tables and median reductions, and run three independent native VMs per architecture without rebuilding the release.

### Changed

- Plan eligible native SVG rendering in the final orientation so turns and flips can use a smaller decoded raster. Apply the requested orientation and residual resize after rendering.

- Archive the full `5779424` and `28e1a37` comparisons with protocol 2.3. Every response and pixel budget passes. The SVG optimization brings the ten rotated SVG long chains to ten strict wins on Linux x86_64 in both modes and nine default / eight forced native on ARM64. Keep every case's metrics with the source and image pins.

- Stream plain RGB8 PNG rows directly into BMP for eligible flips and half-turns, with complete stream validation and the existing source and output limits. Use the lossless Rust PNG encoder for eligible grayscale and grayscale-alpha rasters, preserving genuine alpha and keeping color profiles, palette encoding, higher bit depths and interlacing on their corresponding paths.

- Record three complete 1,000-case protocol 2.2 comparisons on native Linux x86_64 and ARM64. Each completes all timed responses and passes the pixel budgets in both modes. The `a6cd40f` comparison reaches 777 strict wins on x86_64 and 803 on ARM64 in default mode. Publish the metrics and every loss in the benchmark documentation.

- Retain the exact 256-color palette through eligible opaque Indexed8 PNG quarter-turns and flips into lossless BMP8, avoiding RGB expansion and preserving complete-stream validation and resource limits. Extend bounded native DIB40/BI_RGB8 input to complete RGB palettes, preserving the one-band gray path and enforcing the decoded-byte ceiling before allocation. Small outputs, size targeting, other transforms, alpha and color-managed sources keep their existing routes; encoded bytes and ETags can change.
- Encode native BW8 BMP directly with a complete lossless gray palette when its exact file size is no larger than RGB24, retaining small-image byte-target successes. Add bounded native BI_RGB8/DIB40 grayscale-palette input for roundtrips, preserving palette mappings and source budgets before raster allocation; other native/built-in BMP routes stay unchanged.
- Render eligible bounded byte SVG rasters once, verify every alpha sample and use RGB processing only when all are opaque. Keep true transparency, antialiasing and metadata, with existing HDR/linear/large-raster fallbacks.
- Retain owned native response frames through HTTP output and encode RGB BMP directly into its output vector, eliminating duplicate payload allocations without changing encoded bytes, raster alignment, frame validation or worker limits.
- Decode complete opaque Indexed8 PNG palettes into bounded owned RGB, preserving palette output policy and native fallbacks for transparency, color management, interlace and other precision requirements.
- Build pinned libvips 8.18.7, quantizr 1.4.3, libheif 1.23.7 and AOM 3.15.3 for the Linux native worker, retain all native codecs and ship their corresponding sources and notices. Verify loaded SDK/runtime libraries and real codec roundtrips before accepting an image.
- Materialize eligible bounded byte rasters before SmartCrop analysis and native filters, avoiding repeated upstream evaluation while retaining higher-precision/large-image fallbacks. Identity crops and absent filters keep their direct route.
- Refresh README and benchmark results with the actual published 0.4.0 image: all 25 cases win CPU, p50, p95 and lifetime peak cgroup memory in default and forced-native modes, in every run on three native Linux VMs per architecture. Retain all 64,800 timed requests, output-size/pixel differences, hardware identities and earlier losses separately. These observations do not certify perceptual equivalence or a universal performance guarantee.

### Fixed

- Remove the unused Pebble binary from the runtime filesystem. It came from the Ubuntu base image and embeds the Go version affected by CVE-2026-97054.

- Preserve native BMP transparency through an explicit V5 alpha mask and decode the same layout on reimport. Choose background compositing precision from the source color space and premultiply transparent images for native blur and sharpening.
- Require the canonical V2 candidate corpus and one collection identity during collection and import. Retain late imports in history according to their original start time.

- Keep opaque native RGB/grayscale samples unchanged when a background is supplied, and avoid charging convolution scratch to identity geometry such as large flips and quarter-turns. Preserve actual resize computation and pixel limits.
- Match mirrored integer pixelation blocks, preserve straight RGBA samples in native 32-bit BMP output, and composite transparent JPEG output over the requested background or white by default.
- Retain palette representation when stripping private metadata and inherit native PNG palette precision and quantizer defaults instead of silently exporting truecolor pixels.
- Match reduced WebP decoding and scaled SVG rendering before residual geometry, while preserving original source admission, crop coordinates and resource limits.
- Decode valid indexed PNG fixtures in the independent native smoke-check reader, including packed palette indices and transparency.
- Validate corrected source against all 1,000 imgproxy cases on native Linux x86_64 and ARM64: both default and forced-native modes complete every trial within the frozen numerical pixel budgets. Retain all 80 raw reports and the original 120-file corpus identity; preflight has no timing-win claim.
- Import opaque BI_RGB24/32 BMP directly into owned three-band RGB rasters, preserving row order and ignoring RGB32 padding. Avoid synthetic-alpha resampling and AVIF auxiliary alpha encoding while retaining real ICO/PNG alpha. Rounding, encoded representations and ETags can change.

## [0.4.0] - 2026-11-16

### Added

- Reusable 25-case benchmarks on native Linux AMD64 and ARM64, with one verified public corpus, pinned imgproxy 4.0.24, balanced signed HTTP requests, child-inclusive CPU, lifetime cgroup peaks, output sizes and decoded-pixel checks. Preserve every raw sample and both complete observed cohorts.
- Strict default/forced-native qualification requiring at least three independent runs per architecture. Keep failed observations separate from qualified snapshots and the published 0.3.0 baseline; verify source/image identities, archive hashes, statistics and generated documentation.
- Reusable native allocator/page-policy and JPEG filter-strip diagnostics, separate from HTTP performance qualification.

### Changed

- Default to `PIXSTRIDE_ENGINE=auto`: validated opaque PNG identities keep the built-in route, while other eligible transforms use reusable isolated native workers. Explicit `builtin` legacy routing and forced `native` remain available. Native routing/resampling can change pixels, encoded bytes and ETags; change signed cachebusters or invalidate transformed-image caches when upgrading.
- Reuse workers with bounded nonblocking framed pipes, absolute request deadlines, renewed per-image CPU limits, bounded admission and recycling after 256 successful images. Do not cache images, results or libvips operations.
- Move Docker builds/runtime from Alpine/musl to digest-pinned Ubuntu 26.04/glibc with distribution libvips/codecs and packaged security updates. Rust 1.100 remains required; the compiler is pinned to 1.100.0. Published 0.3.0 images retain their original Alpine behavior.
- Accelerate PNG filtering/compression with SSE2/NEON, crc32fast and libdeflate; decode eligible plain eight-bit PNGs with a bounded Rust decoder. Retain complete structure, CRC, DEFLATE, Adler/trailer, filter and original source-budget validation. Metadata/profile/HDR/palette/interlace/animation declarations retain the general route.
- Validate and move unchanged plain RGB8 PNG frames inside the isolated native worker instead of recompressing them. Keep byte targets, resource limits and fallback behavior.
- Reduce eligible JPEGs during decoding and stream eligible TIFFs, preserving original dimensions/admission and random access when geometry or quality retries need it. Release redundant source/raster ownership without weakening complete-source validation.
- Render eligible fontless SVG-to-PNG with resvg inside the bounded worker. Deny external resource resolution and preserve XML/source/pixel/deadline/address-space admission; unsupported declarations retain the native renderer.
- Use bounded SIMD premultiplied Lanczos3 for eligible plain RGBA8 PNG-to-JPEG downscaling, preserving geometry/DPR, requested quality and the existing pixel-agreement envelope. Other precision, metadata/HDR, linear-light or complex geometry retains the native route.
- Retain System allocation. GNU native workers use one glibc arena, startup base-page policy, default small allocation caches and fixed 4-MiB mmap/trim thresholds, with existing deadlines, 1-GiB address-space ceiling and recycling. Development-only mimalloc probes do not override service allocation.
- Stream eligible JPEG blur/sharpen output through up to 32 rows, capped at 4 MiB per strip; preserve native samples and materialized byte-target retries.
- Pin the benchmark client to spare CPUs outside the services' unchanged two-CPU cpuset. Record kernel-verified affinity and preserve older reports with their original protocol.
- Record final forced-native wins in all 25 cases on both architectures, in each of three runs. Default routing wins 25/25 AMD64 and 24/25 ARM64: graphics-crop p95 remains 0.71% higher in one ARM64 run, so the complete both-mode target is not met. Preserve the preceding cohort's two AMD64 p95 losses too. These shared-VM observations are not universal or certified equal-quality claims; some encoded files are larger. See [all cases, provenance and limits](BENCHMARKS.md#current-measured-result).

### Fixed

- Enforce exact encoded-byte limits for accelerated SVG-to-PNG output, including targets too small for a lossless result.
- Keep random source access when JPEG/TIFF geometry can discard rows, including implicit vertical fill/auto crops and fill-down. Reject truncated JPEG tails even when a crop uses only earlier rows.
- Correct allocator-diagnostic startup after client-affinity metadata changes and reject two-run publication/reading of new both-mode snapshots while retaining historical qualification criteria.
- Exercise the same host-driven default-routing container smoke checks in ordinary and tagged CI after the Ubuntu migration.

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

[Unreleased]: https://github.com/lansbe/pixstride/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/lansbe/pixstride/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/lansbe/pixstride/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/lansbe/pixstride/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/lansbe/pixstride/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/lansbe/pixstride/releases/tag/v0.1.0
