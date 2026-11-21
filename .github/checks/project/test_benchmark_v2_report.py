"""Complete cohorts and raw evidence, rather than favorable subsets, determine V2 counts."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[3]/'scripts'))
from benchmark_fixtures import ROOT,SOURCES,corpus_hash,sha256
from benchmark_metrics import balanced_orders,summarize_batches
from benchmark_runtime import ENGINES
from benchmark_v2 import protocol
from benchmark_v2_catalog import CATALOG,SUITE,catalog,fixture_specs
from benchmark_v2_collect import MIMES
from test_benchmark_release import release_fixture
from benchmark_v2_report import aggregate,validate_shard
from benchmark_v2_snapshot import record,check,summary,with_summary,sync


def evidence(mode='preflight'):
    canonical=catalog()['cases'];versions=json.loads((ROOT/'benchmarks/released-versions.json').read_text())
    versions['pixstride']=release_fixture()[0]
    files={s['name']:dict(bytes=10,sha256=sha256(s['name'].encode()),**{k:v for k,v in s.items() if k!='name'}) for s in fixture_specs()}
    fixtures=dict(schema_version=1,suite_id=SUITE,catalog_sha256=sha256(CATALOG.read_bytes()),sources=SOURCES,
                  files=files,generator=dict(pillow='13.0.0',jpeg='test',zlib='test',webp='test'))
    fixtures['corpus_sha256']=corpus_hash(fixtures);reports=[]
    pixels={bg:dict(mae=0,rmse=0,max_error=0,psnr_db=None,exact=True) for bg in ('black','white','alpha')}
    pixels['exact_visible_pixels']=True
    for arch in ('amd64','arm64'):
        for rep in range(1,4 if mode=='measure' else 2):
            images={'pixstride':release_fixture(arch)[1],
                    'imgproxy':dict(reference=versions['imgproxy']['image'],version='imgproxy 4.0.24',
                        architecture=arch,os='linux',image_id='sha256:'+('7' if arch=='amd64' else '8')*64,
                        repo_digests=[versions['imgproxy']['image']])}
            for shard in range(40):
                cases=[]
                for definition in canonical[shard*25:(shard+1)*25]:
                    c=copy.deepcopy(definition);offset=(c['rank']-1)%3
                    c.update(status='complete',preflight_order=list(ENGINES[offset:]+ENGINES[:offset]),engines={})
                    for engine in ENGINES:
                        ref=engine=='imgproxy';cpu=48000 if ref else 24000
                        batches=[]
                        if mode=='measure':
                            for i,order in enumerate(balanced_orders(ENGINES,6)):
                                values=[(2_000_000 if ref else 1_000_000)+rep*10+c['rank']]*24
                                batches.append(dict(cpu_usec=cpu,user_usec=cpu-200,system_usec=200,
                                    nr_throttled=0,throttled_usec=0,index=i,position=order.index(engine),
                                    latencies_ns=values,wall_elapsed_ns=sum(values)+50_000_000))
                        peak=2097152 if ref else 1048576
                        entry=dict(status='success',attempted=True,batches=batches,
                            output=dict(bytes=1000,sha256='a'*64,mime=MIMES[c['format']],dimensions=c['dimensions']),
                            final_counters=dict(cpu_usec=1000000,user_usec=800000,system_usec=200000,
                                nr_throttled=0,throttled_usec=0,oom_kill=0,memory_peak_bytes=peak),
                            memory_at_end=dict(anon=1024,file=0,kernel=0))
                        if mode=='measure':entry['summary']={**summarize_batches(batches),'cgroup_peak_memory_bytes':peak}
                        if not ref:entry['pixels_vs_imgproxy']=pixels
                        c['engines'][engine]=entry
                    cases.append(c)
                reports.append(dict(schema_version=2,suite_id=SUITE,status='complete',mode=mode,
                    profile='complete-shard',shard_index=shard,shard_count=40,repetition=rep,
                    started_at='2026-11-17T00:00:00+00:00',finished_at='2026-11-17T01:00:00+00:00',
                    harness_revision='b'*40,harness_dirty=False,workflow_url=None,
                    host=dict(architecture=arch,selected_cpus=[0,1],client_cpus=[2,3],available_cpus=4,
                              logical_cpus=4,cpu_model=['synthetic test'],runner_label='test'),
                    images=images,protocol=protocol(mode),fixtures=fixtures,
                    catalog_sha256=sha256(CATALOG.read_bytes()),case_order=[c['id'] for c in cases],cases=cases))
    return reports,versions


def service_error(entry):
    entry.clear();entry.update(status='error',attempted=True,batches=[],
                               error=dict(type='ValueError',message='HTTP 422',phase='preflight',batch=None))


class ReportV2Tests(unittest.TestCase):
    def test_v2_target_updates_preserve_legacy_pins_and_old_cohorts_without_claiming_new_results(self):
        reports,pins=evidence()
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);(root/'benchmarks/results/v2').mkdir(parents=True)
            legacy=(ROOT/'benchmarks/released-versions.json').read_bytes()
            (root/'benchmarks/released-versions.json').write_bytes(legacy)
            (root/'benchmarks/versions-v2.json').write_text(json.dumps(pins))
            for name in ('README.md','BENCHMARKS_V2.md'):(root/name).write_text(with_summary('Before',summary(None)))
            paths=[]
            for i,r in enumerate(reports):
                p=root/f'{i}.json';p.write_text(json.dumps(r));paths.append(p)
            record(paths,'preflight',root)
            next_pins=copy.deepcopy(pins)
            next_pins['pixstride'].update(version='0.5.0',revision='8'*40,
                image='ghcr.io/lansbe/pixstride:0.5.0@sha256:'+'8'*64)
            (root/'benchmarks/versions-v2.json').write_text(json.dumps(next_pins))
            sync(root);previous=check(root)['preflight']
            self.assertEqual(previous['pins']['pixstride']['version'],'0.4.0')
            self.assertEqual((root/'benchmarks/released-versions.json').read_bytes(),legacy)
            text=(root/'README.md').read_text()
            self.assertIn('0.5.0',text);self.assertIn('pending',text);self.assertNotIn('1000/1000',text)

    def test_release_provenance_requires_explicit_published_target_and_exact_reference_version(self):
        reports,pins=evidence()
        for kind in ('published','reference-version','counter'):
            with self.subTest(kind=kind):
                broken=copy.deepcopy(reports[0]);target=copy.deepcopy(pins)
                if kind=='published':target['pixstride'].pop('published')
                elif kind=='reference-version':broken['images']['imgproxy']['version']='imgproxy 4.0.240'
                else:broken['cases'][0]['engines']['imgproxy']['final_counters']['memory_peak_bytes']=.5
                with self.assertRaises(ValueError):validate_shard(broken,target)

    def test_preflight_validates_all_thousand_but_never_claims_timing_wins(self):
        reports,pins=evidence();result=aggregate(reports,'preflight',pins=pins)
        self.assertTrue(result['coverage_complete']);self.assertTrue(result['reference_valid'])
        self.assertIsNone(result['qualified']);self.assertEqual(result['report_count'],80)
        for arch in ('amd64','arm64'):
            self.assertEqual(len(result['architectures'][arch]['cases']),1000)
            self.assertEqual(result['architectures'][arch]['reference_valid_cases'],1000)
            self.assertIsNone(result['architectures'][arch]['by_mode']['pixstride_default']['confirmed_cases'])

    def test_pixstride_error_and_quality_regression_remain_in_thousand_case_denominator(self):
        reports,pins=evidence();case=reports[0]['cases'][0]
        service_error(case['engines']['pixstride_default'])
        own=reports[0]['cases'][1]['engines']['pixstride_native']
        own['pixels_vs_imgproxy']=copy.deepcopy(own['pixels_vs_imgproxy'])
        own['pixels_vs_imgproxy']['black'].update(mae=20,rmse=20,exact=False)
        own['pixels_vs_imgproxy']['exact_visible_pixels']=False
        result=aggregate(reports,'preflight',pins=pins);arch=result['architectures']['amd64']
        self.assertTrue(result['reference_valid']);self.assertEqual(arch['by_mode']['pixstride_default']['http_valid_cases'],999)
        self.assertEqual(arch['by_mode']['pixstride_native']['quality_valid_cases'],999)
        self.assertEqual(arch['cases'][0]['modes']['pixstride_default']['status'],'engine-error')
        self.assertEqual(arch['cases'][1]['modes']['pixstride_native']['status'],'quality-regression')
        self.assertIsNone(result['qualified'])

    def test_reference_error_is_not_a_valid_fast_comparison(self):
        reports,pins=evidence();service_error(reports[0]['cases'][0]['engines']['imgproxy'])
        result=aggregate(reports,'preflight',pins=pins)
        self.assertFalse(result['reference_valid'])
        self.assertEqual(result['architectures']['amd64']['reference_valid_cases'],999)

    def test_missing_decoded_comparison_is_unknown_rather_than_zero_pixel_error(self):
        reports,pins=evidence();service_error(reports[0]['cases'][0]['engines']['pixstride_native'])
        result=aggregate(reports,'preflight',pins=pins)
        own=result['architectures']['amd64']['cases'][0]['modes']['pixstride_native']
        self.assertIsNone(own['max_mae']);self.assertIsNone(own['max_rmse'])
        self.assertIsNone(own['max_alpha_mae']);self.assertEqual(own['size_ratios'],[])

    def test_incomplete_duplicated_mixed_or_relabelled_cohorts_fail(self):
        reports,pins=evidence()
        for kind in ('missing','duplicate','mode','harness','corpus','catalog','partial','shard','case','native','proof'):
            with self.subTest(kind=kind):
                data=list(reports);data[0]=copy.deepcopy(data[0]);r=data[0]
                if kind=='missing':data.pop()
                elif kind=='duplicate':data[-1]=data[0]
                elif kind=='mode':r['mode']='measure'
                elif kind=='harness':r['harness_revision']='c'*40
                elif kind=='corpus':r['fixtures']['files'][r['cases'][0]['source']]['sha256']='f'*64
                elif kind=='catalog':r['catalog_sha256']='f'*64
                elif kind=='partial':r['profile']='partial-shard'
                elif kind=='shard':r['shard_index']=39
                elif kind=='case':r['cases'][0]['options']+='/q:1'
                elif kind=='native':r['images']['pixstride']['architecture']='arm64'
                elif kind=='proof':r['images']['pixstride']['release']['version']='0.3.0'
                with self.assertRaises(ValueError):aggregate(data,'preflight',pins=pins)

    def test_full_cohort_requires_every_metric_and_repeat_without_pixel_regression(self):
        reports,pins=evidence('measure');result=aggregate(reports,'measure',pins=pins)
        self.assertTrue(result['qualified']);self.assertEqual(result['report_count'],240)
        for arch in ('amd64','arm64'):
            self.assertEqual(result['architectures'][arch]['by_mode']['pixstride_native']['confirmed_cases'],1000)
        reports[0]['cases'][0]['deadline_exceeded']=True
        self.assertFalse(aggregate(reports,'measure',pins=pins)['qualified'])
        reports[0]['cases'][0].pop('deadline_exceeded')
        row=reports[0]['cases'][0]['engines']['pixstride_native']
        row['final_counters']['memory_peak_bytes']=4194304
        row['summary']['cgroup_peak_memory_bytes']=4194304
        result=aggregate(reports,'measure',pins=pins)
        self.assertFalse(result['qualified'])
        self.assertEqual(result['architectures']['amd64']['by_mode']['pixstride_native']['confirmed_cases'],999)
        self.assertEqual(result['architectures']['amd64']['cases'][0]['modes']['pixstride_native']['status'],'loss')
        broken=copy.deepcopy(reports[0]);broken['cases'][1]['engines']['pixstride_default']['summary']['latency_p95_ms']=.001
        with self.assertRaises(ValueError):validate_shard(broken,pins)
        # Cloning even one case into another named repetition cannot count as confirmation.
        by_key={(r['host']['architecture'],r['repetition'],r['shard_index']):r for r in reports}
        c1=by_key['arm64',1,0]['cases'][0];c2=by_key['arm64',2,0]['cases'][0]
        for engine in ('pixstride_native',):
            c2['engines'][engine]['batches']=c1['engines'][engine]['batches']
            c2['engines'][engine]['summary']=dict(c1['engines'][engine]['summary'])
        with self.assertRaises(ValueError):aggregate(reports,'measure',pins=pins)

    def test_full_reference_failure_is_retained_as_an_observed_failure_not_dropped(self):
        reports,pins=evidence('measure');service_error(reports[0]['cases'][0]['engines']['imgproxy'])
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);(root/'benchmarks/results/v2').mkdir(parents=True)
            (root/'benchmarks/versions-v2.json').write_text(json.dumps(pins))
            for name in ('README.md','BENCHMARKS_V2.md'):(root/name).write_text(with_summary('Before',summary(None)))
            paths=[]
            for i,r in enumerate(reports):
                p=root/f'{i}.json';p.write_text(json.dumps(r));paths.append(p)
            result=record(paths,'measure',root)
            self.assertFalse(result['reference_valid']);self.assertFalse(result['qualified'])
            self.assertEqual(result['retained_timed_requests'],2591856)
            self.assertEqual(check(root)['measure'],result)
            self.assertEqual(result['architectures']['amd64']['reference_valid_cases'],999)
            self.assertIn('999/1000',(root/'README.md').read_text())

    def test_snapshot_retains_failure_evidence_hashes_and_recomputes_docs(self):
        reports,pins=evidence();service_error(reports[0]['cases'][0]['engines']['pixstride_native'])
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);(root/'benchmarks/results/v2').mkdir(parents=True)
            (root/'benchmarks/versions-v2.json').write_text(json.dumps(pins))
            for name in ('README.md','BENCHMARKS_V2.md'):
                (root/name).write_text('Before\n<!-- benchmark-v2:summary:start -->\npending\n<!-- benchmark-v2:summary:end -->\nAfter\n')
            paths=[]
            for i,r in enumerate(reports):
                p=root/f'{i}.json';p.write_text(json.dumps(r));paths.append(p)
            result=record(paths,'preflight',root)
            self.assertFalse(result['architectures']['amd64']['by_mode']['pixstride_native']['http_valid_cases']==1000)
            self.assertEqual(check(root)['preflight'],result)
            text=(root/'README.md').read_text();self.assertIn('not measured',text)
            self.assertIn('999/1000',text);self.assertIn('Before',text);self.assertIn('After',text)
            (root/'README.md').write_text(text.replace('999/1000','1000/1000'))
            with self.assertRaises(ValueError):check(root)
            (root/'README.md').write_text(text)
            archive=next((root/'benchmarks/results/v2').glob('*.raw.json.gz'))
            archive.write_bytes(archive.read_bytes()+b'bad')
            with self.assertRaises(ValueError):check(root)

    def test_replacing_a_cohort_keeps_old_evidence_and_authenticates_history_filename(self):
        reports,pins=evidence()
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);(root/'benchmarks/results/v2').mkdir(parents=True)
            (root/'benchmarks/versions-v2.json').write_text(json.dumps(pins))
            for name in ('README.md','BENCHMARKS_V2.md'):(root/name).write_text(with_summary('Before',summary(None)))
            paths=[]
            for i,r in enumerate(reports):
                p=root/f'{i}.json';p.write_text(json.dumps(r));paths.append(p)
            record(paths,'preflight',root)
            first=(root/'benchmarks/results/v2/preflight-index.json').read_bytes()
            reports[0]['started_at']='2026-11-17T00:01:00+00:00'
            paths[0].write_text(json.dumps(reports[0]));record(paths,'preflight',root)
            history=next((root/'benchmarks/results/v2').glob('history-*-index.json'))
            self.assertEqual(history.read_bytes(),first);self.assertEqual(len(list(history.parent.glob('*.raw.json.gz'))),81)
            check(root)
            history.rename(history.parent/'history-preflight-000000000000-index.json')
            with self.assertRaises(ValueError):check(root)
