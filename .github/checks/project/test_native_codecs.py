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
    def test_palette_quantization_preserves_reference_rgb_raster(self):
        result = subprocess.run([sys.executable, str(CHECK), '--codec-prefix',
                                 os.environ['PIXSTRIDE_CODEC_TEST_PREFIX']],
                                text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        # Independently decoded quantizr1.4.3 reference output for the public
        # 64x64 RGB arithmetic fixture; catches the different imagequant backend.
        self.assertEqual(report.get('palette_probe', {}).get('rgb_sha256'),
                         'a38c850f0ff23a0b3856bd6ee036c7c282d6b1f1701fb42231b2c26f9c2d2618')

    @unittest.skipUnless(os.environ.get('PIXSTRIDE_CODEC_TEST_PREFIX'),
                         'requires the native Linux codec build')
    def test_installed_stack_encodes_and_decodes_avif_and_heic(self):
        result = subprocess.run([sys.executable, str(CHECK), '--codec-prefix',
                                 os.environ['PIXSTRIDE_CODEC_TEST_PREFIX']],
                                text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report['versions'], {'libvips': '8.18.7', 'libheif': '1.23.7', 'aom': 'v3.15.3', 'quantizr': '1.4.3'})
        self.assertEqual(report['roundtrips'], ['avif', 'heic'])
        self.assertIn('aom', report['encoders']['avif'])
        self.assertIn('x265', report['encoders']['heic'])


if __name__ == '__main__':
    unittest.main()
