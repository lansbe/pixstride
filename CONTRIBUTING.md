# Contributing

Use English names, comments and documentation. Keep pull requests focused and preserve image fidelity, resource bounds and explicit failure for unsupported behavior. Contributions are licensed under MIT.

The code is divided into `src/codecs` (formats and metadata), `src/processing` (geometry/effects/pipeline), `src/network` (HTTP, sources, admission), and `src/security` (signatures). `config.rs`, `buffer.rs`, and `os.rs` own startup, aligned memory, and administrator-selected file reads. First-party modules in `src/` should stay below 500 lines; split by responsibility, not arbitrary fragments. Imported generated dependency code retains its upstream layout and must include license, provenance and a documented minimal patch. Every unsafe operation needs a verifiable bounds, ownership or platform-ABI argument.

Run `cargo fmt --check`, `cargo clippy --locked --offline -- -D warnings`, and build/run the Docker image. Validate supported platforms and compare decoded pixels for codec changes; include bounded synthetic regression cases in the pull request. Never submit personal images, machine paths, secret values, private URLs, private research archives or unsanitized benchmark dumps. Public benchmark evidence follows the restrictions below. External libraries and Rust crates are permitted; document their purpose, license, supported platforms and resource behavior, and commit the updated lockfile.

Benchmarks must state hardware, compiler or immutable tested image/binary identity, output bytes and decoded-pixel differences, cache state, concurrency, CPU accounting, HTTP percentiles and peak memory with its accounting scope. Process RSS and cgroup charges are different metrics and must be named explicitly. A faster different output is not an equal-quality win. Report security issues privately as described in [SECURITY.md](SECURITY.md).

Use obviously fictitious credentials in synthetic tests, without real provider key prefixes or token formats, so fixtures do not trigger secret-scanning alerts.

## Branches and pull requests

`master` is the default, stable branch. `develop` is the integration branch. Both are permanent and protected against deletion and force-pushes. Changes go through pull requests with the documentation check and both container checks passing. Merge commits preserve ancestry across these two lines and make branch cleanup verifiable. No extra permanent `beta` branch is needed.

