#!/usr/bin/env python3
"""Qualify repeated 25-case candidate results without selecting favorable modes/runs."""
import argparse
import hashlib
import json
import math
from pathlib import Path

from benchmark_fixtures import ROOT
from benchmark_report import check_results_directory, load, validate_pair, validate_report

METRICS = ('cpu_ms_per_request', 'latency_p50_ms', 'latency_p95_ms', 'cgroup_peak_memory_bytes')
MODES = ('pixstride_default', 'pixstride_native')
BACKGROUNDS = ('black', 'white', 'alpha')


def quality_values(entry):
    values = entry.get('pixels_vs_imgproxy', {})
    if type(values.get('exact_visible_pixels')) is not bool:
        raise ValueError('Missing exact-pixel observation')
    for bg in BACKGROUNDS:
        for metric in ('mae', 'rmse'):
            value = values.get(bg, {}).get(metric)
            if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
                raise ValueError('Missing/invalid decoded-pixel evidence')
    return values


def samples_hash(report):
    samples = {c['id']: {m: c['engines'][m]['batches'] for m in (*MODES, 'imgproxy')}
               for c in report['cases']}
    return hashlib.sha256(json.dumps(samples, sort_keys=True).encode()).hexdigest()


def qualify(reports, baselines, performance_modes=MODES):
    performance_modes = tuple(performance_modes)
    if performance_modes not in ((MODES[0],), MODES):
        raise ValueError("Invalid performance-mode qualification target")
    validate_pair(baselines)
    baseline = {b['host']['architecture']: b for b in baselines}
    cohorts = {arch: [r for r in reports if r['host']['architecture'] == arch] for arch in baseline}
    if any(len(rs) < 2 for rs in cohorts.values()) or len(reports) != sum(map(len, cohorts.values())):
        raise ValueError('Require at least two complete native runs on each architecture')
    for report in reports:
        arch = report['host']['architecture']
        validate_report(report)
        other = cohorts['arm64' if arch == 'amd64' else 'amd64'][0]
        validate_pair([report, other])
        if not report['images']['pixstride'].get('candidate'):
            raise ValueError('Qualification requires immutable candidate source provenance')
        base = baseline[arch]
        if (report['cases_sha256'] != base['cases_sha256']
                or report['fixtures']['files'] != base['fixtures']['files']
                or report['images']['imgproxy']['reference'] != base['images']['imgproxy']['reference']):
            raise ValueError('Candidate must retain the published corpus, cases and imgproxy image')
        definitions = lambda r: {c['id']: [c.get(k) for k in ('source', 'options', 'format', 'dimensions')]
                                 for c in r['cases']}
        if definitions(report) != definitions(base):
            raise ValueError('Case options/definitions differ from the published baseline')
    result = {'schema_version': 1, 'qualified': True,
              'candidate': reports[0]['images']['pixstride']['candidate'],
              'criteria': {'default_metrics': METRICS, 'minimum_runs_per_architecture': 2,
                           'close_ratio': .95, 'close_minimum_runs': 3, 'pixel_tolerance': .01},
              'architectures': {}}
    if performance_modes == MODES:
        result['criteria']['performance_modes'] = list(MODES)
    for arch, runs in cohorts.items():
        fingerprints = [samples_hash(r) for r in runs]
        if len(set(fingerprints)) != len(fingerprints):
            raise ValueError('Duplicate raw samples are not independent confirmation')
        base_cases = {c['id']: c for c in baseline[arch]['cases']}
        cases = [{c['id']: c for c in r['cases']} for r in runs]
        rows = []
        for cid, base in sorted(base_cases.items()):
            by_mode = {mode: {'ratios': {m: [] for m in METRICS}, 'noisy_cpu': False}
                       for mode in performance_modes}
            quality_failures = []
            mode_quality = {mode: [] for mode in MODES}
            for index, case_map in enumerate(cases):
                c = case_map[cid]
                ref = c['engines']['imgproxy']['summary']
                for mode in performance_modes:
                    own = c['engines'][mode]['summary']
                    for metric in METRICS:
                        if any(not math.isfinite(v[metric]) or v[metric] <= 0 for v in (own, ref)):
                            raise ValueError('Missing/invalid performance observation')
                        by_mode[mode]['ratios'][metric].append(own[metric] / ref[metric])
                    by_mode[mode]['noisy_cpu'] |= max(own['cpu_batch_cv'], ref['cpu_batch_cv']) > .10
                for mode in MODES:
                    values = quality_values(c['engines'][mode])
                    if (c.get('source', '').endswith('.png') and c.get('format') == 'png'
                            and c.get('options') == '' and not values['exact_visible_pixels']):
                        failure = f'run {index + 1} {mode}: PNG identity is not exact'
                        quality_failures.append(failure)
                        mode_quality[mode].append(failure)
                    for bg in BACKGROUNDS:
                        for metric in ('mae', 'rmse'):
                            limit = max(quality_values(base['engines'][m])[bg][metric] for m in MODES) + .01
                            if values[bg][metric] > limit:
                                failure = f'run {index + 1} {mode}: {bg} {metric} exceeds {limit:.6g}'
                                quality_failures.append(failure)
                                mode_quality[mode].append(failure)
            for mode, observation in by_mode.items():
                ratios = observation['ratios']
                loss = any(max(v) >= 1 for v in ratios.values())
                close = any(max(ratios[m]) >= .95 for m in METRICS[:3])
                observation['status'] = ('quality-regression' if mode_quality[mode] else 'loss' if loss
                    else 'repeat-required' if (close or observation['noisy_cpu']) and len(runs) < 3 else 'confirmed-win')
            severity = {'confirmed-win': 0, 'repeat-required': 1, 'loss': 2, 'quality-regression': 3}
            status = 'quality-regression' if quality_failures else max(
                (v['status'] for v in by_mode.values()), key=severity.get)
            row = {'id': cid, 'label': base.get('label', cid), 'status': status,
                   'ratios': by_mode[MODES[0]]['ratios'],
                   'noisy_cpu': any(v['noisy_cpu'] for v in by_mode.values()),
                   'quality_failures': quality_failures}
            if performance_modes == MODES:
                row['modes'] = by_mode
            rows.append(row)
        confirmed = sum(c['status'] == 'confirmed-win' for c in rows)
        result['architectures'][arch] = {'runs': len(runs), 'confirmed_cases': confirmed,
                                         'samples_sha256': fingerprints, 'cases': rows,
                                         'workflow_urls': sorted({r.get('workflow_url', '') for r in runs})}
        if performance_modes == MODES:
            result['architectures'][arch]['confirmed_cases_by_mode'] = {
                mode: sum(c['modes'][mode]['status'] == 'confirmed-win' for c in rows) for mode in MODES}
        result['qualified'] &= confirmed == 25
    return result


