"""Measure the shipping PixStride configuration against verified, archived V2 evidence."""
import argparse
import gzip
import json
import os
from pathlib import Path
import sys
import uuid

from benchmark import host_metadata, isolate_client
from benchmark_fixtures import ROOT, sha256
from benchmark_peer_baseline import BASE, lane
from benchmark_runtime import Docker, architecture
from benchmark_v2 import protocol
from benchmark_v2_catalog import verify_catalog
from benchmark_v2_collect import collect
from benchmark_v2_fixtures import verify
from benchmark_v2_report import quality_failures

ENGINE = 'pixstride_default'
PLAN = ROOT / 'benchmarks/pixstride-selection.json'


def select(cases, plan, index):
    ranks, lanes = plan['ranks'], plan['lanes']
    if not 0 <= index < lanes <= 32:
        raise ValueError('Invalid measurement lane')
    if ranks == 'all':
        selected = cases
    else:
        by_rank = {c['rank']: c for c in cases}
        if not ranks or len(set(ranks)) != len(ranks):
            raise ValueError('Empty or duplicate selection')
        selected = [by_rank[rank] for rank in ranks]
    return selected[index::lanes]


def history(cases, arch, manifest):
    """Read just the relevant archives. Each quality record is tied to its reference hash."""
    index = json.loads((BASE / 'measure-index.json').read_text())
    for field in ('catalog_sha256', 'corpus_sha256'):
        if index['result'][field] != manifest[field]:
            raise ValueError('Input mismatch: ' + field)
    ids = {c['id'] for c in cases}
    shards = {f's{(c["rank"] - 1) // 25:02}' for c in cases}
    results = {i: [] for i in ids}
    for item in index['reports']:
        parts = item['file'].split('-')
        if parts[2] != arch or parts[4] not in shards:
            continue
        compressed = (BASE / item['file']).read_bytes()
        if sha256(compressed) != item['sha256']:
            raise ValueError('Archive checksum mismatch')
        report = json.loads(gzip.decompress(compressed))
        for c in report['cases']:
            if c['id'] in ids:
                engines = c['engines']
                results[c['id']].append(dict(
                    reference=engines['imgproxy']['output']['sha256'],
                    sha=engines[ENGINE]['output']['sha256'],
                    pixels=engines[ENGINE]['pixels_vs_imgproxy'],
                    repetition=report['repetition']))
    if any(sorted(x['repetition'] for x in rows) != [1, 2, 3] for rows in results.values()):
        raise ValueError('Incomplete archived PixStride history')
    return results


def quality(case, entry, comparable):
    failures = quality_failures(case, entry)
    identical = any(entry['output']['sha256'] == r['sha'] for r in comparable)
    if not identical:
        for bg in ('black', 'white', 'alpha'):
            for metric in ('mae', 'rmse'):
                old = min(r['pixels'][bg][metric] for r in comparable)
                if entry['pixels_vs_imgproxy'][bg][metric] > old + 1e-12:
                    failures.append(f'{bg} {metric} worsened from {old}')
    return dict(identical_to_release=identical, quality_failures=failures)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('fixtures', 'pixels', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--image', required=True)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--lane', type=int, required=True)
    args = parser.parse_args()
    if sys.platform != 'linux':
        raise ValueError('Native Linux measurement required')
    manifest = verify(args.fixtures)
    cases = select(verify_catalog(manifest=manifest), json.loads(PLAN.read_text()), args.lane)
    archived = history(cases, architecture(), manifest)
    docker = Docker()
    image = docker.image(args.image, args.revision)
    settings = protocol('measure', '2.3')
    settings['engine_schedule'] = 'PixStride default only'
    settings['case_order'] = 'selection order within lane'
    available = sorted(os.sched_getaffinity(0))
    cpus = available[:2]
    failed = []
    owner = uuid.uuid4().hex[:12]
    with isolate_client(cpus) as client, args.output.open('w') as out:
        info = dict(arch=architecture(), host=host_metadata(cpus, available, client), image=image,
                    protocol=settings, revision=args.revision, run=os.getenv('GITHUB_RUN_ID'),
                    attempt=os.getenv('GITHUB_RUN_ATTEMPT'), ranks=[c['rank'] for c in cases],
                    corpus=manifest['corpus_sha256'], catalog=manifest['catalog_sha256'])
        print(json.dumps(info), file=out, flush=True)
        for c in cases:
            ref = (args.pixels / str(lane(c)) / c['id']).read_bytes()
            digest = sha256(ref)
            comparable = [r for r in archived[c['id']] if r['reference'] == digest]
            if not comparable:
                raise ValueError('Reference hash mismatch: ' + c['id'])
            result = collect(c, docker, {'pixstride': image}, args.fixtures, cpus, owner,
                             c['rank'], settings, 'measure', {}, lambda: None,
                             engines=(ENGINE,), reference_body=ref)
            entry = result['engines'][ENGINE]
            if entry['status'] == 'success':
                entry.update(quality(c, entry, comparable))
            if entry['status'] != 'success' or entry['quality_failures'] or result.get('deadline_exceeded'):
                failed.append(c['id'])
            entry.update(id=c['id'], rank=c['rank'], reference=digest,
                         deadline_exceeded=result.get('deadline_exceeded', False))
            print(json.dumps(entry, allow_nan=False), file=out, flush=True)
            print(c['id'], entry['status'], entry.get('quality_failures', entry.get('error')), flush=True)
    if failed:
        raise SystemExit('Failed qualification: ' + ', '.join(failed))


if __name__ == '__main__':
    main()
