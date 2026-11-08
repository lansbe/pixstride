# Security

## Reporting

Report vulnerabilities privately through [GitHub Security Advisories](https://github.com/lansbe/pixstride/security/advisories/new). Do not put secrets, private images, internal URLs, or exploit details in public issues.

<!-- generated:support:start -->
Only the latest stable release receives fixes (currently `0.1.1`). Alpha, beta and release-candidate builds are for testing; no response-time SLA is offered.
<!-- generated:support:end -->

## Deployment boundary

Use signed URLs and an exact source-host allowlist. Generate independent secrets with a cryptographic random generator; the key requires at least 32 bytes and the salt 16 bytes, encoded as hexadecimal. Prefer mounted secret files (`PIXSTRIDE_KEY_FILE`, `PIXSTRIDE_SALT_FILE`) over command-line values. Unsigned requests require an explicit opt-in and are intended for an isolated local trial. In unsigned mode, every request, including health checks, requires a `Host` of `localhost`, `127.0.0.1` or `[::1]` and the configured service port; the port can be omitted only for port 80. If a host port is remapped, the header must still use the internal service port. This check does not authenticate clients. Loopback port publishing alone does not isolate container-network peers; restrict access to trusted local clients. Signed URLs are bearer capabilities and can be replayed; they do not provide expiry or per-user authorization.

Terminate inbound TLS and apply rate, connection and request limits at a reverse proxy. Restrict outbound traffic with a network policy as well as the application's allowlist. Do not mount credentials or unrelated directories into the container. Do not log full request URLs at the proxy. Keep container limits enabled: read-only root filesystem, no Linux capabilities, no new privileges, bounded memory/CPU/processes, and a dedicated non-root user. For managed deployments, configure restart policy and a finite shutdown grace period.

## Implemented controls

- Signing is required unless unsigned mode is explicitly enabled; partial or malformed secret configuration fails startup. Decoded HMAC-SHA256 signatures are compared without an early-exit byte comparison.
- Remote fetches require exact allowed hostnames, HTTP(S), ports 80/443 and public IPv4 destinations. The address supplied to the actual socket-opening callback is checked, including redirects and retries. IPv6 is currently disabled. Proxy environment settings are ignored, certificates and hostnames are verified, and HTTPS downgrades are rejected.
- Redirects are manually checked (at most four). Download time is bounded across redirects, source bodies to 64 MiB and upstream headers to 64 KiB. Connections may be reused; image bodies are not cached.
- Four admitted requests, one image-processing operation at a time, bounded overload handling, 16 KiB request headers, a five-second header deadline and a fifteen-second total response-write deadline. GET only, no request bodies, one request per connection.
- Images are bounded to 50 million pixels; individual aligned buffers to 512 MiB. Local reads use a pinned directory descriptor, reject symlinks/non-regular files, and detect ordinary concurrent changes. Local sources are disabled unless configured.
- Unsupported formats, features and color profiles fail explicitly. Source URLs and signing secrets are not logged by the service. Responses use `no-store`, `nosniff` and `no-referrer`.

## Source and processing limits

These limits can reject otherwise valid inputs, even when the general size and pixel limits are met:

- Each DEFLATE stream is limited to 16,384 blocks, including streams used for PNG, TIFF and ICC data. JPEG parsing is limited to 4,096 Huffman table definitions; progressive JPEG decoding is limited to 512 scans and 8,388,608 total block visits across scans, including interleaved padding.
- TIFF strip lengths are limited to 64 MiB in total, counting overlapping ranges each time. SVG raster work has a 200,000,000-unit budget that includes full-area mask passes. A configured watermark file is limited to 16 MiB when read.
- Filtered resizing limits `source_height * output_width` to 16,000,000 pixels and `source_pixels + 2 * (source_height * output_width) + output_pixels` to 100,000,000. This is an admission estimate, not an exact operation count or a CPU deadline.

## Release workflow

The release workflow is configured to stage each platform image by digest, scan and smoke-test that exact platform manifest, then assemble an index from the tested digests. It checks the index and associated SBOM and provenance attestation descriptors before moving the version and commit-SHA tags to the verified index. Promotions are serialized and cannot move a channel backwards according to published SemVer releases. Prereleases only update their own `alpha`, `beta` or `rc` channel, never `latest`. The tag must match Cargo and the dated changelog, and belong to `master` for stable releases or `develop` for prereleases. Existing version tags and GitHub releases are never replaced. The attestation checks do not verify a signature or independently establish the builder's trustworthiness.

Platform images are pushed by digest before validation and may be publicly pullable if the package is public. They are not release-approved until the checks and promotion succeed. GitHub release notes are published from the reviewed changelog only after container validation succeeds. The workflow definition alone does not prove that any particular release completed those checks.

## Residual limits

This early release contains custom parsers, unsafe SIMD/FFI and custom cryptographic code. Defensive tests and resource limits are not a formal audit or proof of memory safety. Processing has no preemptive per-image CPU deadline; a costly input can occupy the image worker. Container CPU quotas limit rate, not total work. Memory limits may terminate a process rather than produce an HTTP error. Use trusted source hosts, admission/rate controls and supervision; do not offer an unrestricted anonymous transformation service.

There is no exhaustive coverage-guided fuzzing claim, full imgproxy compatibility, or equal-quality performance guarantee. Track upstream Rust, libcurl and operating-system updates and rebuild the image; pinned base digests must be updated to receive fixes.

Design references: [OWASP SSRF prevention](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html), [Docker build practices](https://docs.docker.com/build/building/best-practices/), [GitHub Actions security](https://docs.github.com/en/actions/reference/security/secure-use).
