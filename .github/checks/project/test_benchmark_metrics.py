"""Benchmark accounting regressions use hand-calculated counter samples."""
import math
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from benchmark_metrics import (balanced_orders, cgroup_location, histogram_errors,
                               parse_counters, percentile, summarize_batches)


class BenchmarkMetricsTests(unittest.TestCase):
    def test_percentiles_use_all_samples_and_nearest_rank(self):
        self.assertEqual(percentile([100, 1, 2, 3], 0.5), 2)
        self.assertEqual(percentile([100, 1, 2, 3], 0.95), 100)
        self.assertEqual(percentile([7], 0), 7)
        for values, probability in [([], .5), ([1], -1), ([1], 1.1)]:
            with self.assertRaises(ValueError):
                percentile(values, probability)

    def test_cpu_normalizes_each_batch_and_latency_pools_requests(self):
        batches = [dict(cpu_usec=2000, latencies_ns=[1_000_000, 3_000_000]),
                   dict(cpu_usec=16000, latencies_ns=[2_000_000] * 7 + [90_000_000])]
        result = summarize_batches(batches)
        self.assertEqual(result['cpu_ms_per_request'], 1.5)
        self.assertEqual(result['latency_p50_ms'], 2)
        self.assertEqual(result['latency_p95_ms'], 90)
        self.assertEqual(result['requests'], 10)
        with self.assertRaises(ValueError):
            summarize_batches([dict(cpu_usec=1, latencies_ns=[])])
        with self.assertRaises(ValueError):
            summarize_batches([dict(cpu_usec=-1, latencies_ns=[1])])

    def test_engine_schedule_balances_each_position_and_pair(self):
        orders = balanced_orders(['default', 'native', 'imgproxy'], 6)
        self.assertEqual(len(set(orders)), 6)
        for position in range(3):
            for engine in ['default', 'native', 'imgproxy']:
                self.assertEqual(sum(order[position] == engine for order in orders), 2)
        self.assertEqual(balanced_orders(['a', 'b'], 4), [('a', 'b'), ('b', 'a')] * 2)
        with self.assertRaises(ValueError):
            balanced_orders(['a', 'a'], 6)

    def test_counters_include_user_system_children_and_reject_missing_cpu(self):
        result = parse_counters('usage_usec 1500\nuser_usec 900\nsystem_usec 600\nnr_throttled 2\nthrottled_usec 100\n',
                                '1048576\n', 'oom 0\noom_kill 0\n')
        self.assertEqual(result['cpu_usec'], 1500)
        self.assertEqual(result['memory_peak_bytes'], 1048576)
        self.assertEqual(result['nr_throttled'], 2)
        for cpu, memory in [('user_usec 2\n', '12'), ('usage_usec -1\nuser_usec 1\nsystem_usec 0\n', '12'),
                            ('usage_usec 1\nuser_usec 1\nsystem_usec 0\n', 'max')]:
            with self.assertRaises(ValueError):
                parse_counters(cpu, memory, '')

    def test_cgroup_lookup_accepts_unified_paths_and_refuses_escape_or_v1(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'proc/123').mkdir(parents=True)
            group = root / 'groups/system.slice/docker-fixture.scope'
            group.mkdir(parents=True)
            entry = root / 'proc/123/cgroup'
            entry.write_text('0::/system.slice/docker-fixture.scope\n')
            self.assertEqual(cgroup_location(123, root / 'proc', root / 'groups'), group.resolve())
            for text in ['3:cpu:/docker/test\n', '0::/../../outside\n']:
                entry.write_text(text)
                with self.assertRaises(ValueError):
                    cgroup_location(123, root / 'proc', root / 'groups')

    def test_pixel_error_histogram_has_hand_calculated_mae_rmse_and_zero_case(self):
        histogram = [0] * 256
        histogram[0], histogram[2] = 3, 1
        result = histogram_errors(histogram)
        self.assertEqual(result['mae'], .5)
        self.assertEqual(result['rmse'], 1)
        self.assertEqual(result['max_error'], 2)
        self.assertAlmostEqual(result['psnr_db'], 20 * math.log10(255))
        exact = histogram_errors([4] + [0] * 255)
        self.assertIsNone(exact['psnr_db'])
        self.assertTrue(exact['exact'])
        with self.assertRaises(ValueError):
            histogram_errors([0] * 256)


if __name__ == '__main__':
    unittest.main()
