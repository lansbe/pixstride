#!/usr/bin/env python3
"""Publish/check qualified repeated candidate evidence beside the release baseline."""
import argparse
import gzip
import json
from pathlib import Path
import re

from benchmark_fixtures import ROOT, sha256
from benchmark_qualification import METRICS, MODES, load_baselines, markdown, qualify, samples_hash
from benchmark_report import load

INDEX = 'candidate-index.json'
LEGACY_DEFAULT_REVISIONS = {
    'fe350b73b7841251f99a224abe33f46acb970842',
    '6145080e14a5e717a7a85ae9fd7174f894e08e39',
}
DOCS = {'README.md': ('summary', 'summary'), 'BENCHMARKS.md': ('details', 'tables')}


def encoded(value):
    return (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + '\n').encode()


def span(values):
    precision = 8 if .9999 < max(values) < 1 else 4
    return f'{min(values):.{precision}f}–{max(values):.{precision}f}×'


def generated(reports, result):
    if result["criteria"].get("performance_modes") == list(MODES):
        return generated_both(reports, result)
    revision = result['candidate']['revision']
    source = f'[candidate `{revision[:12]}`](https://github.com/lansbe/pixstride/commit/{revision})'
    intro = f'Unreleased {source}; default routing, all retained native runs. '
    intro += 'The published 0.3.0 baseline below is unchanged.\n\n'
    summary = intro + '| Linux | Confirmed cases | Runs | CPU ratio range | HTTP p50 range | HTTP p95 range | Peak cgroup range |\n'
    summary += '|---|---:|---:|---:|---:|---:|---:|\n'
    for arch, cohort in sorted(result['architectures'].items()):
        cells = [span([v for c in cohort['cases'] for v in c['ratios'][metric]]) for metric in METRICS]
        summary += f'| {arch} | {cohort["confirmed_cases"]}/25 | {cohort["runs"]} | ' + ' | '.join(cells) + ' |\n'
    summary += ('\nRatios are Pixstride/imgproxy; lower is better. Counts require lower CPU, p50, p95 and peak '
                'cgroup memory in every run, with extra repeats near parity. Different encoding can increase file size. '
                'Decoded-pixel agreement is checked against the published envelope; this does not certify perceptual equivalence. '
                'See [candidate evidence](benchmarks/results/candidate-index.json) and [all cases](BENCHMARKS.md).\n')
    details = '## Unreleased candidate confirmation\n\n' + intro + markdown(result)
    for arch in sorted(result['architectures']):
        models = sorted({m for r in reports if r['host']['architecture'] == arch for m in r['host'].get('cpu_model', [])})
        details += f'\n{arch} CPU: ' + ', '.join(models or ['see raw metadata']) + '. Two CPUs are assigned per service.\n'
    details += ('\n### Encoded size and decoded-pixel agreement\n\n'
                'Default routing; ranges cover every retained run. Color error is the worst black/white composite '
                'MAE/RMSE against imgproxy; alpha is checked separately. Both modes must pass the pixel gate. '
                'These are pairwise errors, not quality scores.\n\n'
                '| Linux | Case | Encoded size ratio | Max color MAE | Max color RMSE | Max alpha MAE |\n'
                '|---|---|---:|---:|---:|---:|\n')
    for arch in sorted(result['architectures']):
        runs = [{c['id']: c for c in r['cases']} for r in reports if r['host']['architecture'] == arch]
        for cid in sorted(runs[0]):
            pairs = [(r[cid]['engines']['pixstride_default'], r[cid]['engines']['imgproxy']) for r in runs]
            sizes = [own['output']['bytes'] / ref['output']['bytes'] for own, ref in pairs]
            error = lambda metric: max(own['pixels_vs_imgproxy'][bg][metric] for own, _ in pairs for bg in ('black', 'white'))
            alpha = max(own['pixels_vs_imgproxy']['alpha']['mae'] for own, _ in pairs)
            details += f'| {arch} | {cid} | {span(sizes)} | {error("mae"):.6f} | {error("rmse"):.6f} | {alpha:.6f} |\n'
    details += '\nRaw reports, hashes, source/image identities and recomputed qualification: [candidate index](benchmarks/results/candidate-index.json).\n'
    workflows = sorted({r.get('workflow_url', '') for r in reports} - {''})
    if workflows:
        details += '\nRetained workflow evidence: ' + ', '.join(f'[run {i + 1}]({url})' for i, url in enumerate(workflows)) + '.\n'
    return {'README.md': summary, 'BENCHMARKS.md': details}


