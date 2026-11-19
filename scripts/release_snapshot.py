#!/usr/bin/env python3
"""Record/check all six actual published-image runs, including unfavorable results."""
import argparse
from collections import Counter
import gzip
import json
from pathlib import Path
import re
import statistics

from benchmark_fixtures import ROOT, sha256
from benchmark_qualification import METRICS, MODES, load_baselines, markdown, qualify, samples_hash
from benchmark_release import identity
from benchmark_report import load
from candidate_snapshot import DOCS, encoded, generated_both, with_block

INDEX = 'released-index.json'


def pins(root=ROOT, path=None):
    value = json.loads((path or root / 'benchmarks/released-versions.json').read_text())['pixstride']
    if value.get('published') is not True:
        raise ValueError('Released comparisons require explicit published image pins')
    return identity(value)


def qualification(reports, root=ROOT, versions=None, baseline=None):
    if Counter(r['host']['architecture'] for r in reports) != {'amd64': 3, 'arm64': 3}:
        raise ValueError('Released observations require exactly three full runs per architecture')
    return qualify(reports, load_baselines(baseline or root / 'benchmarks/results'),
                   released=pins(root, versions))


def generated(reports, result):
    release = result['release']
    version, revision = release['version'], release['revision']
    status = 'target met' if result['qualified'] else 'target not met'
    intro = (f'Published Pixstride {version} ([release](https://github.com/lansbe/pixstride/releases/tag/v{version})) '
             f'versus imgproxy 4.0.24: **{status}** for the complete both-mode target. '
             'Every run and every loss is retained.\n\n'
             f'Image: `{release["image"]}`. Runtime source: '
             f'[`{revision[:12]}`](https://github.com/lansbe/pixstride/commit/{revision}). '
             f'Harness: [`{result["harness_revision"][:12]}`](https://github.com/lansbe/pixstride/commit/{result["harness_revision"]}). '
             'These are published-image measurements; no local source build replaces the release.\n\n')
    documents = generated_both(reports, result, intro=intro, index=INDEX,
                               heading='## Published release comparison')
    medians = ('\nMedian per-case reductions (median of three runs per case, then median of 25 cases; '
               'positive means less, negative means more; not a workload-weighted average):\n\n'
               '| Linux | Mode | CPU/request | HTTP p50 | HTTP p95 | Peak cgroup memory |\n'
               '|---|---|---:|---:|---:|---:|\n')
    for arch, cohort in sorted(result['architectures'].items()):
        for mode in MODES:
            values = [100 * (1 - statistics.median(statistics.median(c['modes'][mode]['ratios'][metric])
                                                   for c in cohort['cases'])) for metric in METRICS]
            label = 'default' if mode == MODES[0] else 'forced-native'
            medians += f'| {arch} | {label} | ' + ' | '.join(f'{v:+.1f}%' for v in values) + ' |\n'
    documents['README.md'] += medians
    return {name: text.replace('[candidate index]', '[published release index]')
            for name, text in documents.items()}


def document(text, name, body):
    kind, old_anchor = DOCS[name]
    for namespace in ('observed', 'candidate'):
        if f'<!-- {namespace}:{kind}:start -->' in text:
            return with_block(text, kind, kind, body, namespace='released', anchor_namespace=namespace)
    return with_block(text, kind, old_anchor, body, namespace='released')


def record(paths, root=ROOT):
    directory = root / 'benchmarks/results'
    loaded = sorted([load(p) for p in paths], key=lambda p: (p[0]['host']['architecture'], samples_hash(p[0])))
    reports = [r for r, _ in loaded]
    result = qualification(reports, root)
    version = result['release']['version']
    files, archives = [], {}
    for report, raw in loaded:
        arch, fingerprint = report['host']['architecture'], samples_hash(report)
        name = f'released-{version}-{arch}-{fingerprint[:12]}.raw.json.gz'
        if name in archives: raise ValueError('Release archive collision')
        archive = gzip.compress(raw, mtime=0); archives[name] = archive
        files.append(dict(file=name, sha256=sha256(archive), raw_sha256=sha256(raw),
                          architecture=arch, samples_sha256=fingerprint))
    documents = {name: document((root / name).read_text(), name, body)
                 for name, body in generated(reports, result).items()}
    index = dict(schema_version=1, kind='published-release-comparison', release=result['release'],
                 harness_revision=result['harness_revision'], reports=files, qualification=result)
    for name, archive in archives.items(): (directory / name).write_bytes(archive)
    (directory / INDEX).write_bytes(encoded(index))
    for name, body in documents.items(): (root / name).write_text(body)
    return result


def check(root=ROOT):
    directory = root / 'benchmarks/results'
    path = directory / INDEX
    if not path.exists():
        if list(directory.glob('released-*.raw.json.gz')) or any(
                '<!-- released:' in (root / name).read_text() for name in DOCS):
            raise ValueError('Missing published release index')
        return None
    index = json.loads(path.read_text())
    if index.get('schema_version') != 1 or index.get('kind') != 'published-release-comparison':
        raise ValueError('Unsupported published release snapshot')
    if index['release'] != pins(root): raise ValueError('Stale/different release pins')
    reports = []
    for entry in index['reports']:
        name = entry['file']
        if not re.fullmatch(r'released-[0-9]+\.[0-9]+\.[0-9]+(?:-(?:alpha|beta|rc)\.[1-9][0-9]*)?-(amd64|arm64)-[0-9a-f]{12}\.raw\.json\.gz', name):
            raise ValueError('Invalid released archive name')
        archive = directory / name
        if archive.is_symlink() or sha256(archive.read_bytes()) != entry['sha256']:
            raise ValueError('Released archive hash mismatch')
        report, raw = load(archive)
        arch, fingerprint = report['host']['architecture'], samples_hash(report)
        if (sha256(raw) != entry['raw_sha256'] or fingerprint != entry['samples_sha256']
                or arch != entry['architecture']
                or name != f'released-{index["release"]["version"]}-{arch}-{fingerprint[:12]}.raw.json.gz'):
            raise ValueError('Released raw provenance mismatch')
        reports.append(report)
    result = qualification(reports, root)
    if (result['harness_revision'] != index['harness_revision']
            or encoded(result) != encoded(index['qualification'])):
        raise ValueError('Stale published release qualification')
    for name, body in generated(reports, result).items():
        text = (root / name).read_text()
        if text != document(text, name, body): raise ValueError('Stale released documentation: ' + name)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument('--record', type=Path, nargs='+', metavar='REPORT')
    action.add_argument('--qualify', type=Path, nargs='+', metavar='REPORT')
    action.add_argument('--check', action='store_true')
    parser.add_argument('--versions', type=Path)
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--markdown', type=Path)
    args = parser.parse_args()
    if args.qualify:
        result = qualification([load(p)[0] for p in args.qualify], versions=args.versions, baseline=args.baseline)
        if not args.output or not args.markdown: parser.error('--qualify requires --output and --markdown')
        args.output.parent.mkdir(parents=True, exist_ok=True); args.output.write_bytes(encoded(result))
        args.markdown.parent.mkdir(parents=True, exist_ok=True); args.markdown.write_text(markdown(result))
    else:
        result = record(args.record) if args.record else check()
    print('Published comparison: ' + ('target met' if result['qualified'] else 'target not met') if result else 'No published comparison')
    if args.qualify and not result['qualified']: raise SystemExit(1)


if __name__ == '__main__':
    main()
