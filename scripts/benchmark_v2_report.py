#!/usr/bin/env python3
"""Recompute complete V2 cohorts, preserving all errors and separating preflight from timing."""
import argparse
from collections import Counter,defaultdict
import datetime as dt
import gzip
import hashlib
import json
import math
from pathlib import Path
import re

from benchmark_fixtures import ROOT,PILLOW_VERSION,SOURCES,corpus_hash,sha256
from benchmark_metrics import balanced_orders,summarize_batches
from benchmark_release import identity,validate_release_image
from benchmark_report import validate_client_affinity
from benchmark_runtime import ENGINES,require_native_image
from benchmark_v2 import protocol
from benchmark_v2_catalog import CATALOG,SUITE,fixture_specs,select_shard,verify_catalog
from benchmark_v2_collect import MIMES,preflight_order
from release_policy import version_key

MODES=ENGINES[:-1]
METRICS=('cpu_ms_per_request','latency_p50_ms','latency_p95_ms','cgroup_peak_memory_bytes')
MAX_RAW=32*1024*1024


def load(path):
    path=Path(path)
    opener=gzip.open if path.suffix=='.gz' else open
    with opener(path,'rb') as stream:raw=stream.read(MAX_RAW+1)
    if len(raw)>MAX_RAW:raise ValueError('Oversized V2 shard report')
    return json.loads(raw),raw


def load_pins(path=None):
    pins=json.loads((path or ROOT/'benchmarks/versions-v2.json').read_text())
    target(pins);return pins


def target(pins):
    if (pins.get('schema_version')!=1 or pins['pixstride'].get('published') is not True
            or pins['pillow']!=PILLOW_VERSION
            or not re.fullmatch(r'[^\s@]+@sha256:[0-9a-f]{64}',pins['imgproxy']['image'])):
        raise ValueError('V2 requires explicit published immutable target pins and the fixed decoder')
    version_key(pins['imgproxy']['version'])
    return dict(pixstride=identity(pins['pixstride']),imgproxy={k:pins['imgproxy'][k] for k in ('version','image')},
                pillow=pins['pillow'])


def number(value,positive=False):
    return type(value) in (int,float) and math.isfinite(value) and (value>0 if positive else value>=0)


def pixel_values(entry):
    values=entry['pixels_vs_imgproxy']
    if type(values['exact_visible_pixels']) is not bool:raise ValueError('Missing V2 exact-pixel evidence')
    for bg in ('black','white','alpha'):
        for metric in ('mae','rmse'):
            if not number(values[bg][metric]) or values[bg][metric]>255:raise ValueError('Invalid V2 pixel evidence')
    exact=all(values[bg][m]==0 for bg in ('black','white','alpha') for m in ('mae','rmse'))
    if exact!=values['exact_visible_pixels']:raise ValueError('Inconsistent exact-pixel evidence')
    return values


def quality_failures(case,entry):
    values=pixel_values(entry);limits=case['quality'];failures=[]
    for bg in ('black','white','alpha'):
        for metric in ('mae','rmse'):
            limit=limits[('alpha_' if bg=='alpha' else '')+metric]
            if values[bg][metric]>limit:failures.append(f'{bg} {metric} {values[bg][metric]:.6g} > {limit}')
    if limits['exact'] and not values['exact_visible_pixels']:failures.append('Exact visible pixels required')
    return failures


def fixture_metadata(manifest,catalog_hash):
    if (manifest['schema_version']!=1 or manifest['suite_id']!=SUITE
            or manifest['catalog_sha256']!=catalog_hash or manifest['sources']!=SOURCES
            or manifest['generator']['pillow']!=PILLOW_VERSION
            or manifest['corpus_sha256']!=corpus_hash(manifest)):
        raise ValueError('Wrong V2 corpus identity')
    specs=fixture_specs()
    if set(manifest['files'])!={s['name'] for s in specs}:raise ValueError('Incomplete V2 corpus')
    for s in specs:
        info=manifest['files'][s['name']]
        if (any(info[k]!=s[k] for k in ('dimensions','kind','scale','format'))
                or type(info['bytes']) is not int or not 0<info['bytes']<=64*1024*1024
                or not re.fullmatch(r'[0-9a-f]{64}',info['sha256'])):
            raise ValueError('Invalid V2 fixture metadata')


