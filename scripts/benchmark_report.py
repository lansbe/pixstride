#!/usr/bin/env python3
"""Validate raw benchmark evidence and generate reproducible tables/archives."""
import argparse
import copy
import csv
import gzip
import json
import math
from pathlib import Path
import statistics

from benchmark_fixtures import ROOT, corpus_hash, sha256
from benchmark_metrics import balanced_orders, percentile, summarize_batches
from benchmark_runtime import ENGINES, MEMORY_BYTES, require_native_image, validate_candidate_image

LABELS = {'pixstride_default': 'Pixstride default', 'pixstride_native': 'Pixstride native', 'imgproxy': 'imgproxy'}


def validate_client_affinity(report):
    if report['protocol'].get('client_affinity') != 'disjoint from service cpuset':
        raise ValueError('New qualification requires separate client CPU affinity')
    host = report['host']
    service, client = host.get('selected_cpus'), host.get('client_cpus')
    valid = lambda cpus: (isinstance(cpus, list) and bool(cpus)
                          and all(type(c) is int and c >= 0 for c in cpus)
                          and len(set(cpus)) == len(cpus))
    if (not valid(service) or not valid(client) or len(service) != 2
            or set(service) & set(client) or type(host.get('available_cpus')) is not int
            or host['available_cpus'] != len(service) + len(client)):
        raise ValueError('Invalid or overlapping service/client CPU affinity evidence')


def validate_report(report, *, require_client_isolation=False):
    if (report.get('schema_version') != 1 or report.get('status') != 'success'
            or report.get('profile') != 'full' or report.get('harness_dirty') is not False):
        raise ValueError('Only complete full-protocol reports from committed harnesses can be published')
    cases, protocol = report.get('cases', []), report['protocol']
    if len(cases) != 25 or len({c['id'] for c in cases}) != 25:
        raise ValueError('A published report must contain 25 distinct completed cases')
    if (protocol['batches'] < 6 or protocol['batches'] % 6 or protocol['requests_per_batch'] < 24
            or protocol['warmups'] < 5 or protocol['concurrency'] != 1
            or protocol['cpus'] != 2 or protocol['memory_bytes'] != MEMORY_BYTES):
        raise ValueError('Report does not meet the qualified sampling/resource protocol')
    if report['fixtures']['corpus_sha256'] != corpus_hash(report['fixtures']):
        raise ValueError('Corpus manifest is inconsistent')
    if report['host']['architecture'] not in {'amd64', 'arm64'}:
        raise ValueError('Unsupported report architecture')
    if require_client_isolation or 'client_affinity' in protocol:
        validate_client_affinity(report)
    for product in ['pixstride', 'imgproxy']:
        image = report.get('images', {}).get(product, {})
        if not {'architecture', 'os'}.issubset(image):
            raise ValueError('Reports require inspected image architecture and operating system')
        require_native_image(image['architecture'], image['os'], report['host']['architecture'])
        if image.get('candidate') or image.get('reference', '').startswith('sha256:'):
            if product != 'pixstride':
                raise ValueError('The imgproxy reference must remain a published digest-pinned image')
            validate_candidate_image(image, report['harness_revision'])
    orders = balanced_orders(ENGINES, protocol['batches'])
    for case in cases:
        if case.get('status') != 'success' or set(case.get('engines', {})) != set(ENGINES):
            raise ValueError('Missing/failed engine output cannot become a performance result')
        for engine, entry in case['engines'].items():
            batches = entry['batches']
            if len(batches) != protocol['batches']:
                raise ValueError('Missing measurement batches')
            for i, batch in enumerate(batches):
                if (len(batch['latencies_ns']) != protocol['requests_per_batch']
                        or batch['index'] != i or batch['position'] != orders[i].index(engine)):
                    raise ValueError('Incomplete samples or unbalanced engine ordering')
            calculated = summarize_batches(batches)
            for key, value in calculated.items():
                actual = entry['summary'][key]
                expected_values, actual_values = (value, actual) if isinstance(value, list) else ([value], [actual])
                if len(expected_values) != len(actual_values) or any(
                        not math.isclose(a, b, rel_tol=1e-12, abs_tol=1e-12)
                        for a, b in zip(expected_values, actual_values)):
                    raise ValueError('Derived statistics disagree with raw samples: ' + key)
            if entry['summary']['cpu_ms_per_request'] <= 0 or entry['summary']['cgroup_peak_memory_bytes'] <= 0:
                raise ValueError('Invalid CPU or memory accounting')
            if engine != 'imgproxy' and 'exact_visible_pixels' not in entry['pixels_vs_imgproxy']:
                raise ValueError('Performance results require decoded-pixel comparison')
    return report


