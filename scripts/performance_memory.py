#!/usr/bin/env python3
"""Inspect separate diagnostic containers; never alter qualification measurements."""
import argparse
import json
import os
from pathlib import Path
import re
import sys
import time
import uuid

from benchmark_fixtures import ROOT, load_cases, verify_fixtures
from benchmark_runtime import Docker, Server, architecture, request, signed_path, validate_response, require_native_image

FIELDS = {'Size': 'virtual_bytes', 'Rss': 'rss_bytes', 'Pss': 'pss_bytes',
          'Anonymous': 'anonymous_bytes', 'Private_Dirty': 'private_dirty_bytes',
          'Private_Clean': 'private_clean_bytes'}
HEADER = re.compile(r'^[0-9a-f]+-[0-9a-f]+\s+([-rwxps]{4})\s+\S+\s+\S+\s+\d+(?:\s+(.*))?$')


def parse_smaps(text):
    if not text or len(text) > 2 * 1024 * 1024:
        raise ValueError('Unavailable/oversized memory map')
    mappings = []
    for line in text.splitlines():
        header = HEADER.match(line)
        if header:
            permissions, name = header.groups()
            name = name or ''
            category = ('heap' if name == '[heap]' else 'stack' if name.startswith('[stack')
                        else 'anonymous_executable' if 'x' in permissions and (not name or 'memfd:' in name)
                        else 'anonymous' if not name or name.startswith('[anon') else 'file')
            mappings.append({'category': category, **{field: 0 for field in FIELDS.values()}})
        elif mappings:
            field = re.fullmatch(r'(\w+):\s+(\d+) kB', line)
            if field and field[1] in FIELDS:
                mappings[-1][FIELDS[field[1]]] = int(field[2]) * 1024
    if not mappings:
        raise ValueError('No process mappings')
    categories = {}
    for mapping in mappings:
        total = categories.setdefault(mapping['category'], {field: 0 for field in FIELDS.values()})
        for field in FIELDS.values():
            total[field] += mapping[field]
    return {'mapping_count': len(mappings),
            'totals': {field: sum(m[field] for m in mappings) for field in FIELDS.values()},
            'categories': categories,
            'largest_anonymous': sorted([m for m in mappings if m['anonymous_bytes']],
                                        key=lambda m: m['rss_bytes'], reverse=True)[:10]}


def processes(docker, name):
    try:
        children = docker.run('exec', name, 'sh', '-c', 'cat /proc/1/task/*/children', timeout=10)
        pids = ['1'] + sorted({p for p in children.split() if p.isdigit() and p != '1'}, key=int)[:4]
    except RuntimeError:
        pids = ['1']
    result = []
    for index, pid in enumerate(pids):
        role = 'server' if index == 0 else 'native_' + str(index)
        try:
            value = parse_smaps(docker.run('exec', name, 'cat', f'/proc/{pid}/smaps', timeout=10))
            result.append({'role': role, **value})
        except (RuntimeError, ValueError):
            result.append({'role': role, 'unavailable': True})
    return result


def run(args):
    if sys.platform != 'linux' or not re.fullmatch(r'sha256:[0-9a-f]{64}', args.image):
        raise ValueError('Use native Linux and an immutable local candidate image ID')
    manifest = verify_fixtures(args.fixtures)
    cases = load_cases(ROOT / 'benchmarks/cases.json', manifest['files'])
    selected = set(args.cases)
    if not selected.issubset({c['id'] for c in cases}):
        raise ValueError('Unknown diagnostic case')
    docker = Docker()
    image = json.loads(docker.run('image', 'inspect', args.image))[0]
    require_native_image(image['Architecture'], image['Os'])
    reference = json.loads((ROOT / 'benchmarks/versions.json').read_text())['imgproxy']['image']
    docker.image(reference)
    cpus = sorted(os.sched_getaffinity(0))[:2]
    if len(cpus) != 2:
        raise ValueError('Require two CPUs')
    report = {'kind': 'separate-memory-diagnostics-not-performance-qualification',
              'architecture': architecture(), 'image': args.image,
              'revision': image['Config']['Labels'].get('org.opencontainers.image.revision'),
              'reference': reference, 'corpus_sha256': manifest['corpus_sha256'], 'cases': []}
    owner = uuid.uuid4().hex[:12]
    for index, case in enumerate(c for c in cases if c['id'] in selected):
        for engine, reference_image in [('pixstride_default', args.image), ('imgproxy', reference)]:
            entry = {'id': case['id'], 'engine': engine, 'samples': []}
            report['cases'].append(entry)
            with Server(docker, engine, reference_image, args.fixtures, cpus, owner, index) as server:
                completed, digest = 0, None
                for target in [0, 1, 8, 48]:
                    while completed < target:
                        status, headers, body, _ = request(server.port, signed_path(case))
                        output = validate_response(case, status, headers, body, digest)
                        digest = output['sha256']
                        completed += 1
                    time.sleep(.05)
                    entry['samples'].append({'completed_requests': completed,
                                             'processes': processes(docker, server.name)})
                    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print('Separate memory diagnostics completed; no timings or qualification counters collected')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--image', required=True)
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--cases', nargs='+', default=['01-jpeg-identity', '06-photo-crop',
                        '09-photo-sharpen', '19-alpha-jpeg', '24-tiff-png'])
    run(parser.parse_args())


if __name__ == '__main__':
    main()
