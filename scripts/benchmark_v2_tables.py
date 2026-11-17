"""V2 summaries keep preflight coverage, quality and measured wins separate."""
from collections import Counter
from statistics import median

MODES=('pixstride_default','pixstride_native')
METRICS=('cpu_ms_per_request','latency_p50_ms','latency_p95_ms','cgroup_peak_memory_bytes')


def span(values):
    return '—' if not values else f'{min(values):.6f}–{max(values):.6f}×'


def summary(result,pins=None):
    if result is None:
        configured='' if pins is None else (f'Configured published target: Pixstride **{pins["pixstride"]["version"]}** / '
                                             f'imgproxy **{pins["imgproxy"]["version"]}**. ')
        if pins is not None and pins['pixstride'].get('published') is False:
            configured=(f'Configured **UNRELEASED candidate**, package version {pins["pixstride"]["version"]}; '
                        f'source `{pins["pixstride"]["candidate"]["revision"]}`. ')
        return ('Current benchmark: **v2, 1,000 distinct workloads**, 20 processing families and 10 nominal difficulty tiers. '+configured+
                'Native full-catalog preflight is pending; V2 performance is **not measured**. '
                'The retained 25-case timings belong to v1. See [the V2 protocol](BENCHMARKS_V2.md).\n')
    mode=result['mode'];measured=mode=='measure'
    status=('target met' if result['qualified'] else 'target not met') if measured else 'performance not measured'
    text=(f'Current benchmark **v2: 1,000 distinct workloads** — **{status}**. '
          'Every workload and every service/quality failure is retained.\n\n')
    release=result['pins']['pixstride'];ref=result['pins']['imgproxy'];candidate=release.get('published') is False
    if candidate:
        source=release['candidate']
        provenance=(f'**UNRELEASED candidate**, package version {release["version"]} / imgproxy {ref["version"]}. '
            f'Source `{source["revision"]}`; tree `{source["tree"]}`; Dockerfile SHA256 `{source["dockerfile_sha256"]}`; '
            f'Cargo.lock SHA256 `{source["cargo_lock_sha256"]}`. '
            +' '.join(f'{arch} local image `{image}`.' for arch,image in sorted(release['images'].items()))+' '
            +f'Harness `{result["harness_revision"]}`. ')
    else:provenance=(f'Pixstride {release["version"]} / imgproxy {ref["version"]}; published image `{release["image"]}`. '
           f'Runtime source `{release["revision"]}`; harness `{result["harness_revision"]}`. ')
    text+=(provenance+
           f'Retained accepted timed responses: **{result["retained_timed_requests"]:,}**'
           +(' of 2,592,000 planned when all engines complete' if measured else ' (decoded preflight only)')+'.\n\n'
           '| Linux | Mode | Valid reference cases | Completed decoded trials | Within pixel budgets | '
           +('Confirmed CPU/p50/p95/peak wins' if measured else 'Performance')+' |\n'
           '|---|---|---:|---:|---:|---:|\n')
    for arch,cohort in sorted(result['architectures'].items()):
        for own in MODES:
            counts=cohort['by_mode'][own];label='default' if own==MODES[0] else 'forced-native'
            perf=f'{counts["confirmed_cases"]}/1000' if measured else 'not measured'
            text+=(f'| {arch} | {label} | {cohort["reference_valid_cases"]}/1000 | '
                   f'{counts["http_valid_cases"]}/1000 | {counts["quality_valid_cases"]}/1000 | {perf} |\n')
    if candidate and measured:
        text+=candidate_metrics(result)
    kind=('candidate-' if candidate else '')+('measure' if measured else 'preflight')
    text+=('\nNumerical pixel budgets do not certify perceptual equivalence. No failure counts as a fast response. '
           'Nominal difficulty is fixed before measurements and does not guarantee runtime order. '
           f'Raw reports, hashes and all outcomes: [V2 {kind} index](benchmarks/results/v2/{kind}-index.json). '
           'Protocol, tier coverage and rerun commands: [BENCHMARKS_V2.md](BENCHMARKS_V2.md).\n')
    return text


