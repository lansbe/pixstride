#!/usr/bin/env python3
"""Compare native allocator/page policies in fresh containers; not HTTP qualification."""
import argparse
import json
import math
import os
from pathlib import Path
import random
import re
import subprocess
import sys
import tempfile
import uuid

from benchmark_fixtures import ROOT, verify_fixtures
from benchmark import host_metadata
from benchmark_runtime import architecture, candidate_source, require_native_image
from performance_allocator import command


def policies():
    entries = []
    for arenas in (1, 2):
        for threshold in (131072, 4194304):
            for trim in (131072, 4194304):
                for cache in ('1', '4', 'default'):
                    entries.append(dict(id=f'glibc-arena{arenas}-mmap{threshold}-trim{trim}-cache{cache}',
                                        allocator='glibc', disable_thp=True, arenas=arenas,
                                        mmap_threshold=threshold, trim_threshold=trim,
                                        tcache_count=cache, collect=False))
    entries.extend([
        dict(id='glibc-original-control', allocator='glibc', disable_thp=False, arenas=2,
             mmap_threshold=4194304, trim_threshold=4194304, tcache_count='default', collect=False),
        dict(id='mimalloc-original-control', allocator='mimalloc', disable_thp=False, arenas=2,
             purge_delay_ms=1000, collect=False),
    ])
    return entries


def validate(entries):
    configs = {p['id']: p for p in policies()}
    expected = {(key, operation) for key in configs for operation in ('blur', 'sharpen')}
    seen, hashes = set(), {'blur': set(), 'sharpen': set()}
    for entry in entries:
        key = (entry.get('policy'), entry.get('operation'))
        if key not in expected or key in seen:
            raise ValueError('Unknown or duplicate diagnostic policy')
        seen.add(key)
        config, metadata = configs[key[0]], entry.get('metadata', {})
        if type(entry.get('samples')) is not int or entry['samples'] != 48:
            raise ValueError('Incomplete diagnostic sample count')
        for field in ('processing_p50_ms', 'processing_p95_ms', 'cpu_mean_ms', 'cgroup_peak_memory_bytes'):
            value = entry.get(field)
            if type(value) not in (int, float) or not math.isfinite(value) or value <= 0:
                raise ValueError('Invalid diagnostic counter')
        if entry['processing_p50_ms'] > entry['processing_p95_ms']:
            raise ValueError('Invalid diagnostic percentiles')
        digest = entry.get('output_sha256', '')
        if not isinstance(digest, str) or not re.fullmatch('[0-9a-f]{64}', digest):
            raise ValueError('Invalid diagnostic output hash')
        hashes[key[1]].add(digest)
        version = entry.get('allocator_version')
        libc_version = metadata.get('libc_version')
        if (type(version) is not int or metadata.get('allocator') != config['allocator'] or
                not isinstance(libc_version, str) or not re.fullmatch(r'2\.\d+(?:\.\d+)?', libc_version)):
            raise ValueError('Invalid allocator version/provenance')
        expected_version = 30302 if config['allocator'] == 'mimalloc' else 20000 + int(libc_version.split('.')[1])
        if version != expected_version:
            raise ValueError('Allocator version differs from its observed implementation')
        disabled, huge = metadata.get('thp_disabled'), metadata.get('anonymous_huge_bytes')
        if (type(disabled) is not int or disabled not in (0, 1) or type(huge) is not int or huge < 0 or
                (config['disable_thp'] and (disabled != 1 or huge != 0))):
            raise ValueError('Huge-page control failed or memory evidence is missing')
        peak, anonymous = metadata.get('process_peak_rss_bytes'), metadata.get('process_anonymous_bytes')
        if type(peak) is not int or type(anonymous) is not int or not 0 < anonymous <= peak:
            raise ValueError('Missing or invalid resident-memory counters')
    if seen != expected or any(len(values) != 1 for values in hashes.values()):
        raise ValueError('Incomplete matrix or policies changed encoded output')


