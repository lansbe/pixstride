"""Version observation time explicitly without relaxing sampling or old evidence."""
import copy
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
import benchmark_v2 as runner
from benchmark_v2_report import aggregate, validate_shard
from benchmark_v2_tables import summary
from test_benchmark_v2_report import evidence


def revised(report, version='2.1'):
    result = copy.deepcopy(report)
    result.update(schema_version=3, protocol_version=version)
    result['protocol']['case_deadline_seconds'] = {'2.1': 1800, '2.2': 3600}[version]
    return result


class ProtocolRevisionTests(unittest.TestCase):
    def test_revision_21_changes_only_collection_deadline(self):
        for mode in ('preflight', 'measure'):
            legacy = runner.protocol(mode)
            self.assertEqual(legacy['case_deadline_seconds'], 600)
            expected = {**legacy, 'case_deadline_seconds': 1800}
            self.assertEqual(runner.protocol(mode, '2.1'), expected)
            self.assertEqual(runner.protocol(mode, '2.0'), legacy)
        for invalid in ('latest', '2.3', 2.1, None):
            with self.assertRaises(ValueError):
                runner.protocol('measure', invalid)

    def test_legacy_and_explicit_revision_validate(self):
        reports, pins = evidence()
        validate_shard(reports[0], pins)
        validate_shard(revised(reports[0]), pins)

    def test_missing_unknown_and_inconsistent_revision_are_rejected(self):
        reports, pins = evidence()
        valid = revised(reports[0])
        mutations = [
            {**valid, 'schema_version': 2},
            {k: v for k, v in valid.items() if k != 'protocol_version'},
            {**valid, 'protocol_version': '2.0'},
            {**valid, 'protocol_version': 'future'},
            {**valid, 'protocol': runner.protocol('preflight')},
            {**reports[0], 'protocol': valid['protocol']},
        ]
        for report in mutations:
            with self.subTest(schema=report['schema_version'], version=report.get('protocol_version')):
                with self.assertRaises(ValueError):
                    validate_shard(report, pins)

    def test_complete_cohort_labels_revision_and_preserves_legacy(self):
        reports, pins = evidence()
        old = aggregate(reports, 'preflight', pins)
        self.assertNotIn('protocol_version', old)
        result = aggregate([revised(r) for r in reports], 'preflight', pins)
        self.assertEqual(result['protocol_version'], '2.1')
        self.assertEqual(result['report_count'], 80)
        self.assertEqual(result['protocol']['case_deadline_seconds'], 1800)
        self.assertTrue(result['reference_valid'])
        self.assertEqual(aggregate(reports, 'preflight', pins), old)
        self.assertIn('**v2.1: 1,000 distinct workloads**', summary(result))
        self.assertIn('1,800', summary(result))

    def test_mixed_revisions_cannot_form_a_cohort(self):
        reports, pins = evidence()
        with self.assertRaisesRegex(ValueError, 'Mixed V2'):
            aggregate([revised(reports[0]), reports[1]], 'preflight', pins)

    def test_full_measurement_keeps_every_sample_ratio_and_victory_gate(self):
        reports, pins = evidence('measure')
        old = aggregate(reports, 'measure', pins)
        for report in reports:
            report.update(schema_version=3, protocol_version='2.1')
            report['protocol'] = {**report['protocol'], 'case_deadline_seconds': 1800}
        result = aggregate(reports, 'measure', pins)
        self.assertEqual(result.pop('protocol_version'), '2.1')
        self.assertEqual(result['retained_timed_requests'], 2_592_000)
        self.assertTrue(result['qualified'])
        self.assertEqual(result['protocol']['case_deadline_seconds'], 1800)
        result['protocol'] = {**result['protocol'], 'case_deadline_seconds': 600}
        self.assertEqual(result, old)

    def test_cli_defaults_to_22_and_can_rerun_both_previous_protocols(self):
        base = ['benchmark_v2.py', '--fixtures', '/tmp/corpus', '--output', '/tmp/report.json',
                '--shard-index', '0', '--mode', 'measure']
        for extra, expected in (([], '2.2'), (['--protocol-version', '2.0'], '2.0'),
                                (['--protocol-version', '2.1'], '2.1')):
            with patch.object(sys, 'argv', base + extra), patch.object(runner, 'run') as run:
                runner.main()
                self.assertEqual(run.call_args.args[0].protocol_version, expected)

    def test_revision_22_changes_only_collection_deadline(self):
        for mode in ('preflight', 'measure'):
            legacy = runner.protocol(mode, '2.0')
            self.assertEqual(runner.protocol(mode, '2.1'), {**legacy, 'case_deadline_seconds': 1800})
            self.assertEqual(runner.protocol(mode, '2.2'), {**legacy, 'case_deadline_seconds': 3600})

    def test_revision_22_requires_matching_explicit_identity_and_cannot_mix(self):
        reports, pins = evidence()
        newer = revised(reports[0], '2.2')
        validate_shard(newer, pins)
        for mismatch in ({**newer, 'protocol_version': '2.1'},
                         {**newer, 'protocol': revised(reports[0])['protocol']}):
            with self.assertRaises(ValueError):
                validate_shard(mismatch, pins)
        with self.assertRaisesRegex(ValueError, 'Mixed V2'):
            aggregate([newer, revised(reports[1])], 'preflight', pins)

    def test_revision_22_preserves_all_measurements_and_historical_21_summary(self):
        reports, pins = evidence('measure')
        old = aggregate([revised(r) for r in reports], 'measure', pins)
        old_summary = summary(old)
        newer = aggregate([revised(r, '2.2') for r in reports], 'measure', pins)
        self.assertEqual(newer.pop('protocol_version'), '2.2')
        self.assertEqual(newer['retained_timed_requests'], 2_592_000)
        self.assertTrue(newer['qualified'])
        self.assertEqual(newer['protocol']['case_deadline_seconds'], 3600)
        self.assertIn('**v2.2: 1,000 distinct workloads**', summary({**newer, 'protocol_version': '2.2'}))
        self.assertIn('3,600 seconds per case', summary({**newer, 'protocol_version': '2.2'}))
        newer['protocol'] = {**newer['protocol'], 'case_deadline_seconds': 1800}
        self.assertEqual({**newer, 'protocol_version': '2.1'}, old)
        self.assertEqual(summary(old), old_summary)
        self.assertIn('**v2.1: 1,000 distinct workloads**', old_summary)
        self.assertIn('1,800 seconds per case', old_summary)


if __name__ == '__main__':
    unittest.main()
