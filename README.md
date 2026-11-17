# pixstride

A bounded image transformation server written in Rust, with a built-in codec engine and an optional libvips worker. No image cache. HTTPS uses the system libcurl. The project is MIT licensed; dependencies retain their own licenses.

## Run

Local trial with an explicitly allowed public image host:

<!-- generated:run:start -->
```sh
docker run --rm --name pixstride -p 127.0.0.1:8080:8080 \
  --read-only --cap-drop=ALL --security-opt=no-new-privileges \
  --memory=512m --pids-limit=64 --cpus=2 \
  -e PIXSTRIDE_ALLOW_UNSAFE=1 -e PIXSTRIDE_ALLOWED_HOSTS=upload.wikimedia.org \
  ghcr.io/lansbe/pixstride:0.4.0
```
<!-- generated:run:end -->

Open this generic example to resize a public sample image:

```text
http://localhost:8080/unsafe/rs:fit:128:128/plain/https://upload.wikimedia.org/wikipedia/commons/a/a9/Example.jpg@png
```

In unsigned mode, every request, including `/health`, must use `localhost`, `127.0.0.1` or `[::1]` in the `Host` header with the service port (`PIXSTRIDE_PORT`, default 8080). The port may be omitted only when the service port is 80. A Host check does not authenticate clients: loopback port publishing alone does not isolate other containers on the same network.

`/health` is the health endpoint. Every image request reads its source again. Local files require an explicit read-only mount and `PIXSTRIDE_LOCAL_ROOT`; nested relative regular files are accepted, and each path component is opened without following symlinks.

For deployment, omit `PIXSTRIDE_ALLOW_UNSAFE`, configure independent random hexadecimal `PIXSTRIDE_KEY` (at least 32 bytes) and `PIXSTRIDE_SALT` (at least 16 bytes), and keep an exact `PIXSTRIDE_ALLOWED_HOSTS` allowlist. Secrets also accept `_FILE` variables for mounted secret files. Place the service behind a TLS reverse proxy with rate limits. See [SECURITY.md](SECURITY.md).

Generate a signed path using the running container's configured secrets:

```sh
docker exec pixstride pixstride sign '/rs:fit:320:240/plain/https://images.example.org/sample.jpg@png'
```

Replace the example host with your own allowed image host.