def markdown(result):
    if result['criteria'].get('performance_modes') == list(MODES):
        text = f'Candidate `{result["candidate"]["revision"]}`: '
        text += '**25/25 confirmed in both modes on both architectures**.\n\n' if result['qualified'] else '**target not met**.\n\n'
        text += ('Default and forced-native CPU, HTTP p50/p95 and peak cgroup memory must be lower in every retained run. '
                 'Near timing parity or noisy CPU requires three independent runs. Both modes retain the published '
                 'pixel-agreement envelope; PNG identities stay exact. This does not certify perceptual equivalence.\n\n')
        for arch, cohort in sorted(result['architectures'].items()):
            for mode in MODES:
                label = 'default' if mode == MODES[0] else 'forced-native'
                count = cohort['confirmed_cases_by_mode'][mode]
                text += f'### Linux {arch}, {label}: {count}/25 confirmed, {cohort["runs"]} runs\n\n'
                text += '| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |\n|---|---|---:|---:|---:|---:|\n'
                for c in cohort['cases']:
                    observation = c['modes'][mode]
                    def cell(metric):
                        values = observation['ratios'][metric]
                        precision = 6 if max(values) > .999 else 4
                        return f'{min(values):.{precision}f}–{max(values):.{precision}f}×'
                    text += f'| {c["id"]} | {observation["status"]} | ' + ' | '.join(cell(m) for m in METRICS) + ' |\n'
                text += '\n'
            for c in cohort['cases']:
                for failure in c['quality_failures']:
                    text += f'- {c["id"]}: {failure}\n'
        return text

    text = f'Candidate `{result["candidate"]["revision"]}`: '
    text += '**25/25 confirmed on both architectures**.\n\n' if result['qualified'] else '**target not met**.\n\n'
    text += ('Default routing only; CPU, HTTP p50/p95 and peak cgroup memory must be lower in every retained run. '
             'Close timings (within 5%) or noisy CPU require at least three runs. '
             'Both modes retain the published decoded-pixel error envelope; PNG identities stay exact. '
             'This pixel agreement check does not certify perceptual equivalence.\n\n')
    for arch, cohort in sorted(result['architectures'].items()):
        text += f'### Linux {arch}: {cohort["confirmed_cases"]}/25 confirmed, {cohort["runs"]} runs\n\n'
        text += '| Case | Status | CPU ratio | p50 ratio | p95 ratio | Peak cgroup ratio |\n|---|---|---:|---:|---:|---:|\n'
        for c in cohort['cases']:
            cell = lambda m: f'{min(c["ratios"][m]):.3f}–{max(c["ratios"][m]):.3f}×'
            text += f'| {c["id"]} | {c["status"]} | ' + ' | '.join(cell(m) for m in METRICS) + ' |\n'
        text += '\n'
        for c in cohort['cases']:
            for failure in c['quality_failures']:
                text += f'- {c["id"]}: {failure}\n'
    return text


def load_baselines(directory=ROOT / 'benchmarks/results'):
    check_results_directory(directory)
    return [load(directory / f'linux-{arch}.raw.json.gz')[0] for arch in ('amd64', 'arm64')]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reports', type=Path, nargs='+', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--markdown', type=Path)
    args = parser.parse_args()
    result = qualify([load(p)[0] for p in args.reports], load_baselines())
    args.output.write_text(json.dumps(result, indent=2, allow_nan=False) + '\n')
    if args.markdown:
        args.markdown.write_text(markdown(result))
    print('; '.join(f'{arch}: {r["confirmed_cases"]}/25 confirmed ({r["runs"]} runs)'
                    for arch, r in result['architectures'].items()))
    return 0 if result['qualified'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
