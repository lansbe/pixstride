"""Allocator selection needs a complete native diagnostic, never selected samples."""
import copy
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from allocator_diagnostics import policies, validate, config_environment
import allocator_diagnostics
from benchmark import host_metadata


def observations(profile='allocators'):
    return [dict(policy=policy['id'], operation=operation, samples=48,
                 tile_height=policy.get('tile_height', 16),
                 allocator_version=30302 if policy['allocator'] == 'mimalloc' else 20043,
                 processing_p50_ms=20., processing_p95_ms=22., cpu_mean_ms=21.,
                 cgroup_peak_memory_bytes=32 * 1024**2,
                 output_sha256=('a' if operation == 'blur' else 'b') * 64,
                 metadata=dict(allocator=policy['allocator'], libc_version='2.43',
                               thp_disabled=int(policy['disable_thp']), anonymous_huge_bytes=0,
                               process_peak_rss_bytes=48 * 1024**2, process_anonymous_bytes=24 * 1024**2))
            for policy in policies(profile) for operation in ('blur', 'sharpen')]


class AllocatorDiagnosticsTests(unittest.TestCase):
    def test_both_profiles_collect_real_host_metadata_before_the_build_boundary(self):
        read_text = Path.read_text
        def read(path, *args, **kwargs):
            fixtures = {'/proc/cpuinfo': 'model name: synthetic CPU\n',
                        '/etc/os-release': 'ID=synthetic\nVERSION_ID=1\n',
                        '/sys/kernel/mm/transparent_hugepage/enabled': '[madvise]\n'}
            return fixtures[str(path)] if str(path) in fixtures else read_text(path, *args, **kwargs)
        def command(*args, **_):
            if args[:2] == ('docker', 'info'):
                return json.dumps(dict(Architecture=allocator_diagnostics.architecture(), OSType='linux'))
            if args[0] == 'git': return 'b' * 40
            if args[:2] == ('docker', 'build'): raise RuntimeError('build boundary reached')
            if args[:2] == ('docker', 'ps'): return ''
            raise AssertionError(args)
        for profile in ('allocators', 'filters'):
            with self.subTest(profile=profile), tempfile.TemporaryDirectory() as temp, \
                    patch.object(allocator_diagnostics.sys, 'platform', 'linux'), \
                    patch.object(allocator_diagnostics.os, 'sched_getaffinity', return_value={0, 1, 2, 3}, create=True), \
                    patch.dict(allocator_diagnostics.os.environ, {'DOCKER_HOST': 'unix:///synthetic'}), \
                    patch.object(allocator_diagnostics, 'command', side_effect=command), \
                    patch.object(allocator_diagnostics, 'verify_fixtures', return_value={'corpus_sha256': 'a' * 64}), \
                    patch.object(allocator_diagnostics, 'candidate_source', return_value={'revision': 'b' * 40}), \
                    patch.object(allocator_diagnostics.subprocess, 'run'), \
                    patch.object(Path, 'read_text', read), \
                    patch.object(allocator_diagnostics, 'host_metadata', wraps=host_metadata) as metadata:
                args = SimpleNamespace(fixtures=Path(temp), output=Path(temp) / 'result.json', profile=profile)
                with self.assertRaisesRegex(RuntimeError, 'build boundary reached'):
                    allocator_diagnostics.run(args)
                metadata.assert_called_once_with([0, 1], [0, 1, 2, 3], [0, 1, 2, 3])

    def test_filter_profile_requires_every_strip_height_and_both_operations(self):
        entries = observations('filters')
        validate(entries, 'filters')
        self.assertEqual(len(entries), 10)
        self.assertEqual(config_environment(policies('filters')[3])['PROFILE_TILE_HEIGHT'], '64')
        with self.assertRaises(ValueError):
            validate(entries[:-1], 'filters')
        with self.assertRaises(ValueError):
            validate(entries, 'allocators')

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

    def test_the_reported_filter_strip_height_must_match_the_requested_policy(self):
        entries = observations()
        entries[0]['tile_height'] = 128
        with self.assertRaises(ValueError):
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