def validate_pair(reports):
    if len(reports) != 2 or {r['host']['architecture'] for r in reports} != {'amd64', 'arm64'}:
        raise ValueError('Publishing requires one native AMD64 and one native ARM64 report')
    for report in reports:
        validate_report(report)
    left, right = reports
    for key in ['protocol', 'cases_sha256', 'harness_revision']:
        if left[key] != right[key]:
            raise ValueError('Reports use different ' + key)
    if left['fixtures']['files'] != right['fixtures']['files']:
        raise ValueError('Reports do not use identical input bytes')
    if {c['id'] for c in left['cases']} != {c['id'] for c in right['cases']}:
        raise ValueError('Reports do not contain the same cases')
    for product in ['pixstride', 'imgproxy']:
        if left['images'][product].get('candidate') or right['images'][product].get('candidate'):
            if left['images'][product].get('candidate') != right['images'][product].get('candidate'):
                raise ValueError('Candidate images use different source revisions/build inputs')
            continue
        if left['images'][product]['reference'] != right['images'][product]['reference']:
            raise ValueError('Reports use different image versions')


def overview(report, engine):
    pairs = [(c['engines'][engine], c['engines']['imgproxy']) for c in report['cases']]
    ratios = lambda key: [a['summary'][key] / b['summary'][key] for a, b in pairs]
    cpu, latency, memory = (ratios(key) for key in ['cpu_ms_per_request', 'latency_p50_ms', 'cgroup_peak_memory_bytes'])
    return {'lower_cpu_cases': sum(v < 1 for v in cpu), 'lower_p50_cases': sum(v < 1 for v in latency),
            'lower_memory_cases': sum(v < 1 for v in memory),
            'median_cpu_ratio': statistics.median(cpu), 'median_p50_ratio': statistics.median(latency),
            'median_memory_ratio': statistics.median(memory),
            'exact_pixel_cases': sum(a['pixels_vs_imgproxy']['exact_visible_pixels'] for a, _ in pairs),
            'noisy_cpu_cases': sum(max(a['summary']['cpu_batch_cv'], b['summary']['cpu_batch_cv']) > .10 for a, b in pairs)}


def render_overview(reports):
    protocol = reports[0]['protocol']
    images = reports[0]['images']
    candidate = images['pixstride'].get('candidate')
    identity = (f'Unreleased candidate `{candidate["revision"]}` (package version **{images["pixstride"]["capabilities"]["version"]}**)'
                if candidate else f'Images: Pixstride **{images["pixstride"]["capabilities"]["version"]}**')
    text = (f'{identity} / imgproxy **{images["imgproxy"]["version"]}**. '
            f'Per case/mode: {protocol["batches"]} balanced batches × {protocol["requests_per_batch"]} timed requests '
            f'({protocol["batches"] * protocol["requests_per_batch"]} total), after {protocol["warmups"]} warmups.\n\n')
    text += '| Linux | Pixstride mode | Lower CPU | Lower HTTP p50 | Median CPU ratio | Median p50 ratio | Median peak cgroup ratio | Exact visible pixels |\n'
    text += '|---|---|---:|---:|---:|---:|---:|---:|\n'
    for report in sorted(reports, key=lambda r: r['host']['architecture']):
        for engine in ENGINES[:-1]:
            row = overview(report, engine)
            text += (f'| {report["host"]["architecture"]} | {LABELS[engine]} | {row["lower_cpu_cases"]}/25 | '
                     f'{row["lower_p50_cases"]}/25 | {row["median_cpu_ratio"]:.3f}× | '
                     f'{row["median_p50_ratio"]:.3f}× | {row["median_memory_ratio"]:.3f}× | {row["exact_pixel_cases"]}/25 |\n')
    return text