def validate_entry(case,engine,entry,p,mode,reference_success):
    state=entry['status'];batches=entry['batches']
    if state not in ('success','error') or type(entry['attempted']) is not bool:
        raise ValueError('Unfinished/missing V2 engine outcome')
    if state=='success' and not entry['attempted']:raise ValueError('Unattempted engine cannot succeed')
    if state=='error':
        if (not isinstance(entry['error']['message'],str) or not entry['error']['type']
                or entry['error']['phase'] not in ('preflight','warmup','measure','accounting','pixels','deadline')):
            raise ValueError('Missing V2 failure diagnostics')
        if not entry['attempted'] and not (case.get('deadline_exceeded') and entry['error']['phase']=='deadline'):
            raise ValueError('Unattempted engine must retain explicit deadline failure')
    if 'output' in entry:
        output=entry['output']
        if (output['dimensions']!=case['dimensions'] or output['mime']!=MIMES[case['format']]
                or type(output['bytes']) is not int or not 0<output['bytes']<=64*1024*1024
                or not re.fullmatch(r'[0-9a-f]{64}',output['sha256'])):
            raise ValueError('V2 output metadata differs from declared decoded response')
    elif state=='success':raise ValueError('Successful engine has no decoded output')
    if mode=='preflight':
        if batches or 'summary' in entry or 'incomplete_batch' in entry:
            raise ValueError('Preflight cannot masquerade as measured performance')
    else:
        if len(batches)>p['batches'] or (state=='success' and len(batches)!=p['batches']):
            raise ValueError('Incomplete V2 measurement batches')
        orders=balanced_orders(ENGINES,p['batches'])
        if 'incomplete_batch' in entry:
            partial=entry['incomplete_batch'];index=partial['index']
            if (state!='error' or not entry['attempted'] or type(index) is not int
                    or index!=len(batches) or not 0<=index<p['batches']
                    or partial['position']!=orders[index].index(engine)
                    or len(partial['latencies_ns'])>p['requests_per_batch']
                    or any(type(v) is not int or v<=0 for v in partial['latencies_ns'])):
                raise ValueError('Invalid retained V2 incomplete batch')
        for index,batch in enumerate(batches):
            if (batch['index']!=index or batch['position']!=orders[index].index(engine)
                    or len(batch['latencies_ns'])!=p['requests_per_batch']
                    or any(type(v) is not int or v<=0 for v in batch['latencies_ns'])
                    or any(type(batch[k]) is not int or batch[k]<0 for k in
                           ('cpu_usec','user_usec','system_usec','nr_throttled','throttled_usec','wall_elapsed_ns'))
                    or batch['wall_elapsed_ns']<sum(batch['latencies_ns'])):
                raise ValueError('Invalid/unbalanced V2 raw batch samples')
        if 'summary' in entry:
            if len(batches)!=p['batches']:raise ValueError('Partial V2 batches cannot have a full summary')
            calculated=summarize_batches(batches)
            for key,value in calculated.items():
                actual=entry['summary'][key]
                expected,observed=(value,actual) if isinstance(value,list) else ([value],[actual])
                if len(expected)!=len(observed) or any(not number(a) or not math.isclose(a,b,rel_tol=1e-12,abs_tol=1e-12)
                                                       for a,b in zip(observed,expected)):
                    raise ValueError('V2 statistics disagree with raw batches')
        elif state=='success':raise ValueError('Full V2 trial has no recomputable summary')
    if state=='success':
        counters=entry['final_counters']
        if (any(type(counters[k]) is not int or counters[k]<0 for k in
                ('cpu_usec','user_usec','system_usec','nr_throttled','throttled_usec','oom_kill','memory_peak_bytes'))
                or counters['oom_kill']!=0 or counters['memory_peak_bytes']<=0):
            raise ValueError('Invalid successful V2 memory/OOM accounting')
        if mode=='measure':
            if (entry['summary']['cgroup_peak_memory_bytes']!=counters['memory_peak_bytes']
                    or any(not number(entry['summary'][m],True) for m in METRICS)):
                raise ValueError('Invalid V2 CPU/latency/peak-memory observation')
        if engine in MODES and reference_success:pixel_values(entry)


def validate_shard(report,pins=None):
    try:return _validate_shard(report,pins or load_pins())
    except (KeyError,TypeError,IndexError,AttributeError) as error:raise ValueError('Malformed V2 shard evidence') from error


