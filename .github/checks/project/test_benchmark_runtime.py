"""Failures and wrong outputs must never be classified as fast requests."""
import importlib.util
import io
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from benchmark_runtime import counter_delta, require_native_image, signed_path, validate_response


class BenchmarkRuntimeTests(unittest.TestCase):
    def test_signed_path_binds_options_source_and_format(self):
        path = signed_path({'options': 'rs:fit:320:320/q:85', 'source': 'landscape.jpg', 'format': 'jpg'})
        self.assertTrue(path.endswith('/rs:fit:320:320/q:85/plain/local:///landscape.jpg@jpg'))
        self.assertNotIn('unsafe', path)
        self.assertEqual(path.split('/')[1], 'cOO91ZujRXJpzWpTNDKdfVzfpkVWmA5hH3mIB-Ex_Pk')
        changed = signed_path({'options': 'q:85', 'source': 'landscape.jpg', 'format': 'png'})
        self.assertNotEqual(path.split('/')[1], changed.split('/')[1])

    def test_wrong_architecture_or_non_linux_image_is_rejected(self):
        require_native_image('amd64', 'linux', 'x86_64')
        require_native_image('arm64', 'linux', 'aarch64')
        for arch, system, host in [('amd64', 'linux', 'arm64'), ('arm64', 'darwin', 'arm64')]:
            with self.assertRaises(ValueError):
                require_native_image(arch, system, host)

    def test_counter_reset_and_oom_fail_instead_of_negative_scores(self):
        before = dict(cpu_usec=100, user_usec=80, system_usec=20, nr_throttled=1, throttled_usec=10, oom_kill=0)
        after = dict(cpu_usec=500, user_usec=380, system_usec=120, nr_throttled=3, throttled_usec=30, oom_kill=0)
        self.assertEqual(counter_delta(before, after)['cpu_usec'], 400)
        self.assertEqual(counter_delta(before, after)['throttled_usec'], 20)
        for bad in [dict(after, cpu_usec=99), dict(after, oom_kill=1)]:
            with self.assertRaises(ValueError):
                counter_delta(before, bad)

    def test_http_error_and_mime_mismatch_are_never_successful_samples(self):
        case = {'format': 'png', 'dimensions': [1, 1]}
        for status, mime in [(403, 'image/png'), (200, 'text/plain')]:
            with self.assertRaises(ValueError):
                validate_response(case, status, {'content-type': mime}, b'error')

    @unittest.skipUnless(importlib.util.find_spec('PIL'), 'Pillow is installed by the benchmark workflow')
    def test_wrong_dimensions_and_changed_timed_body_are_rejected(self):
        from PIL import Image
        output = io.BytesIO()
        Image.new('RGB', (2, 1), 'red').save(output, format='PNG')
        body = output.getvalue()
        case = {'format': 'png', 'dimensions': [2, 1]}
        metadata = validate_response(case, 200, {'content-type': 'image/png'}, body)
        with self.assertRaises(ValueError):
            validate_response(dict(case, dimensions=[1, 1]), 200, {'content-type': 'image/png'}, body)
        with self.assertRaises(ValueError):
            validate_response(case, 200, {'content-type': 'image/png'}, body + b'changed', metadata['sha256'])


if __name__ == '__main__':
    unittest.main()