def render_tables(reports):
    text = ''
    for report in sorted(reports, key=lambda r: r['host']['architecture']):
        host = report['host']
        cpu = ', '.join(host['cpu_model']).replace('|', '\\|')
        text += f'### Linux {host["architecture"]}\n\n'
        text += f'CPU identifiers: {cpu}; {host["logical_cpus"]} logical CPUs, two selected; {host["os_release"]["PRETTY_NAME"]}, kernel {host["kernel"]}. '
        text += (f'[Measurement run]({report["workflow_url"]}).\n\n'
                 if report.get('workflow_url') else 'Local measurement run.\n\n')
        protocol = report['protocol']
        text += f'{protocol["batches"]} batches × {protocol["requests_per_batch"]} timed requests per case/mode, after {protocol["warmups"]} warmups.\n\n'
        text += f'Harness `{report["harness_revision"]}`; corpus SHA-256 `{report["fixtures"]["corpus_sha256"]}`.\n\n'
        candidate = report['images']['pixstride'].get('candidate')
        if candidate:
            text += (f'Unreleased candidate source `{candidate["revision"]}`; '
                     f'immutable local image `{report["images"]["pixstride"]["image_id"]}`.\n\n')
        text += 'Each triple is **Pixstride default / Pixstride native / imgproxy**. Pixel MAE is **default / native**, taking the larger black/white composite error against imgproxy. Memory is the fresh container lifetime cgroup peak, including startup, warmups, file cache and kernel memory.\n\n'
        text += '| Case | CPU ms/request | HTTP p50 ms | HTTP p95 ms | Peak cgroup MiB | Output KiB | Pixel MAE |\n'
        text += '|---|---:|---:|---:|---:|---:|---:|\n'
        for case in sorted(report['cases'], key=lambda c: c['id']):
            def triple(key, divisor=1):
                return ' / '.join(f'{case["engines"][e]["summary"][key] / divisor:.2f}' for e in ENGINES)
            sizes = ' / '.join(f'{case["engines"][e]["output"]["bytes"] / 1024:.2f}' for e in ENGINES)
            errors = ' / '.join(f'{max(case["engines"][e]["pixels_vs_imgproxy"][b]["mae"] for b in ["black", "white"]):.3f}' for e in ENGINES[:-1])
            text += (f'| {case["id"]}: {case["label"]} | {triple("cpu_ms_per_request")} | '
                     f'{triple("latency_p50_ms")} | {triple("latency_p95_ms")} | '
                     f'{triple("cgroup_peak_memory_bytes", 1024 ** 2)} | {sizes} | {errors} |\n')
        text += '\n'
        for engine in ENGINES[:-1]:
            row = overview(report, engine)
            text += f'{LABELS[engine]}: {row["noisy_cpu_cases"]}/25 CPU comparisons have batch coefficient of variation above 10% in at least one engine. '
        text += 'Treat small timing differences as noise; differing pixels/encoding prevent an equal-quality speed claim.\n\n'
    return text.rstrip() + '\n'


def csv_rows(reports):
    for report in reports:
        for case in sorted(report['cases'], key=lambda c: c['id']):
            for engine in ENGINES:
                entry = case['engines'][engine]
                yield {'architecture': report['host']['architecture'], 'case': case['id'], 'engine': engine,
                       'cpu_ms_per_request': entry['summary']['cpu_ms_per_request'],
                       'http_p50_ms': entry['summary']['latency_p50_ms'], 'http_p95_ms': entry['summary']['latency_p95_ms'],
                       'cgroup_peak_bytes': entry['summary']['cgroup_peak_memory_bytes'], 'output_bytes': entry['output']['bytes']}