def _validate_shard(r,pins):
    if pins['pixstride'].get('published') is not True:
        raise ValueError('V2 requires explicit published-release target pins')
    if (r['schema_version']!=2 or r['suite_id']!=SUITE or r['status']!='complete'
            or r['profile']!='complete-shard' or r['harness_dirty'] is not False
            or not re.fullmatch(r'[0-9a-f]{40}',r['harness_revision'])):
        raise ValueError('Only committed complete V2 shards can be aggregated')
    mode=r['mode'];p=protocol(mode)
    if r['protocol']!=p:raise ValueError('V2 fixed sampling/resources/protocol differs')
    if type(r['repetition']) is not int or not 1<=r['repetition']<=(1 if mode=='preflight' else 3):
        raise ValueError('Invalid V2 repetition')
    if r['shard_count']!=40:raise ValueError('V2 requires exactly 40 shards per run')
    expected=select_shard(verify_catalog(),r['shard_index'])
    if r['case_order']!=[c['id'] for c in expected] or len(r['cases'])!=25:
        raise ValueError('Incomplete/wrong V2 shard case order')
    catalog_hash=sha256(CATALOG.read_bytes())
    if r['catalog_sha256']!=catalog_hash:raise ValueError('V2 catalog hash differs')
    fixture_metadata(r['fixtures'],catalog_hash);validate_client_affinity(r)
    arch=r['host']['architecture']
    if arch not in ('amd64','arm64'):raise ValueError('Invalid V2 native architecture')
    for product in ('pixstride','imgproxy'):
        image=r['images'][product];require_native_image(image['architecture'],image['os'],arch)
        if not re.fullmatch(r'sha256:[0-9a-f]{64}',image['image_id']):raise ValueError('Invalid inspected image ID')
    validate_release_image(r['images']['pixstride'],pins['pixstride'])
    ref=r['images']['imgproxy']
    if (ref['reference']!=pins['imgproxy']['image'] or not re.search(r'(?<![0-9.])'+re.escape(pins['imgproxy']['version'])+r'(?![0-9.])',ref['version'])
            or not any(d.rsplit('@',1)[-1]==ref['reference'].rsplit('@',1)[-1] for d in ref['repo_digests'])):
        raise ValueError('V2 immutable reference image/version differs')
    started,finished=(dt.datetime.fromisoformat(r[k]) for k in ('started_at','finished_at'))
    if not started.tzinfo or not finished.tzinfo or finished<started:raise ValueError('Invalid V2 observation timestamps')
    for actual,definition in zip(r['cases'],expected):
        if any(actual.get(k)!=v for k,v in definition.items()) or actual['status']!='complete':
            raise ValueError('V2 case definitions or completed coverage differ')
        if actual['preflight_order']!=list(preflight_order(actual)) or set(actual['engines'])!=set(ENGINES):
            raise ValueError('Wrong V2 engines or first-touch rotation')
        reference_success=actual['engines']['imgproxy']['status']=='success'
        for engine in ENGINES:validate_entry(actual,engine,actual['engines'][engine],p,mode,reference_success)
    return r


def _row(definition,cases,mode):
    row={k:definition[k] for k in ('id','rank','tier','family')};row['reference_errors']=[];row['modes']={}
    for rep,case in enumerate(cases,1):
        ref=case['engines']['imgproxy']
        if ref['status']!='success':row['reference_errors'].append(dict(repetition=rep,**ref['error']))
    for own_mode in MODES:
        observation=dict(errors=[],quality_failures=[],ratios={m:[] for m in METRICS},size_ratios=[],
                         max_mae=None,max_rmse=None,max_alpha_mae=None,max_alpha_rmse=None)
        for rep,case in enumerate(cases,1):
            own,ref=case['engines'][own_mode],case['engines']['imgproxy']
            if own['status']!='success':observation['errors'].append(dict(repetition=rep,**own['error']))
            if own['status']!='success' or ref['status']!='success':continue
            for failure in quality_failures(definition,own):
                observation['quality_failures'].append(f'run {rep}: {failure}')
            pixels=pixel_values(own)
            for metric in ('mae','rmse'):
                observation['max_'+metric]=max(observation['max_'+metric] or 0,*(pixels[bg][metric] for bg in ('black','white')))
                observation['max_alpha_'+metric]=max(observation['max_alpha_'+metric] or 0,pixels['alpha'][metric])
            observation['size_ratios'].append(own['output']['bytes']/ref['output']['bytes'])
            if mode=='measure':
                for metric in METRICS:observation['ratios'][metric].append(own['summary'][metric]/ref['summary'][metric])
        observation['status']=('reference-error' if row['reference_errors'] else 'engine-error' if observation['errors']
            else 'quality-regression' if observation['quality_failures'] else 'preflight-checked' if mode=='preflight'
            else 'loss' if any(max(v)>=1 for v in observation['ratios'].values()) else 'confirmed-win')
        row['modes'][own_mode]=observation
    return row


