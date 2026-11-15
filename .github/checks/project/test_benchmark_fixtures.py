"""Corpus integrity and quality checks must fail before a measurement can run."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'scripts'))
from benchmark_fixtures import load_cases, verify_fixtures
from benchmark_metrics import pixel_agreement


class BenchmarkCorpusTests(unittest.TestCase):
    @unittest.skipUnless(importlib.util.find_spec('PIL'), 'Pillow is installed by the benchmark workflow')
    def test_photo_normalization_does_not_attach_rgb_icc_to_grayscale_fixture(self):
        from PIL import Image, ImageCms
        from benchmark_fixtures import normalized_photo
        with tempfile.TemporaryDirectory() as directory:
            source, target = Path(directory) / 'source.png', Path(directory) / 'gray.png'
            profile = ImageCms.ImageCmsProfile(ImageCms.createProfile('sRGB')).tobytes()
            Image.new('RGB', (2, 2), 'red').save(source, icc_profile=profile)
            normalized = normalized_photo(source)
            normalized.convert('L').save(target)
            with Image.open(target) as result:
                self.assertEqual(result.mode, 'L')
                self.assertNotIn('icc_profile', result.info)
            self.assertEqual(normalized.getpixel((0, 0)), (255, 0, 0))

    def test_catalog_rejects_duplicates_and_missing_sources(self):
        cases = json.loads((ROOT / 'benchmarks/cases.json').read_text())
        sources = {c['source']: {} for c in cases['cases']}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'cases.json'
            path.write_text(json.dumps(cases))
            self.assertEqual(len(load_cases(path, sources)), 25)
            cases['cases'][-1] = cases['cases'][0]
            path.write_text(json.dumps(cases))
            with self.assertRaises(ValueError):
                load_cases(path, sources)
            with self.assertRaises(ValueError):
                load_cases(ROOT / 'benchmarks/cases.json', {})

    def test_tampered_fixture_and_path_escape_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'input.png').write_bytes(b'abc')
            manifest = {'schema_version': 1, 'files': {'input.png': {
                'sha256': hashlib.sha256(b'abc').hexdigest(), 'bytes': 3, 'dimensions': [1, 1]}}}
            (root / 'manifest.json').write_text(json.dumps(manifest))
            self.assertEqual(verify_fixtures(root)['files']['input.png']['bytes'], 3)
            (root / 'input.png').write_bytes(b'abd')
            with self.assertRaises(ValueError):
                verify_fixtures(root)
            manifest['files']['../input.png'] = manifest['files'].pop('input.png')
            (root / 'manifest.json').write_text(json.dumps(manifest))
            with self.assertRaises(ValueError):
                verify_fixtures(root)

    @unittest.skipUnless(importlib.util.find_spec('PIL'), 'Pillow is installed by the benchmark workflow')
    def test_invisible_rgb_is_ignored_but_alpha_and_visible_pixels_are_measured(self):
        from PIL import Image
        def encoded(pixel):
            output = io.BytesIO()
            Image.new('RGBA', (1, 1), pixel).save(output, format='PNG')
            return output.getvalue()
        result = pixel_agreement(encoded((255, 0, 0, 0)), encoded((0, 255, 0, 0)))
        self.assertTrue(result['exact_visible_pixels'])
        changed = pixel_agreement(encoded((255, 0, 0, 255)), encoded((0, 0, 0, 0)))
        self.assertFalse(changed['exact_visible_pixels'])
        self.assertEqual(changed['alpha']['max_error'], 255)
        self.assertEqual(changed['black']['max_error'], 255)


if __name__ == '__main__':
    unittest.main()
