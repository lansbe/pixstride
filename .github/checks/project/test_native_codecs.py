"""Exercise the native loader boundary; enable the real Linux stack integration explicitly."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
CHECK = ROOT / 'scripts/native_codecs_check.py'


class NativeCodecsTests(unittest.TestCase):
    def test_missing_codec_prefix_fails_without_system_fallback(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run([sys.executable, str(CHECK), '--codec-prefix', directory],
                                    text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('codec stack rejected', result.stderr)
        self.assertEqual(result.stdout, '')

    @unittest.skipUnless(os.environ.get('PIXSTRIDE_CODEC_TEST_PREFIX'),
                         'requires the native Linux codec build')
    def test_missing_private_loader_search_path_rejects_system_codecs(self):
        environment = dict(os.environ)
        environment.pop('LD_LIBRARY_PATH', None)
        result = subprocess.run([sys.executable, str(CHECK), '--codec-prefix',
                                 os.environ['PIXSTRIDE_CODEC_TEST_PREFIX']],
                                env=environment, text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('codec stack rejected', result.stderr)
        self.assertEqual(result.stdout, '')

    @unittest.skipUnless(os.environ.get('PIXSTRIDE_CODEC_TEST_PREFIX'),
                         'requires the native Linux codec build')
    def test_installed_stack_encodes_and_decodes_avif_and_heic(self):
        result = subprocess.run([sys.executable, str(CHECK), '--codec-prefix',
                                 os.environ['PIXSTRIDE_CODEC_TEST_PREFIX']],
                                text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report['versions'], {'libvips': '8.18.0', 'libheif': '1.23.7', 'aom': 'v3.15.3'})
        self.assertEqual(report['roundtrips'], ['avif', 'heic'])
        self.assertIn('aom', report['encoders']['avif'])
        self.assertIn('x265', report['encoders']['heic'])


if __name__ == '__main__':
    unittest.main()
