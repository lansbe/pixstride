# Contributing

Use English names, comments and documentation. Keep pull requests focused and preserve image fidelity, resource bounds and explicit failure for unsupported behavior. Contributions are licensed under MIT.

The code is divided into `src/codecs` (formats and metadata), `src/processing` (geometry/effects/pipeline), `src/network` (HTTP, sources, admission), and `src/security` (signatures). `config.rs`, `buffer.rs`, and `os.rs` own startup, aligned memory, and administrator-selected file reads. First-party modules in `src/` should stay below 500 lines; split by responsibility, not arbitrary fragments. Imported generated dependency code retains its upstream layout and must include license, provenance and a documented minimal patch. Every unsafe operation needs a verifiable bounds, ownership or platform-ABI argument.

Run `cargo fmt --check`, `cargo clippy --locked --offline -- -D warnings`, and build/run the Docker image. Validate supported platforms and compare decoded pixels for codec changes; include bounded synthetic regression cases in the pull request. Never submit personal images, machine paths, secret values, private URLs, research archives or benchmark dumps. External libraries and Rust crates are permitted; document their purpose, license, supported platforms and resource behavior, and commit the updated lockfile.

Benchmarks must state hardware, compiler, output bytes and decoded-pixel differences, cache state, concurrency, CPU accounting, HTTP percentiles and peak RSS. A faster different output is not an equal-quality win. Report security issues privately as described in [SECURITY.md](SECURITY.md).

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

## Compatibility qualification

The optional `native-codecs`, `cloud-sources`, `source-cookies` and `telemetry` features are described in [COMPATIBILITY.md](COMPATIBILITY.md), including dependency licensing and intentional migration differences. Fetch locked dependencies once before running offline checks. Native checks require a C compiler and libjxl headers; native builds require libvips 8.18 development files and the enabled codecs (HEIF AV1/HEVC and JXL included for the full checks). Inspect `pixstride capabilities`; then test actual encoders, not only registered operations.

The container builds/checks both feature configurations and includes the native/cloud/telemetry features. Its native round-trip check runs during the build; runtime smoke tests/scans continue to use the exact image manifest. The workflow also checks native macOS ARM64 and Intel, builds the reference from its pinned commit, compares synthetic decoded outputs, runs HTTP/Unix/keep-alive/framing/OTLP and local vendor-protocol tests and bounded mutation/soak smoke. These jobs gate tagged promotion; they do not publish from branch pushes. Keep generated reports, corpora and logs untracked. See the compatibility guide for reproducible local commands and the limits of this evidence.

## Releases

1. Keep human-written notes in `Unreleased`. For a stable release after betas, include all changes since the previous stable version, not only changes since the last beta.
2. On a short-lived branch, run `python3 scripts/project.py prepare 0.2.0-beta.1` (example). This updates `Cargo.toml`, `Cargo.lock`, the dated changelog, compare links and generated Markdown. It neither commits nor publishes.
3. Review and merge prerelease preparation into `develop`. Stable preparation (for example `0.2.0`) goes into `master`, including the tested `develop` changes. Synchronize release/hotfix changes back into `develop` through a PR. Delete temporary branches after merge.
4. At the merged commit, create and push an annotated tag matching Cargo, for example `git tag -a v0.2.0-beta.1 -m 'Release 0.2.0-beta.1'` then `git push origin v0.2.0-beta.1`. Never move or reuse a published tag.
5. CI verifies tag ancestry, matching versions, an empty `Unreleased`, tests and scans on both native architectures, then promotes the exact tested manifests. Only after that does it create the GitHub release from the changelog.

Stable `vX.Y.Z` tags publish the version, commit SHA and (when newer) `latest`. Prereleases use `vX.Y.Z-alpha.N`, `-beta.N` or `-rc.N`, with N starting at 1; they publish only their own moving channel and a GitHub prerelease. These channels are created by their first successful publication; branch pushes only validate code. Publication is serialized and compares existing GitHub releases and the channel image's version so an older release cannot move a channel backwards, including after partial publication failures. Re-running a failed publish can reuse the same validated digest; a different digest cannot overwrite an existing version. If publication partially fails, rerun only the failed job, inspect existing tags, and do not rebuild/relabel an existing version. GitHub concurrency can replace an older pending promotion when several releases queue up; rerun a canceled release if it is still needed.

The HTTP URL/options, environment variables, CLI and supported image behavior form the public contract. Stable releases follow SemVer; during 0.x, document incompatibilities and increment the minor version. Changes to validation limits can affect previously valid inputs and must be disclosed even when made for security.

This lightweight two-line workflow adapts [GitHub flow](https://docs.github.com/en/get-started/using-github/github-flow) to a separately tested integration line. It avoids the full collection of long-lived Gitflow branches. Version and changelog conventions: [SemVer](https://semver.org/spec/v2.0.0.html), [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