<!-- generated:configuration:start -->
Configuration names: `PIXSTRIDE_ABS_ALLOWED_CONTAINERS`, `PIXSTRIDE_ABS_DENIED_CONTAINERS`, `PIXSTRIDE_ABS_ENDPOINT`, `PIXSTRIDE_ABS_KEY`, `PIXSTRIDE_ABS_KEY_FILE`, `PIXSTRIDE_ABS_NAME`, `PIXSTRIDE_ABS_SAS`, `PIXSTRIDE_ABS_SAS_FILE`, `PIXSTRIDE_AIRBRAKE_ENDPOINT`, `PIXSTRIDE_AIRBRAKE_ENVIRONMENT`, `PIXSTRIDE_AIRBRAKE_PROJECT_ID`, `PIXSTRIDE_AIRBRAKE_PROJECT_KEY`, `PIXSTRIDE_ALLOWED_HOSTS`, `PIXSTRIDE_ALLOWED_PROCESSING_OPTIONS`, `PIXSTRIDE_ALLOWED_PROCESSING_OPTIONS_FILE`, `PIXSTRIDE_ALLOWED_SOURCES`, `PIXSTRIDE_ALLOWED_SOURCES_FILE`, `PIXSTRIDE_ALLOWED_SOURCE_PORTS`, `PIXSTRIDE_ALLOW_LINK_LOCAL_SOURCE_ADDRESSES`, `PIXSTRIDE_ALLOW_LOOPBACK_SOURCE_ADDRESSES`, `PIXSTRIDE_ALLOW_METADATA_CREDENTIALS`, `PIXSTRIDE_ALLOW_ORIGIN`, `PIXSTRIDE_ALLOW_PRIVATE_SOURCE_ADDRESSES`, `PIXSTRIDE_ALLOW_SECURITY_OPTIONS`, `PIXSTRIDE_ALLOW_UNSAFE`, `PIXSTRIDE_ALWAYS_RASTERIZE_SVG`, `PIXSTRIDE_ARGUMENTS_SEPARATOR`, `PIXSTRIDE_AUTO_AVIF`, `PIXSTRIDE_AUTO_JXL`, `PIXSTRIDE_AUTO_ROTATE`, `PIXSTRIDE_AUTO_WEBP`, `PIXSTRIDE_AVIF_SPEED`, `PIXSTRIDE_BASE64_URL_INCLUDES_FILENAME`, `PIXSTRIDE_BASE_URL`, `PIXSTRIDE_BASE_URL_FILE`, `PIXSTRIDE_BIND`, `PIXSTRIDE_BUGSNAG_ENDPOINT`, `PIXSTRIDE_BUGSNAG_KEY`, `PIXSTRIDE_BUGSNAG_STAGE`, `PIXSTRIDE_CACHE_CONTROL_PASSTHROUGH`, `PIXSTRIDE_CLOUD_ROOT_CA_FILE`, `PIXSTRIDE_CLOUD_WATCH_ACCESS_KEY_ID`, `PIXSTRIDE_CLOUD_WATCH_ENDPOINT`, `PIXSTRIDE_CLOUD_WATCH_NAMESPACE`, `PIXSTRIDE_CLOUD_WATCH_REGION`, `PIXSTRIDE_CLOUD_WATCH_SECRET_ACCESS_KEY`, `PIXSTRIDE_CLOUD_WATCH_SERVICE_NAME`, `PIXSTRIDE_CLOUD_WATCH_SESSION_TOKEN`, `PIXSTRIDE_CONCURRENCY`, `PIXSTRIDE_COOKIE_BASE_URL`, `PIXSTRIDE_COOKIE_PASSTHROUGH`, `PIXSTRIDE_COOKIE_PASSTHROUGH_ALL`, `PIXSTRIDE_DATADOG_ENDPOINT`, `PIXSTRIDE_DATADOG_KEY`, `PIXSTRIDE_DATADOG_KEY_FILE`, `PIXSTRIDE_DOWNLOAD_TIMEOUT`, `PIXSTRIDE_ENABLE_CLIENT_HINTS`, `PIXSTRIDE_ENABLE_DEBUG_HEADERS`, `PIXSTRIDE_ENFORCE_AVIF`, `PIXSTRIDE_ENFORCE_JXL`, `PIXSTRIDE_ENFORCE_THUMBNAIL`, `PIXSTRIDE_ENFORCE_WEBP`, `PIXSTRIDE_ENGINE`, `PIXSTRIDE_ENV_ALLOW_METADATA_CREDENTIALS`, `PIXSTRIDE_ENV_AWS_SECRET_ENDPOINT`, `PIXSTRIDE_ENV_AWS_SECRET_ID`, `PIXSTRIDE_ENV_AWS_SECRET_REGION`, `PIXSTRIDE_ENV_AWS_SECRET_VERSION_ID`, `PIXSTRIDE_ENV_AWS_SECRET_VERSION_STAGE`, `PIXSTRIDE_ENV_AWS_SSM_PARAMETERS_ENDPOINT`, `PIXSTRIDE_ENV_AWS_SSM_PARAMETERS_PATH`, `PIXSTRIDE_ENV_AWS_SSM_PARAMETERS_REGION`, `PIXSTRIDE_ENV_GCP_ACCESS_TOKEN`, `PIXSTRIDE_ENV_GCP_ACCESS_TOKEN_FILE`, `PIXSTRIDE_ENV_GCP_ENDPOINT`, `PIXSTRIDE_ENV_GCP_KEY`, `PIXSTRIDE_ENV_GCP_KEY_FILE`, `PIXSTRIDE_ENV_GCP_SECRET_ID`, `PIXSTRIDE_ENV_GCP_SECRET_PROJECT_ID`, `PIXSTRIDE_ENV_GCP_SECRET_VERSION_ID`, `PIXSTRIDE_ENV_GCP_TOKEN_ENDPOINT`, `PIXSTRIDE_ENV_LOCAL_FILE_PATH`, `PIXSTRIDE_ENV_ROOT_CA_FILE`, `PIXSTRIDE_ETAG_BUSTER`, `PIXSTRIDE_FALLBACK_IMAGE_DATA`, `PIXSTRIDE_FALLBACK_IMAGE_HTTP_CODE`, `PIXSTRIDE_FALLBACK_IMAGE_PATH`, `PIXSTRIDE_FALLBACK_IMAGE_TTL`, `PIXSTRIDE_FALLBACK_IMAGE_URL`, `PIXSTRIDE_FORMAT_QUALITY`, `PIXSTRIDE_GCS_ALLOWED_BUCKETS`, `PIXSTRIDE_GCS_CREDENTIAL`, `PIXSTRIDE_GCS_CREDENTIAL_FILE`, `PIXSTRIDE_GCS_CREDENTIAL_PATH`, `PIXSTRIDE_GCS_DENIED_BUCKETS`, `PIXSTRIDE_GCS_ENDPOINT`, `PIXSTRIDE_HEALTH_CHECK_PATH`, `PIXSTRIDE_HONEYBADGER_ENDPOINT`, `PIXSTRIDE_HONEYBADGER_ENV`, `PIXSTRIDE_HONEYBADGER_KEY`, `PIXSTRIDE_IGNORE_SSL_VERIFICATION`, `PIXSTRIDE_JPEG_PROGRESSIVE`, `PIXSTRIDE_JXL_EFFORT`, `PIXSTRIDE_KEEP_ALIVE_TIMEOUT`, `PIXSTRIDE_KEEP_COPYRIGHT`, `PIXSTRIDE_KEY`, `PIXSTRIDE_KEY_FILE`, `PIXSTRIDE_LAST_MODIFIED_BUSTER`, `PIXSTRIDE_LOCAL_ROOT`, `PIXSTRIDE_MAX_ANIMATION_FRAMES`, `PIXSTRIDE_MAX_ANIMATION_FRAME_RESOLUTION`, `PIXSTRIDE_MAX_CLIENTS`, `PIXSTRIDE_MAX_REDIRECTS`, `PIXSTRIDE_MAX_RESULT_DIMENSION`, `PIXSTRIDE_MAX_SRC_FILE_SIZE`, `PIXSTRIDE_MAX_SRC_RESOLUTION`, `PIXSTRIDE_MEMORY_BUDGET_MIB`, `PIXSTRIDE_NEW_RELIC_APP_NAME`, `PIXSTRIDE_NEW_RELIC_ENDPOINT`, `PIXSTRIDE_NEW_RELIC_KEY`, `PIXSTRIDE_NEW_RELIC_KEY_FILE`, `PIXSTRIDE_ONLY_PRESETS`, `PIXSTRIDE_OTLP_AUTHORIZATION`, `PIXSTRIDE_OTLP_AUTHORIZATION_FILE`, `PIXSTRIDE_OTLP_CLIENT_CERT`, `PIXSTRIDE_OTLP_CLIENT_KEY`, `PIXSTRIDE_OTLP_ENDPOINT`, `PIXSTRIDE_OTLP_LOGS_ENDPOINT`, `PIXSTRIDE_OTLP_METRICS_ENDPOINT`, `PIXSTRIDE_OTLP_PROPAGATE_EXTERNAL`, `PIXSTRIDE_OTLP_SERVER_CERT`, `PIXSTRIDE_PATH_PREFIX`, `PIXSTRIDE_PNG_INTERLACED`, `PIXSTRIDE_PNG_QUANTIZATION_COLORS`, `PIXSTRIDE_PNG_QUANTIZE`, `PIXSTRIDE_PORT`, `PIXSTRIDE_PREFERRED_FORMATS`, `PIXSTRIDE_PRESERVE_HDR`, `PIXSTRIDE_PRESETS`, `PIXSTRIDE_PRESETS_FILE`, `PIXSTRIDE_PROCESS_TIMEOUT`, `PIXSTRIDE_PROMETHEUS_BIND`, `PIXSTRIDE_PROMETHEUS_NAMESPACE`, `PIXSTRIDE_QUALITY`, `PIXSTRIDE_S3_ACCESS_KEY_ID`, `PIXSTRIDE_S3_ACCESS_KEY_ID_FILE`, `PIXSTRIDE_S3_ALLOWED_BUCKETS`, `PIXSTRIDE_S3_ASSUME_ROLE_ARN`, `PIXSTRIDE_S3_ASSUME_ROLE_ARN_FILE`, `PIXSTRIDE_S3_ASSUME_ROLE_EXTERNAL_ID`, `PIXSTRIDE_S3_ASSUME_ROLE_EXTERNAL_ID_FILE`, `PIXSTRIDE_S3_BUCKET_REGIONS`, `PIXSTRIDE_S3_DENIED_BUCKETS`, `PIXSTRIDE_S3_ENDPOINT`, `PIXSTRIDE_S3_ENDPOINT_USE_PATH_STYLE`, `PIXSTRIDE_S3_KMS_ENDPOINT`, `PIXSTRIDE_S3_KMS_REGION`, `PIXSTRIDE_S3_REGION`, `PIXSTRIDE_S3_SECRET_ACCESS_KEY`, `PIXSTRIDE_S3_SECRET_ACCESS_KEY_FILE`, `PIXSTRIDE_S3_SESSION_TOKEN`, `PIXSTRIDE_S3_SESSION_TOKEN_FILE`, `PIXSTRIDE_S3_USE_DECRYPTION_CLIENT`, `PIXSTRIDE_SALT`, `PIXSTRIDE_SALT_FILE`, `PIXSTRIDE_SANITIZE_SVG`, `PIXSTRIDE_SECRET`, `PIXSTRIDE_SECRET_FILE`, `PIXSTRIDE_SENTRY_DSN`, `PIXSTRIDE_SENTRY_DSN_FILE`, `PIXSTRIDE_SENTRY_ENVIRONMENT`, `PIXSTRIDE_SENTRY_RELEASE`, `PIXSTRIDE_SET_CANONICAL_HEADER`, `PIXSTRIDE_SIGNATURE_SIZE`, `PIXSTRIDE_SKIP_PROCESSING_FORMATS`, `PIXSTRIDE_SOURCE_URL_QUERY_SEPARATOR`, `PIXSTRIDE_STRIP_COLOR_PROFILE`, `PIXSTRIDE_STRIP_METADATA`, `PIXSTRIDE_SWIFT_ALLOWED_CONTAINERS`, `PIXSTRIDE_SWIFT_API_KEY`, `PIXSTRIDE_SWIFT_API_KEY_FILE`, `PIXSTRIDE_SWIFT_AUTH_URL`, `PIXSTRIDE_SWIFT_AUTH_VERSION`, `PIXSTRIDE_SWIFT_DENIED_CONTAINERS`, `PIXSTRIDE_SWIFT_DOMAIN`, `PIXSTRIDE_SWIFT_ENDPOINT`, `PIXSTRIDE_SWIFT_PASSWORD`, `PIXSTRIDE_SWIFT_PASSWORD_FILE`, `PIXSTRIDE_SWIFT_PROJECT`, `PIXSTRIDE_SWIFT_PROJECT_DOMAIN`, `PIXSTRIDE_SWIFT_REGION`, `PIXSTRIDE_SWIFT_TENANT`, `PIXSTRIDE_SWIFT_TOKEN`, `PIXSTRIDE_SWIFT_TOKEN_FILE`, `PIXSTRIDE_SWIFT_USERNAME`, `PIXSTRIDE_SWIFT_USERNAME_FILE`, `PIXSTRIDE_SWIFT_USER_DOMAIN`, `PIXSTRIDE_TRUSTED_SIGNATURES`, `PIXSTRIDE_TRUSTED_SIGNATURES_FILE`, `PIXSTRIDE_TTL`, `PIXSTRIDE_UNIX_SOCKET`, `PIXSTRIDE_URL_REPLACEMENTS`, `PIXSTRIDE_URL_REPLACEMENTS_FILE`, `PIXSTRIDE_USER_AGENT`, `PIXSTRIDE_USE_ABS`, `PIXSTRIDE_USE_ETAG`, `PIXSTRIDE_USE_GCS`, `PIXSTRIDE_USE_LAST_MODIFIED`, `PIXSTRIDE_USE_LINEAR_COLORSPACE`, `PIXSTRIDE_USE_S3`, `PIXSTRIDE_USE_SWIFT`, `PIXSTRIDE_WATERMARK_DATA`, `PIXSTRIDE_WATERMARK_OPACITY`, `PIXSTRIDE_WATERMARK_PATH`, `PIXSTRIDE_WATERMARK_URL`, `PIXSTRIDE_WEBP_EFFORT`, `PIXSTRIDE_WEBP_PRESET`. See [SECURITY.md](SECURITY.md) for deployment requirements.
<!-- generated:configuration:end -->

