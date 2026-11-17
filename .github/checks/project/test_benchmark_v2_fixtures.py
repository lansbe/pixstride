"""The new input representations are real, deterministic files, not renamed copies."""
import copy
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[3]/'scripts'))
from benchmark_v2_catalog import fixture_specs
from benchmark_v2_fixtures import build_image, svg, write_fixture, inspect_fixture, require_decoders


@unittest.skipUnless(importlib.util.find_spec('PIL'),'Pillow is installed by native benchmark CI')
class FixturesV2Tests(unittest.TestCase):
    def photos(self):
        from PIL import Image
        im=Image.new('RGB',(96,80),'#a537df')
        im.putpixel((0,0),(1,2,3))
        return {'landscape.jpg':im,'earth.jpg':im.transpose(Image.Transpose.FLIP_LEFT_RIGHT)}

    def test_twelve_real_input_representations_are_deterministic_and_bounded(self):
        require_decoders()
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);photos=self.photos();hashes=[]
            for original in fixture_specs()[:12]:
                spec={**original,'dimensions':[64,48]}
                first=write_fixture(spec,root,photos)
                second=write_fixture(spec,root,photos)
                self.assertEqual(first,second,spec['kind'])
                self.assertEqual(first['dimensions'],[64,48])
                self.assertLess(first['bytes'],64*1024*1024)
                hashes.append(first['sha256'])
                if spec['format']!='svg':
                    from PIL import Image
                    with Image.open(root/spec['name']) as image:
                        image.load();self.assertEqual(getattr(image,'n_frames',1),1)
            self.assertEqual(len(set(hashes)),12)

    def test_metadata_hash_and_dimensions_cannot_be_rewritten_silently(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);spec={**fixture_specs()[2],'dimensions':[64,48]}
            info=write_fixture(spec,root,self.photos());path=root/spec['name']
            self.assertEqual(inspect_fixture(path,spec),info)
            wrong=copy.deepcopy(spec);wrong['dimensions']=[63,48]
            with self.assertRaises(ValueError):inspect_fixture(path,wrong)
            path.write_bytes(path.read_bytes()+b'altered')
            self.assertNotEqual(inspect_fixture(path,spec)['sha256'],info['sha256'])

    def test_alpha_palette_gray_and_vector_stress_different_processing_paths(self):
        photos=self.photos();modes={}
        for original in fixture_specs()[:12]:
            spec={**original,'dimensions':[64,48]}
            if spec['format']=='svg':
                document=svg(spec)
                self.assertIn('<path',document);self.assertIn('linearGradient',document)
                self.assertNotIn('http://',document.replace('http://www.w3.org/2000/svg',''))
            else:
                with build_image(spec,photos) as image:modes[spec['kind']]=image.mode
        self.assertEqual(modes['gradient-alpha'],'RGBA')
        self.assertEqual(modes['indexed-palette'],'P')
        self.assertEqual(modes['gray-ramp'],'L')
