# imgproxy OSS compatibility

The reference is **imgproxy OSS 4.0.21**, commit `302105ccd183d0038ef71aa11ab4ff3de5f0c989`. The immutable multi-platform image and platform manifests are recorded in [.github/checks/compatibility/reference.json](.github/checks/compatibility/reference.json). This is a compatibility target, not a claim of complete drop-in equivalence.

Compatibility has three separate meanings: accepting a URL, producing the same HTTP/image behavior, and producing equivalent decoded pixels. The synthetic comparison checks dimensions, media type and pixels with per-operation tolerances in [cases.json](.github/checks/compatibility/cases.json). Metadata, alpha, 16-bit samples, animation timing, errors, resource limits and HTTP contracts have separate checks. Compressed bytes need not match. A passing small corpus does not establish equivalence for every image.

## Architecture and dependencies

The built-in engine remains the default. `native-codecs` adds a libvips worker, selected with `PIXSTRIDE_ENGINE=native`. Operations that need this engine select it automatically; builds without it return an explicit unsupported-operation error. Each native image runs in a fresh child, with typed options and bounded binary pipes. No source URL or credential appears in its arguments or inherited environment. Children have no image cache and are killed and reaped after `PIXSTRIDE_PROCESS_TIMEOUT` (default 10 seconds, range 1–120). This is process isolation for resource/lifetime control, not an OS sandbox. libvips and its format libraries remain trusted native code.

`rs-vips` (MIT), with a [documented local C-char/GLib ownership patch](vendor/rs-vips/README.md), wraps dynamically linked libvips (LGPL-2.1-or-later); libvips delegates to system codec, colour and SVG libraries. The container includes its HEIF/JXL plugins and AV1/HEVC encoders. Codec availability depends on the installed system packages. `pixstride capabilities` reports registered operations; `scripts/native_check.py` tests actual codec round trips, including encoders behind the HEIF plugin. Registering a saver alone does not prove a particular HEIF encoder works. x265 has its own GPL licensing terms. The application's MIT license does not replace any dependency's license; retain package notices and corresponding source availability when distributing system libraries.

`cloud-sources` adds Apache OpenDAL (Apache-2.0), Tokio and reqwest with system TLS. The custom SDK transport ignores proxies, disables redirects, filters resolved addresses to public IPv4, checks literal destinations, bounds responses and verifies TLS. `telemetry` adds a bounded OTLP/HTTP JSON exporter using reqwest/Tokio. Other direct crates provide bounded GIF encoding, a PNG palette fallback (MIT color_quant), XML parsing, typed worker messages, HTTP dates and platform limits; their licenses are recorded in Cargo metadata. Cargo.lock fixes the dependency graph; Dependabot tracks updates. Linux/macOS on ARM64 and x86-64 remain the supported targets.

## Processing matrix

All OSS processing option names and aliases in the pinned reference parser have a parser implementation. Native rows require `native-codecs`; accepting an option is not a promise that every combination is equivalent.

