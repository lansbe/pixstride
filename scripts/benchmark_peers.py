"""Measure an independent image server against frozen V2 cases on native Linux."""
import argparse
import contextlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import time
import uuid

from benchmark import host_metadata, isolate_client
from benchmark_fixtures import ROOT, sha256
from benchmark_metrics import cgroup_location, pixel_agreement, read_counters, summarize_batches
from benchmark_peer_baseline import LANES, lane
from benchmark_runtime import (Docker, MEMORY_BYTES, _free_port, architecture, cleanup_container,
                               counter_delta, request, require_native_image)
from benchmark_v2 import protocol
from benchmark_v2_catalog import verify_catalog
from benchmark_v2_collect import DeadlineExpired, deadline, validate_response
from benchmark_v2_fixtures import verify
from benchmark_v2_report import quality_failures

PEERS = ('thumbor', 'imaginary', 'weserv', 'imageflow', 'ipx')
SETTINGS = protocol('measure', '2.3')


def adapter(peer):
    spec = importlib.util.spec_from_file_location(peer, ROOT / 'benchmarks/peers' / peer / 'adapter.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@contextlib.contextmanager
def server(docker, image, fixtures, cpus):
    owner = uuid.uuid4().hex
    name = 'peer-' + owner
    port = _free_port()
    try:
        docker.run('run', '-d', '--name', name, '--label', 'io.pixstride.benchmark.run=' + owner,
                   '--read-only', '--cap-drop=ALL', '--security-opt=no-new-privileges', '--no-healthcheck',
                   '--memory=' + str(MEMORY_BYTES), '--memory-swap=' + str(MEMORY_BYTES),
                   '--cpus=2', '--cpuset-cpus=' + ','.join(map(str, cpus)), '--pids-limit=128',
                   '--tmpfs', '/tmp:rw,nosuid,nodev,mode=1777', '--tmpfs', '/run:rw,nosuid,nodev',
                   '-e', 'GOMAXPROCS=2', '-e', 'VIPS_CONCURRENCY=1', '-e', 'MALLOC_ARENA_MAX=2',
                   '-v', str(fixtures.resolve()) + ':/images:ro', '-p', f'127.0.0.1:{port}:8080', image)
        until = time.monotonic() + 45
        while True:
            info = docker.inspect(name)
            if not info['State']['Running'] or time.monotonic() > until:
                raise RuntimeError('Peer failed to start: ' + docker.run('logs', '--tail', '10', name))
            try:
                # Docker can accept TCP before the application listens.
                # Wait for an HTTP response (any status) without touching a corpus file.
                request(port, '/')
                break
            except (OSError, http.client.HTTPException):
                time.sleep(.1)
        group = cgroup_location(info['State']['Pid'], container_id=info['Id'])
        yield port, group
    finally:
        cleanup_container(docker, name, owner)


def trial(case, module, image_id, docker, args, cpus):
    row = {'id': case['id']}
    try:
        path = module.path(case)
    except NotImplementedError as error:
        return dict(row, status='unmapped', reason=str(error))
    if not isinstance(path, str) or not path.startswith('/') or ' ' in path:
        raise ValueError('Invalid peer path: ' + str(path))
    row.update(path=path, batches=[])
    reference = (args.pixels / str(lane(case)) / case['id']).read_bytes()
    row['reference_sha256'] = sha256(reference)
    # A broken installation is a job failure, never counted as an unsupported case.
    with server(docker, image_id, args.fixtures, cpus) as (port, group):
        try:
            with deadline(SETTINGS['case_deadline_seconds']):
                status, headers, body, _ = request(port, path)
                row['output'] = validate_response(case, status, headers, body)
                row['pixels_vs_imgproxy'] = pixel_agreement(body, reference, explicit_bmp_alpha=True)
                row['quality_failures'] = quality_failures(case, row)
                if row['quality_failures']:
                    return dict(row, status='quality')
                if args.smoke:
                    return dict(row, status='ready')
                digest = row['output']['sha256']
                for _ in range(SETTINGS['warmups']):
                    status, headers, body, _ = request(port, path)
                    validate_response(case, status, headers, body, digest)
                time.sleep(SETTINGS['cpu_tail_seconds'])
                for batch in range(SETTINGS['batches']):
                    before = read_counters(group)
                    latencies = []
                    for _ in range(SETTINGS['requests_per_batch']):
                        status, headers, body, elapsed = request(port, path)
                        validate_response(case, status, headers, body, digest)
                        latencies.append(elapsed)
                    time.sleep(SETTINGS['cpu_tail_seconds'])
                    row['batches'].append(dict(counter_delta(before, read_counters(group)),
                                               index=batch, latencies_ns=latencies))
                final = read_counters(group)
                if final['oom_kill']:
                    raise ValueError('OOM kill during trial')
                summary = summarize_batches(row['batches'])
                summary['cgroup_peak_memory_bytes'] = final['memory_peak_bytes']
                row.update(status='measured', summary=summary)
        except (ValueError, OSError, http.client.HTTPException, DeadlineExpired) as error:
            row.update(status='error', reason=str(error)[:500])
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--peer', choices=PEERS, required=True)
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--pixels', type=Path, required=True)
    parser.add_argument('--image', required=True)
    parser.add_argument('--lane', type=int, choices=range(LANES), default=0)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--smoke', action='store_true')
    args = parser.parse_args()
    manifest = verify(args.fixtures)
    cases = verify_catalog(manifest=manifest)
    if args.smoke:
        cases = [c for c in cases if c['family'] == 'fit' and c['scale'] == 1]
    else:
        cases = [c for c in cases if lane(c) == args.lane]
    docker = Docker()
    image = json.loads(docker.run('image', 'inspect', args.image))[0]
    require_native_image(image['Architecture'], image['Os'])
    module = adapter(args.peer)
    available = sorted(os.sched_getaffinity(0))
    cpus = available[:2]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with isolate_client(cpus) as client, args.output.open('w') as stream:
        metadata = dict(peer=args.peer, lane=args.lane, arch=architecture(), image=image['Id'],
                        catalog=manifest['catalog_sha256'], corpus=manifest['corpus_sha256'],
                        host=host_metadata(cpus, available, client),
                        commit=subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
                        run=os.getenv('GITHUB_RUN_ID'), attempt=os.getenv('GITHUB_RUN_ATTEMPT'),
                        warmups=SETTINGS['warmups'], batches=SETTINGS['batches'],
                        requests_per_batch=SETTINGS['requests_per_batch'])
        print(json.dumps(metadata), file=stream, flush=True)
        ready = 0
        for case in cases:
            row = trial(case, module, image['Id'], docker, args, cpus)
            ready += row['status'] == 'ready'
            print(case['id'], row['status'], row.get('reason', ''), flush=True)
            print(json.dumps(row, allow_nan=False), file=stream, flush=True)
        if args.smoke and not ready:
            raise RuntimeError('No starter workload produced a comparable image')


if __name__ == '__main__':
    main()