## Scope

The stable container includes the following features. Optional build features are enabled in the published image; source builds select them with Cargo features.

- Built-in input/output codecs and bounded transforms remain available without libvips.
- Optional native engine: PNG/JPEG/WebP/GIF/TIFF/AVIF/HEIC/JPEG XL, BMP/ICO bridges, broader static SVG, 16-bit gray/RGB/alpha, ICC/metadata handling and bounded GIF/WebP animation.
- OSS URL options include presets, expiration, key rotation, DPR/zoom/minimum dimensions, canvas extension, smart/fractional crop, repeated watermarks, quality per format and bounded size targeting.
- GET/HEAD, opt-in bounded keep-alive/pipelining, Bearer authorization, opt-in modern format negotiation, response validators, cache TTL bounded by URL expiration, UTF-8 filenames, CORS, Prometheus and optional OTLP/HTTP traces and request-local source cookies with private response caching.
- Optional S3/KMS V2/V3 client-side decryption with authenticated in-place processing and verified TLS roots.
- Optional staged AWS Secrets Manager/SSM, Google Secret Manager and local configuration files; S3/GCS versions and Swift v1/v2/v3 credentials/catalogs.
- Optional S3/GCS/Azure/Swift readers; nested local files opened without following symlinks. Inbound IPv4/IPv6 TCP or a private Unix socket; bounded admission and optional native concurrency with explicit memory reservations.

