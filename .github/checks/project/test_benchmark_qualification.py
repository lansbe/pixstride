"""Performance acceptance must reject losses, quality regressions and fake repeats."""
import copy
import sys
import tempfile
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from benchmark_metrics import summarize_batches
from benchmark_qualification import load_baselines, qualify
from benchmark_report import publish_results
import json
from test_benchmark_report import measured_report


def evidence(repeats=2, ratio=.7):
    baseline = [measured_report(arch) for arch in ('amd64', 'arm64')]
    reports = []
    for report in baseline:
        for c in report['cases']:
            c.update(source='fixture.jpg', options='rs:fit:1:1', format='png')
            for mode in ('pixstride_default', 'pixstride_native'):
                c['engines'][mode]['pixels_vs_imgproxy'] = {
                    'exact_visible_pixels': True,
                    **{bg: {'mae': 0., 'rmse': 0.} for bg in ('black', 'white', 'alpha')}}
        for repeat in range(repeats):
            candidate = copy.deepcopy(report)
            image = candidate['images']['pixstride']
            image.update(reference='sha256:' + 'e' * 64, image_id='sha256:' + 'e' * 64,
                         candidate=dict(revision='b' * 40, tree='e' * 40,
                                        dockerfile_sha256='f' * 64, cargo_lock_sha256='1' * 64),
                         labels={'org.opencontainers.image.revision': 'b' * 40})
            for c in candidate['cases']:
                for mode in ('pixstride_default', 'pixstride_native'):
                    e = c['engines'][mode]
                    for b in e['batches']:
                        b['cpu_usec'] = int(24000 * ratio) + repeat
                        b['latencies_ns'] = [int(n * ratio) + repeat for n in b['latencies_ns']]
                    e['summary'].update(summarize_batches(e['batches']))
                    e['summary']['cgroup_peak_memory_bytes'] //= 2
            reports.append(candidate)
    return reports, baseline