def load(path):
    raw = Path(path).read_bytes()
    if str(path).endswith('.gz'):
        raw = gzip.decompress(raw)
    return validate_report(json.loads(raw)), raw


def summary_document(report, raw, compressed):
    stem = 'linux-' + report['host']['architecture']
    summary = copy.deepcopy(report)
    summary['kind'] = 'summary'
    summary['raw_evidence'] = {'file': stem + '.raw.json.gz', 'sha256': sha256(compressed), 'json_sha256': sha256(raw)}
    for case in summary['cases']:
        for entry in case['engines'].values():
            for batch in entry['batches']:
                values = batch.pop('latencies_ns')
                batch.update(requests=len(values), latency_p50_ns=percentile(values, .5), latency_p95_ns=percentile(values, .95))
    return summary


def publish_results(loaded, directory):
    validate_pair([report for report, _ in loaded])
    directory.mkdir(parents=True, exist_ok=True)
    for report, raw in loaded:
        stem = 'linux-' + report['host']['architecture']
        compressed = gzip.compress(raw, mtime=0)
        (directory / (stem + '.raw.json.gz')).write_bytes(compressed)
        summary = summary_document(report, raw, compressed)
        (directory / (stem + '.json')).write_text(json.dumps(summary, indent=2, allow_nan=False) + '\n')


def check_results_directory(directory):
    reports = []
    for arch in ['amd64', 'arm64']:
        stem = 'linux-' + arch
        compressed = (directory / (stem + '.raw.json.gz')).read_bytes()
        summary = json.loads((directory / (stem + '.json')).read_text())
        if summary.get('raw_evidence', {}).get('sha256') != sha256(compressed):
            raise ValueError('Raw evidence archive digest mismatch: ' + stem)
        raw = gzip.decompress(compressed)
        report = validate_report(json.loads(raw))
        if summary != summary_document(report, raw, compressed):
            raise ValueError('Published summary differs from raw measurements: ' + stem)
        reports.append(report)
    validate_pair(reports)


def replace_block(path, name, content, check=False):
    text = path.read_text()
    start, end = '<!-- benchmark:' + name + ':start -->', '<!-- benchmark:' + name + ':end -->'
    if text.count(start) != 1 or text.count(end) != 1:
        raise ValueError('Missing or duplicate benchmark marker: ' + str(path))
    before, remainder = text.split(start, 1)
    _, after = remainder.split(end, 1)
    expected = before + start + '\n' + content.rstrip() + '\n' + end + after
    if check and text != expected:
        raise ValueError('Stale generated benchmark table: ' + str(path))
    if not check:
        path.write_text(expected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reports', nargs='+', type=Path,
                        default=[ROOT / f'benchmarks/results/linux-{arch}.raw.json.gz' for arch in ['amd64', 'arm64']])
    parser.add_argument('--markdown', type=Path)
    parser.add_argument('--csv', type=Path)
    parser.add_argument('--publish-results', type=Path)
    parser.add_argument('--update-docs', action='store_true')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    loaded = [load(path) for path in args.reports]
    reports = [report for report, _ in loaded]
    if len(reports) > 1:
        validate_pair(reports)
    if args.publish_results:
        publish_results(loaded, args.publish_results)
    if args.markdown:
        args.markdown.write_text(render_overview(reports) + '\n' + render_tables(reports))
    if args.csv:
        rows = list(csv_rows(reports))
        with args.csv.open('w', newline='') as output:
            writer = csv.DictWriter(output, fieldnames=list(rows[0]))
            writer.writeheader()
            writer.writerows(rows)
    if args.update_docs or args.check:
        validate_pair(reports)
        if args.check:
            check_results_directory(ROOT / 'benchmarks/results')
        replace_block(ROOT / 'README.md', 'summary', render_overview(reports), args.check)
        replace_block(ROOT / 'BENCHMARKS.md', 'tables', render_tables(reports), args.check)
    print('Validated ' + str(len(reports)) + ' complete native Linux reports')


if __name__ == '__main__':
    main()
