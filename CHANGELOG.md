# Changelog

Notable changes are recorded here for users, following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html). During 0.x development, incompatible changes use a new minor version and are called out explicitly.

## [Unreleased]

### Added

- A protected `develop` integration line, separate prerelease channels, automatic merged-branch deletion and a safe local cleanup command.
- Generated Markdown sections, documentation consistency checks and release preparation that synchronizes Cargo versions and the changelog.
- Automatic GitHub release notes from the reviewed changelog after both container architectures pass validation.

### Changed

- Release version tags are immutable. Prereleases never update the stable Docker or GitHub `latest` release, and older releases cannot roll a channel backwards.

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