def generated_both(reports, result):
    revision = result['candidate']['revision']
    intro = (f'Unreleased [candidate `{revision[:12]}`](https://github.com/lansbe/pixstride/commit/{revision}); '
             'default and forced-native, all retained native Linux runs. The published 0.3.0 baseline below is unchanged.\n\n')
    summary = intro + '| Linux | Mode | Confirmed cases | Runs | CPU ratio range | HTTP p50 range | HTTP p95 range | Peak cgroup range |\n'
    summary += '|---|---|---:|---:|---:|---:|---:|---:|\n'
    for arch, cohort in sorted(result['architectures'].items()):
        for mode in MODES:
            label = 'default' if mode == MODES[0] else 'forced-native'
            cells = [span([v for c in cohort['cases'] for v in c['modes'][mode]['ratios'][metric]]) for metric in METRICS]
            summary += f'| {arch} | {label} | {cohort["confirmed_cases_by_mode"][mode]}/25 | {cohort["runs"]} | ' + ' | '.join(cells) + ' |\n'
    summary += ('\nRatios are Pixstride/imgproxy; lower is better. Both modes must have lower CPU, p50, p95 and '
                'peak cgroup memory in every retained run; close/noisy timings require extra repeats. '
                'Encoded files and pixels can differ; the published agreement envelope is checked, without certifying '
                'perceptual equivalence. See [all cases and limits](BENCHMARKS.md) and [raw evidence](benchmarks/results/candidate-index.json).\n')
    details = '## Unreleased candidate confirmation\n\n' + intro + markdown(result)
    for arch in sorted(result['architectures']):
        models = sorted({m for r in reports if r['host']['architecture'] == arch for m in r['host'].get('cpu_model', [])})
        details += f'\n{arch} CPU: ' + ', '.join(models or ['see raw metadata']) + '. Two CPUs are assigned per service.\n'
    details += ('\n### Encoded size and decoded-pixel agreement\n\n'
                'Every retained run and both modes. Color MAE/RMSE are the worst black/white composite '
                'errors in 8-bit channel units; alpha is checked separately. These are pairwise errors, not quality scores.\n\n'
                '| Linux | Mode | Case | Encoded size ratio | Max color MAE | Max color RMSE | Max alpha MAE |\n'
                '|---|---|---|---:|---:|---:|---:|\n')
    for arch in sorted(result['architectures']):
        runs = [{c['id']: c for c in r['cases']} for r in reports if r['host']['architecture'] == arch]
        for mode in MODES:
            label = 'default' if mode == MODES[0] else 'forced-native'
            for cid in sorted(runs[0]):
                pairs = [(r[cid]['engines'][mode], r[cid]['engines']['imgproxy']) for r in runs]
                sizes = [own['output']['bytes'] / ref['output']['bytes'] for own, ref in pairs]
                error = lambda metric: max(own['pixels_vs_imgproxy'][bg][metric] for own, _ in pairs for bg in ('black', 'white'))
                alpha = max(own['pixels_vs_imgproxy']['alpha']['mae'] for own, _ in pairs)
                details += f'| {arch} | {label} | {cid} | {span(sizes)} | {error("mae"):.6f} | {error("rmse"):.6f} | {alpha:.6f} |\n'
    details += '\nRaw reports, hashes, source/image identities and recomputed both-mode qualification: [candidate index](benchmarks/results/candidate-index.json).\n'
    workflows = sorted({r.get('workflow_url', '') for r in reports} - {''})
    if workflows:
        details += '\nRetained workflow evidence: ' + ', '.join(f'[run {i + 1}]({url})' for i, url in enumerate(workflows)) + '.\n'
    return {'README.md': summary, 'BENCHMARKS.md': details}


def with_block(text, kind, anchor, body, *, namespace='candidate', anchor_namespace='benchmark'):
    start, end = f'<!-- {namespace}:{kind}:start -->', f'<!-- {namespace}:{kind}:end -->'
    block = start + '\n' + body.rstrip() + '\n' + end
    if start not in text and end not in text:
        marker = f'<!-- {anchor_namespace}:{anchor}:start -->'
        if text.count(marker) != 1:
            raise ValueError('Missing candidate insertion anchor')
        return text.replace(marker, block + '\n\n' + marker)
    if text.count(start) != 1 or text.count(end) != 1:
        raise ValueError('Duplicate/missing candidate documentation marker')
    before, remainder = text.split(start, 1)
    if end not in remainder:
        raise ValueError('Misordered candidate documentation markers')
    _, after = remainder.split(end, 1)
    return before + block + after


