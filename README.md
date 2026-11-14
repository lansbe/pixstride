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
  ghcr.io/lansbe/pixstride:0.1.1
```
<!-- generated:run:end -->

Open this generic example to resize a public sample image:

```text
http://localhost:8080/unsafe/rs:fit:128:128/plain/https://upload.wikimedia.org/wikipedia/commons/a/a9/Example.jpg@png
```

In unsigned mode, every request, including `/health`, must use `localhost`, `127.0.0.1` or `[::1]` in the `Host` header with the service port (`PIXSTRIDE_PORT`, default 8080). The port may be omitted only when the service port is 80. A Host check does not authenticate clients: loopback port publishing alone does not isolate other containers on the same network.

`/health` is the health endpoint. Every image request reads its source again. Local files require an explicit read-only mount and `PIXSTRIDE_LOCAL_ROOT`; nested relative regular files are accepted in Unreleased builds; each path component is opened without following symlinks. Released 0.1.1 accepts flat names only.

For deployment, omit `PIXSTRIDE_ALLOW_UNSAFE`, configure independent random hexadecimal `PIXSTRIDE_KEY` (at least 32 bytes) and `PIXSTRIDE_SALT` (at least 16 bytes), and keep an exact `PIXSTRIDE_ALLOWED_HOSTS` allowlist. Secrets also accept `_FILE` variables for mounted secret files. Place the service behind a TLS reverse proxy with rate limits. See [SECURITY.md](SECURITY.md).

Generate a signed path using the running container's configured secrets:

```sh
docker exec pixstride pixstride sign '/rs:fit:320:240/plain/https://images.example.org/sample.jpg@png'
```

Replace the example host with your own allowed image host.

<!-- generated:configuration:start -->
Configuration names: `PIXSTRIDE_ABS_ALLOWED_CONTAINERS`, `PIXSTRIDE_ABS_ENDPOINT`, `PIXSTRIDE_ABS_KEY`, `PIXSTRIDE_ABS_KEY_FILE`, `PIXSTRIDE_ABS_NAME`, `PIXSTRIDE_ABS_SAS`, `PIXSTRIDE_ABS_SAS_FILE`, `PIXSTRIDE_AIRBRAKE_ENDPOINT`, `PIXSTRIDE_AIRBRAKE_ENVIRONMENT`, `PIXSTRIDE_AIRBRAKE_PROJECT_ID`, `PIXSTRIDE_AIRBRAKE_PROJECT_KEY`, `PIXSTRIDE_ALLOWED_HOSTS`, `PIXSTRIDE_ALLOWED_PROCESSING_OPTIONS`, `PIXSTRIDE_ALLOWED_PROCESSING_OPTIONS_FILE`, `PIXSTRIDE_ALLOWED_SOURCES`, `PIXSTRIDE_ALLOWED_SOURCES_FILE`, `PIXSTRIDE_ALLOWED_SOURCE_PORTS`, `PIXSTRIDE_ALLOW_LINK_LOCAL_SOURCE_ADDRESSES`, `PIXSTRIDE_ALLOW_LOOPBACK_SOURCE_ADDRESSES`, `PIXSTRIDE_ALLOW_ORIGIN`, `PIXSTRIDE_ALLOW_PRIVATE_SOURCE_ADDRESSES`, `PIXSTRIDE_ALLOW_SECURITY_OPTIONS`, `PIXSTRIDE_ALLOW_UNSAFE`, `PIXSTRIDE_ALWAYS_RASTERIZE_SVG`, `PIXSTRIDE_ARGUMENTS_SEPARATOR`, `PIXSTRIDE_AUTO_AVIF`, `PIXSTRIDE_AUTO_JXL`, `PIXSTRIDE_AUTO_ROTATE`, `PIXSTRIDE_AUTO_WEBP`, `PIXSTRIDE_AVIF_SPEED`, `PIXSTRIDE_BASE64_URL_INCLUDES_FILENAME`, `PIXSTRIDE_BASE_URL`, `PIXSTRIDE_BASE_URL_FILE`, `PIXSTRIDE_BIND`, `PIXSTRIDE_BUGSNAG_ENDPOINT`, `PIXSTRIDE_BUGSNAG_KEY`, `PIXSTRIDE_BUGSNAG_STAGE`, `PIXSTRIDE_CACHE_CONTROL_PASSTHROUGH`, `PIXSTRIDE_CLOUD_WATCH_ACCESS_KEY_ID`, `PIXSTRIDE_CLOUD_WATCH_ENDPOINT`, `PIXSTRIDE_CLOUD_WATCH_NAMESPACE`, `PIXSTRIDE_CLOUD_WATCH_REGION`, `PIXSTRIDE_CLOUD_WATCH_SECRET_ACCESS_KEY`, `PIXSTRIDE_CLOUD_WATCH_SERVICE_NAME`, `PIXSTRIDE_CLOUD_WATCH_SESSION_TOKEN`, `PIXSTRIDE_CONCURRENCY`, `PIXSTRIDE_COOKIE_BASE_URL`, `PIXSTRIDE_COOKIE_PASSTHROUGH`, `PIXSTRIDE_COOKIE_PASSTHROUGH_ALL`, `PIXSTRIDE_DATADOG_ENDPOINT`, `PIXSTRIDE_DATADOG_KEY`, `PIXSTRIDE_DATADOG_KEY_FILE`, `PIXSTRIDE_DOWNLOAD_TIMEOUT`, `PIXSTRIDE_ENABLE_CLIENT_HINTS`, `PIXSTRIDE_ENFORCE_AVIF`, `PIXSTRIDE_ENFORCE_JXL`, `PIXSTRIDE_ENFORCE_THUMBNAIL`, `PIXSTRIDE_ENFORCE_WEBP`, `PIXSTRIDE_ENGINE`, `PIXSTRIDE_ETAG_BUSTER`, `PIXSTRIDE_FALLBACK_IMAGE_DATA`, `PIXSTRIDE_FALLBACK_IMAGE_HTTP_CODE`, `PIXSTRIDE_FALLBACK_IMAGE_PATH`, `PIXSTRIDE_FALLBACK_IMAGE_TTL`, `PIXSTRIDE_FALLBACK_IMAGE_URL`, `PIXSTRIDE_FORMAT_QUALITY`, `PIXSTRIDE_GCS_ALLOWED_BUCKETS`, `PIXSTRIDE_GCS_CREDENTIAL`, `PIXSTRIDE_GCS_CREDENTIAL_FILE`, `PIXSTRIDE_GCS_CREDENTIAL_PATH`, `PIXSTRIDE_GCS_ENDPOINT`, `PIXSTRIDE_HEALTH_CHECK_PATH`, `PIXSTRIDE_HONEYBADGER_ENDPOINT`, `PIXSTRIDE_HONEYBADGER_ENV`, `PIXSTRIDE_HONEYBADGER_KEY`, `PIXSTRIDE_IGNORE_SSL_VERIFICATION`, `PIXSTRIDE_JPEG_PROGRESSIVE`, `PIXSTRIDE_JXL_EFFORT`, `PIXSTRIDE_KEEP_ALIVE_TIMEOUT`, `PIXSTRIDE_KEEP_COPYRIGHT`, `PIXSTRIDE_KEY`, `PIXSTRIDE_KEY_FILE`, `PIXSTRIDE_LAST_MODIFIED_BUSTER`, `PIXSTRIDE_LOCAL_ROOT`, `PIXSTRIDE_MAX_ANIMATION_FRAMES`, `PIXSTRIDE_MAX_ANIMATION_FRAME_RESOLUTION`, `PIXSTRIDE_MAX_CLIENTS`, `PIXSTRIDE_MAX_REDIRECTS`, `PIXSTRIDE_MAX_RESULT_DIMENSION`, `PIXSTRIDE_MAX_SRC_FILE_SIZE`, `PIXSTRIDE_MAX_SRC_RESOLUTION`, `PIXSTRIDE_MEMORY_BUDGET_MIB`, `PIXSTRIDE_NEW_RELIC_APP_NAME`, `PIXSTRIDE_NEW_RELIC_ENDPOINT`, `PIXSTRIDE_NEW_RELIC_KEY`, `PIXSTRIDE_NEW_RELIC_KEY_FILE`, `PIXSTRIDE_ONLY_PRESETS`, `PIXSTRIDE_OTLP_AUTHORIZATION`, `PIXSTRIDE_OTLP_AUTHORIZATION_FILE`, `PIXSTRIDE_OTLP_CLIENT_CERT`, `PIXSTRIDE_OTLP_CLIENT_KEY`, `PIXSTRIDE_OTLP_ENDPOINT`, `PIXSTRIDE_OTLP_LOGS_ENDPOINT`, `PIXSTRIDE_OTLP_METRICS_ENDPOINT`, `PIXSTRIDE_OTLP_PROPAGATE_EXTERNAL`, `PIXSTRIDE_OTLP_SERVER_CERT`, `PIXSTRIDE_PATH_PREFIX`, `PIXSTRIDE_PNG_INTERLACED`, `PIXSTRIDE_PNG_QUANTIZATION_COLORS`, `PIXSTRIDE_PNG_QUANTIZE`, `PIXSTRIDE_PORT`, `PIXSTRIDE_PREFERRED_FORMATS`, `PIXSTRIDE_PRESERVE_HDR`, `PIXSTRIDE_PRESETS`, `PIXSTRIDE_PRESETS_FILE`, `PIXSTRIDE_PROCESS_TIMEOUT`, `PIXSTRIDE_PROMETHEUS_BIND`, `PIXSTRIDE_PROMETHEUS_NAMESPACE`, `PIXSTRIDE_QUALITY`, `PIXSTRIDE_S3_ACCESS_KEY_ID`, `PIXSTRIDE_S3_ACCESS_KEY_ID_FILE`, `PIXSTRIDE_S3_ALLOWED_BUCKETS`, `PIXSTRIDE_S3_ENDPOINT`, `PIXSTRIDE_S3_REGION`, `PIXSTRIDE_S3_SECRET_ACCESS_KEY`, `PIXSTRIDE_S3_SECRET_ACCESS_KEY_FILE`, `PIXSTRIDE_S3_SESSION_TOKEN`, `PIXSTRIDE_S3_SESSION_TOKEN_FILE`, `PIXSTRIDE_SALT`, `PIXSTRIDE_SALT_FILE`, `PIXSTRIDE_SANITIZE_SVG`, `PIXSTRIDE_SECRET`, `PIXSTRIDE_SECRET_FILE`, `PIXSTRIDE_SENTRY_DSN`, `PIXSTRIDE_SENTRY_DSN_FILE`, `PIXSTRIDE_SENTRY_ENVIRONMENT`, `PIXSTRIDE_SENTRY_RELEASE`, `PIXSTRIDE_SET_CANONICAL_HEADER`, `PIXSTRIDE_SIGNATURE_SIZE`, `PIXSTRIDE_SKIP_PROCESSING_FORMATS`, `PIXSTRIDE_STRIP_COLOR_PROFILE`, `PIXSTRIDE_STRIP_METADATA`, `PIXSTRIDE_SWIFT_ALLOWED_CONTAINERS`, `PIXSTRIDE_SWIFT_AUTH_URL`, `PIXSTRIDE_SWIFT_DOMAIN`, `PIXSTRIDE_SWIFT_ENDPOINT`, `PIXSTRIDE_SWIFT_PASSWORD`, `PIXSTRIDE_SWIFT_PASSWORD_FILE`, `PIXSTRIDE_SWIFT_PROJECT`, `PIXSTRIDE_SWIFT_TOKEN`, `PIXSTRIDE_SWIFT_TOKEN_FILE`, `PIXSTRIDE_SWIFT_USERNAME`, `PIXSTRIDE_SWIFT_USERNAME_FILE`, `PIXSTRIDE_TRUSTED_SIGNATURES`, `PIXSTRIDE_TRUSTED_SIGNATURES_FILE`, `PIXSTRIDE_TTL`, `PIXSTRIDE_UNIX_SOCKET`, `PIXSTRIDE_URL_REPLACEMENTS`, `PIXSTRIDE_URL_REPLACEMENTS_FILE`, `PIXSTRIDE_USER_AGENT`, `PIXSTRIDE_USE_ABS`, `PIXSTRIDE_USE_ETAG`, `PIXSTRIDE_USE_GCS`, `PIXSTRIDE_USE_LAST_MODIFIED`, `PIXSTRIDE_USE_LINEAR_COLORSPACE`, `PIXSTRIDE_USE_S3`, `PIXSTRIDE_USE_SWIFT`, `PIXSTRIDE_WATERMARK_DATA`, `PIXSTRIDE_WATERMARK_OPACITY`, `PIXSTRIDE_WATERMARK_PATH`, `PIXSTRIDE_WATERMARK_URL`, `PIXSTRIDE_WEBP_EFFORT`, `PIXSTRIDE_WEBP_PRESET`. See [SECURITY.md](SECURITY.md) for deployment requirements.
<!-- generated:configuration:end -->

## Scope

The stable container example above runs the released 0.1.1 behavior. The following additions are **Unreleased** and require building this source; no new image or release has been published.

- Built-in input/output codecs and bounded transforms remain available without libvips.
- Optional native engine: PNG/JPEG/WebP/GIF/TIFF/AVIF/HEIC/JPEG XL, BMP/ICO bridges, broader static SVG, 16-bit gray/RGB/alpha, ICC/metadata handling and bounded GIF/WebP animation.
- OSS URL options include presets, expiration, key rotation, DPR/zoom/minimum dimensions, canvas extension, smart/fractional crop, repeated watermarks, quality per format and bounded size targeting.
- GET/HEAD, opt-in bounded keep-alive/pipelining, Bearer authorization, opt-in modern format negotiation, response validators, cache TTL bounded by URL expiration, UTF-8 filenames, CORS, Prometheus and optional OTLP/HTTP traces and request-local source cookies with private response caching.
- Optional S3/GCS/Azure/Swift readers; nested local files opened without following symlinks. Inbound IPv4/IPv6 TCP or a private Unix socket; bounded admission and optional native concurrency with explicit memory reservations.

See [COMPATIBILITY.md](COMPATIBILITY.md) for the complete option/configuration matrix, build features, tests and differences. Pixstride is **not a complete drop-in replacement for imgproxy**. Native plugins and supported high-precision combinations vary by deployment; use `pixstride capabilities` and the native checks. HTTP/cloud fetches retain exact-host and port policies, with public IPv4/IPv6 by default and explicit opt-ins for private address classes. Native deadlines do not apply to the default in-process built-in engine.

Bearer and signed URL failures now use OSS status 403; malformed or disabled processing URLs use 404. HTTP framing errors remain 400.

Version 0.1.1 introduced work limits that can reject expensive valid inputs. The new native path adds process/animation/output bounds; see [SECURITY.md](SECURITY.md).

## Benchmarks

Historical engine baseline before the Linux port and deployment hardening: native macOS, Apple M5, imgproxy 4.0.21 / libvips 8.18.7, November 2026. Values below are **Rust / imgproxy**. These do not measure the published container.

| Scenario | CPU ms/request | HTTP p50 ms | Peak RSS MiB |
|---|---:|---:|---:|
| Large PNG to BMP | 11.660 / 18.871 | 12.160 / 18.145 | 12.828 / 115.672 |
| Medium JPEG to BMP | 5.160 / 5.964 | 5.463 / 5.713 | 9.016 / 60.844 |
| Large PNG to JPEG | 26.751 / 21.169 | 26.729 / 18.685 | 12.422 / 111.125 |

Across 20 scenarios: lower CPU in 17, lower HTTP median in 16, lower peak RSS in 20 (83-89% less). Six alternating batches of 24 sequential requests per engine, five warmups per batch; CPU is the median of batch averages (process user + system time), HTTP includes the complete loopback response, RSS includes startup. No application image cache; OS page cache was uncontrolled. Internet download time is excluded.

Only 3/20 outputs were pixel-identical across engines. Filters, JPEG chroma sampling and WebP modes differ, so these results do **not** establish an equal-quality speed advantage. The private benchmark corpus and harness are not distributed. No universal latency or memory guarantee is claimed.

## Versions

`master` is the stable line; `develop` integrates upcoming work. Docker `latest` follows stable releases; `alpha`, `beta` and `rc` are separate testing channels, available after their first successful tagged release. Pin a version or digest for deployment. See the [changelog](CHANGELOG.md) and [release workflow](CONTRIBUTING.md#releases).

## Build

<!-- generated:build:start -->
Linux and macOS, ARM64 or x86-64; Rust 1.99 and libcurl development files.
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