def markdown(result):
    text='# Benchmark v2 complete-cohort report\n\n'+summary(result)
    for arch,cohort in sorted(result['architectures'].items()):
        for own in MODES:
            label='default' if own==MODES[0] else 'forced-native'
            text+=f'\n## {arch}, {label}\n\n'
            for field in ('tier','family'):
                groups=sorted({row[field] for row in cohort['cases']})
                text+=f'### Coverage by {field}\n\n| {field} | Cases | Outcomes |\n|---|---:|---|\n'
                for group in groups:
                    rows=[r for r in cohort['cases'] if r[field]==group]
                    counts=Counter(r['modes'][own]['status'] for r in rows)
                    outcomes=', '.join(f'{k}: {v}' for k,v in sorted(counts.items()))
                    text+=f'| {group} | {len(rows)} | {outcomes} |\n'
            text+=('\n### All 1,000 workloads\n\n'
                   '| Rank | Tier | Case | Outcome | CPU ratio | p50 ratio | p95 ratio | Peak ratio | Size ratio | '
                   'Max composite MAE/RMSE | Max alpha MAE/RMSE |\n'
                   '|---:|---:|---|---|---:|---:|---:|---:|---:|---:|---:|\n')
            for row in cohort['cases']:
                o=row['modes'][own]
                cells=[span(o['ratios'][m]) for m in METRICS]
                color='—' if o['max_mae'] is None else f'{o["max_mae"]:.6f}/{o["max_rmse"]:.6f}'
                alpha='—' if o['max_alpha_mae'] is None else f'{o["max_alpha_mae"]:.6f}/{o["max_alpha_rmse"]:.6f}'
                text+=(f'| {row["rank"]} | {row["tier"]} | {row["id"]} | {o["status"]} | '
                       +' | '.join(cells)+f' | {span(o["size_ratios"])} | {color} | {alpha} |\n')
            failures=[]
            for row in cohort['cases']:
                o=row['modes'][own]
                for error in row['reference_errors']:
                    failures.append(f'{row["id"]}: reference run {error["repetition"]}, {error["phase"]}: {error["message"]}')
                for error in o['errors']:
                    failures.append(f'{row["id"]}: run {error["repetition"]}, {error["phase"]}: {error["message"]}')
                failures.extend(f'{row["id"]}: {f}' for f in o['quality_failures'])
            if failures:
                text+='\n### Retained failures\n\n'
                text+='\n'.join('- '+f.replace('\n',' ').replace('|','/') for f in failures)+'\n'
    return text


def candidate_metrics(result):
    text=('\nAll retained comparable observations, including losses and pixel regressions. '
          'A per-metric case win requires all three repetitions below 1; percentages are reductions versus imgproxy '
          '(negative means higher resource use or latency).\n\n'
          '| Linux | Mode | Metric | Case wins / 1000 | Comparable observations | Ratio range | Median reduction |\n'
          '|---|---|---|---:|---:|---:|---:|\n')
    losing=[]
    for arch,cohort in sorted(result['architectures'].items()):
        for own in MODES:
            observations=[row['modes'][own] for row in cohort['cases']]
            for metric in METRICS:
                values=[v for o in observations for v in o['ratios'][metric]]
                wins=sum(len(o['ratios'][metric])==3 and max(o['ratios'][metric])<1 for o in observations)
                reduction='—' if not values else f'{100*(1-median(values)):.3f}%'
                text+=f'| {arch} | {own} | {metric} | {wins}/1000 | {len(values)} | {span(values)} | {reduction} |\n'
            losses=[row['id'] for row in cohort['cases'] if row['modes'][own]['status']!='confirmed-win']
            losing.append(f'{arch} {own}, all non-winning workloads ({len(losses)}): '+(', '.join(f'`{case}`' for case in losses) or 'none')+'.')
    return text+'\n'+'\n\n'.join(losing)+'\n'