def publish(paths, root=ROOT):
    directory = root / 'benchmarks/results'
    loaded = sorted([load(p) for p in paths],
                    key=lambda p: (p[0]['host']['architecture'], samples_hash(p[0])))
    reports = [r for r, _ in loaded]
    result = qualify(reports, load_baselines(directory))
    if not result['qualified']:
        raise ValueError('Candidate target not met; retain workflow reports and resolve remaining losses')
    files, archives = [], {}
    revision = result['candidate']['revision']
    for report, raw in loaded:
        arch, fingerprint = report['host']['architecture'], samples_hash(report)
        name = f'candidate-{revision[:12]}-{arch}-{fingerprint[:12]}.raw.json.gz'
        if name in archives:
            raise ValueError('Candidate archive name collision')
        archive = gzip.compress(raw, mtime=0)
        archives[name] = archive
        files.append({'file': name, 'sha256': sha256(archive), 'raw_sha256': sha256(raw),
                      'architecture': arch, 'samples_sha256': fingerprint})
    index = {'schema_version': 2, 'candidate': result['candidate'], 'reports': files, 'qualification': result}
    documents = {name: with_block((root / name).read_text(), *DOCS[name], body)
                 for name, body in generated(reports, result).items()}
    for name, archive in archives.items():
        (directory / name).write_bytes(archive)
    (directory / INDEX).write_bytes(encoded(index))
    for name, text in documents.items():
        (root / name).write_text(text)
    return result


def check(root=ROOT):
    directory = root / 'benchmarks/results'
    path = directory / INDEX
    if not path.exists():
        if list(directory.glob('candidate-*.raw.json.gz')) or any('<!-- candidate:' in (root / n).read_text() for n in DOCS):
            raise ValueError('Candidate evidence/index is missing')
        return None
    index = json.loads(path.read_text())
    version = index.get('schema_version')
    if version not in (1, 2):
        raise ValueError('Unsupported candidate snapshot schema')
    if version == 1 and index.get('candidate', {}).get('revision') not in LEGACY_DEFAULT_REVISIONS:
        raise ValueError('Default-only evidence is restricted to historical source revisions')
    reports = []
    for entry in index['reports']:
        name = entry['file']
        if not re.fullmatch(r'candidate-[0-9a-f]{12}-(amd64|arm64)-[0-9a-f]{12}\.raw\.json\.gz', name):
            raise ValueError('Invalid candidate archive name')
        archive = directory / name
        if archive.is_symlink() or sha256(archive.read_bytes()) != entry['sha256']:
            raise ValueError('Candidate archive hash mismatch')
        report, raw = load(archive)
        expected = (f'candidate-{report["images"]["pixstride"]["candidate"]["revision"][:12]}-'
                    f'{report["host"]["architecture"]}-{samples_hash(report)[:12]}.raw.json.gz')
        if (sha256(raw) != entry['raw_sha256'] or samples_hash(report) != entry['samples_sha256']
                or report['host']['architecture'] != entry['architecture'] or name != expected):
            raise ValueError('Candidate raw provenance mismatch')
        reports.append(report)
    result = qualify(reports, load_baselines(directory),
                     performance_modes=(MODES[0],) if version == 1 else MODES)
    if (not result['qualified'] or result['candidate'] != index['candidate']
            or encoded(result) != encoded(index['qualification'])):
        raise ValueError('Stale/unqualified candidate snapshot')
    for name, body in generated(reports, result).items():
        text = (root / name).read_text()
        if text != with_block(text, *DOCS[name], body):
            raise ValueError('Stale candidate documentation: ' + name)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument('--publish', type=Path, nargs='+', metavar='REPORT')
    action.add_argument('--check', action='store_true')
    args = parser.parse_args()
    result = publish(args.publish) if args.publish else check()
    if args.check:
        from candidate_observations import check as check_observations
        if check_observations():
            print('Latest observed cohort, including losses, is consistent')
    print('Qualified candidate evidence and documentation are consistent' if result else 'No candidate snapshot published')


if __name__ == '__main__':
    main()
