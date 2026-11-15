#!/usr/bin/env python3
"""Run 25 HTTP cases against immutable images on native Linux with cgroup v2."""
import argparse
import contextlib
import datetime as dt
import json
import os
from pathlib import Path
import platform
import random
import subprocess
import sys
import time
import uuid

from benchmark_fixtures import ROOT, load_cases, sha256, verify_fixtures
from benchmark_metrics import balanced_orders, pixel_agreement, read_counters, summarize_batches
from benchmark_runtime import (Docker, ENGINES, MEMORY_BYTES, Server, architecture,
                               counter_delta, request, signed_path, validate_response)


def write_report(path, report):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + '.tmp')
    temporary.write_text(json.dumps(report, indent=2, allow_nan=False) + '\n')
    temporary.replace(path)


def host_metadata(cpus):
    cpuinfo = Path('/proc/cpuinfo').read_text()
    model = sorted({line.split(':', 1)[1].strip() for line in cpuinfo.splitlines()
                    if line.startswith(('model name', 'Hardware', 'CPU part')) and ':' in line})
    release = {}
    for line in Path('/etc/os-release').read_text().splitlines():
        if '=' in line:
            key, value = line.split('=', 1)
            if key in {'ID', 'VERSION_ID', 'PRETTY_NAME'}:
                release[key] = value.strip('"')
    return {'architecture': architecture(), 'cpu_model': model, 'logical_cpus': os.cpu_count(),
            'available_cpus': len(os.sched_getaffinity(0)), 'selected_cpus': cpus,
            'kernel': platform.release(), 'os_release': release,
            'python': platform.python_version(), 'docker': None,
            'runner_label': os.environ.get('BENCHMARK_RUNNER', 'unspecified native Linux host')}


@contextlib.contextmanager
def preserve_case_failure(result):
    try:
        yield
    except BaseException as error:
        result['status'] = 'failed'
        result['failure'] = {**result.get('active', {}), 'type': type(error).__name__, 'message': str(error)[:4000]}
        raise


def case_measurement(case, docker, images, fixtures, cpus, owner, index, protocol, progress=None):
    result = progress if progress is not None else {}
    result.update({'id': case['id'], 'label': case['label'], 'source': case['source'],
              'options': case['options'], 'format': case['format'], 'dimensions': case['dimensions'],
              'status': 'running', 'engines': {}})
    path = signed_path(case)
    bodies = {}
    with preserve_case_failure(result), contextlib.ExitStack() as stack:
        servers = {}
        for engine in ENGINES:
            result['active'] = {'phase': 'preflight', 'engine': engine, 'batch': None}
            image = images['imgproxy' if engine == 'imgproxy' else 'pixstride']['reference']
            servers[engine] = stack.enter_context(Server(docker, engine, image, fixtures, cpus, owner, index))
            status, headers, body, _ = request(servers[engine].port, path)
            metadata = validate_response(case, status, headers, body)
            result['engines'][engine] = {'output': metadata, 'batches': []}
            bodies[engine] = body
            result['active'] = {'phase': 'warmup', 'engine': engine, 'batch': None}
            for _ in range(protocol['warmups']):
                status, headers, body, _ = request(servers[engine].port, path)
                validate_response(case, status, headers, body, metadata['sha256'])
        time.sleep(protocol['cpu_tail_seconds'])
        for batch, order in enumerate(balanced_orders(ENGINES, protocol['batches'])):
            for position, engine in enumerate(order):
                result['active'] = {'phase': 'measure', 'engine': engine, 'batch': batch}
                server, entry = servers[engine], result['engines'][engine]
                before = read_counters(server.group)
                latencies = []
                started = time.perf_counter_ns()
                for _ in range(protocol['requests_per_batch']):
                    status, headers, body, elapsed = request(server.port, path)
                    validate_response(case, status, headers, body, entry['output']['sha256'])
                    latencies.append(elapsed)
                # Include deferred worker/GC work in server CPU, outside HTTP latency.
                time.sleep(protocol['cpu_tail_seconds'])
                after = read_counters(server.group)
                counters = counter_delta(before, after)
                counters.update(index=batch, position=position, latencies_ns=latencies,
                                wall_elapsed_ns=time.perf_counter_ns() - started)
                entry['batches'].append(counters)
        for engine, server in servers.items():
            result['active'] = {'phase': 'accounting', 'engine': engine, 'batch': None}
            entry = result['engines'][engine]
            counters = read_counters(server.group)
            if counters['oom_kill']:
                raise ValueError('OOM invalidates this case')
            entry['summary'] = summarize_batches(entry['batches'])
            entry['summary']['cgroup_peak_memory_bytes'] = counters['memory_peak_bytes']
            entry['summary']['throttled_usec'] = sum(b['throttled_usec'] for b in entry['batches'])
            entry['summary']['nr_throttled'] = sum(b['nr_throttled'] for b in entry['batches'])
            # Capture context: peak includes anonymous, file-cache and kernel charges.
            memory = dict(line.split() for line in (server.group / 'memory.stat').read_text().splitlines())
            entry['memory_at_end'] = {key: int(memory.get(key, 0)) for key in ['anon', 'file', 'kernel']}
        for engine in ENGINES[:-1]:
            result['active'] = {'phase': 'pixels', 'engine': engine, 'batch': None}
            result['engines'][engine]['pixels_vs_imgproxy'] = pixel_agreement(bodies[engine], bodies['imgproxy'])
        result['active'] = {'phase': 'cleanup', 'engine': None, 'batch': None}
    result.pop('active', None)
    result['status'] = 'success'
    return result


