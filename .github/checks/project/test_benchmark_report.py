"""Publishing must reject partial, incompatible or inconsistent measurements."""
import copy
import gzip
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import subprocess
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from benchmark_report import validate_pair, validate_report, overview
from benchmark_fixtures import corpus_hash
from benchmark_metrics import balanced_orders, summarize_batches
from benchmark_runtime import ENGINES, MEMORY_BYTES


def measured_report(architecture='amd64'):
    manifest = {'files': {'fixture.png': {'sha256': 'a' * 64, 'bytes': 3, 'dimensions': [1, 1]}}}
    manifest['corpus_sha256'] = corpus_hash(manifest)
    report = {'schema_version': 1, 'status': 'success', 'profile': 'full', 'harness_dirty': False,
              'harness_revision': 'b' * 40, 'cases_sha256': 'c' * 64, 'fixtures': manifest,
              'host': {'architecture': architecture},
              'protocol': {'batches': 6, 'requests_per_batch': 24, 'warmups': 5,
                           'concurrency': 1, 'cpus': 2, 'memory_bytes': MEMORY_BYTES},
              'images': {name: {'reference': name + '@sha256:' + 'd' * 64, 'architecture': architecture, 'os': 'linux'} for name in ['pixstride', 'imgproxy']},
              'cases': []}
    orders = balanced_orders(ENGINES, 6)
    for index in range(25):
        case = {'id': f'case-{index}', 'status': 'success', 'engines': {}}
        for engine in ENGINES:
            batches = [{'cpu_usec': 24000 if engine == 'imgproxy' else 12000,
                        'latencies_ns': [1_000_000, 2_000_000] * 12,
                        'index': i, 'position': order.index(engine)} for i, order in enumerate(orders)]
            case['engines'][engine] = {'batches': batches,
                'output': {'bytes': 10, 'sha256': 'a' * 64, 'mime': 'image/png', 'dimensions': [1, 1]},
                'summary': {**summarize_batches(batches), 'cgroup_peak_memory_bytes': 1048576},
                'pixels_vs_imgproxy': {'exact_visible_pixels': True}}
        report['cases'].append(case)
    return report