class QualificationTests(unittest.TestCase):
    def test_forced_native_losses_block_an_otherwise_winning_cohort(self):
        for metric in ('cpu', 'p50', 'p95', 'memory'):
            reports, baseline = evidence(repeats=3)
            entry = reports[0]['cases'][0]['engines']['pixstride_native']
            if metric == 'memory':
                entry['summary']['cgroup_peak_memory_bytes'] *= 4
            else:
                for batch in entry['batches']:
                    if metric == 'cpu': batch['cpu_usec'] = 26400
                    elif metric == 'p50': batch['latencies_ns'] = [5_000_000] * 24
                    else: batch['latencies_ns'][-3:] = [5_000_000] * 3
                entry['summary'].update(summarize_batches(entry['batches']))
            with self.subTest(metric=metric):
                self.assertFalse(qualify(reports, baseline)['qualified'])

    def test_counts_and_repeat_requirement_are_separate_for_each_mode(self):
        reports, baseline = evidence()
        for report in reports:
            for case in report['cases']:
                entry = case['engines']['pixstride_native']
                for batch in entry['batches']:
                    batch['cpu_usec'] = 23520
                entry['summary'].update(summarize_batches(entry['batches']))
        result = qualify(reports, baseline)
        self.assertEqual(result['architectures']['amd64']['confirmed_cases_by_mode'],
                         {'pixstride_default': 25, 'pixstride_native': 0})
        reports, baseline = evidence(ratio=.98)
        result = qualify(reports, baseline)
        self.assertFalse(result['qualified'])
        self.assertEqual(result['criteria']['performance_modes'], ['pixstride_default', 'pixstride_native'])
        reports, baseline = evidence(repeats=3, ratio=.98)
        result = qualify(reports, baseline)
        self.assertTrue(result['qualified'])
        for arch in ('amd64', 'arm64'):
            self.assertEqual(result['architectures'][arch]['confirmed_cases_by_mode'],
                             {'pixstride_default': 25, 'pixstride_native': 25})

    def test_cli_baseline_uses_verified_raw_archives_with_samples(self):
        _, baselines = evidence()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            publish_results([(b, json.dumps(b).encode()) for b in baselines], root)
            loaded = load_baselines(root)
            self.assertEqual(len(loaded[0]['cases'][0]['engines']['imgproxy']['batches'][0]['latencies_ns']), 24)
            archive = root / 'linux-amd64.raw.json.gz'
            archive.write_bytes(archive.read_bytes() + b'x')
            with self.assertRaises(ValueError):
                load_baselines(root)

    def test_complete_repeated_wins_meet_the_default_target(self):
        reports, baseline = evidence()
        result = qualify(reports, baseline)
        self.assertTrue(result['qualified'])
        self.assertEqual({a: r['confirmed_cases'] for a, r in result['architectures'].items()}, {'amd64': 25, 'arm64': 25})

    def test_native_wins_cannot_replace_a_default_loss(self):
        reports, baseline = evidence(ratio=1.1)
        for r in reports:
            for c in r['cases']:
                entry = c['engines']['pixstride_native']
                for b in entry['batches']:
                    b['cpu_usec'] //= 2
                    b['latencies_ns'] = [n // 2 for n in b['latencies_ns']]
                entry['summary'].update(summarize_batches(entry['batches']))
        self.assertFalse(qualify(reports, baseline)['qualified'])

    def test_close_wins_require_an_extra_independent_full_run(self):
        reports, baseline = evidence(ratio=.98)
        result = qualify(reports, baseline)
        self.assertFalse(result['qualified'])
        self.assertEqual(result['architectures']['amd64']['cases'][0]['status'], 'repeat-required')
        reports, baseline = evidence(repeats=3, ratio=.98)
        self.assertTrue(qualify(reports, baseline)['qualified'])

    def test_a_p95_loss_blocks_cpu_and_p50_wins(self):
        reports, baseline = evidence()
        entry = reports[0]['cases'][0]['engines']['pixstride_default']
        for batch in entry['batches']:
            batch['latencies_ns'][-3:] = [5_000_000] * 3
        entry['summary'].update(summarize_batches(entry['batches']))
        self.assertFalse(qualify(reports, baseline)['qualified'])

    def test_both_modes_keep_the_published_pixel_envelope_and_exact_png_identity(self):
        for mode in ('pixstride_default', 'pixstride_native'):
            reports, baseline = evidence()
            reports[0]['cases'][0]['engines'][mode]['pixels_vs_imgproxy']['black']['mae'] = .011
            self.assertFalse(qualify(reports, baseline)['qualified'])
        reports, baseline = evidence()
        reports[0]['cases'][0].update(source='fixture.png', options='')
        for b in baseline:
            b['cases'][0].update(source='fixture.png', options='')
        for r in reports:
            r['cases'][0].update(source='fixture.png', options='')
        reports[0]['cases'][0]['engines']['pixstride_default']['pixels_vs_imgproxy']['exact_visible_pixels'] = False
        self.assertFalse(qualify(reports, baseline)['qualified'])

    def test_reused_samples_and_mismatched_source_or_corpus_are_rejected(self):
        reports, baseline = evidence()
        for mutation in ('duplicate', 'source', 'corpus', 'options'):
            r = copy.deepcopy(reports)
            if mutation == 'duplicate': r[1] = copy.deepcopy(r[0])
            elif mutation == 'source': r[1]['images']['pixstride']['candidate']['tree'] = '9' * 40
            elif mutation == 'corpus': r[1]['cases_sha256'] = '9' * 64
            else: r[1]['cases'][0]['options'] = 'q:1'
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                qualify(r, baseline)

    def test_nonfinite_or_missing_quality_values_are_rejected(self):
        reports, baseline = evidence()
        for value in (float('nan'), float('inf'), -1):
            r = copy.deepcopy(reports)
            r[0]['cases'][0]['engines']['pixstride_default']['pixels_vs_imgproxy']['alpha']['rmse'] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                qualify(r, baseline)
