# Contributing

Use English names, comments and documentation. Keep pull requests focused and preserve image fidelity, resource bounds and explicit failure for unsupported behavior. Contributions are licensed under MIT.

The code is divided into `src/codecs` (formats and metadata), `src/processing` (geometry/effects/pipeline), `src/network` (HTTP, sources, admission), and `src/security` (signatures). `config.rs`, `buffer.rs`, and `os.rs` own startup, aligned memory, and administrator-selected file reads. Modules should stay below 500 lines; split by responsibility, not arbitrary fragments. Every unsafe operation needs a verifiable bounds, ownership or platform-ABI argument.

Run `cargo fmt --check`, `cargo clippy --locked --offline -- -D warnings`, and build/run the Docker image. Validate supported platforms and compare decoded pixels for codec changes; include bounded synthetic regression cases in the pull request. Never submit personal images, machine paths, secret values, private URLs, research archives or benchmark dumps. Do not add external image libraries or Rust crates without discussing the architecture first.

Benchmarks must state hardware, compiler, output bytes and decoded-pixel differences, cache state, concurrency, CPU accounting, HTTP percentiles and peak RSS. A faster different output is not an equal-quality win. Report security issues privately as described in [SECURITY.md](SECURITY.md).