class BenchmarkReportTests(unittest.TestCase):
    def test_candidate_pair_uses_same_clean_source_but_arch_specific_immutable_image_ids(self):
        reports = [measured_report(), measured_report('arm64')]
        source = dict(revision='b' * 40, tree='e' * 40, dockerfile_sha256='f' * 64, cargo_lock_sha256='1' * 64)
        for index, report in enumerate(reports):
            image = report['images']['pixstride']
            image.update(reference='sha256:' + str(index + 2) * 64,
                         image_id='sha256:' + str(index + 2) * 64,
                         candidate=copy.deepcopy(source),
                         labels={'org.opencontainers.image.revision': 'b' * 40})
        validate_pair(reports)
        from benchmark_report import render_overview
        reports[0]['images']['pixstride']['capabilities'] = {'version': '0.3.0'}
        reports[0]['images']['imgproxy']['version'] = '4.0.24'
        rendered = render_overview([reports[0]])
        self.assertIn('Unreleased candidate', rendered)
        self.assertIn('b' * 40, rendered)
        for field in ['revision', 'tree', 'dockerfile_sha256', 'cargo_lock_sha256']:
            changed = copy.deepcopy(reports)
            changed[1]['images']['pixstride']['candidate'][field] = '9' * len(source[field])
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate_pair(changed)

    def test_candidate_report_rejects_missing_or_mismatched_source_and_image_provenance(self):
        original = measured_report()
        image = original['images']['pixstride']
        image.update(reference='sha256:' + '2' * 64, image_id='sha256:' + '2' * 64,
                     candidate=dict(revision='b' * 40, tree='e' * 40, dockerfile_sha256='f' * 64, cargo_lock_sha256='1' * 64),
                     labels={'org.opencontainers.image.revision': 'b' * 40})
        validate_report(original)
        for change in ['mutable', 'image_id', 'revision', 'label', 'missing_source']:
            report = copy.deepcopy(original)
            image = report['images']['pixstride']
            if change == 'mutable': image['reference'] = 'pixstride:candidate'
            elif change == 'image_id': image['image_id'] = 'sha256:' + '3' * 64
            elif change == 'revision': image['candidate']['revision'] = 'a' * 40
            elif change == 'label': image['labels']['org.opencontainers.image.revision'] = 'a' * 40
            else: del image['candidate']
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate_report(report)

    def test_host_and_image_architecture_must_agree_before_publication(self):
        report = measured_report('arm64')
        report['images']['pixstride']['architecture'] = 'amd64'
        with self.assertRaises(ValueError):
            validate_report(report)
        report = measured_report()
        report['images']['imgproxy']['os'] = 'darwin'
        with self.assertRaises(ValueError):
            validate_report(report)

    def test_noise_statistics_used_in_prose_must_match_raw_batches(self):
        report = measured_report()
        report['cases'][0]['engines'][ENGINES[0]]['summary']['cpu_batch_cv'] = -99
        with self.assertRaises(ValueError):
            validate_report(report)

    def test_csv_export_rejects_incomparable_reports_without_creating_table(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            left, right = root / 'amd.json', root / 'arm.json'
            left.write_text(json.dumps(measured_report()))
            changed = measured_report('arm64')
            changed['protocol']['warmups'] = 6
            right.write_text(json.dumps(changed))
            target = root / 'result.csv'
            completed = subprocess.run([sys.executable, str(Path(__file__).resolve().parents[3] / 'scripts/benchmark_report.py'),
                                        '--reports', str(left), str(right), '--csv', str(target)], capture_output=True)
            self.assertNotEqual(completed.returncode, 0)
            self.assertFalse(target.exists())

    def test_published_archives_preserve_samples_and_detect_summary_drift(self):
        from benchmark_report import publish_results, check_results_directory
        reports = [measured_report(), measured_report('arm64')]
        loaded = [(r, json.dumps(r).encode()) for r in reports]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            publish_results(loaded, root)
            check_results_directory(root)
            compressed = (root / 'linux-amd64.raw.json.gz').read_bytes()
            self.assertEqual(gzip.decompress(compressed), loaded[0][1])
            summary_path = root / 'linux-amd64.json'
            summary = json.loads(summary_path.read_text())
            self.assertEqual(summary['raw_evidence']['sha256'], hashlib.sha256(compressed).hexdigest())
            self.assertEqual(summary['cases'][0]['engines'][ENGINES[0]]['batches'][0]['requests'], 24)
            summary['cases'][0]['engines'][ENGINES[0]]['summary']['cpu_ms_per_request'] = 999
            summary_path.write_text(json.dumps(summary))
            with self.assertRaises(ValueError):
                check_results_directory(root)

    def test_missing_cases_or_samples_and_stale_derived_statistics_fail(self):
        original = measured_report()
        validate_report(original)
        for change in ['case', 'sample', 'summary', 'profile', 'dirty']:
            report = copy.deepcopy(original)
            if change == 'case': report['cases'].pop()
            elif change == 'sample': report['cases'][0]['engines'][ENGINES[0]]['batches'][0]['latencies_ns'].pop()
            elif change == 'summary': report['cases'][0]['engines'][ENGINES[0]]['summary']['cpu_ms_per_request'] = 999
            elif change == 'profile': report['profile'] = 'preflight-or-partial'
            else: report['harness_dirty'] = True
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate_report(report)

    def test_pair_requires_identical_corpus_protocol_and_images_on_both_architectures(self):
        amd, arm = measured_report(), measured_report('arm64')
        validate_pair([amd, arm])
        for change in ['corpus', 'protocol', 'image', 'architecture']:
            broken = copy.deepcopy(arm)
            if change == 'corpus': broken['fixtures']['files']['fixture.png']['sha256'] = 'e' * 64
            elif change == 'protocol': broken['protocol']['warmups'] = 6
            elif change == 'image': broken['images']['pixstride']['reference'] = 'different@sha256:' + 'f' * 64
            else: broken['host']['architecture'] = 'amd64'
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate_pair([amd, broken])

    def test_overview_counts_observations_from_all_cases_without_selecting_winners(self):
        result = overview(measured_report(), ENGINES[0])
        self.assertEqual(result['lower_cpu_cases'], 25)
        self.assertEqual(result['lower_p50_cases'], 0)
        self.assertEqual(result['median_cpu_ratio'], .5)
        self.assertEqual(result['exact_pixel_cases'], 25)


if __name__ == '__main__':
    unittest.main()