Version 0.4.0 source builds default to `PIXSTRIDE_ENGINE=auto`: validated opaque PNG identities keep their built-in path, while other eligible transforms use reusable isolated workers when `native-codecs` is enabled. `PIXSTRIDE_ENGINE=builtin` selects legacy routing; `native` forces the worker. PNG compression uses libdeflate and runtime-accelerated CRC32, eligible PNG input uses a bounded Rust decoder with explicit Adler32/CRC checks, source and raster ownership avoids redundant native copies, and JPEG downscaling starts during decoding; eligible fontless SVG-to-PNG requests use resvg inside the bounded worker. Eligible plain RGBA8 PNG-to-JPEG downscaling uses bounded SIMD premultiplied Lanczos3. Inside the isolated worker, fully validated plain RGB8 PNG requests with unchanged geometry can return the owned source representation without recompression; metadata/alpha/encoding changes keep the full route. Builds retain the system allocator. GNU Linux native workers use one glibc arena and base pages, with their deadlines, address-space ceiling and recycling policy intact. Images and libvips operations are not cached. Codec/routing changes can change pixels, encoded bytes and ETags; change the signed cachebuster when upgrading. Eligible TIFF input streams through the worker; bounded cropped JPEG input finishes decoding before geometry, and eligible JPEG filter output streams through up to 32-row strips, capped at 4 MiB per strip. The published 0.3.0 images and benchmark evidence below retain their original behavior.