def run(args):
    if sys.platform != 'linux':
        raise ValueError('Run this benchmark on a native Linux host; macOS timings are not Linux results')
    if args.architecture and args.architecture != architecture():
        raise ValueError('Runner architecture does not match the requested architecture')
    cpus = sorted(os.sched_getaffinity(0))[:2]
    if len(cpus) != 2:
        raise ValueError('The benchmark requires two available logical CPUs')
    if not 1 <= args.batches <= 60 or not 1 <= args.requests <= 1000 or not 0 <= args.warmups <= 100:
        raise ValueError('Benchmark counts exceed the bounded supported ranges')
    manifest = verify_fixtures(args.fixtures)
    cases = load_cases(args.cases, manifest['files'])
    if args.case:
        selected = set(args.case)
        if not selected.issubset({c['id'] for c in cases}):
            raise ValueError('Unknown benchmark case')
        cases = [c for c in cases if c['id'] in selected]
    if args.preflight:
        args.batches, args.requests, args.warmups = 1, 1, 0
    full = len(cases) == 25 and args.batches >= 6 and args.batches % 6 == 0 and args.requests >= 24 and args.warmups >= 5
    protocol = {'batches': args.batches, 'requests_per_batch': args.requests, 'warmups': args.warmups,
                'concurrency': 1, 'cpus': 2, 'memory_bytes': MEMORY_BYTES, 'pids_limit': 128,
                'cpu_tail_seconds': .05, 'case_order_seed': 0x504958,
                'engine_schedule': 'all six permutations, repeated', 'http_connection': 'fresh per request',
                'source_cache': 'warm OS page cache, no application result cache',
                'cpu_scope': 'container-ID-verified local cgroup including descendants; client excluded',
                'memory_scope': 'fresh per-case container lifetime peak, including startup, cache and kernel; not RSS'}
    versions = json.loads(args.versions.read_text())
    docker = Docker(args.docker)
    owner = uuid.uuid4().hex[:12]
    images = {}
    if args.candidate_revision and not args.pixstride_image:
        raise ValueError('Candidate revision requires an immutable local Pixstride image ID')
    for product, override in [('pixstride', args.pixstride_image), ('imgproxy', args.imgproxy_image)]:
        images[product] = docker.image(override or versions[product]['image'],
                                       candidate_revision=args.candidate_revision if product == 'pixstride' else None)
    # Probe metadata in separate containers so CLI startup cannot inflate measured peaks.
    images['pixstride']['capabilities'] = json.loads(docker.probe(images['pixstride']['reference'], ['capabilities'], owner, 'pixstride'))
    images['imgproxy']['version'] = docker.probe(images['imgproxy']['reference'], ['imgproxy', 'version'], owner, 'imgproxy')
    if not args.pixstride_image and images['pixstride']['capabilities'].get('version') != versions['pixstride']['version']:
        raise ValueError('Pixstride image version differs from the pinned version')
    if not args.imgproxy_image and versions['imgproxy']['version'] not in images['imgproxy']['version']:
        raise ValueError('imgproxy image version differs from the pinned version')
    revision = subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip()
    dirty = subprocess.run(['git', '-C', str(ROOT), 'diff', '--quiet', 'HEAD']).returncode != 0
    report = {'schema_version': 1, 'status': 'running', 'profile': 'full' if full else 'preflight-or-partial',
              'started_at': dt.datetime.now(dt.timezone.utc).isoformat(),
              'harness_revision': revision, 'harness_dirty': dirty,
              'workflow_url': None, 'host': host_metadata(cpus), 'images': images,
              'protocol': protocol, 'fixtures': manifest, 'cases_sha256': sha256(args.cases.read_bytes()),
              'case_order': [], 'cases': [],
              'limitations': ['Shared-host observations, not universal guarantees or an ISA comparison.',
                              'Packaged dependencies and output pixels/quality can differ.',
                              'Peak cgroup memory includes file cache/kernel and is not process RSS.',
                              'One active request, warm cache, local sources; no network-origin or high-concurrency claim.']}
    if os.environ.get('GITHUB_ACTIONS') == 'true':
        report['workflow_url'] = os.environ['GITHUB_SERVER_URL'] + '/' + os.environ['GITHUB_REPOSITORY'] + '/actions/runs/' + os.environ['GITHUB_RUN_ID']
    report['host']['docker'] = json.loads(docker.run('version', '--format', '{{json .Server}}'))['Version']
    random.Random(protocol['case_order_seed']).shuffle(cases)
    report['case_order'] = [case['id'] for case in cases]
    write_report(args.output, report)
    try:
        for index, case in enumerate(cases):
            print(f'{index + 1}/{len(cases)} {case["id"]}', flush=True)
            result = {'id': case['id'], 'status': 'running', 'engines': {}}
            report['cases'].append(result)
            write_report(args.output, report)
            case_measurement(case, docker, images, args.fixtures, cpus, owner, index, protocol, progress=result)
            write_report(args.output, report)
        report['status'] = 'success'
    except BaseException as error:
        report['status'] = 'failed'
        report['failure'] = {'case': case['id'], **result.get('failure', {}),
                             'type': type(error).__name__, 'message': str(error)[:4000]}
        raise
    finally:
        report['finished_at'] = dt.datetime.now(dt.timezone.utc).isoformat()
        write_report(args.output, report)
    print(f'{len(cases)} cases completed, {args.batches * args.requests} measured requests per case/engine; profile={report["profile"]}', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--cases', type=Path, default=ROOT / 'benchmarks/cases.json')
    parser.add_argument('--versions', type=Path, default=ROOT / 'benchmarks/versions.json')
    parser.add_argument('--docker', default='docker')
    parser.add_argument('--architecture', choices=['amd64', 'arm64'])
    parser.add_argument('--pixstride-image')
    parser.add_argument('--candidate-revision', help='Full clean checkout SHA for a locally built immutable candidate image')
    parser.add_argument('--imgproxy-image')
    parser.add_argument('--batches', type=int, default=6)
    parser.add_argument('--requests', type=int, default=24)
    parser.add_argument('--warmups', type=int, default=5)
    parser.add_argument('--case', action='append')
    parser.add_argument('--preflight', action='store_true')
    args = parser.parse_args()
    try:
        run(args)
    except (ValueError, OSError, RuntimeError, subprocess.SubprocessError) as error:
        parser.exit(1, 'Benchmark failed: ' + str(error) + '\n')


if __name__ == '__main__':
    main()
