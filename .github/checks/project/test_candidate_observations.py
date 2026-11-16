"""Failed measured cohorts remain inspectable without becoming qualified wins."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from candidate_snapshot import publish
from candidate_observations import record, check
import test_candidate_snapshot as snapshot_tests


class ObservationTests(unittest.TestCase):
    def fixture(self, root):
        paths, directory = snapshot_tests.CandidateSnapshotTests().fixture(root)
        publish(paths, root)
        report = json.loads(paths[0].read_text())
        report['cases'][0]['engines']['pixstride_native']['summary']['cgroup_peak_memory_bytes'] *= 4
        paths[0].write_text(json.dumps(report))
        return paths, directory

    def test_failed_full_cohort_retains_losses_and_preserves_qualified_index(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            paths, directory = self.fixture(root)
            historical = (directory / 'candidate-index.json').read_bytes()
            result = record(paths, root)
            self.assertFalse(result['qualified'])
            self.assertEqual(result['architectures']['amd64']['confirmed_cases_by_mode'],
                             {'pixstride_default': 25, 'pixstride_native': 24})
            self.assertEqual((directory / 'candidate-index.json').read_bytes(), historical)
            self.assertEqual(check(root), result)
            text = (root / 'README.md').read_text()
            self.assertIn('target not met', text)
            self.assertIn('observed-index.json', text)
            self.assertIn('forced-native', text)

    def test_recording_requires_all_three_runs_on_each_architecture(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            paths, _ = self.fixture(root)
            for selected in (paths[:-1], paths + paths[:1]):
                with self.assertRaisesRegex(ValueError, 'three'):
                    record(selected, root)

    def test_checks_recompute_raw_hashes_qualification_and_documentation(self):
        for tampering in ('archive', 'qualification', 'document'):
            with self.subTest(tampering=tampering), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                paths, directory = self.fixture(root)
                record(paths, root)
                index_path = directory / 'observed-index.json'
                index = json.loads(index_path.read_text())
                if tampering == 'archive':
                    path = directory / index['reports'][0]['file']
                    path.write_bytes(path.read_bytes() + b'x')
                elif tampering == 'qualification':
                    index['qualification']['qualified'] = True
                    index_path.write_text(json.dumps(index))
                else:
                    path = root / 'README.md'
                    path.write_text(path.read_text().replace('target not met', 'all wins'))
                with self.assertRaises(ValueError):
                    check(root)
