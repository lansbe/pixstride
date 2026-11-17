"""Unreleased cohorts bind every shard to one clean source and image per architecture."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[3]/'scripts'))
from benchmark_fixtures import ROOT
from benchmark_v2_report import aggregate,target,validate_shard
from benchmark_v2_snapshot import record,check,with_summary
from benchmark_v2_tables import summary
from test_benchmark_v2_report import evidence,service_error

SOURCE=dict(revision='b'*40,tree='c'*40,dockerfile_sha256='d'*64,cargo_lock_sha256='e'*64)
IDS=dict(amd64='sha256:'+'1'*64,arm64='sha256:'+'2'*64)


def candidate_evidence(mode='preflight'):
    reports,pins=evidence(mode)
    pins['pixstride']=dict(published=False,version='0.4.0',candidate=dict(SOURCE),images=dict(IDS))
    for report in reports:
        arch=report['host']['architecture']
        report['images']['pixstride']=dict(reference=IDS[arch],image_id=IDS[arch],architecture=arch,os='linux',
            repo_digests=[],labels={'org.opencontainers.image.revision':SOURCE['revision']},
            candidate=dict(SOURCE),capabilities=dict(version='0.4.0'))
    return reports,pins


class CandidateV2Tests(unittest.TestCase):
    def test_typed_unreleased_target_accepts_complete_preflight_but_flag_alone_is_insufficient(self):
        reports,pins=candidate_evidence()
        result=aggregate(reports,'preflight',pins)
        self.assertEqual(result['report_count'],80);self.assertIsNone(result['qualified'])
        self.assertFalse(result['pins']['pixstride']['published'])
        broken=copy.deepcopy(pins);broken['pixstride']={'published':False}
        with self.assertRaises(ValueError):target(broken)
        published=json.loads((ROOT/'benchmarks/versions-v2.json').read_text())
        with self.assertRaises(ValueError):validate_shard(reports[0],published)
        with self.assertRaises(ValueError):aggregate(reports,'preflight')

    def test_wrong_source_identity_image_label_runtime_or_native_arch_cannot_qualify(self):
        reports,pins=candidate_evidence()
        for field in ('revision','tree','dockerfile_sha256','cargo_lock_sha256','label','image_id','reference','version','native','harness','release'):
            with self.subTest(field=field):
                report=copy.deepcopy(reports[0]);image=report['images']['pixstride']
                if field in SOURCE:image['candidate'][field]='f'*len(SOURCE[field])
                elif field=='label':image['labels']['org.opencontainers.image.revision']='f'*40
                elif field in ('image_id','reference'):image[field]='sha256:'+'f'*64
                elif field=='version':image['capabilities']['version']='0.3.0'
                elif field=='native':image['architecture']='arm64'
                elif field=='harness':report['harness_revision']='f'*40
                else:image['release']={'version':'0.4.0'}
                with self.assertRaises(ValueError):validate_shard(report,pins)
        for field in ('revision','tree','dockerfile_sha256','cargo_lock_sha256'):
            broken=copy.deepcopy(pins);broken['pixstride']['candidate'][field]=123
            with self.assertRaises(ValueError):target(broken)

    def test_measured_candidate_retains_losses_and_rejects_missing_mixed_and_replayed_samples(self):
        reports,pins=candidate_evidence('measure');result=aggregate(reports,'measure',pins)
        self.assertTrue(result['qualified']);self.assertEqual(result['report_count'],240)
        row=reports[0]['cases'][0]['engines']['pixstride_native']
        row['final_counters']['memory_peak_bytes']=4194304;row['summary']['cgroup_peak_memory_bytes']=4194304
        service_error(reports[0]['cases'][1]['engines']['pixstride_default'])
        result=aggregate(reports,'measure',pins)
        self.assertFalse(result['qualified'])
        self.assertEqual(result['architectures']['amd64']['by_mode']['pixstride_native']['confirmed_cases'],999)
        self.assertEqual(result['architectures']['amd64']['by_mode']['pixstride_default']['http_valid_cases'],999)
        for kind in ('missing','mixed','replay'):
            broken=list(reports)
            if kind=='missing':broken.pop()
            elif kind=='mixed':
                broken[40]=copy.deepcopy(broken[40]);image=broken[40]['images']['pixstride']
                image['reference']=image['image_id']='sha256:'+'f'*64
            else:
                broken[40]=copy.deepcopy(broken[40])
                broken[40]['cases'][2]['engines']['pixstride_native']=copy.deepcopy(broken[0]['cases'][2]['engines']['pixstride_native'])
            with self.assertRaises(ValueError):aggregate(broken,'measure',pins)
        text=summary(result)
        self.assertIn('UNRELEASED',text);self.assertIn('package version 0.4.0',text)
        self.assertIn(SOURCE['revision'],text);self.assertIn('50.000',text)
        self.assertIn(reports[0]['cases'][0]['id'],text)
        self.assertNotIn('published image',text)

    def test_candidate_snapshot_keeps_published_bytes_and_candidate_history_hashes(self):
        reports,pins=candidate_evidence()
        published,published_pins=evidence()
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);(root/'benchmarks').mkdir()
            (root/'benchmarks/versions-v2.json').write_text(json.dumps(published_pins))
            for name in ('README.md','BENCHMARKS_V2.md'):(root/name).write_text('Before\n')
            paths=[]
            for index,report in enumerate(published):
                path=root/f'{index}.json';path.write_text(json.dumps(report));paths.append(path)
            record(paths,'preflight',root)
            old_index=(root/'benchmarks/results/v2/preflight-index.json').read_bytes()
            old_doc=(root/'README.md').read_text();start='<!-- benchmark-v2:summary:start -->'
            old_block=old_doc.split(start)[1].split('<!-- benchmark-v2:summary:end -->')[0]
            for path,report in zip(paths,reports):path.write_text(json.dumps(report))
            result=record(paths,'preflight',root,pins=pins)
            self.assertEqual(check(root)['candidate-preflight'],result)
            self.assertEqual((root/'benchmarks/results/v2/preflight-index.json').read_bytes(),old_index)
            text=(root/'README.md').read_text();self.assertEqual(text.split(start)[1].split('<!-- benchmark-v2:summary:end -->')[0],old_block)
            self.assertIn('<!-- benchmark-v2-candidate:summary:start -->',text)
            reports[0]['started_at']='2026-11-17T00:01:00+00:00';paths[0].write_text(json.dumps(reports[0]))
            record(paths,'preflight',root,pins=pins)
            history=next((root/'benchmarks/results/v2').glob('history-candidate-preflight-*-index.json'))
            check(root)
            history.rename(history.parent/'history-candidate-preflight-000000000000-index.json')
            with self.assertRaises(ValueError):check(root)

    def test_native_build_manifest_and_pins_require_clean_matching_source_without_registry_access(self):
        import benchmark_runtime as runtime
        import benchmark_v2_candidate as candidate
        with tempfile.TemporaryDirectory() as temp:
            repository=Path(temp)
            def git(*args):return subprocess.check_output(['git','-C',temp,*args],text=True).strip()
            git('init','-q');git('config','core.hooksPath','/dev/null');git('config','user.email','test@example.invalid');git('config','user.name','Test')
            (repository/'Dockerfile').write_text('FROM scratch\n')
            (repository/'Cargo.lock').write_text('version = 4\n')
            (repository/'Cargo.toml').write_text('[package]\nname = "pixstride"\nversion = "0.4.0"\n')
            git('add','.');git('commit','-qm','candidate fixture');revision=git('rev-parse','HEAD')
            original_source=runtime.candidate_source
            source=original_source(revision,repository)
            class LocalOnlyDocker(runtime.Docker):
                arch='amd64';image_id=IDS['amd64'];label=revision
                def run(self,*args,**kwargs):
                    if args!=('image','inspect',self.image_id):raise AssertionError(('unexpected Docker operation',args))
                    return json.dumps([dict(Architecture=self.arch,Os='linux',Id=self.image_id,RepoDigests=[],
                        Config=dict(Labels={'org.opencontainers.image.revision':self.label}))])
            docker=LocalOnlyDocker()
            with patch.object(runtime,'candidate_source',side_effect=lambda rev:original_source(rev,repository)), \
                    patch.object(runtime.platform,'machine',return_value='x86_64'):
                manifest=candidate.build_manifest(docker,IDS['amd64'],revision,'amd64',repository)
                self.assertEqual(manifest['candidate'],source);self.assertFalse(manifest['published'])
                self.assertEqual(manifest['image']['image_id'],IDS['amd64'])
                docker.label='f'*40
                with self.assertRaises(ValueError):candidate.build_manifest(docker,IDS['amd64'],revision,'amd64',repository)
                docker.label=revision;(repository/'Cargo.lock').write_text('changed\n')
                with self.assertRaises(ValueError):candidate.build_manifest(docker,IDS['amd64'],revision,'amd64',repository)
            other=copy.deepcopy(manifest);other['architecture']='arm64';other['image'].update(
                architecture='arm64',reference=IDS['arm64'],image_id=IDS['arm64'])
            versions=json.loads((ROOT/'benchmarks/versions-v2.json').read_text())
            pins=candidate.combine([manifest,other],versions,source)
            self.assertEqual(pins['pixstride']['images'],IDS);self.assertEqual(pins['imgproxy'],versions['imgproxy'])
            for kind in ('missing','duplicate','source','version','image','arch','release'):
                data=copy.deepcopy([manifest,other])
                if kind=='missing':data.pop()
                elif kind=='duplicate':data[1]=data[0]
                elif kind=='source':data[1]['candidate']['tree']='a'*40
                elif kind=='version':data[1]['version']='0.5.0'
                elif kind=='image':data[1]['image']['image_id']=IDS['amd64']
                elif kind=='arch':data[1]['image']['architecture']='amd64'
                else:data[1]['image']['release']={'version':'0.4.0'}
                with self.subTest(kind=kind),self.assertRaises(ValueError):candidate.combine(data,versions,source)

    def test_campaign_matrix_never_skips_architecture_repetition_or_shard(self):
        from benchmark_v2_candidate import matrix
        for mode,count,repetitions in [('preflight',80,{1}),('measure',240,{1,2,3})]:
            jobs=matrix(mode)['include'];self.assertEqual(len(jobs),count)
            for arch in ('amd64','arm64'):
                for rep in repetitions:
                    self.assertEqual({job['shard'] for job in jobs if job['arch']==arch and job['repetition']==rep},set(range(40)))
        with self.assertRaises(ValueError):matrix('partial')

    def test_pending_candidate_is_never_described_as_a_published_package(self):
        _,pins=candidate_evidence();text=summary(None,pins)
        self.assertIn('UNRELEASED',text);self.assertIn(SOURCE['revision'],text)
        self.assertIn('not measured',text);self.assertNotIn('published target',text)
