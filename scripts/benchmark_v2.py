#!/usr/bin/env python3
"""Current benchmark v2: 1000 fixed workloads, native Linux, explicit preflight/measure modes."""
import argparse
import datetime as dt
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import uuid

from benchmark import host_metadata,isolate_client,write_report
from benchmark_fixtures import ROOT,sha256
from benchmark_release import capture_release
from benchmark_runtime import Docker,MEMORY_BYTES,architecture
from benchmark_v2_catalog import CATALOG,SUITE,select_shard,verify_catalog
from benchmark_v2_collect import collect
from benchmark_v2_fixtures import verify


def protocol(mode):
    if mode not in ('preflight','measure'):raise ValueError('V2 mode must be preflight or measure')
    measured=mode=='measure'
    return dict(batches=6 if measured else 0,requests_per_batch=24 if measured else 0,
                warmups=5 if measured else 0,concurrency=1,cpus=2,memory_bytes=MEMORY_BYTES,
                pids_limit=128,client_affinity='disjoint from service cpuset',cpu_tail_seconds=.05,
                engine_schedule='all six permutations, repeated',http_connection='fresh per request',
                case_order='ascending frozen nominal work rank within each shard',case_deadline_seconds=600,
                source_cache='warm OS page cache, no application result cache',
                cpu_scope='container-ID-verified local cgroup including descendants; client excluded',
                memory_scope='fresh per-case container lifetime peak, including startup, cache and kernel; not RSS')


def selected_cases(index,case_ids,cases=None):
    selected=select_shard(cases if cases is not None else verify_catalog(),index)
    if case_ids:
        ids=set(case_ids)
        if not ids.issubset({c['id'] for c in selected}):raise ValueError('Requested case is outside this V2 shard')
        selected=[c for c in selected if c['id'] in ids]
    return selected,bool(case_ids)


def run(args):
    if sys.platform!='linux':raise ValueError('V2 requires native Linux; local macOS results cannot qualify')
    if args.architecture and args.architecture!=architecture():raise ValueError('V2 requested/actual architecture differs')
    if type(args.repetition) is not int or not 1<=args.repetition<=3:raise ValueError('V2 repetition must be 1..3')
    if args.mode=='preflight' and args.repetition!=1:raise ValueError('Preflight uses repetition 1 without timing qualification')
    manifest=verify(args.fixtures);cases,partial=selected_cases(args.shard_index,args.case,verify_catalog(CATALOG,manifest))
    p=protocol(args.mode);available=sorted(os.sched_getaffinity(0));cpus=available[:2]
    docker=Docker(args.docker);owner=uuid.uuid4().hex[:12];versions=json.loads(args.versions.read_text());images={}
    if versions['pixstride'].get('published') is not True:raise ValueError('V2 requires explicit published image pins')
    with isolate_client(cpus) as client:
        for product in ('pixstride','imgproxy'):images[product]=docker.image(versions[product]['image'])
        images['pixstride']['capabilities']=json.loads(docker.probe(images['pixstride']['reference'],['capabilities'],owner,'pixstride'))
        images['imgproxy']['version']=docker.probe(images['imgproxy']['reference'],['imgproxy','version'],owner,'imgproxy')
        if not re.search(r'(?<![0-9.])'+re.escape(versions['imgproxy']['version'])+r'(?![0-9.])',images['imgproxy']['version']):
            raise ValueError('V2 competitor version differs')
        capture_release(docker,images['pixstride'],versions['pixstride'])
        revision=subprocess.check_output(['git','-C',str(ROOT),'rev-parse','HEAD'],text=True).strip()
        dirty=(subprocess.run(['git','-C',str(ROOT),'diff','--quiet','HEAD']).returncode!=0 or
               bool(subprocess.check_output(['git','-C',str(ROOT),'ls-files','--others','--exclude-standard']).strip()))
        report=dict(schema_version=2,suite_id=SUITE,status='running',mode=args.mode,
                    profile='partial-shard' if partial else 'complete-shard',shard_index=args.shard_index,
                    shard_count=40,repetition=args.repetition,started_at=dt.datetime.now(dt.timezone.utc).isoformat(),
                    harness_revision=revision,harness_dirty=dirty,workflow_url=None,host=host_metadata(cpus,available,client),
                    images=images,protocol=p,fixtures=manifest,catalog_sha256=sha256(CATALOG.read_bytes()),
                    case_order=[c['id'] for c in cases],cases=[],
                    limitations=['Nominal difficulty does not guarantee monotonically increasing runtime.',
                        'Static, sequential local sources; no animation/HDR/high-precision/network/concurrency claim.',
                        'Different encoders/pixels; numerical agreement budgets are not perceptual equivalence.',
                        'Shared VMs and different CPUs; no universal or ISA performance guarantee.',
                        'Lifetime cgroup peak includes startup, file cache and kernel; not process RSS.'])
        if os.environ.get('GITHUB_ACTIONS')=='true':
            report['workflow_url']=os.environ['GITHUB_SERVER_URL']+'/'+os.environ['GITHUB_REPOSITORY']+'/actions/runs/'+os.environ['GITHUB_RUN_ID']
        report['host']['docker']=json.loads(docker.run('version','--format','{{json .Server}}'))['Version']
        flush=lambda:write_report(args.output,report)
        flush()
        try:
            for index,case in enumerate(cases):
                if sorted(os.sched_getaffinity(0))!=client:raise ValueError('V2 client affinity changed')
                print(f'{index+1}/{len(cases)} rank={case["rank"]} tier={case["tier"]} {case["id"]}',flush=True)
                progress={};report['cases'].append(progress);flush()
                collect(case,docker,images,args.fixtures,cpus,owner,index,p,args.mode,progress,flush)
            report['status']='complete'
        except BaseException as error:
            report['status']='interrupted';report['failure']=dict(type=type(error).__name__,message=str(error)[:2000]);raise
        finally:
            report['finished_at']=dt.datetime.now(dt.timezone.utc).isoformat();flush()
        print(f'V2 {args.mode} complete: {len(cases)}/1000 workload definitions; shard {args.shard_index}/40',flush=True)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--versions',type=Path,default=ROOT/'benchmarks/released-versions.json')
    parser.add_argument('--mode',choices=['preflight','measure'],default='preflight')
    parser.add_argument('--shard-index',type=int,required=True);parser.add_argument('--repetition',type=int,default=1)
    parser.add_argument('--architecture',choices=['amd64','arm64']);parser.add_argument('--docker',default='docker')
    parser.add_argument('--case',action='append');args=parser.parse_args()
    try:run(args)
    except (ValueError,OSError,RuntimeError,subprocess.SubprocessError) as error:parser.exit(1,'V2 benchmark failed: '+str(error)+'\n')


if __name__=='__main__':main()