def aggregate(reports,mode,pins=None):
    pins=pins or load_pins();target(pins);protocol(mode)
    repetitions=1 if mode=='preflight' else 3
    expected={(arch,rep,shard) for arch in ('amd64','arm64') for rep in range(1,repetitions+1) for shard in range(40)}
    indexed={};cohort=None;native_ids={}
    for report in reports:
        validate_shard(report,pins)
        if report['mode']!=mode:raise ValueError('Mixed V2 preflight/measurement modes')
        arch=report['host']['architecture'];key=(arch,report['repetition'],report['shard_index'])
        if key in indexed:raise ValueError('Duplicated V2 shard/repetition')
        indexed[key]=report
        identity={k:report[k] for k in ('harness_revision','catalog_sha256','fixtures','protocol')}
        if cohort is None:cohort=identity
        elif identity!=cohort:raise ValueError('Mixed V2 harness/corpus/catalog/protocol')
        ids=tuple(report['images'][p]['image_id'] for p in ('pixstride','imgproxy'))
        if arch in native_ids and ids!=native_ids[arch]:raise ValueError('Mixed V2 native image identities')
        native_ids[arch]=ids
    if set(indexed)!=expected:raise ValueError('V2 requires all 1000 cases: 80 preflight or 240 measured native shards')
    result=dict(schema_version=1,suite_id=SUITE,mode=mode,coverage_complete=True,reference_valid=True,
                qualified=None if mode=='preflight' else True,report_count=len(reports),
                retained_timed_requests=sum(len(b['latencies_ns']) for r in reports for c in r['cases']
                                            for e in c['engines'].values() for b in e['batches'])
                    +sum(len(e.get('incomplete_batch',{}).get('latencies_ns',[])) for r in reports
                         for c in r['cases'] for e in c['engines'].values()),
                catalog_sha256=cohort['catalog_sha256'],harness_revision=cohort['harness_revision'],
                corpus_sha256=cohort['fixtures']['corpus_sha256'],pins=pins,protocol=cohort['protocol'],architectures={})
    definitions=verify_catalog()
    for arch in ('amd64','arm64'):
        runs=[]
        for rep in range(1,repetitions+1):
            cases=[c for shard in range(40) for c in indexed[arch,rep,shard]['cases']];runs.append(cases)
        if mode=='measure':
            for index in range(1000):
                for engine in ENGINES:
                    fingerprints=[]
                    for cases in runs:
                        entry=cases[index]['engines'][engine]
                        samples=dict(batches=entry['batches'],incomplete=entry.get('incomplete_batch'))
                        if entry['batches'] or entry.get('incomplete_batch',{}).get('latencies_ns'):
                            fingerprints.append(hashlib.sha256(json.dumps(samples,sort_keys=True).encode()).hexdigest())
                    if len(set(fingerprints))!=len(fingerprints):
                        raise ValueError('Duplicated per-case/engine samples are not independent V2 confirmation')
        rows=[_row(d,[run[i] for run in runs],mode) for i,d in enumerate(definitions)]
        reference_valid=sum(not row['reference_errors'] for row in rows)
        modes={}
        for own in MODES:
            observations=[row['modes'][own] for row in rows]
            modes[own]=dict(http_valid_cases=sum(not o['errors'] for o in observations),
                quality_valid_cases=sum(not o['errors'] and not o['quality_failures'] and not row['reference_errors']
                                        for row,o in zip(rows,observations)),
                confirmed_cases=None if mode=='preflight' else sum(o['status']=='confirmed-win' for o in observations),
                status_counts=dict(sorted(Counter(o['status'] for o in observations).items())))
        result['architectures'][arch]=dict(repetitions=repetitions,reference_valid_cases=reference_valid,
                                          by_mode=modes,cases=rows)
        result['reference_valid'] &= reference_valid==1000
        if mode=='measure':result['qualified'] &= all(m['confirmed_cases']==1000 for m in modes.values())
    return result


def main():
    from benchmark_v2_tables import markdown
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--reports',type=Path,nargs='+',required=True)
    parser.add_argument('--mode',choices=['preflight','measure'],required=True);parser.add_argument('--versions',type=Path)
    parser.add_argument('--output',type=Path,required=True);parser.add_argument('--markdown',type=Path,required=True)
    args=parser.parse_args();result=aggregate([load(p)[0] for p in args.reports],args.mode,load_pins(args.versions))
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2)+'\n')
    args.markdown.parent.mkdir(parents=True,exist_ok=True);args.markdown.write_text(markdown(result))
    print(f'V2 {args.mode}: complete1000coverage; reference_valid={result["reference_valid"]}; qualified={result["qualified"]}')
    if not result['reference_valid'] or (args.mode=='measure' and not result['qualified']):raise SystemExit(1)


if __name__=='__main__':main()