| Lot | OSS feature/options | Implementation and verification |
|---|---|---|
| 3 | Plain/Base64 source paths; `format/f/ext`; long/short aliases | Bounded parser; HMAC binds the original normalized path before preset expansion. Existing signature/encoding regressions retained. |
| 3 | `preset/pr`, default presets, nested presets, repeated options | Ordered expansion; later values override earlier values. Startup validates definitions. Cycles, depth >8, >64 definitions, >256 expanded options and >16 KiB expansion fail. |
| 3 | Key/salt rotation; `expires/exp`, `cachebuster/cb` | Up to eight paired keys/salts; CLI signs with the first pair. Expiry is checked before source access and caps response TTL. Cachebuster participates in the signed path. |
| 4 | `resize/rs`, `size/s`, `resizing_type/rt`, `width/w`, `height/h`, `enlarge/el` | Existing fit/fill/force/auto plus native fill-down. Differential dimension/pixel cases. |
| 4 | `min-width/mw`, `min-height/mh`, `zoom/z`, `dpr` | Native float-scale planning, final crop, bounded dimensions; differential cases for min dimensions, uniform/nonuniform zoom and DPR. |
| 4 | `extend/ex`, `extend_aspect_ratio/extend_ar/exar` | Native transparent canvas and gravity offsets. Background is applied separately. Differential extension/offset cases. |
| 4 | `gravity/g`, `crop/c`, focus points, smart gravity, fractional crop/offsets | Native smart crop and final-orientation source crop; ordinary quarter-turn/flip/focus behavior retained. Differential crop/rotation/offset cases. |
| 4 | `trim/t`, `padding/pd`, `auto_rotate/ar`, `rotate/rot`, `flip/fl`, `background/bg` | Existing bounded operations plus native trim equal axes, high-precision backgrounds and native EXIF rotation. |
| 4 | `blur/bl`, `sharpen/sh`, `pixelate/pix` | Built-in path retained; native libvips operations available. Differential blur/sharpen cases. Filter and codec choices can change pixels. |
| 4 | `watermark/wm`, repeat gravity, configured path/URL/Base64 | Bounded image read per relevant request; no watermark image cache. Opacity, scale, gravity, offsets and tiling; HDR composition preserves the working pixel type. |
| 7 | `quality/q`, `format_quality/fq`, `max_bytes/mb` | Native lossy encoders and bounded search (at most eight attempts). Impossible targets fail; lossless formats without a quality search fail if their output exceeds the target. |
| 7 | PNG/JPEG/WebP/TIFF/ICO variants; PNG quantization | Native loading/saving complements built-in codecs; lossy WebP, grayscale and wider native TIFF/JPEG variants. PNG palette output is opt-in. Round trips and independent PNG/GIF/WebP readers. |
| 8 | `strip_metadata/sm`, `keep_copyright/kcr`, `strip_color_profile/scp` | Native metadata/profile handling; copyright filters EXIF/XMP/IPTC fields and removes unrelated private fields. Tests check retained rights and removed descriptions. Unknown copyright schemas are not preserved. |
| 8 | `preserve_hdr/ph` | Native gray/RGB/RGBA 16-bit pipeline, ICC conversion, resampling, backgrounds and watermarking. PNG16 identity samples are exact in the synthetic checks. AVIF/HEIC use 12-bit output. Unsupported high-precision output combinations fail explicitly; no blanket HDR equivalence claim. |
| 9 | AVIF, HEIC, JXL, `enforce_thumbnail/eth` | Native read/write and HEIF embedded-thumbnail loading. Actual 10-format round trips in native checks. AVIF dimensions below 16 use the reference's PNG/JPEG fallback. |
| 9 | Extended static SVG | librsvg rendering behind strict XML admission: paths/transforms/gradients/masks/text supported by the installed renderer. Active content, external references, DTDs, imported CSS and excessive structure are rejected. Font availability changes text rendering. |
| 10 | GIF/WebP animation and multipage TIFF | Native frame composition, cumulative pixel accounting and frame transforms; GIF/WebP durations and loop counts retained. GIF output has a bounded Rust fallback. First frame remains the default. |
| 5/10 | `skip_processing/skp`, `raw` | Native header/loaded-frame validation precedes returning original bytes; this does not sanitize every container frame or metadata field. SVG output requires SVG input. These options intentionally preserve original metadata/content. |
| 5 | `filename/fn`, `return_attachment/att` | UTF-8 percent/Base64 names, RFC 5987 disposition, correct extension after format fallback; control characters/path separators refused. |
| 3/10 | `max_src_resolution/msr`, `max_src_file_size/msfs`, `max_animation_frames/maf`, `max_animation_frame_resolution/mafr`, `max_result_dimension/mrd` | URL limits cannot raise absolute server byte/pixel ceilings. Animation is limited to 256 frames and 50M cumulative pixels. Output format dimension limits apply. |

## Configuration and migration

`IMGPROXY_*` variables are not silently imported. Configure the explicit equivalents below. A `default` preset supplies processing defaults without another duplicate configuration system, for example `PIXSTRIDE_PRESETS='default=q:85/maf:16;thumb=rs:fill:160:160/g:sm'`. Select `/pr:thumb/` in the signed URL; explicit later options override the preset. Preset definitions use `name=option/option`, separated by semicolons or newlines; this differs from imgproxy's configuration file syntax.

