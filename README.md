# pixstride

A small image transformation server written in Rust. No image libraries, external Rust crates, or image cache. HTTPS uses the system libcurl. MIT licensed.

## Run

Local trial with an explicitly allowed public image host:

```sh
docker run --rm --name pixstride -p 127.0.0.1:8080:8080 \
  --read-only --cap-drop=ALL --security-opt=no-new-privileges \
  --memory=512m --pids-limit=64 --cpus=2 \
  -e PIXSTRIDE_ALLOW_UNSAFE=1 -e PIXSTRIDE_ALLOWED_HOSTS=upload.wikimedia.org \
  ghcr.io/lansbe/pixstride:0.1.0
```

Open this generic example to resize a public sample image:

```text
http://localhost:8080/unsafe/rs:fit:128:128/plain/https://upload.wikimedia.org/wikipedia/commons/a/a9/Example.jpg@png
```

`/health` is the health endpoint. Every image request reads its source again. Local files require an explicit read-only mount and `PIXSTRIDE_LOCAL_ROOT`; only flat regular filenames are accepted.

For deployment, omit `PIXSTRIDE_ALLOW_UNSAFE`, configure independent random hexadecimal `PIXSTRIDE_KEY` (at least 32 bytes) and `PIXSTRIDE_SALT` (at least 16 bytes), and keep an exact `PIXSTRIDE_ALLOWED_HOSTS` allowlist. Secrets also accept `_FILE` variables for mounted secret files. Place the service behind a TLS reverse proxy with rate limits. See [SECURITY.md](SECURITY.md).

Generate a signed path using the running container's configured secrets:

```sh
docker exec pixstride pixstride sign '/rs:fit:320:240/plain/https://images.example.org/sample.jpg@png'
```

Replace the example host with your own allowed image host. Optional settings: `PIXSTRIDE_BIND` (native default `127.0.0.1`, container `0.0.0.0`), `PIXSTRIDE_PORT` (8080), `PIXSTRIDE_LOCAL_ROOT`, `PIXSTRIDE_WATERMARK_PATH`, and `PIXSTRIDE_WATERMARK_OPACITY` (0-1).

## Scope

- Input: PNG, baseline/progressive JPEG, lossless WebP (VP8L), BMP/PPM, first-frame GIF, selected TIFF/ICO variants, and a restricted static SVG subset.
- Output: PNG, baseline JPEG, lossless WebP, BMP, GIF, TIFF, ICO.
- Resize, crop, gravity, quarter-turn rotation, flip, padding, background, blur, sharpen, pixelation, trimming, local watermarks, EXIF orientation and supported RGB ICC conversion.
- Uses a subset of imgproxy URL syntax, for example `rs:fit:320:240`, `g:ce`, `q:80`. Unsupported options fail explicitly. This is **not a complete drop-in replacement for imgproxy**. Lossy WebP input, AVIF, HEIC, JPEG XL, animation and general SVG rendering are unsupported.

## Benchmarks

Historical engine baseline before the Linux port and deployment hardening: native macOS, Apple M5, imgproxy 4.0.21 / libvips 8.18.7, November 2026. Values below are **Rust / imgproxy**. These are not measurements of the published container.

| Scenario | CPU ms/request | HTTP p50 ms | Peak RSS MiB |
|---|---:|---:|---:|
| Large PNG to BMP | 11.660 / 18.871 | 12.160 / 18.145 | 12.828 / 115.672 |
| Medium JPEG to BMP | 5.160 / 5.964 | 5.463 / 5.713 | 9.016 / 60.844 |
| Large PNG to JPEG | 26.751 / 21.169 | 26.729 / 18.685 | 12.422 / 111.125 |

Across 20 scenarios: lower CPU in 17, lower HTTP median in 16, lower peak RSS in 20 (83-89% less). Six alternating batches of 24 sequential requests per engine, five warmups per batch; CPU is the median of batch averages (process user + system time), HTTP includes the complete loopback response, RSS includes startup. No application image cache; OS page cache was uncontrolled. Internet download time is excluded.

Only 3/20 outputs were pixel-identical across engines. Filters, JPEG chroma sampling and WebP modes differ, so these results do **not** establish an equal-quality speed advantage. The private benchmark corpus and harness are not distributed. No universal latency or memory guarantee is claimed.

## Build

Linux and macOS, ARM64 or x86-64; Rust 1.99 and libcurl development files:

```sh
cargo build --release --locked --offline
./target/release/pixstride --help
# Or build the container locally:
docker build -t pixstride .
```

Early release: custom codecs and cryptography have not received an independent security audit. [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [MIT](LICENSE)
