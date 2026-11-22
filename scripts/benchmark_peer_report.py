"""Join independent peer trials to the three archived Pixstride 0.5.0 measurements."""
import argparse
from collections import Counter
import csv
import json
import math
from pathlib import Path
import statistics

from benchmark_peer_baseline import LANES, baseline, lane
from benchmark_peers import PEERS, SETTINGS
from benchmark_metrics import summarize_batches
from benchmark_v2_catalog import verify_catalog
from benchmark_v2_report import quality_failures

METRICS = {'cpu': 'cpu_ms_per_request', 'p50': 'latency_p50_ms', 'p95': 'latency_p95_ms',
           'memory': 'cgroup_peak_memory_bytes', 'bytes': 'bytes'}
MODES = ('pixstride_default', 'pixstride_native')


def load_rows(paths):
    cases = {c['id']: c for c in verify_catalog()}
    archives = {arch: baseline(arch) for arch in ('amd64', 'arm64')}
    rows = {}
    images = {}
    for path in paths:
        with path.open() as stream:
            metadata = json.loads(next(stream))
            peer, arch = metadata['peer'], metadata['arch']
            result, samples = archives[arch]
            if metadata['corpus'] != result['corpus_sha256'] or metadata['catalog'] != result['catalog_sha256']:
                raise ValueError('Input and baseline hashes differ')
            if peer not in PEERS or metadata['host']['architecture'] != arch:
                raise ValueError('Unknown peer or host architecture mismatch')
            image = images.setdefault((peer, arch), metadata['image'])
            if image != metadata['image']:
                raise ValueError('The peer image changed across lanes')
            if any(metadata[k] != SETTINGS[k] for k in ('batches', 'requests_per_batch', 'warmups')):
                raise ValueError('Sampling protocol differs')
            for line in stream:
                entry = json.loads(line)
                case = cases[entry['id']]
                key = (peer, arch, case['id'])
                if key in rows or lane(case) != metadata['lane']:
                    raise ValueError('Duplicate or misplaced case ' + str(key))
                runs = samples[case['id']]
                if entry['status'] != 'unmapped' and entry['reference_sha256'] not in {r['reference'] for r in runs}:
                    raise ValueError('Reference pixels differ from history')
                identical_model = [r for r in runs if r['cpu_model'] == metadata['host']['cpu_model']]
                row = dict(peer=peer, arch=arch, id=case['id'], rank=case['rank'], family=case['family'],
                           source=case['input_kind'], scale=case['scale'], output=case['format'],
                           status=entry['status'], reason=entry.get('reason') or ', '.join(entry.get('quality_failures', [])),
                           same_cpu_runs=len(identical_model), hardware=' / '.join(metadata['host']['cpu_model']),
                           image_id=image, workflow_run=metadata['run'])
                if entry['status'] not in ('measured', 'unmapped', 'error', 'quality'):
                    raise ValueError('Unknown collection state')
                if entry['status'] in ('quality', 'measured'):
                    pixels = entry['pixels_vs_imgproxy']
                    row['exact_pixels_vs_imgproxy'] = pixels['exact_visible_pixels']
                    for metric in ('mae', 'rmse'):
                        row['composite_' + metric] = max(pixels[bg][metric] for bg in ('black', 'white'))
                        row['alpha_' + metric] = pixels['alpha'][metric]
                    failed = quality_failures(case, entry)
                    if failed != entry['quality_failures'] or bool(failed) != (entry['status'] == 'quality'):
                        raise ValueError('Inconsistent pixel comparison')
                if entry['status'] == 'measured':
                    batches = entry['batches']
                    if (len(batches) != SETTINGS['batches'] or
                        any(len(b['latencies_ns']) != SETTINGS['requests_per_batch'] for b in batches)):
                        raise ValueError('Incomplete samples')
                    actual = summarize_batches(batches)
                    if any(entry['summary'][k] != v for k, v in actual.items()):
                        raise ValueError('Summary differs from raw samples')
                    values = dict(entry['summary'], bytes=entry['output']['bytes'])
                    if any(not math.isfinite(values[m]) or values[m] <= 0 for m in METRICS.values()):
                        raise ValueError('Invalid peer metrics')
                    for mode, label in zip(MODES, ('default', 'native')):
                        row[label + '_identical_bytes_runs'] = sum(
                            r[mode]['output_sha256'] == entry['output']['sha256'] for r in runs)
                    compare = identical_model or runs
                    for short, metric in METRICS.items():
                        row['peer_' + short] = values[metric]
                        for mode, prefix in zip(MODES, ('default_', 'native_')):
                            values_old = [r[mode][metric] for r in compare]
                            row[prefix + short] = statistics.median(values_old)
                            row[prefix + short + '_ratio'] = row[prefix + short] / values[metric]
                            row[prefix + short + '_min_ratio'] = min(r[mode][metric] for r in runs) / values[metric]
                    row['throttled_usec'] = sum(b['throttled_usec'] for b in batches)
                rows[key] = row
    needed = {(p, a, i) for p in PEERS for a in archives for i in cases}
    if rows.keys() != needed:
        raise ValueError(f'Coverage incomplete: {len(rows)} of {len(needed)}')
    return list(rows.values())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports', type=Path, nargs='+')
    parser.add_argument('--csv', type=Path, required=True)
    args = parser.parse_args()
    rows = sorted(load_rows(args.reports), key=lambda r: (r['peer'], r['arch'], r['rank']))
    columns = list(dict.fromkeys(k for row in rows for k in row))
    with args.csv.open('w') as stream:
        writer = csv.DictWriter(stream, columns, lineterminator='\n')
        writer.writeheader()
        writer.writerows(rows)
    print('Rows by status:', Counter(r['status'] for r in rows))


if __name__ == '__main__':
    main()
