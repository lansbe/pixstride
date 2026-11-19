#!/usr/bin/env python3
"""Record/check all six measured runs, including failures; never qualify a loss."""
import argparse
from collections import Counter
import gzip
import json
import re
from pathlib import Path

from benchmark_fixtures import ROOT, sha256
from benchmark_qualification import load_baselines, qualify, samples_hash
from benchmark_report import load
from candidate_snapshot import DOCS, encoded, generated_both, with_block

INDEX = 'observed-index.json'


def qualification(reports, directory):
    if Counter(r['host']['architecture'] for r in reports) != {'amd64': 3, 'arm64': 3}:
        raise ValueError('Observations require exactly three full runs on each architecture')
    return qualify(reports, load_baselines(directory))


def generated(reports, result):
    status = 'target met' if result['qualified'] else 'target not met'
    documents = generated_both(reports, result)
    for name, text in documents.items():
        text = text.replace('## Unreleased candidate confirmation', '## Retained pre-release source cohort')
        text = text.replace('benchmarks/results/candidate-index.json', 'benchmarks/results/' + INDEX)
        documents[name] = (f'Retained pre-release source cohort: **{status}**. Every retained run is included; '
                           'these observations do not replace the historical qualified snapshot.\n\n' + text)
    return documents


def document(text, kind, body):
    return with_block(text, kind, kind, body, namespace='observed', anchor_namespace='candidate')


def record(paths, root=ROOT):
    directory = root / 'benchmarks/results'
    loaded = sorted([load(p) for p in paths],
                    key=lambda p: (p[0]['host']['architecture'], samples_hash(p[0])))
    reports = [r for r, _ in loaded]
    result = qualification(reports, directory)
    archives, files = {}, []
    for report, raw in loaded:
        arch, fingerprint = report['host']['architecture'], samples_hash(report)
        revision = report['images']['pixstride']['candidate']['revision']
        name = f'observed-{revision[:12]}-{arch}-{fingerprint[:12]}.raw.json.gz'
        if name in archives:
            raise ValueError('Observation archive collision')
        archive = gzip.compress(raw, mtime=0)
        archives[name] = archive
        files.append(dict(file=name, sha256=sha256(archive), raw_sha256=sha256(raw),
                          architecture=arch, samples_sha256=fingerprint))
    documents = {name: document((root / name).read_text(), DOCS[name][0], body)
                 for name, body in generated(reports, result).items()}
    index = dict(schema_version=1, kind='observed-both-modes', candidate=result['candidate'],
                 reports=files, qualification=result)
    for name, archive in archives.items():
        (directory / name).write_bytes(archive)
    (directory / INDEX).write_bytes(encoded(index))
    for name, body in documents.items():
        (root / name).write_text(body)
    return result


def check(root=ROOT):
    directory = root / 'benchmarks/results'
    path = directory / INDEX
    if not path.exists():
        if list(directory.glob('observed-*.raw.json.gz')) or any(
                '<!-- observed:' in (root / name).read_text() for name in DOCS):
            raise ValueError('Missing observed cohort index')
        return None
    index = json.loads(path.read_text())
    if index.get('schema_version') != 1 or index.get('kind') != 'observed-both-modes':
        raise ValueError('Unsupported observation index')
    reports = []
    for entry in index['reports']:
        name = entry['file']
        if not re.fullmatch(r'observed-[0-9a-f]{12}-(amd64|arm64)-[0-9a-f]{12}\.raw\.json\.gz', name):
            raise ValueError('Invalid observation archive name')
        archive = directory / name
        if archive.is_symlink() or sha256(archive.read_bytes()) != entry['sha256']:
            raise ValueError('Observation archive hash mismatch')
        report, raw = load(archive)
        arch, fingerprint = report['host']['architecture'], samples_hash(report)
        revision = report['images']['pixstride']['candidate']['revision']
        if (sha256(raw) != entry['raw_sha256'] or fingerprint != entry['samples_sha256']
                or arch != entry['architecture']
                or name != f'observed-{revision[:12]}-{arch}-{fingerprint[:12]}.raw.json.gz'):
            raise ValueError('Observation raw provenance mismatch')
        reports.append(report)
    result = qualification(reports, directory)
    if result['candidate'] != index['candidate'] or encoded(result) != encoded(index['qualification']):
        raise ValueError('Stale observation qualification')
    for name, body in generated(reports, result).items():
        text = (root / name).read_text()
        if text != document(text, DOCS[name][0], body):
            raise ValueError('Stale observation documentation: ' + name)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument('--record', type=Path, nargs='+', metavar='REPORT')
    action.add_argument('--check', action='store_true')
    args = parser.parse_args()
    result = record(args.record) if args.record else check()
    print('Observed cohort: ' + ('target met' if result['qualified'] else 'target not met') if result else 'No observed cohort')


if __name__ == '__main__':
    main()