See [COMPATIBILITY.md](COMPATIBILITY.md) for the complete option/configuration matrix, build features, tests and differences. Pixstride is **not a complete drop-in replacement for imgproxy**. Native plugins and supported high-precision combinations vary by deployment; use `pixstride capabilities` and the native checks. HTTP/cloud fetches retain exact-host and port policies, with public IPv4/IPv6 by default and explicit opt-ins for private address classes. Native deadlines do not apply to the in-process built-in path.

Bearer and signed URL failures now use OSS status 403; malformed or disabled processing URLs use 404. HTTP framing errors remain 400.

Version 0.1.1 introduced work limits that can reject expensive valid inputs. The native path also has process/animation/output bounds; see [SECURITY.md](SECURITY.md).

## Benchmarks

The current reusable suite is **benchmark v2: 1,000 distinct workloads**, ordered into ten nominal difficulty tiers. It covers 20 processing families, twelve static input representations/content types and five output codecs, with larger images, alpha, entropy-heavy textures, extreme aspect ratios and long geometry/filter chains. It compares default and forced-native Pixstride against imgproxy on native Linux x86_64 and ARM64. See [BENCHMARKS_V2.md](BENCHMARKS_V2.md) for coverage, resources, fixed quality budgets, complete-cohort rules and rerun commands.

