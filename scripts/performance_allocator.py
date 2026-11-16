#!/usr/bin/env python3
"""Compare allocator policies in separate native diagnostic containers after qualification measurements."""
import argparse
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import uuid

from benchmark_fixtures import ROOT, verify_fixtures
from benchmark_runtime import architecture, candidate_source, require_native_image

DELAYS = (0, 1, 10, 100, 1000)
OPERATIONS = ('blur', 'sharpen')


def validate_observations(entries):
    expected = {(delay, operation) for delay in DELAYS for operation in OPERATIONS}
    observed, hashes = set(), {operation: set() for operation in OPERATIONS}
    for entry in entries:
        key = (entry.get('purge_delay_ms'), entry.get('operation'))
        if (type(key[0]) is not int or key not in expected or key in observed or
                type(entry.get('samples')) is not int or entry['samples'] != 48 or
                type(entry.get('allocator_version')) is not int or entry['allocator_version'] != 30302):
            raise ValueError('Incomplete, duplicate or incompatible allocator observations')
        observed.add(key)
        for field in ('processing_p50_ms', 'processing_p95_ms', 'cpu_mean_ms', 'cgroup_peak_memory_bytes'):
            value = entry.get(field)
            if type(value) not in (int, float) or not math.isfinite(value) or value <= 0:
                raise ValueError('Invalid allocator diagnostic counter')
        if entry['processing_p50_ms'] > entry['processing_p95_ms']:
            raise ValueError('Invalid latency quantiles')
        digest = entry.get('output_sha256')
        if not isinstance(digest, str) or not re.fullmatch('[0-9a-f]{64}', digest):
            raise ValueError('Invalid output checksum')
        hashes[key[1]].add(digest)
    if observed != expected or any(len(values) != 1 for values in hashes.values()):
        raise ValueError('Missing policies or allocator policies changed encoded output')


def command(*args, capture=False):
    return subprocess.run(list(args), check=True, text=True, timeout=900,
                          stdout=subprocess.PIPE if capture else None).stdout


def run(args):
    if sys.platform != 'linux' or not hasattr(os, 'sched_getaffinity'):
        raise ValueError('Native Linux and a local Docker daemon are required')
    endpoint = os.environ.get('DOCKER_HOST') or json.loads(command(
        'docker', 'context', 'inspect', capture=True))[0]['Endpoints']['docker']['Host']
    if not endpoint.startswith('unix://'):
        raise ValueError('Require a local Docker daemon')
    daemon = json.loads(command('docker', 'info', '--format', '{{json .}}', capture=True))
    require_native_image(daemon['Architecture'], daemon['OSType'])
    cpus = sorted(os.sched_getaffinity(0))[:2]
    if len(cpus) != 2:
        raise ValueError('Require two logical CPUs')
    manifest = verify_fixtures(args.fixtures)
    revision = command('git', '-C', str(ROOT), 'rev-parse', 'HEAD', capture=True).strip()
    source = candidate_source(revision)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    owner = 'allocator-' + uuid.uuid4().hex[:12]
    builder, image = 'pixstride:allocator-build-' + owner, 'pixstride:allocator-probe-' + owner
    result = {'kind': 'allocator-policy-diagnostic-not-performance-qualification', 'complete': False,
              'architecture': architecture(), 'corpus_sha256': manifest['corpus_sha256'],
              'revision': revision, 'source': source,
              'scope': 'Native JPEG primitives; excludes HTTP/Rust IPC; 6 warmups and 48 samples; fresh containers.',
              'policies': []}
    try:
        command('docker', 'build', '--target', 'build', '-t', builder, str(ROOT))
        with tempfile.TemporaryDirectory(prefix='pixstride-allocator-') as temporary:
            context = Path(temporary)
            for name in ('benchmarks/allocator_profile.c', 'THIRD_PARTY_NOTICES.md'):
                (context / Path(name).name).write_bytes((ROOT / name).read_bytes())
            (context / 'Dockerfile').write_text(f'''FROM {builder}
COPY allocator_profile.c /tmp/allocator_profile.c
COPY THIRD_PARTY_NOTICES.md /usr/share/doc/pixstride/THIRD_PARTY_NOTICES.md
RUN --network=none set -- /usr/local/cargo/registry/src/*/libmimalloc-sys-0.1.49/c_src/mimalloc/v3; test "$#" -eq 1; \\
    cc -O3 -DNDEBUG -DMI_DEBUG=0 -DMI_BUILD_RELEASE -DMI_MALLOC_OVERRIDE -DMI_NO_THP=1 \\
    -ftls-model=initial-exec -fno-builtin-malloc -I "$1/include" "$1/src/static.c" \\
    /tmp/allocator_profile.c $(pkg-config --cflags --libs vips) -pthread -ldl -lm -o /usr/local/bin/allocator-profile
ENTRYPOINT ["/usr/local/bin/allocator-profile"]
''')
            command('docker', 'build', '-t', image, str(context))
        inspected = json.loads(command('docker', 'image', 'inspect', image, capture=True))[0]
        require_native_image(inspected['Architecture'], inspected['Os'])
        image_id = result['image_id'] = inspected['Id']
        for delay in DELAYS:
            for operation in OPERATIONS:
                raw = command('docker', 'run', '--rm', '--network', 'none', '--read-only',
                    '--cpuset-cpus', ','.join(map(str, cpus)), '--cpus', '2', '--memory', '1g',
                    '--memory-swap', '1g', '--pids-limit', '128', '--cap-drop', 'ALL',
                    '--label', 'io.pixstride.benchmark.run=' + owner, '--security-opt', 'no-new-privileges',
                    '--mount', 'type=bind,src=' + str(args.fixtures.resolve()) + ',dst=/data,readonly',
                    '--env', 'MIMALLOC_ARENA_RESERVE=64MiB', '--env', 'MIMALLOC_PURGE_DELAY=' + str(delay),
                    '--env', 'VIPS_DISC_THRESHOLD=1073741824', image_id, '/data/landscape.jpg', operation, capture=True)
                observation = json.loads(raw)
                observation['purge_delay_ms'] = delay
                result['policies'].append(observation)
                args.output.write_text(json.dumps(result, indent=2, allow_nan=False) + '\n')
                print(operation, delay, observation['cpu_mean_ms'], observation['cgroup_peak_memory_bytes'], flush=True)
        validate_observations(result['policies'])
        result['complete'] = True
        args.output.write_text(json.dumps(result, indent=2, allow_nan=False) + '\n')
    finally:
        # Best effort, bounded cleanup must not hide the original diagnostic failure.
        try:
            containers = command('docker', 'ps', '-aq', '--filter', 'label=io.pixstride.benchmark.run=' + owner, capture=True).split()
            if containers:
                command('docker', 'rm', '-f', *containers)
        except (subprocess.SubprocessError, OSError) as error:
            print('Allocator container cleanup failed:', error, file=sys.stderr)
        try:
            subprocess.run(['docker', 'image', 'rm', image, builder], check=False, timeout=60,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        except (subprocess.SubprocessError, OSError) as error:
            print('Allocator image cleanup failed:', error, file=sys.stderr)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    run(parser.parse_args())
