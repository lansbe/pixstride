"""Actual published images use strict both-mode gates without candidate provenance."""
import copy
import json
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from benchmark_qualification import qualify
from benchmark_metrics import summarize_batches
from benchmark_report import publish_results
from candidate_snapshot import publish as publish_candidate
from release_snapshot import record, check
from test_benchmark_qualification import evidence
from test_benchmark_release import release_fixture


def released_evidence():
    reports, baseline = evidence()
    for report in reports:
        pins, image = release_fixture(report['host']['architecture'])
        report['images']['pixstride'] = image
    return reports, baseline, pins


class ReleaseSnapshotTests(unittest.TestCase):
    def test_cli_gate_retains_failed_result_and_returns_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); paths, directory = self.fixture(root)
            report = json.loads(paths[0].read_text())
            report['cases'][0]['engines']['pixstride_native']['summary']['cgroup_peak_memory_bytes'] *= 4
            paths[0].write_text(json.dumps(report))
            output, markdown = root / 'output/qualification.json', root / 'output/qualification.md'
            command = [sys.executable, str(Path(__file__).resolve().parents[3] / 'scripts/release_snapshot.py'),
                       '--qualify', *map(str, paths), '--versions', str(root / 'benchmarks/released-versions.json'),
                       '--baseline', str(directory), '--output', str(output), '--markdown', str(markdown)]
            completed = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(completed.returncode, 1, completed.stderr)
            self.assertFalse(json.loads(output.read_text())['qualified'])
            self.assertIn('target not met', markdown.read_text())

    def fixture(self, root):
        candidates, baseline = evidence()
        directory = root / 'benchmarks/results'
        publish_results([(r, json.dumps(r).encode()) for r in baseline], directory)
        (root / 'README.md').write_text('Before\n<!-- benchmark:summary:start -->\nold\n<!-- benchmark:summary:end -->\n')
        (root / 'BENCHMARKS.md').write_text('Before\n<!-- benchmark:tables:start -->\nold\n<!-- benchmark:tables:end -->\n')
        old_paths = []
        for i, report in enumerate(candidates):
            p = root / f'candidate-{i}.json'; p.write_text(json.dumps(report)); old_paths.append(p)
        publish_candidate(old_paths, root)
        reports, _, pins = released_evidence()
        (root / 'benchmarks/released-versions.json').write_text(json.dumps({'pixstride': pins}))
        paths = []
        for i, report in enumerate(reports):
            p = root / f'release-{i}.json'; p.write_text(json.dumps(report)); paths.append(p)
        return paths, directory

    def test_published_qualification_separates_runtime_from_harness_source(self):
        reports, baseline, pins = released_evidence()
        result = qualify(reports, baseline, released=pins)
        self.assertTrue(result['qualified'])
        self.assertNotIn('candidate', result)
        self.assertEqual(result['release']['revision'], '9' * 40)
        self.assertEqual(result['harness_revision'], 'b' * 40)
        with self.assertRaises(ValueError): qualify(reports, baseline)
        for bad in ('revision', 'image', 'version'):
            changed = copy.deepcopy(pins)
            changed[bad] = '0.3.0' if bad == 'version' else ('8' * 40 if bad == 'revision' else 'other@sha256:' + 'a' * 64)
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                qualify(reports, baseline, released=changed)

    def test_record_roundtrip_preserves_baseline_and_qualified_candidate_index(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); paths, directory = self.fixture(root)
            old = {p.name: p.read_bytes() for p in directory.iterdir()}
            result = record(paths, root)
            self.assertTrue(result['qualified'])
            self.assertEqual(check(root), result)
            for name, data in old.items(): self.assertEqual((directory / name).read_bytes(), data)
            text = (root / 'README.md').read_text()
            self.assertIn('Published Pixstride 0.4.0', text)
            self.assertIn('released-index.json', text)
            self.assertIn('forced-native', text)

    def test_every_loss_is_recorded_and_not_qualified(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); paths, _ = self.fixture(root)
            report = json.loads(paths[0].read_text())
            entry = report['cases'][0]['engines']['pixstride_native']
            for batch in entry['batches']: batch['latencies_ns'][-3:] = [5_000_000] * 3
            entry['summary'].update(summarize_batches(entry['batches']))
            paths[0].write_text(json.dumps(report))
            result = record(paths, root)
            self.assertFalse(result['qualified'])
            self.assertEqual(result['architectures']['amd64']['confirmed_cases_by_mode']['pixstride_native'], 24)
            self.assertEqual(check(root), result)
            self.assertIn('target not met', (root / 'README.md').read_text())

    def test_missing_or_duplicate_runs_cannot_be_recorded_as_a_full_cohort(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); paths, _ = self.fixture(root)
            for selected in (paths[:-1], [paths[0], paths[0], *paths[2:]]):
                with self.assertRaises(ValueError): record(selected, root)

    def test_checks_recompute_provenance_hashes_statistics_and_documents(self):
        for bad in ('archive', 'qualification', 'document', 'pins'):
            with self.subTest(bad=bad), tempfile.TemporaryDirectory() as temp:
                root = Path(temp); paths, directory = self.fixture(root); record(paths, root)
                index_path = directory / 'released-index.json'
                index = json.loads(index_path.read_text())
                if bad == 'archive':
                    p = directory / index['reports'][0]['file']; p.write_bytes(p.read_bytes() + b'x')
                elif bad == 'qualification':
                    index['qualification']['qualified'] = False; index_path.write_text(json.dumps(index))
                elif bad == 'document':
                    p = root / 'README.md'; p.write_text(p.read_text().replace('Published Pixstride 0.4.0', 'wrong'))
                else:
                    p = root / 'benchmarks/released-versions.json'
                    pins = json.loads(p.read_text()); pins['pixstride']['revision'] = 'a' * 40; p.write_text(json.dumps(pins))
                with self.assertRaises(ValueError): check(root)
