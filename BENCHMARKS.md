# Linux performance benchmarks

The suite measures 25 predefined HTTP image-processing cases on native Linux AMD64 and ARM64. [released-versions.json](benchmarks/released-versions.json) pins the current published images by immutable multi-platform digest; [versions.json](benchmarks/versions.json) preserves the initial Pixstride 0.3.0/imgproxy 4.0.24 baseline. Raw reports retain actual image identities and published OCI proof independently of the harness source. Pixstride default routing and forced-native mode are separate observations. In 0.4.0 the default `auto` route keeps eligible opaque PNG identities in the built-in engine and sends other eligible transformations through the native worker. Forced-native uses that worker for every case. The worker can itself select optimized Rust paths or libvips; this is a routing comparison, not a Rust-versus-C comparison.

New candidate qualification requires every CPU/p50/p95/peak-memory comparison to win in both default and forced-native modes, in every retained run. Per-mode tables and counts remain separate; one mode cannot replace a loss in the other. Historical default-only snapshots below retain their original target and source labels.

This compares packaged services, including their different libraries and deployment behavior. It does not isolate programming-language performance. The [compatibility suite](COMPATIBILITY.md) still targets imgproxy 4.0.21.

## Current measured result

The published **Pixstride 0.4.0** image, runtime source [`98868fa66288`](https://github.com/lansbe/pixstride/commit/98868fa66288a2b27a24116f29f82ece11a9cef2), was measured on 2026-11-16 against pinned imgproxy 4.0.24 in the [complete six-VM workflow](https://github.com/lansbe/pixstride/actions/runs/43975836467). Both default (`auto`) and forced-native confirm **25/25 cases on both architectures**: CPU/request, HTTP p50, HTTP p95 and lifetime peak cgroup memory are lower in every retained run. All decoded-pixel checks pass the unchanged agreement envelope; PNG identities remain exact. There are no failed comparisons in this cohort.

| Linux | Mode | Median CPU reduction | Median p50 reduction | Median p95 reduction | Median peak cgroup reduction |
| --- | --- | ---: | ---: | ---: | ---: |
| x86_64 | default | 45.1% | 31.7% | 30.1% | 38.5% |
| x86_64 | forced-native | 44.6% | 31.1% | 29.2% | 38.1% |
| ARM64 | default | 18.5% | 15.0% | 16.0% | 42.7% |
| ARM64 | forced-native | 19.4% | 15.4% | 17.6% | 43.9% |

These are median per-case reductions: first take the median ratio over three runs for each case, then the median over 25 cases, then compute `100 × (1 − ratio)`. They are not a workload-weighted average. The narrowest winning comparison is forced-native ARM64 progressive JPEG p95, 1.98% lower in one run; shared-VM variability still matters. Alpha PNG identity produces files 17.6% larger than imgproxy despite its CPU/latency/memory wins. Equal requested quality does not establish perceptual equivalence.

All six VMs use Ubuntu 24.04.5, kernel `7.0.0-1017-azure`, four available logical CPUs, service CPUs `[0, 1]` and client CPUs `[2, 3]`. Each service has a two-CPU quota and 1-GiB memory limit. The x86_64 repetitions use Intel Xeon Platinum 8370C, AMD EPYC 9V74 and AMD EPYC 7763 respectively; ARM64 reports CPU part `0xd49` on all three VMs. Services are compared on the same VM within each repetition. These different machines do not isolate an architecture effect.

The [released-image index](benchmarks/results/released-index.json) retains all six compressed raw reports, hashes, exact OCI image proof and recomputed qualification, independently of the harness revision [`0fd4f487c543`](https://github.com/lansbe/pixstride/commit/0fd4f487c543a8ad16b9d4f481de93c26527faed). There are 144 timed requests per case and engine per VM: **64,800 total**, with no dropped samples, retries for favorable timing or source image rebuild. Both older complete pre-release cohorts, their losses and the published 0.3.0 baseline remain intact below. Differences from their measurements do not establish an additional runtime optimization.

<details>
<summary>Historical pre-release results, limits and diagnostic losses</summary>

## Historical pre-release campaigns

Source [`20f202a5b5e1`](https://github.com/lansbe/pixstride/commit/20f202a5b5e17eb0d8e61795e87d04a77a0cac77), [full six-run post-review workflow](https://github.com/lansbe/pixstride/actions/runs/43820275305): forced-native confirms **25/25 cases on both architectures**. Default routing confirms 25/25 on AMD64 and 24/25 on ARM64. CPU/request, HTTP p50 and peak cgroup memory win all 25 cases in both modes in every run. The complete both-mode target is **not met**, because one ARM64 default comparison loses p95:

| Mode | Case | Pixstride p95 | imgproxy p95 | Ratio |
| --- | --- | ---: | ---: | ---: |
| Default | 22 flat graphics square crop | 22.768 ms | 22.608 ms | 1.0071× |

The difference is 0.71%, and remains a loss under the strict gate. No quota throttling, errors, outlier exclusions or favorable run selection are involved. The cause of the remaining timing difference is not established.

The two original forced-native losses are resolved in every retained run: case 21 AMD64 now uses 92.0–92.5% less CPU, and case 09 ARM64 uses 20.4–28.1% less peak cgroup memory than imgproxy in this cohort. Both cases also win p50/p95 and the other resource comparison. All decoded-pixel checks pass without changing the agreement envelope; PNG identities remain exact. Alpha PNG identity still produces files 17.6% larger than imgproxy.

The [retained pre-release observed index](benchmarks/results/observed-index.json) preserves all six compressed raw reports, hashes and recomputed failures. Generated tables below include both modes and all 25 cases, output sizes and pixel errors. It is separate from the historical default-only qualified index.

The [preceding complete cohort](benchmarks/results/observed-47fa85d33c40-index.json), source `47fa85d33c40`, is also retained with all six raw reports. It confirmed 25/25 ARM64 in both modes and 24/25 AMD64 in each: AMD64 repetition 3 lost native JPEG identity p95 by 10.0% and default SVG p95 by 12.1%. Neither its samples nor its failures are removed.

The post-review campaign was triggered automatically by fixes to diagnostic startup and qualification/publication repeat counting. The engine source, runtime build configuration and HTTP measurement implementation are unchanged between these two sources. Their timing differences cannot be attributed to a speed optimization. Both complete cohorts are retained regardless of outcome; their variation is another reason these shared-VM measurements do not establish a universal performance guarantee.

## Historical candidate limits and diagnostic losses

The qualified `6145080e14a5` cohort retains all three runs on each architecture. Default routing wins every CPU/p50/p95/peak-memory comparison in the 25 cases. Its smallest memory difference is case 09 (sharpen) on ARM64: ratio 0.99950006, only 16 KiB. Treat that memory result as near parity on shared VMs.

In that historical cohort, forced-native was a separate diagnostic. In one AMD64 run, case 21 (flat PNG identity) uses 8.7% more CPU and has p50/p95 29.5%/29.2% higher than imgproxy. In one ARM64 run, case 09 uses 8.9% more peak memory. Those losses remain in its raw reports; that historical 25/25 target covered the default configuration with its general routing rules.

RGBA8 PNG-to-JPEG resizing uses premultiplied Lanczos3 from a different implementation, with the same requested JPEG quality. In case 19, its worst composite pixel MAE/RMSE are approximately 1.308/1.967 in 8-bit channel units, within the predeclared envelope. It preserves 48 other default/native encoded outputs from the preceding implementation; these macOS development checks supplement the full Linux pixel checks. PNG alpha identity output is 17.6% larger than imgproxy. Every case's sizes/errors are listed below.

The [preceding corrected-source cohort](https://github.com/lansbe/pixstride/actions/runs/43763779426) was rejected at 24/25 ARM64 because case 19 had a p95 loss. That historical snapshot measures the subsequent SIMD implementation and retains its entire six-run cohort. Earlier qualified `fe350b73b784` archives remain available beside the new evidence and in Git history; their source predates the final validation fixes.

The subsequent forced-native work also rejected complete six-run cohorts rather than promoting their winning subsets:

| Candidate source | Full workflow | Reason for rejection |
| --- | --- | --- |
| `c1cec09c0cc7` | [43773116743](https://github.com/lansbe/pixstride/actions/runs/43773116743) | ARM64 sharpen CPU/latency and a native memory loss with per-image glibc reclamation |
| `150a9b2958d3` | [43775903980](https://github.com/lansbe/pixstride/actions/runs/43775903980) | ARM64 sharpen peak memory with paired reclamation |
| `25f0f4204589` | [43778666469](https://github.com/lansbe/pixstride/actions/runs/43778666469) | ARM64 sharpen CPU/latency and some memory losses with an independent SIMD JPEG resize |
| `508e96fd1404` | [43781284541](https://github.com/lansbe/pixstride/actions/runs/43781284541) | Sharpen latency and some ARM64 CPU/memory losses with native raster materialization |
| `df9caf100d5c` | [43791983997](https://github.com/lansbe/pixstride/actions/runs/43791983997) | All timings won, but the allocator's default page retention lost peak memory in several cases |
| `6fecbfee441b` | [43798474537](https://github.com/lansbe/pixstride/actions/runs/43798474537) | Immediate purging reduced memory but introduced timing losses and still lost sharpen peak memory |
| `70726e0ac196` | [43800922455](https://github.com/lansbe/pixstride/actions/runs/43800922455) | End-of-request allocator collection still lost JPEG/WebP memory and several timing comparisons |
| `0a33aef385ac` | [43807416642](https://github.com/lansbe/pixstride/actions/runs/43807416642) | Forced-native won all 25 in every run; one ARM64 default sharpen p95 loss rejected the entire both-mode cohort |
| `7dc1d103c167` | [43812425608](https://github.com/lansbe/pixstride/actions/runs/43812425608) | One AMD64 native thumbnail p95 loss and a sharpen peak-memory loss in both modes with 64-row strips |
| `47fa85d33c40` | [43816001961](https://github.com/lansbe/pixstride/actions/runs/43816001961) | Both original native losses resolved; one AMD64 runner loses native JPEG identity and default SVG p95. All six reports are committed as observations |
| `20f202a5b5e1` | [43820275305](https://github.com/lansbe/pixstride/actions/runs/43820275305) | Forced-native wins all 25 on both architectures; default ARM64 graphics crop p95 +0.71% rejects the entire both-mode target. All six reports are committed |

Earlier failed measurements remain available in workflow artifacts and are not mixed into a qualified snapshot. Artifact retention is finite; both current/preceding observed cohorts and published historical cohorts retain compressed raw samples independently.

</details>

## Corpus

[cases.json](benchmarks/cases.json) contains every source, processing option, output format and expected dimension. The 25 cases cover identity, thumbnails, medium images, square/center crops, DPR, rotation, blur, sharpening, format conversion, progressive JPEG, alpha/backgrounds, flat graphics, grayscale, TIFF and SVG.

Two public photographs are downloaded with bounded sizes and verified SHA-256 hashes:

- [Fronalpstock](https://commons.wikimedia.org/wiki/File:Fronalpstock_big.jpg), Hannes Röst, [CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/). Derivatives are converted to sRGB, resized to 2048×920 and re-encoded; grayscale is derived from this image. The derivatives retain the same license.
- [The Blue Marble](https://commons.wikimedia.org/wiki/File:The_Earth_seen_from_Apollo_17.jpg), NASA / Apollo 17 crew, public domain as a US NASA work. It is converted to sRGB, cropped/resized to 1600×1600 and re-encoded as JPEG/TIFF.

Alpha shapes, flat graphics and a restricted SVG are deterministic synthetic fixtures under the project's MIT license. [benchmark_fixtures.py](scripts/benchmark_fixtures.py) uses Pillow 13.0.0 outside measurement and removes inherited ICC/EXIF metadata after color conversion. ICC preservation, HDR, animation and remote origin fetching are outside this performance corpus.

The manifest records input dimensions, sizes, hashes, attribution and generator/encoder versions. CI generates the corpus once and sends the identical artifact to both architectures. For manual two-host comparisons, copy that prepared directory instead of regenerating it: different codec builds can produce different input bytes. Media/source caches remain untracked; attribution accompanies the shared corpus artifact.

## Protocol and interpretation

New runs pin the Python HTTP client and its subprocesses to the remaining available logical CPUs, disjoint from the two service CPUs, and record the kernel-verified affinity. At least three available CPUs are required. Both services retain their identical two-CPU allocation; corpus, requests, ordering, counters and quality criteria are unchanged. Older snapshots, including the rejected cohorts above, did not constrain client affinity. Their samples remain intact. This removes a possible harness contention source; it does not establish the cause of their latency spikes or remove shared-host noise. Cross-cohort timing changes cannot be attributed solely to the service when the client placement differs.

Each case starts fresh containers for Pixstride default, Pixstride native and imgproxy. Each receives the same two logical CPUs, a two-CPU quota, 1 GiB memory with no additional swap, a 128-process limit, read-only root and dropped capabilities. There is one active request. `VIPS_CONCURRENCY=1` and `GOMAXPROCS=2` are configured; imgproxy's maximum clients is one. Metadata probes run in separate containers so they cannot inflate measured peaks.

Both products validate the same HMAC-SHA256 signed URL and server-wide Bearer token. Sources are local read-only files. Container healthchecks and imgproxy request logging are disabled. No application result cache is used; the native engines disable the libvips operation cache.

The default full protocol gives each engine one decoded-output preflight and five warmups per case. Six measured batches use all six permutations of the three engines' order, with 24 sequential requests per batch: **144 measured requests per case/mode, 10,800 per architecture**. Larger balanced batch/sample counts are supported and recorded in the generated results. Case order uses a recorded fixed shuffle seed. Source pages are warm; OS caches are not dropped between samples.

HTTP latency uses a monotonic nanosecond clock from request/connection initiation through the complete loopback response read. Every request uses a fresh TCP connection. Body validation, decoding, fixture preparation, downloads and image pulls are outside latency timing. p50/p95 use nearest rank over all 144 samples, not percentiles of batch medians. This is a sequential warm-cache/local-source workload, not a saturation, keep-alive or production-network test.

Host cgroup-v2 `cpu.stat` deltas include all service processes and native children, excluding the client. The window covers a batch plus 50 ms for deferred worker/GC work. CPU/request is the median of six batch CPU/request averages. User/system CPU and quota-throttling counters remain in the raw data.

`memory.peak` is the fresh per-case container's lifetime peak, including startup, preflight, warmups, transient workers, file cache and kernel charges. It is **not RSS**. Shared page-cache charging can affect comparisons. End-of-case anonymous/file/kernel counters are included for context. This metric cannot substantiate the old README's process-RSS savings claim. The [kernel documentation](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html) defines these counters.

Every response must be HTTP 200 with the expected media type and dimensions. Every timed body's hash must match its fully decoded preflight output. Errors, OOM, counter resets, missing measurements and architecture mismatches fail the run. Partial/preflight runs cannot be published as full results.

Independent decoding compares outputs against imgproxy over black and white backgrounds, with alpha measured separately. Reports contain MAE, RMSE, maximum difference, PSNR and exact-visible-pixel agreement; null PSNR means an exact match (infinite PSNR). These measure pairwise pixel agreement, not certified perceptual quality. Equal `q` values do not equalize different encoders, chroma sampling, filters or lossy/lossless modes. Output size accompanies timing.

Shared GitHub VMs have uncontrolled scheduling/frequency/noisy-neighbor conditions. CPU model/allocation, kernel, OS, Docker, limits, image/platform identities, input hashes, harness revision and workflow URL are recorded without hostnames or personal paths. Compare services within each architecture: the two CPU models do not isolate an ISA effect. Small differences and high batch variability do not establish a universal advantage.

## Rerun

Use a native AMD64/ARM64 Linux host with a local Docker daemon, cgroup v2, at least three available logical CPUs (two service CPUs and the remaining client CPUs), Python 3.11+ and room for three 1-GiB containers plus the client. A remote daemon cannot provide the local host's container accounting.

```sh
python3 -m venv .venv-benchmark
.venv-benchmark/bin/python -m pip install --only-binary=:all: Pillow==13.0.0
.venv-benchmark/bin/python scripts/benchmark_fixtures.py --output .benchmark-data --cache .benchmark-cache
.venv-benchmark/bin/python scripts/benchmark.py --fixtures .benchmark-data --output .benchmark-results/preflight.json --preflight
.venv-benchmark/bin/python scripts/benchmark.py --fixtures .benchmark-data --output .benchmark-results/report.json
.venv-benchmark/bin/python scripts/benchmark_report.py --reports .benchmark-results/report.json --markdown .benchmark-results/table.md --csv .benchmark-results/cases.csv
```

Copy the same prepared corpus to the second architecture. Image overrides via `--pixstride-image` / `--imgproxy-image` require immutable `@sha256:` references. A published Pixstride image must still match the selected `--versions` file, including its release version and source revision; another release needs its own pins file. Source candidates use the explicit provenance mode described below. `--case`, lower sample counts and `--preflight` support development trials and cannot pass full-publication validation.

The [Benchmarks workflow](.github/workflows/benchmark.yml) shares one corpus across three native `ubuntu-24.04` and three `ubuntu-24.04-arm` jobs, measuring the pinned published images without rebuilding them. Pushes on `codex/benchmark-release-*` or the original benchmark branch trigger it; `workflow_dispatch` supports manual reruns after integration. Timing ratios do not gate unrelated PRs. Correctness, input integrity and accounting do gate a benchmark run. Failures retain diagnostics and available partial JSON; owned containers are cleaned. Raw report artifacts have 90-day retention; corpus media have seven days.

For a published-release comparison, retain all three full reports per architecture and recompute the strict both-mode target:

```sh
python3 scripts/release_snapshot.py --qualify released-*/report.json --output qualification.json --markdown qualification.md
python3 scripts/release_snapshot.py --record released-*/report.json
python3 scripts/release_snapshot.py --check
```

Here `released-*` are the six extracted report directories. `--qualify` returns 1 when any CPU/p50/p95/peak or pixel gate fails, while retaining the detailed result. `--record` stores the entire cohort even when the target fails: six deterministic raw gzip archives, their hashes and recomputed qualification in [released-index.json](benchmarks/results/released-index.json), plus separate generated documentation blocks. It rejects missing, duplicated or incompatible evidence. It verifies the published OCI index and native platform manifests against their digests, release version/source annotations, the inspected image config and runtime capabilities. Runtime source and harness revision are recorded separately. Recording measurements creates no software release or registry image.

The [Candidate benchmarks workflow](.github/workflows/benchmark-candidate.yml) builds the current clean checkout independently on three AMD64 and three ARM64 runners. It uses the same shared corpus, pinned imgproxy image, 25 cases, three modes and full measurement protocol. Candidate images stay local to their runners; there is no registry or release publication. Each is addressed by its inspected immutable Docker image ID and labelled with the exact Git revision. Reports retain source tree, Dockerfile and Cargo.lock hashes; cross-architecture validation requires identical source provenance while allowing platform-specific image IDs. Manual candidate runs use `--pixstride-image sha256:<local-image-id> --candidate-revision <full-clean-commit-sha>` after building with `--label org.opencontainers.image.revision=<full-clean-commit-sha>`.

The candidate workflow's final qualification job requires lower CPU, HTTP p50/p95 and peak cgroup memory in both modes in every retained run for all 25 cases on both architectures. New both-mode qualification always requires at least three independent full runs per architecture, including comparisons within 5% of parity or with CPU batch CV above 10%. Two otherwise winning runs remain `repeat-required`. Historical default-only qualification retains its original minimum of two, with three for close/noisy timings. Any observed loss remains a loss. Neither mode can substitute for a loss in the other. Both modes must stay within the worse of the two published modes' per-case composite/alpha MAE and RMSE, with 0.01 numerical tolerance; identity PNGs stay exact. This is an agreement envelope, not a perceptual quality certification. Source, corpus, options, competitor identity and raw-sample independence are checked. New schema2 candidate snapshots publish both-mode counts, all per-mode metric ranges, output sizes and pixel errors. Historical schema1 default-only evidence keeps its original source revision and criteria. The final job fails until the entire target is met, while measurement artifacts retain all losses. Earlier candidate workflow successes (before this gate was added) mean only that measurements and correctness checks completed.

Run the same qualification locally after retaining at least three full reports per architecture (and all additional repeats):

```sh
python3 scripts/benchmark_qualification.py --reports amd64-*/report.json arm64-*/report.json --output qualification.json --markdown qualification.md
```

The JSON and Markdown list every case, CPU/p50/p95/memory ratio range, required repeats and pixel regressions. Exit code 1 means the target is not met. Input corruption or incompatible evidence raises an error. The published 0.3.0 results below remain the immutable baseline for the pixel-agreement envelope; candidate qualification does not replace released-image measurements.

Publish a qualified candidate separately, retaining every full report from its cohort:

```sh
python3 scripts/candidate_snapshot.py --publish amd64-*/report.json arm64-*/report.json
python3 scripts/candidate_snapshot.py --check
```

The publisher rejects remaining losses, incomplete or incompatible evidence and duplicate samples before updating documentation. It stores all retained raw reports as deterministic gzip archives, with compressed/raw SHA-256 hashes and recomputed qualification in `benchmarks/results/candidate-index.json`. Separate generated candidate blocks include every case's timing/memory range, encoded sizes and decoded-pixel errors. The release baseline stays intact. CI checks archive integrity, provenance, recomputed metrics and documentation drift. Unqualified attempts remain in their workflow artifacts; no favorable subset can replace the full cohort.

The original baseline publisher accepts one full raw report per architecture. These commands maintain the historical 0.3.0 blocks; use `release_snapshot.py` above for new published-release observations:

```sh
python3 scripts/benchmark_report.py --reports amd64/report.json arm64/report.json --publish-results benchmarks/results --update-docs
python3 scripts/benchmark_report.py --check
python3 scripts/benchmark_report.py --markdown /tmp/pixstride-benchmarks.md --csv /tmp/pixstride-benchmarks.csv
```

Committed evidence contains losslessly compressed raw samples plus readable summaries. Publication requires identical input files, catalog, harness revision, image references and protocol across architectures, and recalculates statistics from samples. Generated Markdown blocks are checked for drift. Prose and output-quality limitations still require human review.

## Allocator diagnostics

After all qualification measurements, one ARM64 runner compares five allocator purge delays and end-of-request collection in separate fresh containers with the same two-CPU/1-GiB limits. This optional diagnostic runs native JPEG resize/blur/sharpen primitives with 6 warmups and 48 samples. It includes response deallocation and checks identical encoded output across policies; HTTP, Rust IPC and comparison against imgproxy are absent. Its JSON is marked diagnostic and cannot qualify or publish full benchmark results.

```sh
python3 scripts/performance_allocator.py --fixtures .benchmark-data --output /tmp/allocator-profile.json
```

The tool builds the current native builder stage and a local probe using the pinned allocator C source. It retains CPU/processing percentiles and lifetime cgroup peaks, removes owned containers/images, and never publishes an image. Formal qualification still requires the entire six-run, 25-case HTTP cohort after every runtime policy change.

### Allocator and Linux page-policy matrix

The separate [Allocator diagnostics workflow](.github/workflows/allocator-diagnostics.yml) runs a reusable primitive-only comparison on each native architecture:

```sh
python3 scripts/allocator_diagnostics.py --fixtures .benchmark-data --output /tmp/allocator-diagnostics.json
```

It compares 26 policies, each in fresh containers for JPEG blur and sharpen: glibc with one/two arenas, fixed 128-KiB/4-MiB mmap and trim thresholds, partial/default thread caches and disabled transparent huge pages; original glibc and pinned mimalloc controls retain inherited page policy. All 52 observations must contain valid counters, actual allocator/libc versions, process huge-page state, peak resident/current anonymous bytes and identical per-operation output hashes. Policy configurations are included in the report. The earlier 24-policy diagnostic also tested fully disabled thread caches and allocator collection; it remains in its workflow artifacts and does not qualify HTTP performance. Configurations run in a recorded deterministic shuffle, with the same prepared corpus on each runner, two CPUs and 1 GiB. The checked inputs and host/source identities accompany the diagnostic JSON; independently prepared architecture corpora are not a cross-architecture qualification cohort.

The workflow defaults to the `filters` profile on branch pushes; manual dispatch can choose `allocators`. The filter profile compares 8/16/32/64/128-row output strips with the selected System/one-arena/default-cache policy, two operations and ten fresh containers. It checks the reported strip height against each requested policy and requires identical encoded output across all heights. Run it locally on native Linux with:

```sh
python3 scripts/allocator_diagnostics.py --profile filters --fixtures .benchmark-data --output /tmp/filter-diagnostics.json
```

The service selects up to 32 rows for eligible JPEG filter output, bounded to 4 MiB per output strip for very wide images. This trades temporary working regions against repeated filter/halo preparation; input admission, requested filters, encoder quality and pixel math remain the same. The complete HTTP cohort, rather than this primitive diagnostic, establishes any comparison with imgproxy.

The [native filter diagnostic](https://github.com/lansbe/pixstride/actions/runs/43812425780), source `7dc1d103c167`, measured the following sharpen tradeoff. These are individual diagnostic processes with 48 samples each, without HTTP or an imgproxy comparison; all five tested strip heights produced identical encoded hashes for each operation on both architectures.

| Linux | Strip rows | Mean CPU/sample (ms) | Lifetime cgroup peak (MiB) |
| --- | ---: | ---: | ---: |
| AMD64 | 16 | 23.863 | 16.781 |
| AMD64 | 32 | 23.283 | 18.508 |
| AMD64 | 64 | 23.298 | 23.273 |
| ARM64 | 16 | 28.321 | 16.469 |
| ARM64 | 32 | 27.218 | 18.766 |
| ARM64 | 64 | 27.006 | 22.977 |

The selected 32-row cap reduces repeated filter work while limiting the additional working memory seen with 64 rows. It still needs the complete HTTP qualification; this table alone makes no claim of beating imgproxy.

The C adapter is compiled only into diagnostic probes; it is absent from the service. Its glibc collection adapter calls `malloc_trim` only for a requested collection trial. Huge-page suppression starts in the allocator environment before exec, then a process-local control disables later promotion; these settings affect each probe process, not the host. JSON is explicitly diagnostic and cannot qualify the 25 HTTP cases. After choosing a runtime policy, repeat the complete six-run HTTP cohort; diagnostic rankings cannot substitute for that evidence. The glibc 2.43 [release discussion](https://sourceware.org/pipermail/libc-alpha/2026-January/173985.html) records default AArch64 huge-page support; the [kernel controls](https://www.kernel.org/doc/html/latest/admin-guide/mm/transhuge.html) explain per-process disabling.

## Measured results

Current evidence: [published 0.4.0 index and all six raw archives](benchmarks/results/released-index.json). The following tables retain every case, both modes and the full observed ranges across three independent runs per architecture.

<!-- released:details:start -->
## Published release comparison

Published Pixstride 0.4.0 ([release](https://github.com/lansbe/pixstride/releases/tag/v0.4.0)) versus imgproxy 4.0.24: **target met** for the complete both-mode target. Every run and every loss is retained.

Image: `ghcr.io/lansbe/pixstride:0.4.0@sha256:eed8c44e8ad694d78eb450ec67b674d4bb3d6185039d969b673f7f3164e22690`. Runtime source: [`98868fa66288`](https://github.com/lansbe/pixstride/commit/98868fa66288a2b27a24116f29f82ece11a9cef2). Harness: [`0fd4f487c543`](https://github.com/lansbe/pixstride/commit/0fd4f487c543a8ad16b9d4f481de93c26527faed). These are published-image measurements; no local source build replaces the release.

Published Pixstride 0.4.0: **25/25 confirmed in both modes on both architectures**.

Default and forced-native CPU, HTTP p50/p95 and peak cgroup memory must be lower in every retained run. At least three independent runs per architecture are required, including near/noisy timings. Both modes retain the published pixel-agreement envelope; PNG identities stay exact. This does not certify perceptual equivalence.

### Linux amd64, default: 25/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.6789–0.7013× | 0.7429–0.7667× | 0.7318–0.7420× | 0.4548–0.5034× |
| 02-jpeg-thumbnail | confirmed-win | 0.8308–0.9202× | 0.8857–0.9376× | 0.8749–0.9133× | 0.6382–0.6501× |
| 03-jpeg-medium | confirmed-win | 0.7697–0.8222× | 0.8359–0.8798× | 0.8163–0.8547× | 0.5395–0.5606× |
| 04-jpeg-square | confirmed-win | 0.8451–0.8877× | 0.8952–0.9271× | 0.8831–0.9150× | 0.6144–0.6303× |
| 05-jpeg-retina | confirmed-win | 0.7825–0.8300× | 0.8577–0.8964× | 0.8554–0.8771× | 0.6035–0.6167× |
| 06-photo-crop | confirmed-win | 0.4084–0.4179× | 0.4644–0.4883× | 0.4644–0.4882× | 0.7923–0.8425× |
| 07-photo-rotate | confirmed-win | 0.7505–0.7629× | 0.7803–0.8139× | 0.6919–0.7131× | 0.5314–0.5420× |
| 08-photo-blur | confirmed-win | 0.7801–0.8668× | 0.8113–0.8786× | 0.7895–0.8598× | 0.7281–0.7396× |
| 09-photo-sharpen | confirmed-win | 0.7622–0.8211× | 0.7859–0.8300× | 0.7806–0.8238× | 0.8113–0.8353× |
| 10-png-identity | confirmed-win | 0.1193–0.1348× | 0.1426–0.1641× | 0.1436–0.1635× | 0.2794–0.2849× |
| 11-png-thumbnail | confirmed-win | 0.3813–0.4669× | 0.4193–0.5224× | 0.4185–0.5124× | 0.5546–0.5658× |
| 12-png-jpeg | confirmed-win | 0.4513–0.4986× | 0.4709–0.5351× | 0.4823–0.5249× | 0.5456–0.5517× |
| 13-png-bmp | confirmed-win | 0.4298–0.5369× | 0.4616–0.5717× | 0.4661–0.5690× | 0.5373–0.5853× |
| 14-photo-webp | confirmed-win | 0.8446–0.9063× | 0.8557–0.9142× | 0.8453–0.9024× | 0.6601–0.6821× |
| 15-earth-png | confirmed-win | 0.3631–0.3827× | 0.4941–0.5472× | 0.5026–0.5430× | 0.6958–0.7129× |
| 16-progressive-jpeg | confirmed-win | 0.8023–0.8328× | 0.8236–0.8675× | 0.7873–0.8358× | 0.5423–0.6237× |
| 17-alpha-identity | confirmed-win | 0.4102–0.4522× | 0.5086–0.5511× | 0.5642–0.6702× | 0.7104–0.7180× |
| 18-alpha-thumbnail | confirmed-win | 0.5545–0.5903× | 0.6950–0.7419× | 0.6882–0.7473× | 0.5926–0.6188× |
| 19-alpha-jpeg | confirmed-win | 0.3919–0.4532× | 0.4230–0.4648× | 0.4272–0.4914× | 0.5157–0.5422× |
| 20-alpha-webp | confirmed-win | 0.7764–0.8320× | 0.8010–0.8492× | 0.7994–0.8452× | 0.6918–0.7053× |
| 21-graphics-identity | confirmed-win | 0.0662–0.0782× | 0.0902–0.1026× | 0.0963–0.1014× | 0.1633–0.1717× |
| 22-graphics-square | confirmed-win | 0.5382–0.5867× | 0.6799–0.7484× | 0.6737–0.7386× | 0.6292–0.6563× |
| 23-grayscale-jpeg | confirmed-win | 0.4627–0.5485× | 0.4876–0.5642× | 0.4814–0.6010× | 0.6334–0.6403× |
| 24-tiff-png | confirmed-win | 0.3738–0.4043× | 0.4479–0.4893× | 0.4458–0.4768× | 0.6871–0.6926× |
| 25-svg-raster | confirmed-win | 0.5270–0.5700× | 0.5895–0.6225× | 0.5773–0.5883× | 0.4682–0.4722× |

### Linux amd64, forced-native: 25/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.6747–0.7011× | 0.7456–0.7651× | 0.7400–0.7520× | 0.4649–0.5073× |
| 02-jpeg-thumbnail | confirmed-win | 0.8313–0.9211× | 0.8854–0.9429× | 0.8857–0.9136× | 0.6347–0.6545× |
| 03-jpeg-medium | confirmed-win | 0.7700–0.8183× | 0.8325–0.8784× | 0.8178–0.8655× | 0.5369–0.5518× |
| 04-jpeg-square | confirmed-win | 0.8456–0.8820× | 0.8941–0.9219× | 0.8850–0.9044× | 0.6143–0.6314× |
| 05-jpeg-retina | confirmed-win | 0.7789–0.8204× | 0.8579–0.8956× | 0.8528–0.8768× | 0.6038–0.6194× |
| 06-photo-crop | confirmed-win | 0.4097–0.4190× | 0.4659–0.4884× | 0.4677–0.4886× | 0.7936–0.8511× |
| 07-photo-rotate | confirmed-win | 0.7516–0.7626× | 0.7838–0.8127× | 0.6933–0.7200× | 0.5291–0.5470× |
| 08-photo-blur | confirmed-win | 0.7726–0.8646× | 0.8036–0.8739× | 0.7877–0.8538× | 0.7323–0.7382× |
| 09-photo-sharpen | confirmed-win | 0.7628–0.8203× | 0.7844–0.8274× | 0.7849–0.8256× | 0.7958–0.8509× |
| 10-png-identity | confirmed-win | 0.1357–0.1527× | 0.1586–0.1817× | 0.1598–0.1820× | 0.4600–0.4673× |
| 11-png-thumbnail | confirmed-win | 0.3806–0.4673× | 0.4190–0.5226× | 0.4149–0.5138× | 0.5556–0.5666× |
| 12-png-jpeg | confirmed-win | 0.4486–0.4989× | 0.4663–0.5334× | 0.4639–0.5217× | 0.5306–0.5435× |
| 13-png-bmp | confirmed-win | 0.4293–0.5105× | 0.4607–0.5480× | 0.4716–0.5471× | 0.5665–0.5972× |
| 14-photo-webp | confirmed-win | 0.8448–0.9055× | 0.8570–0.9134× | 0.8463–0.9017× | 0.6574–0.6863× |
| 15-earth-png | confirmed-win | 0.3617–0.3779× | 0.4915–0.5424× | 0.4969–0.5351× | 0.6907–0.7123× |
| 16-progressive-jpeg | confirmed-win | 0.8024–0.8306× | 0.8110–0.8706× | 0.7853–0.8292× | 0.6121–0.6299× |
| 17-alpha-identity | confirmed-win | 0.3893–0.5279× | 0.5039–0.6610× | 0.5063–0.6686× | 0.7073–0.7188× |
| 18-alpha-thumbnail | confirmed-win | 0.5530–0.5797× | 0.6924–0.7393× | 0.7130–0.7284× | 0.5934–0.6216× |
| 19-alpha-jpeg | confirmed-win | 0.3881–0.4543× | 0.4183–0.4670× | 0.4159–0.4936× | 0.5176–0.5439× |
| 20-alpha-webp | confirmed-win | 0.7743–0.8371× | 0.8000–0.8527× | 0.7915–0.8550× | 0.6709–0.7118× |
| 21-graphics-identity | confirmed-win | 0.0680–0.0797× | 0.0920–0.1041× | 0.0985–0.1042× | 0.2727–0.2906× |
| 22-graphics-square | confirmed-win | 0.5375–0.5945× | 0.6809–0.7515× | 0.6732–0.7297× | 0.6294–0.6667× |
| 23-grayscale-jpeg | confirmed-win | 0.4628–0.5435× | 0.4878–0.5649× | 0.4905–0.6009× | 0.6367–0.6499× |
| 24-tiff-png | confirmed-win | 0.3766–0.4060× | 0.4497–0.4909× | 0.4569–0.4847× | 0.6874–0.6900× |
| 25-svg-raster | confirmed-win | 0.5272–0.5700× | 0.5896–0.6214× | 0.5803–0.5867× | 0.4623–0.4702× |

### Linux arm64, default: 25/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.7447–0.7505× | 0.7629–0.7729× | 0.7555–0.7801× | 0.4663–0.4704× |
| 02-jpeg-thumbnail | confirmed-win | 0.9287–0.9362× | 0.9498–0.9592× | 0.9359–0.9445× | 0.5940–0.6245× |
| 03-jpeg-medium | confirmed-win | 0.8772–0.8850× | 0.8985–0.9007× | 0.8727–0.8867× | 0.4449–0.5154× |
| 04-jpeg-square | confirmed-win | 0.9190–0.9288× | 0.9457–0.9549× | 0.9407–0.9437× | 0.5981–0.6209× |
| 05-jpeg-retina | confirmed-win | 0.8844–0.8898× | 0.9019–0.9064× | 0.8829–0.8987× | 0.5485–0.6027× |
| 06-photo-crop | confirmed-win | 0.5810–0.5943× | 0.6908–0.7057× | 0.6692–0.6840× | 0.6945–0.7670× |
| 07-photo-rotate | confirmed-win | 0.8383–0.8554× | 0.8286–0.8399× | 0.7601–0.8291× | 0.4975–0.5299× |
| 08-photo-blur | confirmed-win | 0.9363–0.9426× | 0.9444–0.9470× | 0.9329–0.9540× | 0.6739–0.6842× |
| 09-photo-sharpen | confirmed-win | 0.8611–0.8743× | 0.8722–0.8769× | 0.8668–0.8729× | 0.7344–0.7632× |
| 10-png-identity | confirmed-win | 0.1844–0.1853× | 0.2154–0.2187× | 0.2141–0.2171× | 0.2307–0.2511× |
| 11-png-thumbnail | confirmed-win | 0.7333–0.7636× | 0.7868–0.8276× | 0.7571–0.8102× | 0.4580–0.5111× |
| 12-png-jpeg | confirmed-win | 0.7906–0.8062× | 0.8445–0.8623× | 0.8117–0.8334× | 0.4395–0.5197× |
| 13-png-bmp | confirmed-win | 0.8107–0.8472× | 0.8394–0.8685× | 0.8439–0.8721× | 0.4761–0.4947× |
| 14-photo-webp | confirmed-win | 0.9059–0.9101× | 0.9123–0.9141× | 0.9038–0.9128× | 0.5338–0.6266× |
| 15-earth-png | confirmed-win | 0.5907–0.5969× | 0.8605–0.8729× | 0.7518–0.7865× | 0.5825–0.6196× |
| 16-progressive-jpeg | confirmed-win | 0.8832–0.9045× | 0.9072–0.9346× | 0.8444–0.9390× | 0.5108–0.5746× |
| 17-alpha-identity | confirmed-win | 0.4277–0.4601× | 0.4969–0.5468× | 0.4873–0.5432× | 0.6718–0.6799× |
| 18-alpha-thumbnail | confirmed-win | 0.7897–0.8216× | 0.9147–0.9495× | 0.8196–0.8691× | 0.5912–0.6060× |
| 19-alpha-jpeg | confirmed-win | 0.5654–0.6271× | 0.5854–0.6484× | 0.5848–0.6687× | 0.5003–0.5031× |
| 20-alpha-webp | confirmed-win | 0.8399–0.8442× | 0.8498–0.8518× | 0.8466–0.8539× | 0.5462–0.6356× |
| 21-graphics-identity | confirmed-win | 0.1078–0.1085× | 0.1316–0.1323× | 0.1291–0.1322× | 0.1474–0.1580× |
| 22-graphics-square | confirmed-win | 0.7976–0.8181× | 0.9176–0.9438× | 0.8412–0.8794× | 0.6098–0.6270× |
| 23-grayscale-jpeg | confirmed-win | 0.7628–0.7662× | 0.7704–0.7900× | 0.7851–0.8294× | 0.5711–0.5980× |
| 24-tiff-png | confirmed-win | 0.6227–0.6415× | 0.8343–0.8647× | 0.8089–0.8411× | 0.6243–0.6448× |
| 25-svg-raster | confirmed-win | 0.3602–0.3639× | 0.4146–0.4155× | 0.4113–0.4288× | 0.4275–0.4325× |

### Linux arm64, forced-native: 25/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.7460–0.7469× | 0.7663–0.7774× | 0.7550–0.7669× | 0.4753–0.4782× |
| 02-jpeg-thumbnail | confirmed-win | 0.9289–0.9359× | 0.9517–0.9538× | 0.9277–0.9404× | 0.5963–0.6200× |
| 03-jpeg-medium | confirmed-win | 0.8761–0.8813× | 0.8962–0.8998× | 0.8734–0.9115× | 0.4394–0.5155× |
| 04-jpeg-square | confirmed-win | 0.9230–0.9257× | 0.9487–0.9499× | 0.9339–0.9409× | 0.6043–0.6228× |
| 05-jpeg-retina | confirmed-win | 0.8818–0.8906× | 0.9021–0.9166× | 0.8839–0.8991× | 0.5521–0.6048× |
| 06-photo-crop | confirmed-win | 0.5844–0.5940× | 0.6935–0.6993× | 0.6745–0.6765× | 0.6924–0.7953× |
| 07-photo-rotate | confirmed-win | 0.8393–0.8563× | 0.8259–0.8424× | 0.7610–0.8290× | 0.4984–0.5323× |
| 08-photo-blur | confirmed-win | 0.9341–0.9410× | 0.9418–0.9504× | 0.9514–0.9575× | 0.6681–0.6800× |
| 09-photo-sharpen | confirmed-win | 0.8618–0.8707× | 0.8702–0.8768× | 0.8688–0.8721× | 0.7390–0.7583× |
| 10-png-identity | confirmed-win | 0.2094–0.2115× | 0.2395–0.2439× | 0.2382–0.2430× | 0.4080–0.4461× |
| 11-png-thumbnail | confirmed-win | 0.7605–0.7735× | 0.8198–0.8385× | 0.7811–0.8746× | 0.4583–0.5121× |
| 12-png-jpeg | confirmed-win | 0.7926–0.7992× | 0.8435–0.8574× | 0.8175–0.8294× | 0.4535–0.5142× |
| 13-png-bmp | confirmed-win | 0.7979–0.8415× | 0.8310–0.8667× | 0.8176–0.8748× | 0.4721–0.4955× |
| 14-photo-webp | confirmed-win | 0.9058–0.9099× | 0.9117–0.9136× | 0.9032–0.9120× | 0.5262–0.6208× |
| 15-earth-png | confirmed-win | 0.5903–0.5975× | 0.8606–0.8733× | 0.7513–0.7869× | 0.5780–0.6241× |
| 16-progressive-jpeg | confirmed-win | 0.9304–0.9352× | 0.9557–0.9677× | 0.9029–0.9802× | 0.4870–0.5633× |
| 17-alpha-identity | confirmed-win | 0.3864–0.3871× | 0.4481–0.4551× | 0.4371–0.4509× | 0.6704–0.6824× |
| 18-alpha-thumbnail | confirmed-win | 0.7957–0.8177× | 0.9178–0.9440× | 0.8365–0.8631× | 0.5769–0.6185× |
| 19-alpha-jpeg | confirmed-win | 0.6250–0.6450× | 0.6453–0.6676× | 0.6466–0.7355× | 0.4969–0.5044× |
| 20-alpha-webp | confirmed-win | 0.8367–0.8511× | 0.8464–0.8593× | 0.8388–0.8657× | 0.5507–0.6353× |
| 21-graphics-identity | confirmed-win | 0.1101–0.1108× | 0.1340–0.1345× | 0.1316–0.1336× | 0.2588–0.2759× |
| 22-graphics-square | confirmed-win | 0.8026–0.8164× | 0.9269–0.9461× | 0.8502–0.8816× | 0.5960–0.6297× |
| 23-grayscale-jpeg | confirmed-win | 0.7628–0.7639× | 0.7697–0.7920× | 0.7713–0.7981× | 0.5596–0.5944× |
| 24-tiff-png | confirmed-win | 0.6193–0.6353× | 0.8332–0.8489× | 0.8046–0.8514× | 0.6303–0.6440× |
| 25-svg-raster | confirmed-win | 0.3597–0.4483× | 0.4141–0.5012× | 0.4130–0.5132× | 0.4271–0.4297× |


amd64 CPU: AMD EPYC 7763 64-Core Processor, AMD EPYC 9V74 80-Core Processor, Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz. Two CPUs are assigned per service.

arm64 CPU: 0xd49. Two CPUs are assigned per service.

### Encoded size and decoded-pixel agreement

Every retained run and both modes. Color MAE/RMSE are the worst black/white composite errors in 8-bit channel units; alpha is checked separately. These are pairwise errors, not quality scores.

| Linux | Mode | Case | Encoded size ratio | Max color MAE | Max color RMSE | Max alpha MAE |
|---|---|---|---:|---:|---:|---:|
| amd64 | default | 01-jpeg-identity | 1.0074–1.0074× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 02-jpeg-thumbnail | 1.0307–1.0307× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 03-jpeg-medium | 1.0083–1.0083× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 04-jpeg-square | 1.0221–1.0221× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 05-jpeg-retina | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 06-photo-crop | 1.0094–1.0094× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 07-photo-rotate | 1.0072–1.0072× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 08-photo-blur | 1.0288–1.0288× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 09-photo-sharpen | 1.0102–1.0102× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 10-png-identity | 1.0141–1.0141× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 11-png-thumbnail | 1.0156–1.0156× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 12-png-jpeg | 1.0080–1.0080× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 13-png-bmp | 0.99994056–0.99994056× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 14-photo-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 15-earth-png | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 16-progressive-jpeg | 1.0099–1.0099× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 17-alpha-identity | 1.1763–1.1763× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 18-alpha-thumbnail | 1.0247–1.0247× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 19-alpha-jpeg | 1.0370–1.0370× | 1.308351 | 1.967487 | 0.000000 |
| amd64 | default | 20-alpha-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 21-graphics-identity | 0.9971–0.9971× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 22-graphics-square | 1.0567–1.0567× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 23-grayscale-jpeg | 1.0158–1.0158× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 24-tiff-png | 1.0087–1.0087× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 25-svg-raster | 0.9278–0.9278× | 0.009113 | 0.095461 | 0.000000 |
| amd64 | forced-native | 01-jpeg-identity | 1.0074–1.0074× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 02-jpeg-thumbnail | 1.0307–1.0307× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 03-jpeg-medium | 1.0083–1.0083× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 04-jpeg-square | 1.0221–1.0221× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 05-jpeg-retina | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 06-photo-crop | 1.0094–1.0094× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 07-photo-rotate | 1.0072–1.0072× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 08-photo-blur | 1.0288–1.0288× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 09-photo-sharpen | 1.0102–1.0102× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 10-png-identity | 1.0141–1.0141× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 11-png-thumbnail | 1.0156–1.0156× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 12-png-jpeg | 1.0080–1.0080× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 13-png-bmp | 0.99994056–0.99994056× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 14-photo-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 15-earth-png | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 16-progressive-jpeg | 1.0099–1.0099× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 17-alpha-identity | 1.1763–1.1763× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 18-alpha-thumbnail | 1.0247–1.0247× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 19-alpha-jpeg | 1.0370–1.0370× | 1.308351 | 1.967487 | 0.000000 |
| amd64 | forced-native | 20-alpha-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 21-graphics-identity | 0.9971–0.9971× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 22-graphics-square | 1.0567–1.0567× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 23-grayscale-jpeg | 1.0158–1.0158× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 24-tiff-png | 1.0087–1.0087× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 25-svg-raster | 0.9278–0.9278× | 0.009113 | 0.095461 | 0.000000 |
| arm64 | default | 01-jpeg-identity | 1.0074–1.0074× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 02-jpeg-thumbnail | 1.0307–1.0307× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 03-jpeg-medium | 1.0083–1.0083× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 04-jpeg-square | 1.0221–1.0221× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 05-jpeg-retina | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 06-photo-crop | 1.0094–1.0094× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 07-photo-rotate | 1.0072–1.0072× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 08-photo-blur | 1.0288–1.0288× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 09-photo-sharpen | 1.0104–1.0104× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 10-png-identity | 1.0141–1.0141× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 11-png-thumbnail | 1.0156–1.0156× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 12-png-jpeg | 1.0080–1.0080× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 13-png-bmp | 0.99994056–0.99994056× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 14-photo-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 15-earth-png | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 16-progressive-jpeg | 1.0099–1.0099× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 17-alpha-identity | 1.1763–1.1763× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 18-alpha-thumbnail | 1.0247–1.0247× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 19-alpha-jpeg | 1.0370–1.0370× | 1.308351 | 1.967487 | 0.000000 |
| arm64 | default | 20-alpha-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 21-graphics-identity | 0.9971–0.9971× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 22-graphics-square | 1.0567–1.0567× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 23-grayscale-jpeg | 1.0158–1.0158× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 24-tiff-png | 1.0087–1.0087× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 25-svg-raster | 0.9278–0.9278× | 0.009113 | 0.095461 | 0.000000 |
| arm64 | forced-native | 01-jpeg-identity | 1.0074–1.0074× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 02-jpeg-thumbnail | 1.0307–1.0307× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 03-jpeg-medium | 1.0083–1.0083× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 04-jpeg-square | 1.0221–1.0221× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 05-jpeg-retina | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 06-photo-crop | 1.0094–1.0094× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 07-photo-rotate | 1.0072–1.0072× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 08-photo-blur | 1.0288–1.0288× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 09-photo-sharpen | 1.0104–1.0104× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 10-png-identity | 1.0141–1.0141× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 11-png-thumbnail | 1.0156–1.0156× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 12-png-jpeg | 1.0080–1.0080× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 13-png-bmp | 0.99994056–0.99994056× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 14-photo-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 15-earth-png | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 16-progressive-jpeg | 1.0099–1.0099× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 17-alpha-identity | 1.1763–1.1763× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 18-alpha-thumbnail | 1.0247–1.0247× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 19-alpha-jpeg | 1.0370–1.0370× | 1.308351 | 1.967487 | 0.000000 |
| arm64 | forced-native | 20-alpha-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 21-graphics-identity | 0.9971–0.9971× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 22-graphics-square | 1.0567–1.0567× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 23-grayscale-jpeg | 1.0158–1.0158× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 24-tiff-png | 1.0087–1.0087× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 25-svg-raster | 0.9278–0.9278× | 0.009113 | 0.095461 | 0.000000 |

Raw reports, hashes, source/image identities and recomputed both-mode qualification: [published release index](benchmarks/results/released-index.json).

Retained workflow evidence: [run 1](https://github.com/lansbe/pixstride/actions/runs/43975836467).
<!-- released:details:end -->

<details>
<summary>Historical pre-release and 0.3.0 result tables</summary>

<!-- observed:details:start -->
Retained pre-release source cohort: **target not met**. Every retained run is included; these observations do not replace the historical qualified snapshot.

## Retained pre-release source cohort

Unreleased [candidate `20f202a5b5e1`](https://github.com/lansbe/pixstride/commit/20f202a5b5e17eb0d8e61795e87d04a77a0cac77); default and forced-native, all retained native Linux runs. The published 0.3.0 baseline below is unchanged.

Candidate `20f202a5b5e17eb0d8e61795e87d04a77a0cac77`: **target not met**.

Default and forced-native CPU, HTTP p50/p95 and peak cgroup memory must be lower in every retained run. At least three independent runs per architecture are required, including near/noisy timings. Both modes retain the published pixel-agreement envelope; PNG identities stay exact. This does not certify perceptual equivalence.

### Linux amd64, default: 25/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.6283–0.6781× | 0.7029–0.7529× | 0.6752–0.7661× | 0.4696–0.5022× |
| 02-jpeg-thumbnail | confirmed-win | 0.8305–0.8946× | 0.8864–0.9358× | 0.8365–0.9548× | 0.6508–0.6747× |
| 03-jpeg-medium | confirmed-win | 0.7620–0.7866× | 0.8252–0.8479× | 0.8173–0.8378× | 0.5585–0.5686× |
| 04-jpeg-square | confirmed-win | 0.8462–0.8909× | 0.8996–0.9119× | 0.8558–0.9173× | 0.6409–0.6679× |
| 05-jpeg-retina | confirmed-win | 0.7838–0.7904× | 0.8553–0.8641× | 0.8027–0.8516× | 0.5975–0.6214× |
| 06-photo-crop | confirmed-win | 0.4063–0.4243× | 0.4647–0.4719× | 0.4653–0.4694× | 0.8083–0.8336× |
| 07-photo-rotate | confirmed-win | 0.7417–0.7787× | 0.7715–0.8020× | 0.6980–0.7130× | 0.5209–0.5466× |
| 08-photo-blur | confirmed-win | 0.7870–0.8602× | 0.8251–0.8651× | 0.8098–0.8995× | 0.6969–0.7123× |
| 09-photo-sharpen | confirmed-win | 0.7761–0.8000× | 0.7971–0.8054× | 0.7968–0.8060× | 0.7554–0.8060× |
| 10-png-identity | confirmed-win | 0.1217–0.1237× | 0.1455–0.1469× | 0.1431–0.1491× | 0.2786–0.2940× |
| 11-png-thumbnail | confirmed-win | 0.4063–0.4410× | 0.4501–0.4899× | 0.4549–0.4990× | 0.5484–0.5504× |
| 12-png-jpeg | confirmed-win | 0.4637–0.4960× | 0.4912–0.5307× | 0.4904–0.5234× | 0.5260–0.5500× |
| 13-png-bmp | confirmed-win | 0.4499–0.4990× | 0.4845–0.5367× | 0.5001–0.5501× | 0.5722–0.5904× |
| 14-photo-webp | confirmed-win | 0.8482–0.9003× | 0.8588–0.9086× | 0.8441–0.9024× | 0.6366–0.6713× |
| 15-earth-png | confirmed-win | 0.3623–0.3744× | 0.4837–0.4975× | 0.4819–0.5054× | 0.6735–0.7081× |
| 16-progressive-jpeg | confirmed-win | 0.7835–0.8163× | 0.8086–0.8566× | 0.7740–0.8298× | 0.5262–0.5779× |
| 17-alpha-identity | confirmed-win | 0.3868–0.5567× | 0.4944–0.7010× | 0.5023–0.7896× | 0.7033–0.7387× |
| 18-alpha-thumbnail | confirmed-win | 0.5343–0.5533× | 0.6759–0.7092× | 0.6399–0.7004× | 0.6159–0.6453× |
| 19-alpha-jpeg | confirmed-win | 0.3881–0.5373× | 0.4170–0.5588× | 0.4156–0.5612× | 0.5184–0.5267× |
| 20-alpha-webp | confirmed-win | 0.7753–0.8205× | 0.7975–0.8440× | 0.7887–0.8351× | 0.6726–0.6856× |
| 21-graphics-identity | confirmed-win | 0.0734–0.0772× | 0.0983–0.1032× | 0.0993–0.1092× | 0.1651–0.1660× |
| 22-graphics-square | confirmed-win | 0.5417–0.6966× | 0.6893–0.8798× | 0.6873–0.8764× | 0.6073–0.6626× |
| 23-grayscale-jpeg | confirmed-win | 0.4531–0.4613× | 0.4766–0.4987× | 0.4521–0.5078× | 0.6264–0.6550× |
| 24-tiff-png | confirmed-win | 0.4000–0.4061× | 0.4622–0.4906× | 0.4574–0.4842× | 0.6807–0.6972× |
| 25-svg-raster | confirmed-win | 0.5229–0.6256× | 0.5895–0.6940× | 0.5625–0.6692× | 0.4541–0.4728× |

### Linux amd64, forced-native: 25/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.6392–0.6747× | 0.7130–0.7479× | 0.6773–0.7573× | 0.4712–0.4955× |
| 02-jpeg-thumbnail | confirmed-win | 0.8274–0.8930× | 0.8843–0.9360× | 0.8721–0.9358× | 0.6474–0.6725× |
| 03-jpeg-medium | confirmed-win | 0.7694–0.7819× | 0.8367–0.8413× | 0.8072–0.8356× | 0.5556–0.5745× |
| 04-jpeg-square | confirmed-win | 0.8459–0.8927× | 0.8960–0.9134× | 0.8633–0.9266× | 0.6410–0.6695× |
| 05-jpeg-retina | confirmed-win | 0.7863–0.8017× | 0.8569–0.8692× | 0.8031–0.8474× | 0.5958–0.6076× |
| 06-photo-crop | confirmed-win | 0.4092–0.4256× | 0.4659–0.4692× | 0.4642–0.4669× | 0.8072–0.8350× |
| 07-photo-rotate | confirmed-win | 0.7339–0.7762× | 0.7684–0.7988× | 0.6749–0.7197× | 0.5174–0.5541× |
| 08-photo-blur | confirmed-win | 0.7842–0.8607× | 0.8227–0.8784× | 0.8070–0.8533× | 0.6931–0.7139× |
| 09-photo-sharpen | confirmed-win | 0.7800–0.8008× | 0.8039–0.8141× | 0.7935–0.8220× | 0.7438–0.8090× |
| 10-png-identity | confirmed-win | 0.1379–0.1407× | 0.1610–0.1637× | 0.1599–0.1640× | 0.4532–0.4773× |
| 11-png-thumbnail | confirmed-win | 0.4051–0.4445× | 0.4476–0.4915× | 0.4538–0.4923× | 0.5481–0.5512× |
| 12-png-jpeg | confirmed-win | 0.4609–0.4970× | 0.4892–0.5314× | 0.4812–0.5275× | 0.5337–0.5395× |
| 13-png-bmp | confirmed-win | 0.4504–0.4935× | 0.4835–0.5324× | 0.4965–0.5345× | 0.5722–0.5954× |
| 14-photo-webp | confirmed-win | 0.8494–0.9021× | 0.8593–0.9134× | 0.8473–0.9108× | 0.6420–0.6641× |
| 15-earth-png | confirmed-win | 0.3607–0.3821× | 0.4924–0.4954× | 0.4957–0.5295× | 0.6727–0.7126× |
| 16-progressive-jpeg | confirmed-win | 0.7841–0.8256× | 0.8146–0.8617× | 0.7685–0.8142× | 0.5230–0.5794× |
| 17-alpha-identity | confirmed-win | 0.3876–0.5970× | 0.4960–0.7833× | 0.5027–0.8243× | 0.7033–0.7365× |
| 18-alpha-thumbnail | confirmed-win | 0.5351–0.5524× | 0.6748–0.7093× | 0.6493–0.6928× | 0.6062–0.6327× |
| 19-alpha-jpeg | confirmed-win | 0.3857–0.5384× | 0.4118–0.5610× | 0.4155–0.5443× | 0.5136–0.5324× |
| 20-alpha-webp | confirmed-win | 0.7750–0.8211× | 0.7968–0.8453× | 0.7896–0.8270× | 0.6579–0.6912× |
| 21-graphics-identity | confirmed-win | 0.0752–0.0799× | 0.1007–0.1087× | 0.1008–0.1087× | 0.2698–0.2785× |
| 22-graphics-square | confirmed-win | 0.5444–0.7250× | 0.6876–0.8949× | 0.6780–0.8963× | 0.6046–0.6833× |
| 23-grayscale-jpeg | confirmed-win | 0.4499–0.4608× | 0.4745–0.4991× | 0.4548–0.4912× | 0.6238–0.6530× |
| 24-tiff-png | confirmed-win | 0.3987–0.4084× | 0.4699–0.4933× | 0.4566–0.4888× | 0.6801–0.6968× |
| 25-svg-raster | confirmed-win | 0.5227–0.6146× | 0.5889–0.6935× | 0.5535–0.6491× | 0.4493–0.4749× |

### Linux arm64, default: 24/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.7408–0.7445× | 0.7631–0.7707× | 0.7395–0.7551× | 0.4655–0.4951× |
| 02-jpeg-thumbnail | confirmed-win | 0.9141–0.9231× | 0.9373–0.9452× | 0.9257–0.9418× | 0.5913–0.6403× |
| 03-jpeg-medium | confirmed-win | 0.8771–0.8822× | 0.8935–0.8971× | 0.8755–0.8970× | 0.4587–0.5449× |
| 04-jpeg-square | confirmed-win | 0.9236–0.9254× | 0.9443–0.9508× | 0.9414–0.9455× | 0.6092–0.6191× |
| 05-jpeg-retina | confirmed-win | 0.8848–0.8895× | 0.9098–0.9169× | 0.8973–0.9014× | 0.5633–0.5885× |
| 06-photo-crop | confirmed-win | 0.5885–0.5976× | 0.6977–0.7119× | 0.6777–0.6979× | 0.6982–0.7631× |
| 07-photo-rotate | confirmed-win | 0.8364–0.8596× | 0.8360–0.8470× | 0.7511–0.8390× | 0.4584–0.5353× |
| 08-photo-blur | confirmed-win | 0.9329–0.9357× | 0.9435–0.9487× | 0.9444–0.9495× | 0.6494–0.7005× |
| 09-photo-sharpen | confirmed-win | 0.8632–0.8703× | 0.8701–0.8717× | 0.8575–0.8745× | 0.7192–0.8001× |
| 10-png-identity | confirmed-win | 0.1834–0.1868× | 0.2148–0.2187× | 0.2132–0.2205× | 0.2190–0.2422× |
| 11-png-thumbnail | confirmed-win | 0.7633–0.7741× | 0.8214–0.8337× | 0.8026–0.8720× | 0.4484–0.5389× |
| 12-png-jpeg | confirmed-win | 0.7956–0.8166× | 0.8461–0.8782× | 0.8242–0.8491× | 0.5041–0.5220× |
| 13-png-bmp | confirmed-win | 0.8150–0.8409× | 0.8484–0.8670× | 0.8482–0.8916× | 0.5048–0.5550× |
| 14-photo-webp | confirmed-win | 0.9031–0.9087× | 0.9089–0.9144× | 0.9018–0.9090× | 0.5442–0.6608× |
| 15-earth-png | confirmed-win | 0.5891–0.6008× | 0.8602–0.8744× | 0.7731–0.7920× | 0.6079–0.6232× |
| 16-progressive-jpeg | confirmed-win | 0.8872–0.8944× | 0.9108–0.9263× | 0.8571–0.9578× | 0.5289–0.5643× |
| 17-alpha-identity | confirmed-win | 0.3896–0.4797× | 0.4587–0.5733× | 0.4618–0.5879× | 0.6523–0.6799× |
| 18-alpha-thumbnail | confirmed-win | 0.7986–0.8392× | 0.9128–0.9765× | 0.8636–0.8763× | 0.5741–0.6028× |
| 19-alpha-jpeg | confirmed-win | 0.5876–0.6421× | 0.6092–0.6563× | 0.6127–0.6713× | 0.4783–0.5304× |
| 20-alpha-webp | confirmed-win | 0.8336–0.8454× | 0.8464–0.8535× | 0.8344–0.8508× | 0.5628–0.6346× |
| 21-graphics-identity | confirmed-win | 0.1075–0.1084× | 0.1315–0.1340× | 0.1304–0.1336× | 0.1562–0.1675× |
| 22-graphics-square | loss | 0.8328–0.8544× | 0.9706–0.9885× | 0.889323–1.007080× | 0.5927–0.6173× |
| 23-grayscale-jpeg | confirmed-win | 0.7580–0.7762× | 0.7806–0.7996× | 0.7864–0.8202× | 0.5992–0.6224× |
| 24-tiff-png | confirmed-win | 0.6421–0.6565× | 0.8618–0.8858× | 0.8522–0.8709× | 0.5806–0.6772× |
| 25-svg-raster | confirmed-win | 0.3448–0.3521× | 0.3996–0.4168× | 0.3730–0.4665× | 0.4285–0.4463× |

### Linux arm64, forced-native: 25/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.7416–0.7478× | 0.7576–0.7753× | 0.7307–0.7588× | 0.4740–0.5133× |
| 02-jpeg-thumbnail | confirmed-win | 0.9145–0.9233× | 0.9408–0.9478× | 0.9275–0.9364× | 0.5898–0.6455× |
| 03-jpeg-medium | confirmed-win | 0.8779–0.8803× | 0.8916–0.9028× | 0.8754–0.8843× | 0.4552–0.5434× |
| 04-jpeg-square | confirmed-win | 0.9242–0.9282× | 0.9469–0.9494× | 0.9362–0.9519× | 0.6038–0.6186× |
| 05-jpeg-retina | confirmed-win | 0.8827–0.8913× | 0.9118–0.9141× | 0.8763–0.9101× | 0.5599–0.5873× |
| 06-photo-crop | confirmed-win | 0.5851–0.6038× | 0.6946–0.7100× | 0.6749–0.7001× | 0.7163–0.7546× |
| 07-photo-rotate | confirmed-win | 0.8374–0.8602× | 0.8373–0.8480× | 0.7464–0.8544× | 0.4638–0.5310× |
| 08-photo-blur | confirmed-win | 0.9340–0.9349× | 0.9428–0.9473× | 0.9399–0.9524× | 0.6556–0.6948× |
| 09-photo-sharpen | confirmed-win | 0.8610–0.8660× | 0.8655–0.8728× | 0.8551–0.8631× | 0.7193–0.7959× |
| 10-png-identity | confirmed-win | 0.2095–0.2211× | 0.2406–0.2503× | 0.2380–0.2488× | 0.3861–0.4097× |
| 11-png-thumbnail | confirmed-win | 0.7472–0.7850× | 0.8046–0.8497× | 0.7781–0.8199× | 0.4497–0.5361× |
| 12-png-jpeg | confirmed-win | 0.7958–0.8117× | 0.8397–0.8729× | 0.8261–0.8503× | 0.5011–0.5331× |
| 13-png-bmp | confirmed-win | 0.8245–0.8445× | 0.8509–0.8860× | 0.8673–0.8871× | 0.5004–0.5580× |
| 14-photo-webp | confirmed-win | 0.9026–0.9080× | 0.9087–0.9142× | 0.9022–0.9088× | 0.5519–0.6537× |
| 15-earth-png | confirmed-win | 0.5891–0.5995× | 0.8601–0.8723× | 0.7734–0.7807× | 0.6110–0.6258× |
| 16-progressive-jpeg | confirmed-win | 0.8785–0.9382× | 0.9092–0.9651× | 0.8643–0.9676× | 0.5325–0.5436× |
| 17-alpha-identity | confirmed-win | 0.3850–0.4352× | 0.4495–0.5067× | 0.4429–0.5043× | 0.6574–0.6785× |
| 18-alpha-thumbnail | confirmed-win | 0.7865–0.8294× | 0.9101–0.9602× | 0.8216–0.8830× | 0.5604–0.5996× |
| 19-alpha-jpeg | confirmed-win | 0.6282–0.6619× | 0.6498–0.6850× | 0.6610–0.6968× | 0.4874–0.5306× |
| 20-alpha-webp | confirmed-win | 0.8366–0.8462× | 0.8479–0.8517× | 0.8437–0.8634× | 0.5849–0.6298× |
| 21-graphics-identity | confirmed-win | 0.1094–0.1107× | 0.1336–0.1365× | 0.1329–0.1357× | 0.2652–0.2799× |
| 22-graphics-square | confirmed-win | 0.8347–0.8513× | 0.9710–0.9883× | 0.8978–0.9078× | 0.5924–0.6167× |
| 23-grayscale-jpeg | confirmed-win | 0.7596–0.7643× | 0.7838–0.7909× | 0.7977–0.8005× | 0.5906–0.6281× |
| 24-tiff-png | confirmed-win | 0.6313–0.6623× | 0.8468–0.8932× | 0.8292–0.8930× | 0.5801–0.6755× |
| 25-svg-raster | confirmed-win | 0.3451–0.3548× | 0.4021–0.4178× | 0.3835–0.4170× | 0.4277–0.4460× |


amd64 CPU: AMD EPYC 7763 64-Core Processor, AMD EPYC 9V45 96-Core Processor. Two CPUs are assigned per service.

arm64 CPU: 0xd49. Two CPUs are assigned per service.

### Encoded size and decoded-pixel agreement

Every retained run and both modes. Color MAE/RMSE are the worst black/white composite errors in 8-bit channel units; alpha is checked separately. These are pairwise errors, not quality scores.

| Linux | Mode | Case | Encoded size ratio | Max color MAE | Max color RMSE | Max alpha MAE |
|---|---|---|---:|---:|---:|---:|
| amd64 | default | 01-jpeg-identity | 1.0074–1.0074× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 02-jpeg-thumbnail | 1.0307–1.0307× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 03-jpeg-medium | 1.0083–1.0083× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 04-jpeg-square | 1.0221–1.0221× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 05-jpeg-retina | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 06-photo-crop | 1.0094–1.0094× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 07-photo-rotate | 1.0072–1.0072× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 08-photo-blur | 1.0288–1.0288× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 09-photo-sharpen | 1.0102–1.0102× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 10-png-identity | 1.0141–1.0141× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 11-png-thumbnail | 1.0156–1.0156× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 12-png-jpeg | 1.0080–1.0080× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 13-png-bmp | 0.99994056–0.99994056× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 14-photo-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 15-earth-png | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 16-progressive-jpeg | 1.0099–1.0099× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 17-alpha-identity | 1.1763–1.1763× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 18-alpha-thumbnail | 1.0247–1.0247× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 19-alpha-jpeg | 1.0370–1.0370× | 1.308351 | 1.967487 | 0.000000 |
| amd64 | default | 20-alpha-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 21-graphics-identity | 0.9971–0.9971× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 22-graphics-square | 1.0567–1.0567× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 23-grayscale-jpeg | 1.0158–1.0158× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 24-tiff-png | 1.0087–1.0087× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | default | 25-svg-raster | 0.9278–0.9278× | 0.009113 | 0.095461 | 0.000000 |
| amd64 | forced-native | 01-jpeg-identity | 1.0074–1.0074× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 02-jpeg-thumbnail | 1.0307–1.0307× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 03-jpeg-medium | 1.0083–1.0083× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 04-jpeg-square | 1.0221–1.0221× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 05-jpeg-retina | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 06-photo-crop | 1.0094–1.0094× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 07-photo-rotate | 1.0072–1.0072× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 08-photo-blur | 1.0288–1.0288× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 09-photo-sharpen | 1.0102–1.0102× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 10-png-identity | 1.0141–1.0141× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 11-png-thumbnail | 1.0156–1.0156× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 12-png-jpeg | 1.0080–1.0080× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 13-png-bmp | 0.99994056–0.99994056× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 14-photo-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 15-earth-png | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 16-progressive-jpeg | 1.0099–1.0099× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 17-alpha-identity | 1.1763–1.1763× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 18-alpha-thumbnail | 1.0247–1.0247× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 19-alpha-jpeg | 1.0370–1.0370× | 1.308351 | 1.967487 | 0.000000 |
| amd64 | forced-native | 20-alpha-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 21-graphics-identity | 0.9971–0.9971× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 22-graphics-square | 1.0567–1.0567× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 23-grayscale-jpeg | 1.0158–1.0158× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 24-tiff-png | 1.0087–1.0087× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | forced-native | 25-svg-raster | 0.9278–0.9278× | 0.009113 | 0.095461 | 0.000000 |
| arm64 | default | 01-jpeg-identity | 1.0074–1.0074× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 02-jpeg-thumbnail | 1.0307–1.0307× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 03-jpeg-medium | 1.0083–1.0083× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 04-jpeg-square | 1.0221–1.0221× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 05-jpeg-retina | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 06-photo-crop | 1.0094–1.0094× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 07-photo-rotate | 1.0072–1.0072× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 08-photo-blur | 1.0288–1.0288× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 09-photo-sharpen | 1.0104–1.0104× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 10-png-identity | 1.0141–1.0141× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 11-png-thumbnail | 1.0156–1.0156× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 12-png-jpeg | 1.0080–1.0080× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 13-png-bmp | 0.99994056–0.99994056× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 14-photo-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 15-earth-png | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 16-progressive-jpeg | 1.0099–1.0099× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 17-alpha-identity | 1.1763–1.1763× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 18-alpha-thumbnail | 1.0247–1.0247× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 19-alpha-jpeg | 1.0370–1.0370× | 1.308351 | 1.967487 | 0.000000 |
| arm64 | default | 20-alpha-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 21-graphics-identity | 0.9971–0.9971× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 22-graphics-square | 1.0567–1.0567× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 23-grayscale-jpeg | 1.0158–1.0158× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 24-tiff-png | 1.0087–1.0087× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | default | 25-svg-raster | 0.9278–0.9278× | 0.009113 | 0.095461 | 0.000000 |
| arm64 | forced-native | 01-jpeg-identity | 1.0074–1.0074× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 02-jpeg-thumbnail | 1.0307–1.0307× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 03-jpeg-medium | 1.0083–1.0083× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 04-jpeg-square | 1.0221–1.0221× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 05-jpeg-retina | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 06-photo-crop | 1.0094–1.0094× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 07-photo-rotate | 1.0072–1.0072× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 08-photo-blur | 1.0288–1.0288× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 09-photo-sharpen | 1.0104–1.0104× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 10-png-identity | 1.0141–1.0141× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 11-png-thumbnail | 1.0156–1.0156× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 12-png-jpeg | 1.0080–1.0080× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 13-png-bmp | 0.99994056–0.99994056× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 14-photo-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 15-earth-png | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 16-progressive-jpeg | 1.0099–1.0099× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 17-alpha-identity | 1.1763–1.1763× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 18-alpha-thumbnail | 1.0247–1.0247× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 19-alpha-jpeg | 1.0370–1.0370× | 1.308351 | 1.967487 | 0.000000 |
| arm64 | forced-native | 20-alpha-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 21-graphics-identity | 0.9971–0.9971× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 22-graphics-square | 1.0567–1.0567× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 23-grayscale-jpeg | 1.0158–1.0158× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 24-tiff-png | 1.0087–1.0087× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | forced-native | 25-svg-raster | 0.9278–0.9278× | 0.009113 | 0.095461 | 0.000000 |

Raw reports, hashes, source/image identities and recomputed both-mode qualification: [candidate index](benchmarks/results/observed-index.json).

Retained workflow evidence: [run 1](https://github.com/lansbe/pixstride/actions/runs/43820275305).
<!-- observed:details:end -->

## Historical qualified candidate tables

<!-- candidate:details:start -->
## Unreleased candidate confirmation

Unreleased [candidate `6145080e14a5`](https://github.com/lansbe/pixstride/commit/6145080e14a5e717a7a85ae9fd7174f894e08e39); default routing, all retained native runs. The published 0.3.0 baseline below is unchanged.

Candidate `6145080e14a5e717a7a85ae9fd7174f894e08e39`: **25/25 confirmed on both architectures**.

Default routing only; CPU, HTTP p50/p95 and peak cgroup memory must be lower in every retained run. Close timings (within 5%) or noisy CPU require at least three runs. Both modes retain the published decoded-pixel error envelope; PNG identities stay exact. This pixel agreement check does not certify perceptual equivalence.

### Linux amd64: 25/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.676–0.701× | 0.749–0.761× | 0.742–0.756× | 0.492–0.583× |
| 02-jpeg-thumbnail | confirmed-win | 0.878–0.927× | 0.913–0.957× | 0.889–0.941× | 0.671–0.704× |
| 03-jpeg-medium | confirmed-win | 0.809–0.823× | 0.868–0.877× | 0.825–0.869× | 0.718–0.756× |
| 04-jpeg-square | confirmed-win | 0.877–0.912× | 0.910–0.945× | 0.884–0.941× | 0.729–0.763× |
| 05-jpeg-retina | confirmed-win | 0.823–0.844× | 0.887–0.899× | 0.867–0.876× | 0.713–0.720× |
| 06-photo-crop | confirmed-win | 0.414–0.445× | 0.464–0.491× | 0.465–0.493× | 0.781–0.829× |
| 07-photo-rotate | confirmed-win | 0.733–0.784× | 0.737–0.817× | 0.681–0.730× | 0.594–0.623× |
| 08-photo-blur | confirmed-win | 0.867–0.899× | 0.891–0.909× | 0.862–0.897× | 0.800–0.877× |
| 09-photo-sharpen | confirmed-win | 0.821–0.883× | 0.851–0.887× | 0.838–0.880× | 0.922–0.974× |
| 10-png-identity | confirmed-win | 0.120–0.136× | 0.142–0.163× | 0.142–0.163× | 0.270–0.293× |
| 11-png-thumbnail | confirmed-win | 0.371–0.475× | 0.408–0.530× | 0.401–0.525× | 0.610–0.620× |
| 12-png-jpeg | confirmed-win | 0.437–0.501× | 0.454–0.533× | 0.450–0.531× | 0.586–0.599× |
| 13-png-bmp | confirmed-win | 0.417–0.512× | 0.447–0.549× | 0.450–0.554× | 0.603–0.633× |
| 14-photo-webp | confirmed-win | 0.877–0.917× | 0.885–0.925× | 0.876–0.924× | 0.816–0.834× |
| 15-earth-png | confirmed-win | 0.370–0.442× | 0.498–0.562× | 0.480–0.539× | 0.692–0.728× |
| 16-progressive-jpeg | confirmed-win | 0.852–0.878× | 0.890–0.909× | 0.779–0.905× | 0.782–0.801× |
| 17-alpha-identity | confirmed-win | 0.482–0.746× | 0.625–0.871× | 0.644–0.875× | 0.601–0.606× |
| 18-alpha-thumbnail | confirmed-win | 0.526–0.666× | 0.662–0.808× | 0.671–0.721× | 0.606–0.615× |
| 19-alpha-jpeg | confirmed-win | 0.424–0.452× | 0.446–0.470× | 0.453–0.471× | 0.532–0.540× |
| 20-alpha-webp | confirmed-win | 0.814–0.861× | 0.834–0.873× | 0.834–0.872× | 0.891–0.896× |
| 21-graphics-identity | confirmed-win | 0.066–0.102× | 0.090–0.128× | 0.095–0.130× | 0.160–0.176× |
| 22-graphics-square | confirmed-win | 0.545–0.698× | 0.680–0.778× | 0.687–0.786× | 0.672–0.685× |
| 23-grayscale-jpeg | confirmed-win | 0.473–0.542× | 0.494–0.566× | 0.474–0.571× | 0.669–0.678× |
| 24-tiff-png | confirmed-win | 0.375–0.425× | 0.446–0.490× | 0.447–0.500× | 0.682–0.712× |
| 25-svg-raster | confirmed-win | 0.563–0.589× | 0.609–0.652× | 0.587–0.719× | 0.447–0.478× |

### Linux arm64: 25/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.735–0.741× | 0.759–0.769× | 0.735–0.747× | 0.553–0.630× |
| 02-jpeg-thumbnail | confirmed-win | 0.918–0.925× | 0.935–0.945× | 0.925–0.941× | 0.741–0.783× |
| 03-jpeg-medium | confirmed-win | 0.873–0.876× | 0.886–0.888× | 0.877–0.881× | 0.676–0.790× |
| 04-jpeg-square | confirmed-win | 0.914–0.923× | 0.937–0.945× | 0.925–0.938× | 0.747–0.824× |
| 05-jpeg-retina | confirmed-win | 0.883–0.886× | 0.897–0.913× | 0.897–0.906× | 0.744–0.815× |
| 06-photo-crop | confirmed-win | 0.532–0.548× | 0.630–0.652× | 0.608–0.633× | 0.699–0.779× |
| 07-photo-rotate | confirmed-win | 0.763–0.789× | 0.771–0.778× | 0.692–0.759× | 0.553–0.632× |
| 08-photo-blur | confirmed-win | 0.950–0.960× | 0.961–0.971× | 0.958–0.974× | 0.838–0.992× |
| 09-photo-sharpen | confirmed-win | 0.891–0.895× | 0.895–0.899× | 0.898–0.903× | 0.919–1.000× |
| 10-png-identity | confirmed-win | 0.183–0.184× | 0.214–0.217× | 0.213–0.214× | 0.217–0.261× |
| 11-png-thumbnail | confirmed-win | 0.706–0.719× | 0.762–0.777× | 0.727–0.751× | 0.590–0.613× |
| 12-png-jpeg | confirmed-win | 0.752–0.764× | 0.800–0.813× | 0.769–0.788× | 0.494–0.742× |
| 13-png-bmp | confirmed-win | 0.761–0.786× | 0.795–0.820× | 0.783–0.810× | 0.536–0.613× |
| 14-photo-webp | confirmed-win | 0.903–0.905× | 0.910–0.912× | 0.902–0.906× | 0.720–0.781× |
| 15-earth-png | confirmed-win | 0.588–0.592× | 0.859–0.861× | 0.765–0.776× | 0.628–0.697× |
| 16-progressive-jpeg | confirmed-win | 0.837–0.880× | 0.874–0.903× | 0.808–0.898× | 0.755–0.849× |
| 17-alpha-identity | confirmed-win | 0.393–0.398× | 0.457–0.463× | 0.449–0.458× | 0.694–0.792× |
| 18-alpha-thumbnail | confirmed-win | 0.801–0.821× | 0.916–0.945× | 0.828–0.876× | 0.648–0.702× |
| 19-alpha-jpeg | confirmed-win | 0.500–0.509× | 0.516–0.526× | 0.518–0.530× | 0.614–0.671× |
| 20-alpha-webp | confirmed-win | 0.840–0.849× | 0.846–0.856× | 0.847–0.863× | 0.746–0.797× |
| 21-graphics-identity | confirmed-win | 0.108–0.109× | 0.132–0.132× | 0.129–0.130× | 0.179–0.224× |
| 22-graphics-square | confirmed-win | 0.688–0.726× | 0.798–0.842× | 0.731–0.791× | 0.733–0.757× |
| 23-grayscale-jpeg | confirmed-win | 0.751–0.756× | 0.760–0.777× | 0.764–0.773× | 0.788–0.851× |
| 24-tiff-png | confirmed-win | 0.546–0.568× | 0.734–0.768× | 0.709–0.743× | 0.644–0.670× |
| 25-svg-raster | confirmed-win | 0.359–0.363× | 0.410–0.415× | 0.403–0.416× | 0.489–0.543× |


amd64 CPU: AMD EPYC 9V74 80-Core Processor, Intel(R) Xeon(R) 6975P-C, Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz. Two CPUs are assigned per service.

arm64 CPU: 0xd49. Two CPUs are assigned per service.

### Encoded size and decoded-pixel agreement

Default routing; ranges cover every retained run. Color error is the worst black/white composite MAE/RMSE against imgproxy; alpha is checked separately. Both modes must pass the pixel gate. These are pairwise errors, not quality scores.

| Linux | Case | Encoded size ratio | Max color MAE | Max color RMSE | Max alpha MAE |
|---|---|---:|---:|---:|---:|
| amd64 | 01-jpeg-identity | 1.0074–1.0074× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 02-jpeg-thumbnail | 1.0307–1.0307× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 03-jpeg-medium | 1.0083–1.0083× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 04-jpeg-square | 1.0221–1.0221× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 05-jpeg-retina | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 06-photo-crop | 1.0094–1.0094× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 07-photo-rotate | 1.0072–1.0072× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 08-photo-blur | 1.0288–1.0288× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 09-photo-sharpen | 1.0102–1.0102× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 10-png-identity | 1.0141–1.0141× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 11-png-thumbnail | 1.0156–1.0156× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 12-png-jpeg | 1.0080–1.0080× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 13-png-bmp | 0.99994056–0.99994056× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 14-photo-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 15-earth-png | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 16-progressive-jpeg | 1.0099–1.0099× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 17-alpha-identity | 1.1763–1.1763× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 18-alpha-thumbnail | 1.0247–1.0247× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 19-alpha-jpeg | 1.0370–1.0370× | 1.308351 | 1.967487 | 0.000000 |
| amd64 | 20-alpha-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 21-graphics-identity | 0.9971–0.9971× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 22-graphics-square | 1.0567–1.0567× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 23-grayscale-jpeg | 1.0158–1.0158× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 24-tiff-png | 1.0087–1.0087× | 0.000000 | 0.000000 | 0.000000 |
| amd64 | 25-svg-raster | 0.9278–0.9278× | 0.009113 | 0.095461 | 0.000000 |
| arm64 | 01-jpeg-identity | 1.0074–1.0074× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 02-jpeg-thumbnail | 1.0307–1.0307× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 03-jpeg-medium | 1.0083–1.0083× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 04-jpeg-square | 1.0221–1.0221× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 05-jpeg-retina | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 06-photo-crop | 1.0094–1.0094× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 07-photo-rotate | 1.0072–1.0072× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 08-photo-blur | 1.0288–1.0288× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 09-photo-sharpen | 1.0104–1.0104× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 10-png-identity | 1.0141–1.0141× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 11-png-thumbnail | 1.0156–1.0156× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 12-png-jpeg | 1.0080–1.0080× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 13-png-bmp | 0.99994056–0.99994056× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 14-photo-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 15-earth-png | 1.0098–1.0098× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 16-progressive-jpeg | 1.0099–1.0099× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 17-alpha-identity | 1.1763–1.1763× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 18-alpha-thumbnail | 1.0247–1.0247× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 19-alpha-jpeg | 1.0370–1.0370× | 1.308351 | 1.967487 | 0.000000 |
| arm64 | 20-alpha-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 21-graphics-identity | 0.9971–0.9971× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 22-graphics-square | 1.0567–1.0567× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 23-grayscale-jpeg | 1.0158–1.0158× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 24-tiff-png | 1.0087–1.0087× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 25-svg-raster | 0.9278–0.9278× | 0.009113 | 0.095461 | 0.000000 |

Raw reports, hashes, source/image identities and recomputed qualification: [candidate index](benchmarks/results/candidate-index.json).

Retained workflow evidence: [run 1](https://github.com/lansbe/pixstride/actions/runs/43766833208).
<!-- candidate:details:end -->

## Historical published 0.3.0 baseline

Evidence: [AMD64 summary](benchmarks/results/linux-amd64.json) / [raw samples](benchmarks/results/linux-amd64.raw.json.gz), [ARM64 summary](benchmarks/results/linux-arm64.json) / [raw samples](benchmarks/results/linux-arm64.raw.json.gz).

<!-- benchmark:tables:start -->
### Linux amd64

CPU identifiers: Intel(R) Xeon(R) 6973P-C; 4 logical CPUs, two selected; Ubuntu 24.04.5 LTS, kernel 7.0.0-1017-azure. [Measurement run](https://github.com/lansbe/pixstride/actions/runs/43647143217).

6 batches × 24 timed requests per case/mode, after 5 warmups.

Harness `820a33d7768d57fff29a1e1cf1e119313c7eeb65`; corpus SHA-256 `adbe2d78914554327b17d40ea793d747fcc00a9499e4b1085b7a40cd2b8dd9c1`.

Each triple is **Pixstride default / Pixstride native / imgproxy**. Pixel MAE is **default / native**, taking the larger black/white composite error against imgproxy. Memory is the fresh container lifetime cgroup peak, including startup, warmups, file cache and kernel memory.

| Case | CPU ms/request | HTTP p50 ms | HTTP p95 ms | Peak cgroup MiB | Output KiB | Pixel MAE |
|---|---:|---:|---:|---:|---:|---:|
| 01-jpeg-identity: Photo JPEG identity | 106.60 / 47.36 / 29.98 | 105.86 / 47.16 / 19.55 | 119.18 / 52.57 / 21.93 | 13.77 / 22.57 / 52.41 | 565.40 / 416.46 / 413.40 | 2.463 / 0.000 |
| 02-jpeg-thumbnail: Photo JPEG thumbnail | 64.27 / 40.15 / 10.31 | 63.83 / 41.54 / 9.91 | 72.48 / 46.80 / 11.21 | 13.08 / 22.57 / 23.95 | 16.49 / 12.52 / 11.99 | 3.776 / 1.987 |
| 03-jpeg-medium: Photo JPEG medium | 96.73 / 50.04 / 26.09 | 96.11 / 47.18 / 21.63 | 110.86 / 57.00 / 24.89 | 15.66 / 23.46 / 46.97 | 208.00 / 157.90 / 156.59 | 2.481 / 0.000 |
| 04-jpeg-square: Photo square crop | 70.78 / 43.75 / 12.12 | 71.68 / 46.95 / 11.63 | 81.85 / 47.43 / 13.25 | 13.52 / 22.54 / 28.79 | 23.70 / 18.12 / 17.40 | 3.582 / 1.865 |
| 05-jpeg-retina: Photo JPEG DPR 2 | 51.17 / 52.48 / 18.03 | 52.22 / 52.66 / 15.42 | 57.55 / 58.22 / 17.55 | 23.66 / 23.63 / 34.08 | 92.90 / 92.90 / 88.89 | 2.266 / 2.266 |
| 06-photo-crop: Photo center crop | 92.40 / 140.49 / 63.74 | 92.43 / 139.11 / 55.42 | 104.56 / 159.58 / 62.86 | 14.47 / 21.53 / 33.29 | 861.68 / 812.07 / 817.20 | 0.035 / 0.000 |
| 07-photo-rotate: Photo 90 degree rotation | 114.81 / 54.25 / 35.84 | 114.43 / 52.47 / 29.21 | 118.72 / 57.65 / 33.69 | 19.86 / 21.63 / 52.67 | 555.71 / 404.63 / 401.74 | 2.515 / 0.000 |
| 08-photo-blur: Photo thumbnail and blur | 88.59 / 53.36 / 15.71 | 88.24 / 52.23 / 15.11 | 99.64 / 62.17 / 17.10 | 13.67 / 26.58 / 27.04 | 27.57 / 21.98 / 21.31 | 1.623 / 0.676 |
| 09-photo-sharpen: Photo thumbnail and sharpen | 83.82 / 57.12 / 26.48 | 80.97 / 56.78 / 25.56 | 94.42 / 67.02 / 29.31 | 13.57 / 28.75 / 29.62 | 78.09 / 57.71 / 55.60 | 6.609 / 3.163 |
| 10-png-identity: Photo PNG identity | 41.58 / 672.80 / 281.51 | 41.73 / 658.37 / 230.15 | 43.39 / 737.18 / 263.85 | 10.70 / 29.88 / 52.00 | 2962.68 / 2885.08 / 2921.55 | 0.000 / 0.000 |
| 11-png-thumbnail: Photo PNG thumbnail | 112.60 / 85.51 / 60.01 | 111.09 / 88.64 / 54.29 | 125.10 / 99.11 / 60.23 | 15.95 / 29.21 / 46.54 | 81.93 / 77.61 / 78.64 | 1.052 / 0.000 |
| 12-png-jpeg: Photo PNG to JPEG | 140.14 / 82.84 / 64.94 | 139.44 / 83.69 / 58.26 | 157.15 / 90.57 / 66.12 | 17.95 / 30.03 / 52.97 | 206.89 / 157.06 / 155.80 | 2.465 / 0.000 |
| 13-png-bmp: Photo PNG to BMP | 103.52 / 85.63 / 60.03 | 104.32 / 85.13 / 56.15 | 118.09 / 95.82 / 62.24 | 12.42 / 29.90 / 50.61 | 1380.05 / 1380.05 / 1380.13 | 0.159 / 0.000 |
| 14-photo-webp: Earth JPEG to WebP | 138.44 / 138.07 / 114.34 | 138.54 / 138.51 / 111.87 | 153.83 / 153.82 / 125.35 | 27.46 / 27.41 / 42.09 | 140.91 / 140.91 / 140.91 | 0.000 / 0.000 |
| 15-earth-png: Earth JPEG to PNG | 127.65 / 126.95 / 51.73 | 125.65 / 123.70 / 37.79 | 140.48 / 138.98 / 43.07 | 16.63 / 25.24 / 31.02 | 494.61 / 482.68 / 482.82 | 0.665 / 0.673 |
| 16-progressive-jpeg: Progressive JPEG resize | 195.66 / 73.85 / 34.87 | 193.84 / 72.78 / 31.35 | 216.19 / 82.94 / 37.83 | 39.59 / 34.37 / 52.86 | 138.86 / 105.36 / 103.57 | 3.297 / 2.218 |
| 17-alpha-identity: Alpha PNG identity | 35.58 / 58.81 / 22.85 | 36.07 / 62.02 / 18.39 | 37.08 / 67.18 / 19.66 | 11.41 / 16.56 / 30.66 | 29.46 / 30.80 / 28.67 | 0.000 / 0.000 |
| 18-alpha-thumbnail: Alpha PNG thumbnail | 29.27 / 76.61 / 20.66 | 29.59 / 68.54 / 16.60 | 31.75 / 74.05 / 18.58 | 10.88 / 21.63 / 39.18 | 67.21 / 75.43 / 71.85 | 0.529 / 0.516 |
| 19-alpha-jpeg: Alpha on white JPEG | 26.25 / 55.26 / 14.23 | 26.72 / 56.92 / 13.76 | 30.40 / 62.12 / 15.57 | 10.91 / 21.57 / 38.93 | 19.85 / 14.96 / 14.42 | 3.481 / 1.183 |
| 20-alpha-webp: Alpha PNG to WebP | 79.99 / 81.08 / 39.13 | 82.07 / 82.10 / 38.29 | 87.29 / 87.53 / 42.84 | 22.64 / 22.53 / 37.48 | 33.34 / 33.34 / 34.52 | 1.122 / 1.122 |
| 21-graphics-identity: Large flat graphics PNG | 5.48 / 112.34 / 55.05 | 5.78 / 107.73 / 43.80 | 6.63 / 123.07 / 46.53 | 5.11 / 24.20 / 37.16 | 16.66 / 16.96 / 16.71 | 0.000 / 0.000 |
| 22-graphics-square: Flat graphics square crop | 84.62 / 56.64 / 19.90 | 85.30 / 56.85 / 16.27 | 94.65 / 62.96 / 18.23 | 17.97 / 25.71 / 40.97 | 29.61 / 25.57 / 26.23 | 0.280 / 0.000 |
| 23-grayscale-jpeg: Grayscale PNG to JPEG | 26.14 / 34.23 / 10.47 | 26.51 / 36.61 / 10.08 | 30.33 / 41.87 / 11.38 | 8.98 / 12.94 / 23.93 | 12.13 / 11.12 / 10.95 | 1.458 / 0.000 |
| 24-tiff-png: RGB TIFF to PNG | 113.06 / 168.51 / 65.64 | 113.28 / 163.31 / 54.16 | 128.38 / 183.86 / 60.04 | 29.20 / 44.44 / 55.51 | 777.61 / 751.17 / 760.78 | 0.171 / 0.000 |
| 25-svg-raster: SVG graphics rasterization | 99.20 / 66.64 / 6.00 | 99.80 / 61.90 / 5.93 | 112.87 / 68.61 / 7.27 | 9.30 / 24.75 / 26.12 | 12.99 / 14.16 / 4.29 | 0.466 / 0.501 |

Pixstride default: 0/25 CPU comparisons have batch coefficient of variation above 10% in at least one engine. Pixstride native: 0/25 CPU comparisons have batch coefficient of variation above 10% in at least one engine. Treat small timing differences as noise; differing pixels/encoding prevent an equal-quality speed claim.

### Linux arm64

CPU identifiers: 0xd49; 4 logical CPUs, two selected; Ubuntu 24.04.5 LTS, kernel 7.0.0-1017-azure. [Measurement run](https://github.com/lansbe/pixstride/actions/runs/43647143217).

6 batches × 24 timed requests per case/mode, after 5 warmups.

Harness `820a33d7768d57fff29a1e1cf1e119313c7eeb65`; corpus SHA-256 `adbe2d78914554327b17d40ea793d747fcc00a9499e4b1085b7a40cd2b8dd9c1`.

Each triple is **Pixstride default / Pixstride native / imgproxy**. Pixel MAE is **default / native**, taking the larger black/white composite error against imgproxy. Memory is the fresh container lifetime cgroup peak, including startup, warmups, file cache and kernel memory.

| Case | CPU ms/request | HTTP p50 ms | HTTP p95 ms | Peak cgroup MiB | Output KiB | Pixel MAE |
|---|---:|---:|---:|---:|---:|---:|
| 01-jpeg-identity: Photo JPEG identity | 91.30 / 54.19 / 29.44 | 91.86 / 57.57 / 21.57 | 92.24 / 57.74 / 24.23 | 13.62 / 22.17 / 61.73 | 565.40 / 416.46 / 413.40 | 2.463 / 0.000 |
| 02-jpeg-thumbnail: Photo JPEG thumbnail | 51.35 / 50.84 / 11.94 | 51.84 / 52.24 / 11.80 | 52.40 / 52.48 / 12.24 | 12.81 / 22.03 / 26.32 | 16.49 / 12.52 / 11.99 | 3.776 / 1.987 |
| 03-jpeg-medium: Photo JPEG medium | 79.91 / 62.39 / 34.94 | 80.39 / 62.49 / 32.07 | 80.90 / 63.10 / 33.64 | 15.48 / 22.99 / 56.62 | 208.00 / 157.90 / 156.59 | 2.481 / 0.000 |
| 04-jpeg-square: Photo square crop | 53.72 / 48.59 / 14.95 | 54.14 / 52.23 / 14.62 | 54.84 / 52.54 / 15.06 | 13.24 / 22.12 / 34.04 | 23.70 / 18.12 / 17.40 | 3.582 / 1.865 |
| 05-jpeg-retina: Photo JPEG DPR 2 | 57.70 / 57.54 / 20.53 | 57.38 / 57.37 / 18.80 | 57.94 / 57.86 / 19.97 | 23.18 / 23.21 / 39.80 | 92.90 / 92.90 / 88.89 | 2.266 / 2.266 |
| 06-photo-crop: Photo center crop | 88.23 / 132.07 / 48.31 | 88.67 / 133.80 / 40.45 | 89.40 / 133.96 / 42.19 | 14.34 / 21.05 / 35.61 | 861.68 / 812.07 / 817.20 | 0.035 / 0.000 |
| 07-photo-rotate: Photo 90 degree rotation | 99.09 / 57.55 / 34.63 | 99.58 / 57.57 / 29.95 | 100.24 / 57.93 / 32.49 | 19.66 / 21.20 / 60.46 | 555.71 / 404.63 / 401.74 | 2.515 / 0.000 |
| 08-photo-blur: Photo thumbnail and blur | 77.46 / 62.63 / 20.04 | 77.93 / 62.50 / 19.69 | 78.63 / 67.69 / 20.46 | 13.38 / 26.16 / 30.12 | 27.57 / 21.98 / 21.31 | 1.623 / 0.676 |
| 09-photo-sharpen: Photo thumbnail and sharpen | 70.93 / 73.66 / 32.51 | 71.39 / 77.58 / 31.92 | 72.02 / 77.77 / 32.83 | 13.41 / 28.29 / 32.91 | 78.09 / 57.70 / 55.59 | 6.609 / 3.163 |
| 10-png-identity: Photo PNG identity | 41.91 / 609.78 / 185.84 | 42.35 / 611.46 / 160.44 | 42.97 / 612.22 / 163.17 | 10.38 / 29.57 / 62.81 | 2962.68 / 2885.08 / 2921.55 | 0.000 / 0.000 |
| 11-png-thumbnail: Photo PNG thumbnail | 76.59 / 94.30 / 39.75 | 77.06 / 94.18 / 37.52 | 77.63 / 99.14 / 39.31 | 15.76 / 28.81 / 54.72 | 81.93 / 77.61 / 78.64 | 1.052 / 0.000 |
| 12-png-jpeg: Photo PNG to JPEG | 100.69 / 95.81 / 50.88 | 101.30 / 94.54 / 46.57 | 101.83 / 99.25 / 49.54 | 17.75 / 29.62 / 52.10 | 206.89 / 157.06 / 155.80 | 2.465 / 0.000 |
| 13-png-bmp: Photo PNG to BMP | 69.58 / 94.40 / 43.35 | 70.02 / 94.65 / 41.81 | 70.38 / 99.87 / 43.10 | 12.07 / 29.49 / 61.06 | 1380.05 / 1380.05 / 1380.13 | 0.159 / 0.000 |
| 14-photo-webp: Earth JPEG to WebP | 177.52 / 177.57 / 157.89 | 178.97 / 178.97 / 156.95 | 179.90 / 179.79 / 158.79 | 27.00 / 26.94 / 54.16 | 140.91 / 140.91 / 140.91 | 0.000 / 0.000 |
| 15-earth-png: Earth JPEG to PNG | 100.30 / 125.17 / 40.45 | 100.80 / 119.98 / 27.91 | 101.67 / 124.63 / 31.30 | 16.45 / 24.69 / 30.52 | 494.61 / 482.68 / 482.82 | 0.665 / 0.673 |
| 16-progressive-jpeg: Progressive JPEG resize | 176.81 / 74.87 / 27.81 | 177.41 / 77.66 / 25.82 | 178.20 / 78.41 / 27.02 | 39.48 / 33.89 / 61.83 | 138.86 / 105.36 / 103.57 | 3.297 / 2.218 |
| 17-alpha-identity: Alpha PNG identity | 20.85 / 66.50 / 22.82 | 21.27 / 67.19 / 20.42 | 21.85 / 67.34 / 21.04 | 11.16 / 16.14 / 34.11 | 29.46 / 30.80 / 28.67 | 0.000 / 0.000 |
| 18-alpha-thumbnail: Alpha PNG thumbnail | 24.46 / 81.23 / 20.68 | 24.86 / 77.42 / 18.21 | 25.49 / 79.18 / 20.97 | 10.65 / 21.14 / 47.14 | 67.21 / 75.43 / 71.85 | 0.529 / 0.516 |
| 19-alpha-jpeg: Alpha on white JPEG | 21.07 / 67.98 / 17.46 | 21.48 / 72.24 / 17.30 | 21.97 / 72.57 / 17.91 | 10.67 / 21.07 / 38.41 | 19.85 / 14.96 / 14.42 | 3.481 / 1.183 |
| 20-alpha-webp: Alpha PNG to WebP | 99.60 / 99.57 / 50.11 | 102.65 / 102.67 / 49.87 | 104.15 / 104.09 / 51.12 | 21.95 / 21.85 / 48.14 | 33.34 / 33.34 / 34.52 | 1.122 / 1.122 |
| 21-graphics-identity: Large flat graphics PNG | 5.93 / 135.40 / 53.16 | 6.35 / 138.04 / 47.04 | 6.49 / 138.27 / 48.99 | 4.62 / 24.01 / 40.13 | 16.66 / 16.96 / 16.71 | 0.000 / 0.000 |
| 22-graphics-square: Flat graphics square crop | 49.39 / 74.71 / 21.94 | 49.75 / 73.17 / 19.36 | 50.67 / 77.39 / 21.71 | 17.66 / 25.20 / 54.35 | 29.61 / 25.57 / 26.23 | 0.280 / 0.000 |
| 23-grayscale-jpeg: Grayscale PNG to JPEG | 16.90 / 35.69 / 9.60 | 17.31 / 36.89 / 9.54 | 17.66 / 37.27 / 9.97 | 8.76 / 12.43 / 27.12 | 12.13 / 11.12 / 10.95 | 1.458 / 0.000 |
| 24-tiff-png: RGB TIFF to PNG | 92.22 / 161.95 / 54.23 | 92.83 / 157.18 / 40.40 | 93.83 / 158.26 / 42.25 | 29.10 / 43.87 / 64.49 | 777.61 / 751.17 / 760.78 | 0.171 / 0.000 |
| 25-svg-raster: SVG graphics rasterization | 104.72 / 67.19 / 7.48 | 105.35 / 67.12 / 7.52 | 105.78 / 68.41 / 8.12 | 9.16 / 24.30 / 29.75 | 12.99 / 14.16 / 4.29 | 0.466 / 0.501 |

Pixstride default: 0/25 CPU comparisons have batch coefficient of variation above 10% in at least one engine. Pixstride native: 0/25 CPU comparisons have batch coefficient of variation above 10% in at least one engine. Treat small timing differences as noise; differing pixels/encoding prevent an equal-quality speed claim.
<!-- benchmark:tables:end -->

The second allocator diagnostic ([run 43804940282](https://github.com/lansbe/pixstride/actions/runs/43804940282)) completed all measurements but failed its ARM64 integrity check: three processes retained a 2-MiB huge page allocated before the constructor disabled promotion. Its partial diagnostic report is retained; it does not qualify HTTP performance. Suppression is now also configured before libc startup. The 25-case corpus/protocol and its acceptance rules are unchanged.

</details>