def run(args):
    if sys.platform != 'linux' or not hasattr(os, 'sched_getaffinity'):
        raise ValueError('Use native Linux and a local Docker daemon')
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
    owner = 'allocator-matrix-' + uuid.uuid4().hex[:12]
    builder, image = 'pixstride:builder-' + owner, 'pixstride:probe-' + owner
    result = dict(kind='allocator-comparison-not-performance-qualification', schema_version=1, complete=False,
                  architecture=architecture(), host=host_metadata(cpus), source=source, corpus_sha256=manifest['corpus_sha256'],
                  policy_configurations=policies(),
                  host_thp_mode=Path('/sys/kernel/mm/transparent_hugepage/enabled').read_text().strip(),
                  scope='Native primitives: 6 warmups/48 samples; no HTTP/Rust IPC/imgproxy.', observations=[])
    try:
        command('docker', 'build', '--target', 'build', '-t', builder, str(ROOT))
        with tempfile.TemporaryDirectory(prefix='pixstride-allocator-matrix-') as temporary:
            context = Path(temporary)
            for name in ('benchmarks/allocator_profile.c', 'benchmarks/allocator_diagnostics.h', 'THIRD_PARTY_NOTICES.md'):
                (context / Path(name).name).write_bytes((ROOT / name).read_bytes())
            (context / 'Dockerfile').write_text(f'''FROM {builder}
COPY allocator_profile.c allocator_diagnostics.h /tmp/
COPY THIRD_PARTY_NOTICES.md /usr/share/doc/pixstride/THIRD_PARTY_NOTICES.md
RUN --network=none set -eu; set -- /usr/local/cargo/registry/src/*/libmimalloc-sys-0.1.49/c_src/mimalloc/v3; test "$#" -eq 1; \\
    cc -c -O3 -DNDEBUG -DMI_DEBUG=0 -DMI_BUILD_RELEASE -DMI_MALLOC_OVERRIDE -DMI_NO_THP=1 \\
    -ftls-model=initial-exec -fno-builtin-malloc -I "$1/include" "$1/src/static.c" -o /tmp/mimalloc.o; \\
    cc -O3 -Wall -Wextra -Werror -DPROFILE_GLIBC -include /tmp/allocator_diagnostics.h -I "$1/include" \\
    /tmp/allocator_profile.c $(pkg-config --cflags --libs vips) -lm -o /usr/local/bin/probe-glibc; \\
    cc -O3 -Wall -Wextra -Werror -include /tmp/allocator_diagnostics.h -I "$1/include" \\
    /tmp/allocator_profile.c /tmp/mimalloc.o $(pkg-config --cflags --libs vips) -pthread -ldl -lm -o /usr/local/bin/probe-mimalloc; \\
    nm -D --defined-only /usr/local/bin/probe-mimalloc | awk '$3 == "malloc" {{ seen=1 }} END {{ exit(!seen) }}'
''')
            command('docker', 'build', '-t', image, str(context))
        inspected = json.loads(command('docker', 'image', 'inspect', image, capture=True))[0]
        require_native_image(inspected['Architecture'], inspected['Os'])
        image_id = result['image_id'] = inspected['Id']
        configs = policies()
        random.Random(943).shuffle(configs)
        for config in configs:
            for operation in ('blur', 'sharpen'):
                environment = dict(PROFILE_DISABLE_THP=str(int(config['disable_thp'])),
                                   MALLOC_ARENA_MAX=str(config['arenas']),
                                   MALLOC_TRIM_THRESHOLD_=str(config.get('trim_threshold', 4194304)),
                                   MIMALLOC_ARENA_RESERVE='64MiB', VIPS_DISC_THRESHOLD='1073741824')
                if config['allocator'] == 'glibc':
                    environment['MALLOC_MMAP_THRESHOLD_'] = str(config['mmap_threshold'])
                    if config['tcache_count'] != 'default':
                        environment['GLIBC_TUNABLES'] = 'glibc.malloc.tcache_count=' + config['tcache_count']
                else:
                    environment['MIMALLOC_PURGE_DELAY'] = str(config['purge_delay_ms'])
                argv = ['docker', 'run', '--rm', '--network', 'none', '--read-only', '--no-healthcheck',
                        '--cpuset-cpus', ','.join(map(str, cpus)), '--cpus', '2', '--memory', '1g',
                        '--memory-swap', '1g', '--pids-limit', '128', '--cap-drop', 'ALL',
                        '--security-opt', 'no-new-privileges', '--label', 'io.pixstride.benchmark.run=' + owner,
                        '--mount', 'type=bind,src=' + str(args.fixtures.resolve()) + ',dst=/data,readonly',
                        '--entrypoint', '/usr/local/bin/probe-' + config['allocator']]
                for key, value in environment.items():
                    argv.extend(['--env', key + '=' + value])
                argv.extend([image_id, '/data/landscape.jpg', operation])
                if config['collect']:
                    argv.append('collect')
                completed = subprocess.run(argv, check=True, capture_output=True, text=True, timeout=180)
                observation = json.loads(completed.stdout)
                metadata = [line.removeprefix('ALLOCATOR_METADATA ') for line in completed.stderr.splitlines()
                            if line.startswith('ALLOCATOR_METADATA ')]
                if len(metadata) != 1:
                    raise ValueError('Missing allocator/huge-page observation')
                observation.update(policy=config['id'], metadata=json.loads(metadata[0]))
                result['observations'].append(observation)
                args.output.write_text(json.dumps(result, indent=2, allow_nan=False) + '\n')
                print(config['id'], operation, observation['cpu_mean_ms'], observation['cgroup_peak_memory_bytes'], flush=True)
        validate(result['observations'])
        result['complete'] = True
        args.output.write_text(json.dumps(result, indent=2, allow_nan=False) + '\n')
    finally:
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
