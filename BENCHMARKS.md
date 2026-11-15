# Linux performance benchmarks

The suite measures 25 predefined HTTP image-processing cases on native Linux AMD64 and ARM64. [versions.json](benchmarks/versions.json) pins default published images by immutable multi-platform digest; the initial snapshot compares Pixstride 0.3.0 and imgproxy 4.0.24. Raw reports retain the actual image identities for overrides and future runs. Pixstride default routing and forced-native mode are separate observations. Default routing can select native processing when an input or option requires it.

This compares packaged services, including their different libraries and deployment behavior. It does not isolate programming-language performance. The [compatibility suite](COMPATIBILITY.md) still targets imgproxy 4.0.21.

## Corpus

[cases.json](benchmarks/cases.json) contains every source, processing option, output format and expected dimension. The 25 cases cover identity, thumbnails, medium images, square/center crops, DPR, rotation, blur, sharpening, format conversion, progressive JPEG, alpha/backgrounds, flat graphics, grayscale, TIFF and SVG.

Two public photographs are downloaded with bounded sizes and verified SHA-256 hashes:

- [Fronalpstock](https://commons.wikimedia.org/wiki/File:Fronalpstock_big.jpg), Hannes Röst, [CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/). Derivatives are converted to sRGB, resized to 2048×920 and re-encoded; grayscale is derived from this image. The derivatives retain the same license.
- [The Blue Marble](https://commons.wikimedia.org/wiki/File:The_Earth_seen_from_Apollo_17.jpg), NASA / Apollo 17 crew, public domain as a US NASA work. It is converted to sRGB, cropped/resized to 1600×1600 and re-encoded as JPEG/TIFF.

Alpha shapes, flat graphics and a restricted SVG are deterministic synthetic fixtures under the project's MIT license. [benchmark_fixtures.py](scripts/benchmark_fixtures.py) uses Pillow 13.0.0 outside measurement and removes inherited ICC/EXIF metadata after color conversion. ICC preservation, HDR, animation and remote origin fetching are outside this performance corpus.

The manifest records input dimensions, sizes, hashes, attribution and generator/encoder versions. CI generates the corpus once and sends the identical artifact to both architectures. For manual two-host comparisons, copy that prepared directory instead of regenerating it: different codec builds can produce different input bytes. Media/source caches remain untracked; attribution accompanies the shared corpus artifact.

## Protocol and interpretation

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

Use a native AMD64/ARM64 Linux host with a local Docker daemon, cgroup v2, two available logical CPUs, Python 3.11+ and room for three 1-GiB containers plus the client. A remote daemon cannot provide the local host's container accounting.

```sh
python3 -m venv .venv-benchmark
.venv-benchmark/bin/python -m pip install --only-binary=:all: Pillow==13.0.0
.venv-benchmark/bin/python scripts/benchmark_fixtures.py --output .benchmark-data --cache .benchmark-cache
.venv-benchmark/bin/python scripts/benchmark.py --fixtures .benchmark-data --output .benchmark-results/preflight.json --preflight
.venv-benchmark/bin/python scripts/benchmark.py --fixtures .benchmark-data --output .benchmark-results/report.json
.venv-benchmark/bin/python scripts/benchmark_report.py --reports .benchmark-results/report.json --markdown .benchmark-results/table.md --csv .benchmark-results/cases.csv
```

Copy the same prepared corpus to the second architecture. Image overrides via `--pixstride-image` / `--imgproxy-image` require immutable `@sha256:` references. `--case`, lower sample counts and `--preflight` support development trials and cannot pass full-publication validation.

The [Benchmarks workflow](.github/workflows/benchmark.yml) shares one corpus between native `ubuntu-24.04` and `ubuntu-24.04-arm` jobs. Qualification pushes on the benchmark branch trigger it; `workflow_dispatch` supports manual reruns after integration. Timing ratios do not gate unrelated PRs. Correctness, input integrity and accounting do gate a benchmark run. Failures retain diagnostics and available partial JSON; owned containers are cleaned. Raw report artifacts have 90-day retention; corpus media have seven days.

The [Candidate benchmarks workflow](.github/workflows/benchmark-candidate.yml) builds the current clean checkout independently on three AMD64 and three ARM64 runners. It uses the same shared corpus, pinned imgproxy image, 25 cases, three modes and full measurement protocol. Candidate images stay local to their runners; there is no registry or release publication. Each is addressed by its inspected immutable Docker image ID and labelled with the exact Git revision. Reports retain source tree, Dockerfile and Cargo.lock hashes; cross-architecture validation requires identical source provenance while allowing platform-specific image IDs. Manual candidate runs use `--pixstride-image sha256:<local-image-id> --candidate-revision <full-clean-commit-sha>` after building with `--label org.opencontainers.image.revision=<full-clean-commit-sha>`.

The candidate workflow's final qualification job requires lower default CPU, HTTP p50/p95 and peak cgroup memory in every retained run for all 25 cases on both architectures. Wins within 5% of parity or with CPU batch CV above 10% require a third independent full run per architecture; two runs leave those cases marked `repeat-required`. Any observed loss remains a loss. Forced-native wins cannot substitute for a default loss. Both modes must stay within the worse of the two published modes' per-case composite/alpha MAE and RMSE, with 0.01 numerical tolerance; identity PNGs stay exact. This is an agreement envelope, not a perceptual quality certification. Source, corpus, options, competitor identity and raw-sample independence are checked. The final job fails until the entire target is met, while measurement artifacts retain all losses. Earlier candidate workflow successes (before this gate was added) mean only that measurements and correctness checks completed.

Run the same qualification locally after retaining at least two full reports per architecture (and all additional repeats; the workflow runs three by default):

```sh
python3 scripts/benchmark_qualification.py --reports amd64-*/report.json arm64-*/report.json --output qualification.json --markdown qualification.md
```

The JSON and Markdown list every case, CPU/p50/p95/memory ratio range, required repeats and pixel regressions. Exit code 1 means the target is not met. Input corruption or incompatible evidence raises an error. The published 0.3.0 results below remain the release baseline until separate candidate evidence is qualified.

Publish a qualified candidate separately, retaining every full report from its cohort:

```sh
python3 scripts/candidate_snapshot.py --publish amd64-*/report.json arm64-*/report.json
python3 scripts/candidate_snapshot.py --check
```

The publisher rejects remaining losses, incomplete or incompatible evidence and duplicate samples before updating documentation. It stores all retained raw reports as deterministic gzip archives, with compressed/raw SHA-256 hashes and recomputed qualification in `benchmarks/results/candidate-index.json`. Separate generated candidate blocks include every case's timing/memory range, encoded sizes and decoded-pixel errors. The release baseline stays intact. CI checks archive integrity, provenance, recomputed metrics and documentation drift. Unqualified attempts remain in their workflow artifacts; no favorable subset can replace the full cohort.

After collecting one full raw report per architecture:

```sh
python3 scripts/benchmark_report.py --reports amd64/report.json arm64/report.json --publish-results benchmarks/results --update-docs
python3 scripts/benchmark_report.py --check
python3 scripts/benchmark_report.py --markdown /tmp/pixstride-benchmarks.md --csv /tmp/pixstride-benchmarks.csv
```

Committed evidence contains losslessly compressed raw samples plus readable summaries. Publication requires identical input files, catalog, harness revision, image references and protocol across architectures, and recalculates statistics from samples. Generated Markdown blocks are checked for drift. Prose and output-quality limitations still require human review.

## Measured results

Evidence: [AMD64 summary](benchmarks/results/linux-amd64.json) / [complete samples](benchmarks/results/linux-amd64.raw.json.gz), [ARM64 summary](benchmarks/results/linux-arm64.json) / [complete samples](benchmarks/results/linux-arm64.raw.json.gz). Each summary records the compressed archive and decoded JSON hashes. These are the final qualification run's results; earlier development trials are not mixed into this snapshot.

<!-- candidate:details:start -->
## Unreleased candidate confirmation

Unreleased [candidate `fe350b73b784`](https://github.com/lansbe/pixstride/commit/fe350b73b7841251f99a224abe33f46acb970842); default routing, all retained native runs. The published 0.3.0 baseline below is unchanged.

Candidate `fe350b73b7841251f99a224abe33f46acb970842`: **25/25 confirmed on both architectures**.

Default routing only; CPU, HTTP p50/p95 and peak cgroup memory must be lower in every retained run. Close timings (within 5%) or noisy CPU require at least three runs. Both modes retain the published decoded-pixel error envelope; PNG identities stay exact. This pixel agreement check does not certify perceptual equivalence.

### Linux amd64: 25/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.643–0.679× | 0.719–0.748× | 0.695–0.740× | 0.450–0.600× |
| 02-jpeg-thumbnail | confirmed-win | 0.839–0.898× | 0.889–0.935× | 0.904–0.911× | 0.663–0.729× |
| 03-jpeg-medium | confirmed-win | 0.783–0.826× | 0.839–0.871× | 0.810–0.858× | 0.716–0.744× |
| 04-jpeg-square | confirmed-win | 0.859–0.901× | 0.898–0.938× | 0.866–0.898× | 0.735–0.756× |
| 05-jpeg-retina | confirmed-win | 0.786–0.830× | 0.863–0.887× | 0.849–0.871× | 0.696–0.720× |
| 06-photo-crop | confirmed-win | 0.407–0.438× | 0.465–0.483× | 0.463–0.489× | 0.789–0.816× |
| 07-photo-rotate | confirmed-win | 0.738–0.793× | 0.733–0.806× | 0.668–0.761× | 0.560–0.623× |
| 08-photo-blur | confirmed-win | 0.825–0.873× | 0.854–0.906× | 0.558–0.906× | 0.845–0.867× |
| 09-photo-sharpen | confirmed-win | 0.787–0.836× | 0.812–0.858× | 0.820–0.846× | 0.926–0.935× |
| 10-png-identity | confirmed-win | 0.122–0.135× | 0.145–0.159× | 0.144–0.161× | 0.272–0.289× |
| 11-png-thumbnail | confirmed-win | 0.403–0.454× | 0.445–0.497× | 0.448–0.500× | 0.601–0.612× |
| 12-png-jpeg | confirmed-win | 0.464–0.489× | 0.489–0.525× | 0.491–0.525× | 0.578–0.599× |
| 13-png-bmp | confirmed-win | 0.448–0.502× | 0.483–0.534× | 0.490–0.546× | 0.626–0.630× |
| 14-photo-webp | confirmed-win | 0.849–0.909× | 0.859–0.927× | 0.841–0.912× | 0.822–0.857× |
| 15-earth-png | confirmed-win | 0.360–0.438× | 0.493–0.558× | 0.472–0.663× | 0.670–0.690× |
| 16-progressive-jpeg | confirmed-win | 0.787–0.866× | 0.834–0.890× | 0.730–0.900× | 0.753–0.800× |
| 17-alpha-identity | confirmed-win | 0.478–0.749× | 0.608–0.872× | 0.625–0.868× | 0.589–0.632× |
| 18-alpha-thumbnail | confirmed-win | 0.531–0.659× | 0.679–0.792× | 0.639–0.731× | 0.612–0.622× |
| 19-alpha-jpeg | confirmed-win | 0.787–0.877× | 0.824–0.911× | 0.789–0.951× | 0.796–0.846× |
| 20-alpha-webp | confirmed-win | 0.782–0.871× | 0.801–0.884× | 0.799–0.890× | 0.868–0.937× |
| 21-graphics-identity | confirmed-win | 0.074–0.103× | 0.099–0.129× | 0.100–0.131× | 0.164–0.180× |
| 22-graphics-square | confirmed-win | 0.549–0.706× | 0.684–0.884× | 0.700–0.871× | 0.641–0.668× |
| 23-grayscale-jpeg | confirmed-win | 0.452–0.514× | 0.463–0.522× | 0.475–0.538× | 0.611–0.663× |
| 24-tiff-png | confirmed-win | 0.404–0.423× | 0.476–0.490× | 0.475–0.494× | 0.682–0.699× |
| 25-svg-raster | confirmed-win | 0.530–0.598× | 0.594–0.673× | 0.588–0.636× | 0.453–0.475× |

### Linux arm64: 25/25 confirmed, 3 runs

| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |
|---|---|---:|---:|---:|---:|
| 01-jpeg-identity | confirmed-win | 0.737–0.742× | 0.766–0.768× | 0.740–0.743× | 0.560–0.655× |
| 02-jpeg-thumbnail | confirmed-win | 0.918–0.926× | 0.940–0.944× | 0.931–0.940× | 0.749–0.812× |
| 03-jpeg-medium | confirmed-win | 0.873–0.876× | 0.886–0.894× | 0.878–0.886× | 0.684–0.739× |
| 04-jpeg-square | confirmed-win | 0.920–0.923× | 0.943–0.946× | 0.926–0.946× | 0.745–0.810× |
| 05-jpeg-retina | confirmed-win | 0.879–0.884× | 0.896–0.906× | 0.871–0.890× | 0.733–0.828× |
| 06-photo-crop | confirmed-win | 0.532–0.533× | 0.628–0.631× | 0.604–0.616× | 0.693–0.758× |
| 07-photo-rotate | confirmed-win | 0.758–0.813× | 0.767–0.801× | 0.698–0.796× | 0.594–0.738× |
| 08-photo-blur | confirmed-win | 0.950–0.953× | 0.959–0.964× | 0.955–0.963× | 0.894–0.929× |
| 09-photo-sharpen | confirmed-win | 0.889–0.892× | 0.893–0.895× | 0.878–0.894× | 0.933–0.982× |
| 10-png-identity | confirmed-win | 0.184–0.185× | 0.215–0.216× | 0.214–0.215× | 0.242–0.284× |
| 11-png-thumbnail | confirmed-win | 0.702–0.723× | 0.754–0.780× | 0.727–0.754× | 0.557–0.610× |
| 12-png-jpeg | confirmed-win | 0.755–0.760× | 0.803–0.812× | 0.777–0.780× | 0.494–0.510× |
| 13-png-bmp | confirmed-win | 0.765–0.785× | 0.800–0.821× | 0.784–0.807× | 0.527–0.603× |
| 14-photo-webp | confirmed-win | 0.905–0.906× | 0.911–0.911× | 0.904–0.906× | 0.685–0.798× |
| 15-earth-png | confirmed-win | 0.584–0.592× | 0.854–0.865× | 0.743–0.766× | 0.671–0.751× |
| 16-progressive-jpeg | confirmed-win | 0.854–0.884× | 0.884–0.905× | 0.811–0.896× | 0.725–0.886× |
| 17-alpha-identity | confirmed-win | 0.397–0.399× | 0.460–0.467× | 0.452–0.463× | 0.698–0.787× |
| 18-alpha-thumbnail | confirmed-win | 0.796–0.814× | 0.920–0.939× | 0.837–0.852× | 0.654–0.692× |
| 19-alpha-jpeg | confirmed-win | 0.949–0.955× | 0.969–0.973× | 0.963–0.967× | 0.824–0.865× |
| 20-alpha-webp | confirmed-win | 0.834–0.837× | 0.841–0.848× | 0.837–0.865× | 0.785–0.884× |
| 21-graphics-identity | confirmed-win | 0.108–0.109× | 0.131–0.132× | 0.128–0.130× | 0.172–0.215× |
| 22-graphics-square | confirmed-win | 0.718–0.730× | 0.834–0.847× | 0.762–0.775× | 0.679–0.770× |
| 23-grayscale-jpeg | confirmed-win | 0.747–0.757× | 0.769–0.779× | 0.766–0.776× | 0.775–0.881× |
| 24-tiff-png | confirmed-win | 0.548–0.580× | 0.738–0.780× | 0.719–0.764× | 0.607–0.672× |
| 25-svg-raster | confirmed-win | 0.357–0.381× | 0.408–0.431× | 0.413–0.433× | 0.534–0.629× |


amd64 CPU: AMD EPYC 7763 64-Core Processor, AMD EPYC 9V45 96-Core Processor, Intel(R) Xeon(R) 6975P-C. Two CPUs are assigned per service.

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
| amd64 | 19-alpha-jpeg | 1.0381–1.0381× | 0.000000 | 0.000000 | 0.000000 |
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
| arm64 | 19-alpha-jpeg | 1.0381–1.0381× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 20-alpha-webp | 1.0000–1.0000× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 21-graphics-identity | 0.9971–0.9971× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 22-graphics-square | 1.0567–1.0567× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 23-grayscale-jpeg | 1.0158–1.0158× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 24-tiff-png | 1.0087–1.0087× | 0.000000 | 0.000000 | 0.000000 |
| arm64 | 25-svg-raster | 0.9278–0.9278× | 0.009113 | 0.095461 | 0.000000 |

Raw reports, hashes, source/image identities and recomputed qualification: [candidate index](benchmarks/results/candidate-index.json).

Retained workflow evidence: [run 1](https://github.com/lansbe/pixstride/actions/runs/43760099406).
<!-- candidate:details:end -->

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