Unreleased development corrects opaque-background and large identity-geometry rejections, mirrored pixelation, native BMP/JPEG alpha handling, inherited PNG palettes and WebP/SVG reduced decoding. Linux candidate builds pin libvips 8.18.7, quantizr 1.4.3, libheif 1.23.7 and AOM 3.15.3. The [native preflight](https://github.com/lansbe/pixstride/actions/runs/44029890033) passes all 1,000 cases in default and forced-native modes on both architectures, with zero engine or pixel-budget failures. Full three-repetition timings remain pending. The [candidate workflow](.github/workflows/benchmark-v2-candidate.yml) measures source builds separately; the published 0.4.0 baseline below retains its original failures.

Opaque BI_RGB24/32 BMP input now stays in owned RGB storage through native processing, avoiding synthetic alpha work and an unnecessary AVIF alpha plane. Real ICO/PNG transparency is preserved. Numerical rounding, encoded bytes and ETags can change; each source optimization requires a new complete measurement cohort.

SmartCrop and native filters materialize eligible byte rasters once at their processing boundary to avoid repeated upstream rendering. Materialization uses the existing 64-MiB raster limit; higher-precision and larger images keep their typed processing route.

Complete opaque 8-bit PNG palettes decode into owned RGB while retaining inherited palette encoding. Transparent, packed, interlaced and color-managed palettes use the native codec path; checksum and source-size validation still apply.

New collections use **protocol v2.1**: the shared per-case observation budget is 1,800 seconds instead of 600. The first full v2.0 campaign exceeded 600 seconds on three expensive workloads, including imgproxy. All workloads, samples, pixel budgets, service limits and victory criteria stay fixed; original v2.0 observations and their failures are retained under their original protocol.

<!-- benchmark-v2-candidate:summary:start -->
Current benchmark **v2: 1,000 distinct workloads** — **performance not measured**. Every workload and every service/quality failure is retained.

**UNRELEASED candidate**, package version 0.4.0 / imgproxy 4.0.24. Source `8c292ee56eb4373ddce1b16a93af8c5f71da0e51`; tree `7c41d29ddadfc2126dfea8e157b56edfd6a4d9ef`; Dockerfile SHA256 `d472e1042003693278b9371d32b06b1b57692c4a9dd5ac6c06fe6e6c00b54eae`; Cargo.lock SHA256 `1218cb3f213e1ca4108e22534419f415a08b707cb38e8d04af9a01ee6673567a`. amd64 local image `sha256:694c73c1e4a6c7bb69483575f313a8773d88dfdf2881da8f340a4f10aea8eb57`. arm64 local image `sha256:f3534b5a20c2eec1eee5cec2d39740af150d7c6bc386d7ca7bcf0db0421a82bb`. Harness `8c292ee56eb4373ddce1b16a93af8c5f71da0e51`; campaign started `2026-11-17T06:29:34.696191+00:00`. Retained accepted timed responses: **0** (decoded preflight only).

| Linux | Mode | Valid reference cases | Completed decoded trials | Within pixel budgets | Performance |
|---|---|---:|---:|---:|---:|
| amd64 | default | 1000/1000 | 1000/1000 | 1000/1000 | not measured |
| amd64 | forced-native | 1000/1000 | 1000/1000 | 1000/1000 | not measured |
| arm64 | default | 1000/1000 | 1000/1000 | 1000/1000 | not measured |
| arm64 | forced-native | 1000/1000 | 1000/1000 | 1000/1000 | not measured |

Numerical pixel budgets do not certify perceptual equivalence. No failure counts as a fast response. Nominal difficulty is fixed before measurements and does not guarantee runtime order. Raw reports, hashes and all outcomes: [V2 candidate-preflight index](benchmarks/results/v2/candidate-preflight-index.json). Protocol, tier coverage and rerun commands: [BENCHMARKS_V2.md](BENCHMARKS_V2.md).
<!-- benchmark-v2-candidate:summary:end -->

<details>
<summary>Published 0.4.0 preflight baseline</summary>

<!-- benchmark-v2:summary:start -->
Current benchmark **v2: 1,000 distinct workloads** — **performance not measured**. Every workload and every service/quality failure is retained.

Pixstride 0.4.0 / imgproxy 4.0.24; published image `ghcr.io/lansbe/pixstride:0.4.0@sha256:eed8c44e8ad694d78eb450ec67b674d4bb3d6185039d969b673f7f3164e22690`. Runtime source `98868fa66288a2b27a24116f29f82ece11a9cef2`; harness `8222ceb6bc739171bf98b92121a7fe96a03f5dfa`. Retained accepted timed responses: **0** (decoded preflight only).

| Linux | Mode | Valid reference cases | Completed decoded trials | Within pixel budgets | Performance |
|---|---|---:|---:|---:|---:|
| amd64 | default | 1000/1000 | 967/1000 | 826/1000 | not measured |
| amd64 | forced-native | 1000/1000 | 967/1000 | 826/1000 | not measured |
| arm64 | default | 1000/1000 | 967/1000 | 826/1000 | not measured |
| arm64 | forced-native | 1000/1000 | 967/1000 | 826/1000 | not measured |

Numerical pixel budgets do not certify perceptual equivalence. No failure counts as a fast response. Nominal difficulty is fixed before measurements and does not guarantee runtime order. Raw reports, hashes and all outcomes: [V2 preflight index](benchmarks/results/v2/preflight-index.json). Protocol, tier coverage and rerun commands: [BENCHMARKS_V2.md](BENCHMARKS_V2.md).
<!-- benchmark-v2:summary:end -->

</details>

<details>
<summary>Historical v1: 25-case results for the published 0.4.0 image</summary>

The published **0.4.0** image was measured on 2026-11-16, on three fresh native Linux VMs per architecture. Both default (`auto`) and forced-native beat imgproxy on CPU/request, HTTP p50/p95 and lifetime peak cgroup memory in **all 25 cases in every run**. Each service has two CPUs and 1 GiB; the client uses separate CPUs. This is a sequential local-source workload on shared VMs. All pixel-agreement checks pass the unchanged envelope; encodings and file sizes can still differ.

<!-- released:summary:start -->
Published Pixstride 0.4.0 ([release](https://github.com/lansbe/pixstride/releases/tag/v0.4.0)) versus imgproxy 4.0.24: **target met** for the complete both-mode target. Every run and every loss is retained.

Image: `ghcr.io/lansbe/pixstride:0.4.0@sha256:eed8c44e8ad694d78eb450ec67b674d4bb3d6185039d969b673f7f3164e22690`. Runtime source: [`98868fa66288`](https://github.com/lansbe/pixstride/commit/98868fa66288a2b27a24116f29f82ece11a9cef2). Harness: [`0fd4f487c543`](https://github.com/lansbe/pixstride/commit/0fd4f487c543a8ad16b9d4f481de93c26527faed). These are published-image measurements; no local source build replaces the release.

| Linux | Mode | Confirmed cases | Runs | CPU ratio range | HTTP p50 range | HTTP p95 range | Peak cgroup range |
|---|---|---:|---:|---:|---:|---:|---:|
| amd64 | default | 25/25 | 3 | 0.0662–0.9202× | 0.0902–0.9376× | 0.0963–0.9150× | 0.1633–0.8425× |
| amd64 | forced-native | 25/25 | 3 | 0.0680–0.9211× | 0.0920–0.9429× | 0.0985–0.9136× | 0.2727–0.8511× |
| arm64 | default | 25/25 | 3 | 0.1078–0.9426× | 0.1316–0.9592× | 0.1291–0.9540× | 0.1474–0.7670× |
| arm64 | forced-native | 25/25 | 3 | 0.1101–0.9410× | 0.1340–0.9677× | 0.1316–0.9802× | 0.2588–0.7953× |

Ratios are Pixstride/imgproxy; lower is better. Both modes must have lower CPU, p50, p95 and peak cgroup memory in every retained run, with at least three independent runs per architecture. Encoded files and pixels can differ; the published agreement envelope is checked, without certifying perceptual equivalence. See [all cases and limits](BENCHMARKS.md) and [raw evidence](benchmarks/results/released-index.json).

Median per-case reductions (median of three runs per case, then median of 25 cases; positive means less, negative means more; not a workload-weighted average):

| Linux | Mode | CPU/request | HTTP p50 | HTTP p95 | Peak cgroup memory |
|---|---|---:|---:|---:|---:|
| amd64 | default | +45.1% | +31.7% | +30.1% | +38.5% |
| amd64 | forced-native | +44.6% | +31.1% | +29.2% | +38.1% |
| arm64 | default | +18.5% | +15.0% | +16.0% | +42.7% |
| arm64 | forced-native | +19.4% | +15.4% | +17.6% | +43.9% |
<!-- released:summary:end -->

<details>
<summary>Historical pre-release cohorts and the published 0.3.0 baseline</summary>

<!-- observed:summary:start -->
Retained pre-release source cohort: **target not met**. Every retained run is included; these observations do not replace the historical qualified snapshot.

Unreleased [candidate `20f202a5b5e1`](https://github.com/lansbe/pixstride/commit/20f202a5b5e17eb0d8e61795e87d04a77a0cac77); default and forced-native, all retained native Linux runs. The published 0.3.0 baseline below is unchanged.

| Linux | Mode | Confirmed cases | Runs | CPU ratio range | HTTP p50 range | HTTP p95 range | Peak cgroup range |
|---|---|---:|---:|---:|---:|---:|---:|
| amd64 | default | 25/25 | 3 | 0.0734–0.9003× | 0.0983–0.9358× | 0.0993–0.9548× | 0.1651–0.8336× |
| amd64 | forced-native | 25/25 | 3 | 0.0752–0.9021× | 0.1007–0.9360× | 0.1008–0.9358× | 0.2698–0.8350× |
| arm64 | default | 24/25 | 3 | 0.1075–0.9357× | 0.1315–0.9885× | 0.1304–1.0071× | 0.1562–0.8001× |
| arm64 | forced-native | 25/25 | 3 | 0.1094–0.9382× | 0.1336–0.9883× | 0.1329–0.9676× | 0.2652–0.7959× |

Ratios are Pixstride/imgproxy; lower is better. Both modes must have lower CPU, p50, p95 and peak cgroup memory in every retained run, with at least three independent runs per architecture. Encoded files and pixels can differ; the published agreement envelope is checked, without certifying perceptual equivalence. See [all cases and limits](BENCHMARKS.md) and [raw evidence](benchmarks/results/observed-index.json).
<!-- observed:summary:end -->

The [preceding six-run cohort](benchmarks/results/observed-47fa85d33c40-index.json) remains intact, including its two AMD64 p95 losses. The engine and HTTP measurement code are unchanged between those runs and the final post-review campaign above; timing variation is not evidence of a further speed optimization.


### Historical default-only qualified cohort

<!-- candidate:summary:start -->
Unreleased [candidate `6145080e14a5`](https://github.com/lansbe/pixstride/commit/6145080e14a5e717a7a85ae9fd7174f894e08e39); default routing, all retained native runs. The published 0.3.0 baseline below is unchanged.

| Linux | Confirmed cases | Runs | CPU ratio range | HTTP p50 range | HTTP p95 range | Peak cgroup range |
|---|---:|---:|---:|---:|---:|---:|
| amd64 | 25/25 | 3 | 0.0660–0.9272× | 0.0896–0.9570× | 0.0952–0.9408× | 0.1604–0.9740× |
| arm64 | 25/25 | 3 | 0.1085–0.9601× | 0.1316–0.9711× | 0.1289–0.9741× | 0.1792–0.9995× |

Ratios are Pixstride/imgproxy; lower is better. Counts require lower CPU, p50, p95 and peak cgroup memory in every run, with extra repeats near parity. Different encoding can increase file size. Decoded-pixel agreement is checked against the published envelope; this does not certify perceptual equivalence. See [candidate evidence](benchmarks/results/candidate-index.json) and [all cases](BENCHMARKS.md).
<!-- candidate:summary:end -->

### Historical published 0.3.0 baseline

<!-- benchmark:summary:start -->
Images: Pixstride **0.3.0** / imgproxy **4.0.24**. Per case/mode: 6 balanced batches × 24 timed requests (144 total), after 5 warmups.

| Linux | Pixstride mode | Lower CPU | Lower HTTP p50 | Median CPU ratio | Median p50 ratio | Median peak cgroup ratio | Exact visible pixels |
|---|---|---:|---:|---:|---:|---:|---:|
| amd64 | Pixstride default | 2/25 | 2/25 | 2.468× | 2.629× | 0.377× | 4/25 |
| amd64 | Pixstride native | 0/25 | 0/25 | 2.390× | 2.861× | 0.628× | 14/25 |
| arm64 | Pixstride default | 3/25 | 2/25 | 1.987× | 2.237× | 0.327× | 4/25 |
| arm64 | Pixstride native | 0/25 | 0/25 | 2.803× | 3.173× | 0.548× | 14/25 |
<!-- benchmark:summary:end -->

Ratios are Pixstride / imgproxy; below 1 means less observed CPU, lower HTTP p50 or a lower peak cgroup charge. Counts describe these cases, with equal weight per case. Batch order is balanced across modes. CPU includes native child processes. Peak cgroup memory includes startup, file cache and kernel memory and differs from RSS.

</details>

Outputs can differ in pixels, compression and size even at the same quality setting. These observations do **not** establish an equal-quality speed advantage, a universal memory/latency guarantee, or an AMD64-versus-ARM64 performance claim. The two architectures use different runner CPUs. This public, reproducible suite replaces the previous private 20-case macOS baseline.

</details>

## Versions

`master` is the stable line; `develop` integrates upcoming work. Docker `latest` follows stable releases; `alpha`, `beta` and `rc` are separate testing channels, available after their first successful tagged release. Pin a version or digest for deployment. See the [changelog](CHANGELOG.md) and [release workflow](CONTRIBUTING.md#releases).

Version 0.3.0 integrates the tested `develop` changes into `master`. Before upgrading from 0.2.x, set `PIXSTRIDE_SIGNATURE_SIZE` to the same 16–32-byte size on every signer and server and regenerate any shorter signed URLs. Source patterns now apply to normalized initial HTTP URLs and every redirect destination. Bearer-protected responses add `Authorization` to `Vary`; purge previously cached protected responses and configure caches to honor this header. Failed response writes close keep-alive connections. See [SECURITY.md](SECURITY.md) for deployment details.

The native PNG palette fallback uses color_quant 2.0.0. Its corrected quantizer can change output pixels, bytes and ETags compared with 1.1.0; change the signed cachebuster or invalidate transformed-image caches on upgrade. See the [dependency migration audit](COMPATIBILITY.md#dependency-upgrade-audit) for scope and validation.

## Build

<!-- generated:build:start -->
Linux and macOS, ARM64 or x86-64; Rust 1.100 and libcurl development files.
<!-- generated:build:end -->

```sh
cargo fetch --locked
cargo build --release --locked --offline
./target/release/pixstride --help
# Optional engine, storage readers and OTLP exporter (libvips development files required):
cargo build --release --all-features --locked --offline
./target/release/pixstride capabilities
PIXSTRIDE_ENGINE=native ./target/release/pixstride process input.png output.avif /unsafe/plain/local:///image@avif
# Or build the container locally:
docker build -t pixstride .
```

Early release: security review and testing do not prove the custom codecs and cryptography safe. [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [MIT](LICENSE)

## Support

<a href="https://buymeacoffee.com/lansbe">
  <img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me a Coffee" height="40">
</a>