| imgproxy setting/family | Pixstride equivalent |
|---|---|
| KEY, SALT and rotation | `PIXSTRIDE_KEY`, `PIXSTRIDE_SALT`: comma-separated matching hex lists, or `_FILE`; independent minimum lengths 32/16 bytes. |
| PRESETS | `PIXSTRIDE_PRESETS` or `PIXSTRIDE_PRESETS_FILE`; bounded definitions with a reserved `default` preset. |
| LOCAL_FILESYSTEM_ROOT | `PIXSTRIDE_LOCAL_ROOT`; relative nested regular files, no symlinks or dot/empty path components; pinned directory descriptors at every level. |
| ALLOWED_SOURCES / download restrictions | Exact `PIXSTRIDE_ALLOWED_HOSTS`, plus per-provider bucket/container allowlists. Arbitrary prefix rules, private destinations, proxy environment variables and remote IPv6 are intentionally unsupported. |
| BIND | `PIXSTRIDE_BIND` IP and `PIXSTRIDE_PORT`; IPv4/IPv6 inbound TCP. `PIXSTRIDE_UNIX_SOCKET` selects Unix instead of TCP, with mode 0600 in an administrator-owned directory not writable by group/others. Existing paths are never unlinked at startup. Healthcheck supports both listeners. |
| CONCURRENCY | `PIXSTRIDE_CONCURRENCY` 1–4; default 1. `PIXSTRIDE_MAX_CLIENTS` defaults to 4, range concurrency–16. Parallel processing requires the native engine. Increasing concurrency above one or clients above four requires `PIXSTRIDE_MEMORY_BUDGET_MIB >= concurrency*1536 + clients*64`, at most 16384. This reserves admission capacity; a container/cgroup remains the hard RSS limit. |
| REQUEST/PROCESSING TIMEOUT | Native `PIXSTRIDE_PROCESS_TIMEOUT` 1–120 seconds, default 10; queue deadline 15 seconds. In-process built-in processing retains structural budgets and has no preemptive CPU deadline. |
| SECRET | `PIXSTRIDE_SECRET` or `_FILE`; Bearer authorization covers health, metrics and image routes. URL signatures are still checked independently. |
| TTL, USE_ETAG, USE_LAST_MODIFIED, LAST_MODIFIED_BUSTER | `PIXSTRIDE_TTL` 0–31536000, default 0 (`no-store`); `PIXSTRIDE_USE_ETAG`, `PIXSTRIDE_USE_LAST_MODIFIED`, `PIXSTRIDE_LAST_MODIFIED_BUSTER`. ETag hashes output bytes; validators are checked after source validation/processing. Cache expiry never outlives the URL. Change the signed cachebuster for processing/configuration changes. |
| ALLOW_ORIGIN | `PIXSTRIDE_ALLOW_ORIGIN`; one explicit CORS value. OPTIONS preflight permits GET/HEAD and selected headers without exposing image data; other methods/headers/origins are refused. |
| AUTO_WEBP, AUTO_AVIF, AUTO_JXL | `PIXSTRIDE_AUTO_WEBP`, `PIXSTRIDE_AUTO_AVIF`, `PIXSTRIDE_AUTO_JXL`; off by default, native engine required. Explicit format wins. Explicit accepted MIME types with positive quality are checked in JXL/AVIF/WebP priority order; `Vary: Accept` is emitted. |
| WATERMARK_PATH, WATERMARK_URL, WATERMARK_DATA, WATERMARK_OPACITY | Corresponding `PIXSTRIDE_*`; select exactly one path/URL/Base64 source. URL uses the same host/socket policy as image HTTP fetches. Opacity is 0–1. |
| PNG_QUANTIZE | `PIXSTRIDE_PNG_QUANTIZE=1` for native palette output. The installed libvips/imagequant quantizer is preferred; a bounded Rust RGBA palette encoder provides a fallback. Its NeuQuant output/quality settings are not pixel-equivalent to imagequant. |
| Quality, frame, metadata, geometry defaults | Use the `default` preset and the OSS URL options listed above. |
| Prometheus | `/metrics` on the authenticated service listener; Rust request counts, fixed status classes, active requests and duration histogram. No URL, host, source filename or token labels. A distinct metrics listener/metric names matching Go imgproxy are not provided. |
| OpenTelemetry | Build `telemetry`; set `PIXSTRIDE_OTLP_ENDPOINT` to the complete traces endpoint (for example `http://collector:4318/v1/traces`), optional `PIXSTRIDE_OTLP_AUTHORIZATION[_FILE]`. OTLP/HTTP JSON, 64 queued events, one-second export timeout, drop/failure counters, no URL/headers/source attributes or inbound trace propagation. No retry queue on disk. |