Create a short-lived descriptive branch from `develop` (for example `feature/png-option`, `fix/header-limit` or `codex/task-name`) and target `develop`. Hotfixes can start from `master`; merge them back into `develop` after release. Keep changes small and use [Conventional Commit](https://www.conventionalcommits.org/en/v1.0.0/) PR titles such as `fix(http): preserve the write deadline`. Include breaking changes and validation evidence in the PR description.

GitHub automatically deletes merged head branches in this repository; permanent branch protection prevents deleting `master` and `develop`. Fork branches remain controlled by their owners. Never delete a branch with unique work or one checked out in another worktree. After merging, synchronize your checkout and clean local merged branches:

```sh
python3 scripts/cleanup_branches.py          # fetch/prune, then preview
python3 scripts/cleanup_branches.py --apply  # delete only eligible local branches
```

The cleanup command checks ancestry against fresh `origin/master` and `origin/develop`; `git branch -d` rechecks the chosen upstream and protects checked-out worktrees. It does not force-delete or remove files. Repository-local `git config fetch.prune true` also removes stale remote-tracking references during fetches.

## Documentation

Project scripts require Python 3.11 or newer (including the standard-library `tomllib`).

Update user-visible behavior, limitations and configuration in the relevant Markdown files in the same PR. Add notable changes under `CHANGELOG.md` / `Unreleased`, grouped as Added, Changed, Deprecated, Removed, Fixed or Security. Explain compatibility changes and migration steps. Do not paste a commit log or unverified performance claims into release notes.

```sh
python3 scripts/project.py sync
python3 scripts/project.py check
python3 -m unittest discover -s .github/checks/project -v
```

Generated blocks synchronize the stable Docker example, Rust requirement, configuration-name inventory and supported stable version from Cargo, the sources and the changelog. CI fails if they drift or local Markdown links break. It also requires changelog and user-documentation updates with runtime changes, and workflow documentation with infrastructure changes. Review is still required: automation cannot verify that prose accurately describes image quality, security guarantees or benchmark results. Edit prose outside generated blocks; never invent results to satisfy a check.

## Rust toolchain updates

The builder and native CI use Rust 1.100.0; Cargo declares Rust 1.100 as the minimum. Install the matching toolchain with `rustup toolchain install 1.100.0 --profile minimal --component rustfmt,clippy`, then use `cargo +1.100.0` for local checks. The Docker builder remains on Alpine 3.24 and is pinned to its multi-platform digest.

The [Rust 1.99.1 and 1.100.0 release notes](https://doc.rust-lang.org/1.100.0/releases.html) and [Cargo 1.100 changelog](https://doc.rust-lang.org/cargo/CHANGELOG.html#cargo-1100-2026-11-12) were reviewed for this upgrade. Rust 1.99.1 fixes C-ABI boolean returns on x86-64, a missing `wcslen` fallback and macOS linker library discovery. Rust 1.100 changes never-type fallback on all editions, closure capture in `if let`, trait-object checks and allocator-related inference; compile and test every feature set rather than changing the edition or opting into new APIs. Cargo changes its internal build-cache layout: project scripts use the stable `target/debug/pixstride` and `target/release/pixstride` executables, not internal dependency/cache paths. Keep FFI probes, Clippy with warnings denied, both feature configurations and the native platform checks enabled when updating the compiler.

## Compatibility qualification

The optional `native-codecs`, `cloud-sources`, `cloud-config`, `s3-decryption`, `source-cookies` and `telemetry` features are described in [COMPATIBILITY.md](COMPATIBILITY.md), including dependency licensing and intentional migration differences. Fetch locked dependencies once before running offline checks. All source builds require a C compiler for bundled libdeflate. The unreleased Dockerfile uses Ubuntu 26.04/glibc and pinned Rust 1.100.0; published 0.3.0 containers retain Alpine/musl. Native checks also require libjxl headers; native builds require libvips 8.18 development files and the enabled codecs (HEIF AV1/HEVC and JXL included for the full checks). S3 cryptographic checks need OpenSSL 3 development files, its CLI and pkg-config. Inspect `pixstride capabilities`; then test actual encoders, not only registered operations. `config_sources` stages administrator-selected configuration before any server threads and keeps the bootstrap SDK transport separate from image-source policy.

The container builds/checks both feature configurations and includes all optional features. Its native round-trip check runs during the build; runtime smoke tests/scans continue to use the exact image manifest. The workflow also checks native macOS ARM64 and Intel, builds the pinned reference, compares decoded outputs, runs HTTP/Unix/keep-alive/framing/OTLP, secret-manager bootstrap and real TLS S3/KMS tests, and bounded mutation/soak smoke. macOS installs OpenSSL 3 explicitly and exports its pkg-config path; the fixture generator uses the installed C API. These jobs gate tagged promotion; they do not publish from branch pushes. Keep generated reports, corpora, temporary certificates/keys and logs untracked. See the compatibility guide for reproducible commands and the limits of this evidence.

## Performance benchmarks

Native changes must also pass `python3 scripts/native_pool_check.py --binary target/debug/pixstride`. It exercises reuse without image caching, mixed requests, invalid input, child crashes, wall-clock timeouts, bounded recycling/concurrency, cumulative CPU-budget renewal and shutdown cleanup. This check runs in native CI and the container build.

Use the reusable [25-case Linux benchmark](BENCHMARKS.md) for performance comparisons. Its shared public corpus, immutable image pins, signed HTTP requests, balanced batches, child-inclusive cgroup CPU and peak-memory accounting are separate from compatibility qualification. Compare the complete case set and include pixel differences/output sizes; do not turn a fast error or different-quality output into a win. Run the same prepared inputs on native AMD64 and ARM64 hosts. The standalone workflow retains raw measurements and does not impose noisy timing thresholds on ordinary PRs.

Regenerate Markdown tables with `python3 scripts/benchmark_report.py --update-docs` after installing reviewed raw evidence in `benchmarks/results`. Run `python3 scripts/benchmark_report.py --check` to verify their consistency. Commit only public sanitized JSON summaries and losslessly compressed raw evidence, with image/corpus/harness identities and workflow provenance. Keep downloaded media, caches, logs and personal host paths untracked. See the guide for setup, all 25 cases and full rerun/publication commands.

Qualified repeated candidates use `python3 scripts/candidate_snapshot.py --publish amd64-*/report.json arm64-*/report.json`, preserving the release baseline. `python3 scripts/candidate_snapshot.py --check` recomputes qualification from every retained archive and checks generated candidate tables. Failed cohorts stay in workflow artifacts. The optional memory diagnostics run in new containers after all formal measurements; their process aggregates omit addresses, PIDs and file paths and never qualify performance.

## Releases

1. Keep human-written notes in `Unreleased`. For a stable release after betas, include all changes since the previous stable version, not only changes since the last beta.
2. On a short-lived branch, run `python3 scripts/project.py prepare 0.2.0-beta.1` (example). This updates `Cargo.toml`, `Cargo.lock`, the dated changelog, compare links and generated Markdown. It neither commits nor publishes.
3. Review and merge prerelease preparation into `develop`. Stable preparation (for example `0.2.0`) goes into `master`, including the tested `develop` changes. Synchronize release/hotfix changes back into `develop` through a PR. Delete temporary branches after merge.
4. At the merged commit, create and push an annotated tag matching Cargo, for example `git tag -a v0.2.0-beta.1 -m 'Release 0.2.0-beta.1'` then `git push origin v0.2.0-beta.1`. Never move or reuse a published tag.
5. CI verifies tag ancestry, matching versions, an empty `Unreleased`, tests and scans on both native architectures, then promotes the exact tested manifests. Only after that does it create the GitHub release from the changelog.

Stable `vX.Y.Z` tags publish the version, commit SHA and (when newer) `latest`. Prereleases use `vX.Y.Z-alpha.N`, `-beta.N` or `-rc.N`, with N starting at 1; they publish only their own moving channel and a GitHub prerelease. These channels are created by their first successful publication; branch pushes only validate code. Publication is serialized and compares existing GitHub releases and the channel image's version so an older release cannot move a channel backwards, including after partial publication failures. Re-running a failed publish can reuse the same validated digest; a different digest cannot overwrite an existing version. If publication partially fails, rerun only the failed job, inspect existing tags, and do not rebuild/relabel an existing version. GitHub concurrency can replace an older pending promotion when several releases queue up; rerun a canceled release if it is still needed.

The HTTP URL/options, environment variables, CLI and supported image behavior form the public contract. Stable releases follow SemVer; during 0.x, document incompatibilities and increment the minor version. Changes to validation limits can affect previously valid inputs and must be disclosed even when made for security.

This lightweight two-line workflow adapts [GitHub flow](https://docs.github.com/en/get-started/using-github/github-flow) to a separately tested integration line. It avoids the full collection of long-lived Gitflow branches. Version and changelog conventions: [SemVer](https://semver.org/spec/v2.0.0.html), [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
