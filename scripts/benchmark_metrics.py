"""Numerical definitions shared by the Linux runner and offline reports."""
import itertools
import math
import re
from pathlib import Path
import statistics


def percentile(values, probability):
    if not values or not 0 <= probability <= 1:
        raise ValueError('Percentiles require samples and a probability in [0, 1]')
    ordered = sorted(values)
    return ordered[max(0, math.ceil(probability * len(ordered)) - 1)]


def balanced_orders(engines, batches):
    if not engines or len(set(engines)) != len(engines) or batches < 1:
        raise ValueError('Use distinct engines and at least one batch')
    orders = list(itertools.permutations(engines))
    return [orders[i % len(orders)] for i in range(batches)]


def summarize_batches(batches):
    if not batches or any(not b['latencies_ns'] or b['cpu_usec'] < 0 for b in batches):
        raise ValueError('Each batch requires nonnegative CPU and timed requests')
    values = [v / 1_000_000 for b in batches for v in b['latencies_ns']]
    if any(not math.isfinite(v) or v < 0 for v in values):
        raise ValueError('Latency samples must be finite and nonnegative')
    cpu = [b['cpu_usec'] / len(b['latencies_ns']) / 1000 for b in batches]
    mean = statistics.mean(cpu)
    return {'requests': len(values), 'batches': len(batches),
            'cpu_ms_per_request': statistics.median(cpu),
            'cpu_batch_ms_per_request': cpu,
            'cpu_batch_cv': statistics.pstdev(cpu) / mean if mean else 0,
            'latency_p50_ms': percentile(values, .5),
            'latency_p95_ms': percentile(values, .95),
            'latency_min_ms': min(values), 'latency_max_ms': max(values)}


def histogram_errors(histogram):
    if len(histogram) % 256 or any(n < 0 for n in histogram) or not sum(histogram):
        raise ValueError('Expected nonempty 256-bin difference histograms')
    count = sum(histogram)
    mae = sum((i % 256) * n for i, n in enumerate(histogram)) / count
    mse = sum((i % 256) ** 2 * n for i, n in enumerate(histogram)) / count
    maximum = max(i % 256 for i, n in enumerate(histogram) if n)
    return {'mae': mae, 'rmse': math.sqrt(mse), 'max_error': maximum,
            'psnr_db': 10 * math.log10(255 ** 2 / mse) if mse else None,
            'exact': maximum == 0}


def _flat_counters(text):
    try:
        entries = [line.split() for line in text.splitlines() if line.strip()]
        result = {key: int(value) for key, value in entries}
        if len(result) != len(entries) or any(value < 0 for value in result.values()):
            raise ValueError()
        return result
    except (TypeError, ValueError) as error:
        raise ValueError('Invalid cgroup counter data') from error


def parse_counters(cpu_text, memory_peak_text, events_text):
    cpu, events = _flat_counters(cpu_text), _flat_counters(events_text)
    try:
        peak = int(memory_peak_text.strip())
        result = {'cpu_usec': cpu['usage_usec'], 'user_usec': cpu['user_usec'],
                  'system_usec': cpu['system_usec'], 'memory_peak_bytes': peak,
                  'nr_throttled': cpu.get('nr_throttled', 0),
                  'throttled_usec': cpu.get('throttled_usec', 0),
                  'oom_kill': events.get('oom_kill', 0)}
        if peak < 0:
            raise ValueError()
        return result
    except (KeyError, ValueError) as error:
        raise ValueError('Require cgroup-v2 CPU, peak-memory and event counters') from error


def cgroup_location(pid, proc_root=Path('/proc'), cgroup_root=Path('/sys/fs/cgroup'), container_id=None):
    if not isinstance(pid, int) or pid < 1:
        raise ValueError('A running container PID is required')
    lines = (Path(proc_root) / str(pid) / 'cgroup').read_text().splitlines()
    unified = [line[3:] for line in lines if line.startswith('0::')]
    if len(unified) != 1 or not unified[0].startswith('/') or '..' in Path(unified[0]).parts:
        raise ValueError('Benchmark requires a host-visible cgroup-v2 hierarchy')
    root = Path(cgroup_root).resolve()
    group = (root / unified[0].lstrip('/')).resolve()
    if not group.is_relative_to(root) or not group.is_dir():
        raise ValueError('Container cgroup is unavailable from this host')
    if container_id is not None:
        if not re.fullmatch(r'[0-9a-f]{64}', container_id):
            raise ValueError('Require the full inspected Docker container ID')
        identities = {container_id, 'docker-' + container_id, 'docker-' + container_id + '.scope'}
        if not identities.intersection(group.parts):
            raise ValueError('Docker PID belongs to a foreign/non-local container cgroup')
    return group


def read_counters(group):
    return parse_counters((group / 'cpu.stat').read_text(),
                          (group / 'memory.peak').read_text(),
                          (group / 'memory.events').read_text())


def pixel_agreement(actual, reference):
    """Decode outside timing; hidden RGB in transparent pixels is not an error."""
    from PIL import Image, ImageChops
    import io
    with Image.open(io.BytesIO(actual)) as a, Image.open(io.BytesIO(reference)) as b:
        if a.size != b.size:
            raise ValueError('Cannot compare images with different dimensions')
        a, b = a.convert('RGBA'), b.convert('RGBA')
    differences = {}
    for name, color in [('black', (0, 0, 0, 255)), ('white', (255, 255, 255, 255))]:
        left = Image.alpha_composite(Image.new('RGBA', a.size, color), a).convert('RGB')
        right = Image.alpha_composite(Image.new('RGBA', b.size, color), b).convert('RGB')
        differences[name] = histogram_errors(ImageChops.difference(left, right).histogram())
    differences['alpha'] = histogram_errors(ImageChops.difference(a.getchannel('A'), b.getchannel('A')).histogram())
    differences['exact_visible_pixels'] = all(v['exact'] for v in differences.values())
    return differences