Direct DataDog/New Relic/Honeybadger/Sentry client configuration, Go runtime metrics/pprof and cloud secret-manager configuration are not implemented. Prometheus and an OTLP collector can feed compatible monitoring services, but this is not identical to each vendor's imgproxy integration. HTTP supports GET/HEAD and bounded CORS OPTIONS preflight, one request per connection; terminate TLS and handle compression/connection reuse at a reverse proxy.

## Storage configuration

Enable only the providers used. Credentials support `_FILE` where shown and are reread for each source request; replacing a mounted token file takes effect on the next request. SDK credential providers handle their own service-account/web-identity exchange during that request. Metadata endpoints on private/link-local IPs remain blocked; supply workload credentials through files or a public identity endpoint instead.

| Scheme | Required policy and credentials |
|---|---|
| `s3://bucket/key` | `PIXSTRIDE_USE_S3=1`, `PIXSTRIDE_S3_ALLOWED_BUCKETS`, exact endpoint hostname in allowed hosts; `PIXSTRIDE_S3_REGION`, optional `PIXSTRIDE_S3_ENDPOINT`; `PIXSTRIDE_S3_ACCESS_KEY_ID`, `PIXSTRIDE_S3_SECRET_ACCESS_KEY`, optional `PIXSTRIDE_S3_SESSION_TOKEN` (each accepts `_FILE`). SDK public identity credential flows are available; private instance/ECS metadata access is refused. |
| `gs://bucket/key`, `gcs://bucket/key` | `PIXSTRIDE_USE_GCS=1`, `PIXSTRIDE_GCS_ALLOWED_BUCKETS`; optional `PIXSTRIDE_GCS_ENDPOINT`; `PIXSTRIDE_GCS_CREDENTIAL[_FILE]` (service-account JSON) or `PIXSTRIDE_GCS_CREDENTIAL_PATH`. Data host allowlisted; public Google identity endpoints are allowed for SDK exchange. |
| `abs://container/key` | `PIXSTRIDE_USE_ABS=1`, `PIXSTRIDE_ABS_ALLOWED_CONTAINERS`, `PIXSTRIDE_ABS_NAME`, optional `PIXSTRIDE_ABS_ENDPOINT`; `PIXSTRIDE_ABS_KEY[_FILE]` or `PIXSTRIDE_ABS_SAS[_FILE]`. Public Azure identity flows use the SDK. |
| `swift://container/key` | `PIXSTRIDE_USE_SWIFT=1`, `PIXSTRIDE_SWIFT_ALLOWED_CONTAINERS`, HTTPS `PIXSTRIDE_SWIFT_ENDPOINT`, `PIXSTRIDE_SWIFT_TOKEN[_FILE]`. Supply a rotating token file, or `PIXSTRIDE_SWIFT_AUTH_URL` (complete Keystone v3 tokens endpoint), `PIXSTRIDE_SWIFT_USERNAME[_FILE]`, `PIXSTRIDE_SWIFT_PASSWORD[_FILE]`, `PIXSTRIDE_SWIFT_PROJECT` and optional `PIXSTRIDE_SWIFT_DOMAIN` (default `Default`). Password authentication obtains a fresh project-scoped token for each request; its identity host must also be allowlisted. Storage endpoint selection remains explicit. |

