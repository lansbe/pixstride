"""Candidate publication preserves the release baseline and checks raw evidence."""
import json
import sys
import tempfile
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from benchmark_report import publish_results
from test_benchmark_qualification import evidence
from candidate_snapshot import check, publish


class CandidateSnapshotTests(unittest.TestCase):
    def fixture(self, root):
        reports, baselines = evidence(repeats=3)
        results = root / 'benchmarks/results'
        results.mkdir(parents=True)
        publish_results([(r, json.dumps(r).encode()) for r in baselines], results)
        (root / 'README.md').write_text('Before\n<!-- benchmark:summary:start -->\nrelease baseline\n<!-- benchmark:summary:end -->\n')
        (root / 'BENCHMARKS.md').write_text('Before\n<!-- benchmark:tables:start -->\nrelease tables\n<!-- benchmark:tables:end -->\n')
        paths = []
        for i, report in enumerate(reports):
            path = root / f'input-{i}.json'
            path.write_text(json.dumps(report))
            paths.append(path)
        return paths, results

    def test_roundtrip_retains_all_runs_and_original_release_archives(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            paths, results = self.fixture(root)
            before = {p.name: p.read_bytes() for p in results.iterdir()}
            publish(paths, root)
            checked = check(root)
            self.assertTrue(checked['qualified'])
            self.assertEqual([a['runs'] for a in checked['architectures'].values()], [3, 3])
            self.assertEqual(len(list(results.glob('candidate-*.raw.json.gz'))), 6)
            self.assertEqual(before, {n: (results / n).read_bytes() for n in before})
            self.assertIn('25/25', (root / 'README.md').read_text())
            self.assertIn('release baseline', (root / 'README.md').read_text())
            first = (results / 'candidate-index.json').read_bytes()
            publish(paths, root)
            self.assertEqual(first, (results / 'candidate-index.json').read_bytes())

    def test_archive_corruption_and_stale_documentation_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            paths, results = self.fixture(root)
            publish(paths, root)
            readme = root / 'README.md'
            readme.write_text(readme.read_text().replace('25/25', '24/25'))
            with self.assertRaises(ValueError):
                check(root)
            publish(paths, root)
            archive = next(results.glob('candidate-*.raw.json.gz'))
            archive.write_bytes(archive.read_bytes() + b'x')
            with self.assertRaises(ValueError):
                check(root)

    def test_failed_target_cannot_replace_a_verified_snapshot(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            paths, results = self.fixture(root)
            publish(paths, root)
            before = (results / 'candidate-index.json').read_bytes()
            report = json.loads(paths[0].read_text())
            report['cases'][0]['engines']['pixstride_default']['summary']['cgroup_peak_memory_bytes'] *= 4
            paths[0].write_text(json.dumps(report))
            with self.assertRaises(ValueError):
                publish(paths, root)
            self.assertEqual(before, (results / 'candidate-index.json').read_bytes())

    def test_absent_snapshot_is_optional_until_publication(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.fixture(root)
            self.assertIsNone(check(root))

    def test_misordered_document_markers_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            paths, _ = self.fixture(root)
            publish(paths, root)
            path = root / 'README.md'
            text = path.read_text().replace('candidate:summary:start', 'candidate:summary:temporary')
            text = text.replace('candidate:summary:end', 'candidate:summary:start')
            path.write_text(text.replace('candidate:summary:temporary', 'candidate:summary:end'))
            with self.assertRaises(ValueError):
                check(root)
