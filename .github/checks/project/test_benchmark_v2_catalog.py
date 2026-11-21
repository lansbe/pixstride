"""V2 coverage is frozen before either service is measured."""
from collections import Counter
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[3]/'scripts'))
from benchmark_v2_catalog import catalog, fixture_specs, select_shard, verify_catalog, ROOT


class CatalogV2Tests(unittest.TestCase):
    def test_long_chains_cover_alpha_entropy_vectors_and_extreme_aspects(self):
        stress=[c for c in catalog()['cases'] if c['family']=='long-chain' and c['scale']==10]
        self.assertEqual({c['input_kind'] for c in stress},
                         {'gradient-alpha','panorama-lossy','texture-lossless','poster-bmp','vector-curves'})
        for case in stress:
            resize=next(op for op in case['options'].split('/') if op.startswith('rs:'))
            self.assertTrue(resize.endswith(':1'),'Rotated extreme aspects must explicitly permit required enlargement')

    def test_dpr_and_zoom_require_actual_resampling_instead_of_resolving_to_identity(self):
        cases=catalog()['cases']
        for case in cases:
            if case['family'] in ('dpr','zoom'):
                w,h=case['source_dimensions'];ow,oh=case['dimensions']
                self.assertLess(ow*oh,w*h,case['id'])
        first=next(c for c in cases if c['family']=='dpr' and c['scale']==1 and c['variant']==1)
        self.assertEqual(first['source_dimensions'],[256,128])
        self.assertEqual(first['dimensions'],[128,64])

    def test_thousand_semantically_distinct_workloads_with_predeclared_progression(self):
        document=catalog();cases=document['cases']
        self.assertEqual(document['suite_id'],'pixstride-v2-1000')
        self.assertEqual(len(cases),1000)
        self.assertEqual(len({c['id'] for c in cases}),1000)
        signature=lambda c:(c['source'],tuple(sorted(c['options'].split('/'))),c['format'],tuple(c['dimensions']))
        self.assertEqual(len({signature(c) for c in cases}),1000)
        self.assertEqual([c['rank'] for c in cases],list(range(1,1001)))
        self.assertEqual(Counter(c['tier'] for c in cases),dict.fromkeys(range(1,11),100))
        self.assertEqual(sorted(c['work_units'] for c in cases),[c['work_units'] for c in cases])
        self.assertEqual(len({c['family'] for c in cases}),20)
        self.assertEqual({c['format'] for c in cases},{'png','jpg','webp','bmp','avif'})
        self.assertEqual(len({c['input_kind'] for c in cases}),12)
        self.assertEqual(len(fixture_specs()),120)
        for c in cases:
            keys=[op.split(':')[0] for op in c['options'].split('/')]
            self.assertEqual(len(keys),len(set(keys)),c['id'])
            self.assertGreater(c['work_units'],0)
            self.assertTrue(all(type(x) is int and 0<x<=8192 for x in c['dimensions']))
        old=json.loads((ROOT/'benchmarks/cases.json').read_text())['cases']
        self.assertFalse({signature(c) for c in cases}&{signature(c) for c in old})

    def test_canonical_catalog_rejects_drift_duplicates_and_malformed_geometry(self):
        for mutation in ('option','duplicate','dimension','tier','budget'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temp:
                doc=copy.deepcopy(catalog());c=doc['cases'][0]
                if mutation=='option':c['options']+='/q:1'
                elif mutation=='duplicate':doc['cases'][-1]=copy.deepcopy(c)
                elif mutation=='dimension':c['dimensions'][0]=0
                elif mutation=='tier':c['tier']=10
                else:c['quality']['mae']=255
                p=Path(temp)/'cases.json';p.write_text(json.dumps(doc))
                with self.assertRaises(ValueError):verify_catalog(p)

    def test_checked_catalog_and_complete_nonoverlapping_shards(self):
        cases=verify_catalog(ROOT/'benchmarks/cases-v2.json')
        shards=[select_shard(cases,index) for index in range(40)]
        self.assertEqual([len(s) for s in shards],[25]*40)
        self.assertEqual([c['id'] for s in shards for c in s],[c['id'] for c in cases])
        for index in (-1,40,True):
            with self.assertRaises(ValueError):select_shard(cases,index)
        manifest={'files':{s['name']:{'dimensions':s['dimensions']} for s in fixture_specs()}}
        self.assertEqual(verify_catalog(ROOT/'benchmarks/cases-v2.json',manifest),cases)
        manifest['files'][cases[0]['source']]['dimensions']=[1,1]
        with self.assertRaises(ValueError):verify_catalog(ROOT/'benchmarks/cases-v2.json',manifest)