All four SDK paths have injected-transport tests with synthetic credentials. Stat then conditional range-read binds the body to the stat ETag when available, validates the full returned size, and applies the 64 MiB source ceiling and a 15-second operation deadline. Configure regional endpoints correctly: cloud redirects are refused. Credential exchange traffic and connection reuse are not reported as exact source connection counts. These tests do not substitute for deployment checks against an actual account's IAM, TLS, region and credential policy.

## Qualification

Run after `cargo fetch --locked`:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features --locked --offline -- -D warnings
cargo test --locked --offline
cargo test --all-features --locked --offline
cargo build --all-features --locked --offline
sh .github/checks/abi/run-abi-check.sh .
python3 scripts/native_check.py --binary target/debug/pixstride
python3 scripts/server_check.py --binary target/debug/pixstride --requests 100
python3 scripts/robustness.py --binary target/debug/pixstride --cases 150
python3 scripts/compatibility.py --binary target/debug/pixstride --reference-binary /path/to/pinned-imgproxy --report /tmp/compatibility.json
```

Build the reference from the exact source commit in reference.json (`go build -o /path/to/pinned-imgproxy ./cli` with its required Go/libvips versions). The comparison report records the executable SHA-256; the CI checks the checkout commit before building it. Native codecs and reference must use comparable libvips/codec builds to assess pixel differences. Reports/corpora/logs stay outside the repository; only synthetic generators and assertions are committed.

CI adds macOS ARM64/Intel checks, native round trips, HTTP/Unix/OTLP integration, a 100-request concurrent soak and deterministic mutation smoke. The Linux AMD64/ARM64 Docker builds run both feature configurations and real native codec checks before release promotion; `python3 scripts/container_check.py --image pixstride:check` also checks the restricted runtime, modern encoders, authorization, SSRF refusal, health and metrics. Its optional `--remote-url` checks a public HTTPS fixture outside mandatory CI. A configured workflow is not evidence that remote jobs have run successfully. Docker and the other three platform combinations must pass their own jobs before a release is approved.

The mutation runner uses a fixed seed and bounded malformed PNG/GIF/SVG inputs. It catches parent crashes, native crash signals and harness deadlines. It is not a coverage-guided fuzzing campaign. Longer runs use `--cases`/`--requests` up to 10000. Synthetic soak results are correctness evidence, not an equal-quality performance benchmark. The historical README benchmark does not measure this native engine.

## Deliberate differences and remaining limits

- The complete imgproxy environment-variable surface, vendor integrations and credential chains are not drop-in compatible. The explicit mappings and exceptions above are the supported migration contract.
- Native children have Linux 1 GiB address-space limits, CPU/fd/core limits, bounded input/output and wall-clock termination. macOS does not apply the Linux address-space limit. Admission memory reservations estimate capacity; they do not measure total RSS or provide a cross-platform hard memory guarantee.
- The native SVG renderer supports a broader static subset but rejects external assets/fonts/styles/scripts. Embedded ICC profiles retained with `scp:0` cannot currently be combined with native watermark composition; convert to sRGB (`scp:1`) first. HDR support is format/working-space dependent, not arbitrary scene-linear/HDR-transfer-function equivalence.
- GIF/WebP timing/loop cases are checked. Other animated formats, every disposal/blend combination, HDR animations and all metadata/container variants are not exhaustively qualified.
- Dependency/plugin versions, font sets, rounding, compression options and format fallbacks can change outputs. Unsupported combinations fail; inspect the error and the installed capabilities rather than assuming a complete imgproxy replacement.
- PDF/Photoshop/video/RAW previews, per-request watermark images/text, object detection and the Pro internal image cache remain outside the OSS target. No image cache is added.

Reference: [OSS processing options](https://docs.imgproxy.net/usage/processing), [formats](https://docs.imgproxy.net/image_formats_support), [configuration](https://docs.imgproxy.net/configuration/options), [OTLP wire format](https://opentelemetry.io/docs/specs/otlp/).
