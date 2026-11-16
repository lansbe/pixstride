"""A diagnostic is complete only with comparable, valid observations."""
import copy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from performance_allocator import validate_observations


def observations():
    policies = [(delay, 'none') for delay in (0, 1, 10, 100, 1000)] + [(1000, 'request')]
    return [dict(operation=operation, purge_delay_ms=delay, collection=collection, samples=48,
                 allocator_version=30302, processing_p50_ms=20., processing_p95_ms=22.,
                 cpu_mean_ms=21., cgroup_peak_memory_bytes=32 * 1024**2,
                 output_sha256=('a' if operation == 'blur' else 'b') * 64)
            for delay, collection in policies for operation in ('blur', 'sharpen')]


class AllocatorProfileTests(unittest.TestCase):
    def test_complete_policies_preserve_each_operations_output(self):
        validate_observations(observations())
        changed = observations()
        changed[-1]['output_sha256'] = 'c' * 64
        with self.assertRaises(ValueError):
            validate_observations(changed)

    def test_partial_or_duplicate_profiles_cannot_be_complete(self):
        for entries in (observations()[:-1], observations() + [observations()[0]],
                        observations()[:-1] + [observations()[0]]):
            with self.subTest(entries=len(entries)), self.assertRaises(ValueError):
                validate_observations(entries)

    def test_invalid_counters_and_provenance_fail_closed(self):
        for field, value in [('samples', 47), ('samples', True), ('allocator_version', 30303),
                             ('purge_delay_ms', False), ('operation', 'resize'), ('collection', 'unknown'),
                             ('cpu_mean_ms', float('nan')), ('processing_p50_ms', 0),
                             ('processing_p95_ms', float('inf')), ('cpu_mean_ms', True),
                             ('cgroup_peak_memory_bytes', 0), ('output_sha256', 'x' * 64)]:
            entries = copy.deepcopy(observations())
            entries[0][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                validate_observations(entries)


if __name__ == '__main__':
    unittest.main()
