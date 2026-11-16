"""Allocator selection needs a complete native diagnostic, never selected samples."""
import copy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from allocator_diagnostics import policies, validate, config_environment


def observations():
    return [dict(policy=policy['id'], operation=operation, samples=48,
                 allocator_version=30302 if policy['allocator'] == 'mimalloc' else 20043,
                 processing_p50_ms=20., processing_p95_ms=22., cpu_mean_ms=21.,
                 cgroup_peak_memory_bytes=32 * 1024**2,
                 output_sha256=('a' if operation == 'blur' else 'b') * 64,
                 metadata=dict(allocator=policy['allocator'], libc_version='2.43',
                               thp_disabled=int(policy['disable_thp']), anonymous_huge_bytes=0,
                               process_peak_rss_bytes=48 * 1024**2, process_anonymous_bytes=24 * 1024**2))
            for policy in policies() for operation in ('blur', 'sharpen')]


class AllocatorDiagnosticsTests(unittest.TestCase):
    def test_huge_page_policy_is_present_before_libc_startup_and_keeps_cache_setting(self):
        config = next(p for p in policies() if p['allocator'] == 'glibc' and p['tcache_count'] == '4')
        env = config_environment(config)
        self.assertEqual(env['GLIBC_TUNABLES'], 'glibc.malloc.hugetlb=0:glibc.malloc.tcache_count=4')
        self.assertEqual(env['MALLOC_ARENA_MAX'], str(config['arenas']))
        control = next(p for p in policies() if p['id'] == 'glibc-original-control')
        self.assertNotIn('GLIBC_TUNABLES', config_environment(control))

    def test_complete_matrix_keeps_identical_outputs(self):
        entries = observations()
        validate(entries)
        entries[-1]['output_sha256'] = 'c' * 64
        with self.assertRaises(ValueError):
            validate(entries)

    def test_resident_memory_counters_cannot_be_missing_or_impossible(self):
        for field, value in [('process_peak_rss_bytes', 0), ('process_anonymous_bytes', True),
                             ('process_anonymous_bytes', 64 * 1024**2)]:
            entries = observations()
            entries[0]['metadata'][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate(entries)

    def test_missing_duplicate_or_wrong_policies_fail(self):
        for entries in (observations()[:-1], observations() + [observations()[0]],
                        observations()[:-1] + [observations()[0]]):
            with self.assertRaises(ValueError):
                validate(entries)

    def test_bad_measurements_and_ineffective_thp_disable_fail(self):
        for field, value in [('samples', True), ('cpu_mean_ms', float('nan')),
                             ('processing_p95_ms', 0), ('allocator_version', 0), ('allocator_version', 30302),
                             ('policy', 'unknown'), ('output_sha256', 'invalid')]:
            entries = copy.deepcopy(observations())
            entries[0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate(entries)
        entries = observations()
        entry = next(e for e in entries if e['metadata']['thp_disabled'])
        entry['metadata']['anonymous_huge_bytes'] = 2 * 1024**2
        with self.assertRaises(ValueError):
            validate(entries)


if __name__ == '__main__':
    unittest.main()
