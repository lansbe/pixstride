# Benchmark v2: 1,000 distinct workloads

V2 is the current reusable performance suite for Pixstride versus imgproxy on native Linux x86_64 and ARM64. It replaces the 25-case suite as the target for new comparisons. [V1 results and methodology](BENCHMARKS.md) remain intact; their wins do not establish any V2 performance result.

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

## Native validation of Pixstride 0.4.0

The [current complete preflight campaign](https://github.com/lansbe/pixstride/actions/runs/44002136766) ran all 40 shards on each architecture from harness `8222ceb6bc739171bf98b92121a7fe96a03f5dfa`; all 82 workflow jobs completed successfully. All 1,000 imgproxy reference cases per architecture returned independently decoded outputs with the declared format and dimensions. All 119 Python tooling tests passed on Linux before collection.

Both Pixstride modes, on each architecture, completed **967/1000 decoded trials**; **826/1000** also stayed within the predeclared pixel budgets. The remaining 33 cases returned HTTP 422 with `native worker closed output`; another 141 completed but exceeded pixel budgets. The same failure counts occur in default and forced-native mode on both architectures. Their detailed case/family/tier outcomes, error messages, sizes and pixel differences are retained in the [complete preflight index](benchmarks/results/v2/preflight-index.json) and its 80 raw archives. These are observations, not a diagnosis of the worker failures or proof of perceptual quality.

The [first complete campaign](https://github.com/lansbe/pixstride/actions/runs/43996121837), harness `943d9a0e614a9b331d05e84c6f904fd194b117ca`, also passed all 82 jobs and produced the same counts. Its [historical index](benchmarks/results/v2/history-preflight-357e2099eef6-index.json) and all 80 raw archives remain intact. Review regression tests then fixed deadline handling during pixel comparison and protected container cleanup; the current campaign reran every shard using that corrected collector. Neither campaign triggered a case deadline. All 162 native-report/aggregate ZIP digests were verified across the two campaigns, and both local aggregates exactly matched CI.

No requests or tolerances were changed after these results, and no failing case was removed. This validates the new workload definitions and exposes limitations in the published service. V2 CPU/p50/p95/memory wins remain **not measured**: the full three-repeat timing campaign is a separate workflow mode. The older v1 25/25 result cannot be extended to this suite.

## Coverage and increasing difficulty

The checked-in [catalog](benchmarks/cases-v2.json) contains exactly **1,000 distinct requests**: 20 processing families, five content/geometry/encoding variants per family, and ten source scales. These are 1,000 different workload scenarios, not 1,000 unrelated API features or repetitions distinguished only by an ID. No canonical source/options/output-format/dimensions tuple is repeated, and none replaces a v1 request. Generation is deterministic and checked by CI.

The 20 families cover fit, fill, forced aspect ratios, absolute crops, rotations, flips, DPR, nonuniform zoom, canvas extension, asymmetric padding, blur, sharpen, combined filters, offset crops, fractional crops, smart crops, alpha backgrounds, pixelation, AVIF conversion and long filter/geometry chains. Long chains include alpha, entropy-heavy textures, vector curves, panoramas and tall posters. DPR/zoom cases require actual resampling; they cannot resolve to geometric identities.

Twelve input representations/content types exercise baseline and progressive JPEG, RGB/RGBA/paletted/grayscale PNG, lossless/lossy WebP, RGB TIFF, BMP, static GIF and SVG. Requested outputs span JPEG, PNG, WebP, BMP and AVIF. Five variants alter meaningful inputs, geometry, output codec, gravity or quality; changing an identifier alone never creates a case.

The shared corpus has 120 files: twelve representations at ten source edges (256, 384, 512, 768, 1024, 1536, 2048, 2560, 3072 and 4096 pixels). Aspect ratios include square, landscape, portrait, 4:1 panorama and 1:4 poster. The largest input is 16,777,216 pixels. Vector complexity and pattern density increase with scale. Lossless noise, alpha edges, indexed colors and long processing chains add different CPU, memory and encoding pressures.

Cases are sorted *before either service is measured* by:

```text
nominal work = input pixels + output pixels × (pipeline passes + encoder weight)
encoder weights: BMP 1, JPEG 2, PNG 3, WebP 5, AVIF 15
```

Ranks 1–100 form tier 1, 101–200 tier 2, through 901–1000 tier 10: **100 cases per tier**. The catalog records each case's rank, tier, dimensions, nominal work and pipeline passes. This is a transparent effort heuristic, not a measured CPU prediction. Codec shortcuts, content and different CPUs can break runtime monotonicity. Results never determine the case order or which cases remain in the suite. Each shard executes its 25 ranks in ascending order; different shards run in parallel.

The two checksum-pinned public photos and their attribution are retained from v1. They are normalized to sRGB and resized/cropped/re-encoded; synthetic patterns use fixed recipes and seeds. Photo derivatives retain their original license; synthetic patterns are MIT. The prepared corpus includes `ATTRIBUTION.md`. Media and caches are not committed. Every file is bounded at 64 MiB, hashed and checked for format/static-frame/dimensions. SVG is a bounded deterministic fontless document. The same prepared artifact goes to every native job; do not regenerate separate inputs per architecture.

## Preflight versus performance

**Preflight** sends one independently decoded image request per engine/case. It checks all 1,000 cases on each architecture, all three engines, output MIME/dimensions and pixel differences. There are no qualified timing samples or performance wins in this mode. Validating the catalog requires all 1,000 imgproxy responses per architecture to succeed. Pixstride refusals, timeouts, wrong dimensions or pixel-budget failures remain visible; replacing a failed Pixstride case with an easier request is forbidden.

**Measure** uses six balanced engine-order permutations, 24 timed requests per batch and five warmups: **144 timed requests per case/engine**. All successful full engine trials would produce **2,592,000 timed responses** across two architectures, three repetitions and three engines. Reports retain the actual accepted sample count, including incomplete batches; a failure cannot be imputed as zero latency or counted as a win. Even an imgproxy failure during a complete measurement campaign is archived as an observed failure, with no valid timing comparison for that case. It cannot be used to certify the preflight.

Default (`auto`) and forced-native Pixstride are separate observations. Every mode must have lower CPU/request, HTTP p50, HTTP p95 and lifetime peak cgroup memory in every repetition to confirm a case. Qualification requires all 1,000 cases on both architectures, three independent repetitions per case, and all pixel budgets passing. No favorable mode, case, shard, repetition or surviving subset substitutes for a failure.

## Resources and numerical definitions

Each service runs in a fresh per-case container with two service logical CPUs, a two-CPU quota, 1 GiB memory with no extra swap, 128 PIDs, a read-only root and dropped capabilities. The client is pinned to other available logical CPUs; native Linux with at least three available CPUs and host-visible cgroup v2 is required. Images, runtime capabilities, native platform/config IDs, raw OCI index/platform digests, release version/source and the distinct harness revision are verified and recorded. [V2 pins](benchmarks/versions-v2.json) are independent of the immutable v1 release pins.

HTTP latency spans fresh TCP request initiation through the complete loopback response read. Decode, hash/geometry checks, preparation and image pulls are outside the latency clock. p50/p95 use nearest rank across all 144 accepted samples of a successful engine trial. Cgroup CPU includes service children and a 50-ms deferred-work tail; CPU/request is the median of six batch averages. Throttling counters and CPU variability remain in the raw samples. First-touch/preflight engine order rotates by case rank; measured batches balance all six permutations.

Lifetime cgroup `memory.peak` includes startup, preflight, warmups, workers, page cache and kernel charges. It is not process RSS. File-page charging and shared-host scheduling/frequency remain sources of variation. Services are compared within the same case/VM. Hardware can differ between shards and repetitions; this is not an x86-versus-ARM or programming-language performance claim.

The case deadline is 600 seconds and an individual HTTP request is bounded at 60 seconds. The work alarm is cancelled before ownership-checked container cleanup, so an expected deadline cannot abandon a service; total elapsed time, including cleanup, still determines a budget failure. Expected service/codec/resource failures preserve their diagnostics and completed/partial batches; other engines and subsequent cases still run. Infrastructure errors and interruptions retain available partial reports but invalidate complete-coverage claims. Artifact upload after a runner hard kill is best-effort; missing artifacts never become complete results.

## Pixel agreement and file size

Pillow 13.0.0 with AVIF decoding independently decodes every successful preflight output. Comparisons use black/white composites and separate alpha, in 8-bit channel units. Fixed per-case budgets are frozen in the catalog before measurement:

| Output/workload | Maximum composite MAE | Maximum composite RMSE |
|---|---:|---:|
| Lossless-source geometry without interpolation (PNG/BMP/TIFF/static GIF), lossless output | 0 | 0 |
| Other PNG/BMP resampling | 2 | 5 |
| Filter/vector pipelines with lossless output | 4 | 10 |
| JPEG/WebP/AVIF output | 6 | 12 |

Alpha MAE/RMSE are at most 1/3, or exactly zero for exact-pixel cases. Both composites and alpha must pass in every repetition. Missing comparisons are unknown, not zero error. These are numerical disagreement budgets, **not certified perceptual equivalence**. The same requested quality does not equalize different encoders. Encoded-size ratios and all pixel errors accompany every case; larger files are not concealed by CPU wins. Tolerances cannot be relaxed after observing a Pixstride loss. The old 25-case agreement envelope does not apply to these new inputs.

This is a static, sequential, warm-cache, local-source workload. It makes no animation, HDR/high-precision fidelity, remote-origin, keep-alive, saturation or production-network claim.

## Prepare and run

Install Python 3.11+, Docker with buildx, and the pinned independent decoder. Run on a native Linux host with a local Docker daemon/cgroup v2. Preparation may run on another machine, but copy its exact corpus to both native architectures.

```sh
python3 -m venv .venv-benchmark
.venv-benchmark/bin/python -m pip install --only-binary=:all: Pillow==13.0.0
.venv-benchmark/bin/python scripts/benchmark_v2_catalog.py --check
.venv-benchmark/bin/python scripts/benchmark_v2_fixtures.py --output .benchmark-v2-data --cache .benchmark-cache
.venv-benchmark/bin/python scripts/benchmark_v2_fixtures.py --output .benchmark-v2-data --check
.venv-benchmark/bin/python scripts/benchmark_v2.py --fixtures .benchmark-v2-data --mode preflight --shard-index 0 --output .benchmark-v2-results/preflight-0.json
```

There are exactly 40 nonoverlapping shards (indices 0–39), each with 25 cases. `--case ID` is a development filter within one shard and labels its report partial; it cannot pass complete-cohort aggregation. `--mode measure --repetition 1` runs that shard's full timing protocol; repetitions 2 and 3 are additionally required for qualification.

For a complete preflight on each native host:

```sh
benchmark_host=$(uname -m)
for shard in $(seq 0 39); do
  .venv-benchmark/bin/python scripts/benchmark_v2.py --fixtures .benchmark-v2-data \
    --mode preflight --shard-index "$shard" --output ".benchmark-v2-results/native-${benchmark_host}-r1-s${shard}/report.json"
done
```

Copy all 80 `native-*` report directories from both architectures into the coordinator's `.benchmark-v2-results` directory, then aggregate and optionally retain the entire cohort:

```sh
.venv-benchmark/bin/python scripts/benchmark_v2_report.py --reports .benchmark-v2-results/native-*/report.json --mode preflight --output v2-preflight.json --markdown v2-preflight.md
.venv-benchmark/bin/python scripts/benchmark_v2_snapshot.py --record .benchmark-v2-results/native-*/report.json --mode preflight
.venv-benchmark/bin/python scripts/benchmark_v2_snapshot.py --check
```

For a complete timing campaign, collect all 240 reports (40 shards × two architectures × three repetitions), then use the same commands with `--mode measure`. The report command writes JSON/Markdown even if service/quality/performance comparisons fail; exit 1 means invalid reference responses or an unmet full timing target. Missing, duplicated or incompatible evidence is an error. The snapshot stores all outcomes of a complete measurement campaign, including failures. It requires a validated reference for a preflight snapshot. Snapshots are not software releases.

## Native workflow and evidence

### Unreleased candidates

The [candidate workflow](.github/workflows/benchmark-v2-candidate.yml) builds the exact clean checkout once on each native Linux architecture and runs the Dockerfile's application and codec checks. Candidate Linux images use pinned libvips 8.18.7, quantizr 1.4.3, libheif 1.23.7 and AOM 3.15.3; source archives, license notices and runtime-library check results accompany the image. These builds are not a published software release. Docker image archives are distributed as seven-day workflow artifacts, without moving any registry tag.

The two native build manifests bind source revision/tree, Dockerfile and Cargo.lock hashes, package version and inspected native image IDs. `benchmark_v2_candidate.py pins` joins them into an explicit generated pins file outside the checkout. Every shard must use that file, the same clean harness revision and the matching architecture's local image ID. Candidate collection cannot use published provenance, and published collection cannot accept a candidate image.

The candidate workflow also requires corpus SHA256 `4b7467347c61684f350a8ab15eca6ec7dc02708e09c17e31a2d80c1e2579ff05`, matching all 120 input files in the retained release preflight. A generator or codec update that changes their bytes stops preparation before any comparison runs.

Preflight runs all 80 shards; full `measure` mode runs all 240 shards with the same sampling, quality and resource rules above. Up to 20 jobs run concurrently. A candidate VM has a 300-minute administrative bound so that 25 individual 600-second case budgets can finish and retain failures; the case deadline remains 600 seconds. Build jobs have a 120-minute bound. The existing published-image workflow retains its original bounds.

The runner additionally requires `--versions candidate-pins.json --pixstride-image sha256:... --candidate-revision FULL_SHA`. Aggregate and archive candidate reports explicitly:

```sh
.venv-benchmark/bin/python scripts/benchmark_v2_report.py --reports .benchmark-v2-results/native-*/report.json --versions candidate-pins.json --mode measure --output candidate-measure.json --markdown candidate-measure.md
.venv-benchmark/bin/python scripts/benchmark_v2_snapshot.py --record .benchmark-v2-results/native-*/report.json --versions candidate-pins.json --mode measure
.venv-benchmark/bin/python scripts/benchmark_v2_snapshot.py --check
```

Use `--mode preflight` for the complete 80-shard validation cohort. Candidate preflight diagnostics retain reference failures as well as Pixstride failures; inspect all reference/quality counts before proceeding to timings. Candidate indices, history and generated summary markers are separate from published evidence (`candidate-preflight-index.json`, `candidate-measure-index.json`). Full results include every failure, non-winning case and metric ratio; median reductions use all retained comparable observations, including losses and numerical pixel regressions. No candidate result establishes a performance claim for the published 0.4.0 image.

The candidate headline follows the latest complete campaign's first shard start time, derived from validated raw timestamps. A newer measurement replaces an older preflight even when their source commits differ; a newer preflight also supersedes older timings. Equal timestamps with different targets are rejected rather than selecting a favorable result.

### Published images

The [current Benchmarks workflow](.github/workflows/benchmark.yml) prepares one corpus and runs the full 80-job preflight on pushes to `codex/benchmark-v2-*` when the measurement/catalog inputs change. It does not launch the expensive full timing campaign automatically. After integration, `workflow_dispatch` offers `preflight` or `measure`; measure produces 240 independent native VM jobs. There are three repetitions **per case**, distributed over 120 VM jobs per architecture, rather than only three VMs for the entire thousand-case suite. Jobs have a 120-minute bound and keep available reports on failure; the matrix uses fail-fast false and up to 20 concurrent jobs.

Aggregation requires every shard exactly once for every expected architecture/repetition, all 1,000 canonical case definitions, identical corpus/pins/harness/protocol, native image identities and separate client CPUs. Replayed raw samples cannot count as another repetition, even for just one case/engine. Neither a preflight nor a partial shard can become performance evidence. JSON/Markdown list all 1,000 cases for each mode and architecture, plus every tier/family count, size ratio, pixel difference and failure.

Raw report and aggregate artifacts have 90-day retention; the corpus has seven days. Commit complete sanitized snapshots under `benchmarks/results/v2` for durable evidence. Gzip archives carry compressed/raw SHA-256 hashes. Previous cohort indices are preserved by content hash before replacement; raw archives are never overwritten with different samples. Ordinary CI checks archive integrity, recomputed results, catalog and generated documentation independently of noisy timing wins.

To benchmark a newer published release, update only `benchmarks/versions-v2.json`, then run `benchmark_v2_snapshot.py --init` to mark that target pending while preserving older cohorts. Commit before collecting. Runtime release identity and harness revision are different. The v1 pins and evidence stay intact. A new catalog/protocol/decoder policy needs a new benchmark version; do not silently redefine v2 or recast older data as measurements of a new target.
