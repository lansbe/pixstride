"""Memory diagnostics expose aggregate counters without addresses or paths."""
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from performance_memory import parse_smaps


class PerformanceMemoryTests(unittest.TestCase):
    def test_aggregate_heap_anonymous_and_file_without_private_identifiers(self):
        sample = '''10000000-10002000 rw-p 00000000 00:00 0 [heap]
Size: 8 kB
Rss: 8 kB
Pss: 8 kB
Anonymous: 8 kB
Private_Dirty: 8 kB
20000000-20001000 rw-p 00000000 00:00 0
Size: 4 kB
Rss: 4 kB
Pss: 4 kB
Anonymous: 4 kB
30000000-30001000 r-xp 00000000 00:00 9 /synthetic/private/file.so
Size: 4 kB
Rss: 4 kB
Pss: 2 kB
Anonymous: 0 kB
'''
        result = parse_smaps(sample)
        self.assertEqual(result['totals']['rss_bytes'], 16 * 1024)
        self.assertEqual(result['totals']['anonymous_bytes'], 12 * 1024)
        self.assertEqual(result['categories']['heap']['rss_bytes'], 8 * 1024)
        self.assertEqual(result['categories']['file']['pss_bytes'], 2 * 1024)
        self.assertNotIn('/synthetic', json.dumps(result))
        self.assertNotIn('10000000-', json.dumps(result))

    def test_empty_or_oversized_proc_response_is_rejected(self):
        for sample in ['', 'x' * (2 * 1024 * 1024 + 1)]:
            with self.assertRaises(ValueError):
                parse_smaps(sample)
