"""Publishing must reject partial, incompatible or inconsistent measurements."""
import copy
import json
from pathlib import Path
import sys
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
              'images': {name: {'reference': name + '@sha256:' + 'd' * 64} for name in ['pixstride', 'imgproxy']},
              'cases': []}
    orders = balanced_orders(ENGINES, 6)
    for index in range(25):
        case = {'id': f'case-{index}', 'status': 'success', 'engines': {}}
        for engine in ENGINES:
            batches = [{'cpu_usec': 24000 if engine == 'imgproxy' else 12000,
                        'latencies_ns': [1_000_000, 2_000_000] * 12,
                        'index': i, 'position': order.index(engine)} for i, order in enumerate(orders)]
            case['engines'][engine] = {'batches': batches,
                'summary': {**summarize_batches(batches), 'cgroup_peak_memory_bytes': 1048576},
                'pixels_vs_imgproxy': {'exact_visible_pixels': True}}
        report['cases'].append(case)
    return report


class BenchmarkReportTests(unittest.TestCase):
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
