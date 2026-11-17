#!/usr/bin/env python3
"""Archive complete V2 cohorts, including Pixstride failures, without replacing v1 evidence."""
import argparse
import gzip
import json
from pathlib import Path
import re

from benchmark_fixtures import ROOT,sha256
from benchmark_v2_report import aggregate,load,load_pins
from benchmark_v2_tables import summary

DOCS=('README.md','BENCHMARKS_V2.md')
DIRECTORY=Path('benchmarks/results/v2')


def encoded(value):return (json.dumps(value,indent=2,sort_keys=True)+'\n').encode()


def with_summary(text,body):
    start,end='<!-- benchmark-v2:summary:start -->','<!-- benchmark-v2:summary:end -->'
    block=start+'\n'+body.rstrip()+'\n'+end
    if start not in text and end not in text:return text.rstrip()+'\n\n'+block+'\n'
    if text.count(start)!=1 or text.count(end)!=1:raise ValueError('Invalid V2 documentation markers')
    before,remainder=text.split(start,1)
    if end not in remainder:raise ValueError('Misordered V2 documentation markers')
    _,after=remainder.split(end,1)
    return before+block+after


def _index(path,root,current):
    if path.is_symlink():raise ValueError('V2 index must be a regular file')
    raw_index=path.read_bytes();index=json.loads(raw_index)
    if index.get('schema_version')!=1 or index.get('kind')!='v2-complete-cohort':raise ValueError('Invalid V2 index schema')
    if not current:
        expected=f'history-{index["mode"]}-{sha256(raw_index)[:12]}-index.json'
        if path.name!=expected:raise ValueError('V2 historical index filename/hash differs')
    mode=index['mode'];reports=[];directory=root/DIRECTORY
    for entry in index['reports']:
        name=entry['file']
        if not re.fullmatch(r'v2-(preflight|measure)-(amd64|arm64)-r[1-3]-s[0-3][0-9]-[a-f0-9]{12}\.raw\.json\.gz',name):
            raise ValueError('Invalid V2 raw archive filename')
        archive=directory/name
        if archive.is_symlink() or sha256(archive.read_bytes())!=entry['sha256']:raise ValueError('V2 compressed archive hash differs')
        report,raw=load(archive);fingerprint=sha256(raw)
        expected=f'v2-{mode}-{report["host"]["architecture"]}-r{report["repetition"]}-s{report["shard_index"]:02d}-{fingerprint[:12]}.raw.json.gz'
        if fingerprint!=entry['raw_sha256'] or name!=expected:raise ValueError('V2 raw archive binding differs')
        reports.append(report)
    pins=load_pins(root/'benchmarks/released-versions.json') if current else index['result']['pins']
    result=aggregate(reports,mode,pins)
    if not result['reference_valid'] or encoded(result)!=encoded(index['result']):raise ValueError('Stale/invalid V2 aggregate')
    return result,{entry['file'] for entry in index['reports']}


def check(root=ROOT):
    directory=root/DIRECTORY;results={};referenced=set()
    if directory.exists():
        for path in sorted(directory.glob('*-index.json')):
            current=path.name in ('preflight-index.json','measure-index.json')
            result,files=_index(path,root,current);referenced|=files
            if current:
                if path.name!=result['mode']+'-index.json':raise ValueError('V2 index mode differs from filename')
                results[result['mode']]=result
        if {p.name for p in directory.glob('*.raw.json.gz')}!=referenced:
            raise ValueError('V2 raw archives without their complete cohort index')
    body=summary(results.get('measure',results.get('preflight')))
    for name in DOCS:
        path=root/name
        if not path.exists():
            if results:raise ValueError('Missing V2 document')
            continue
        text=path.read_text()
        if '<!-- benchmark-v2:' in text and text!=with_summary(text,body):raise ValueError('Stale V2 generated documentation')
        if results and '<!-- benchmark-v2:' not in text:raise ValueError('Missing V2 generated documentation')
    return results


def sync(root=ROOT):
    results={}
    for mode in ('preflight','measure'):
        path=root/DIRECTORY/(mode+'-index.json')
        if path.exists():results[mode]=_index(path,root,True)[0]
    body=summary(results.get('measure',results.get('preflight')))
    for name in DOCS:
        path=root/name;path.write_text(with_summary(path.read_text(),body))


def record(paths,mode,root=ROOT):
    loaded=sorted([load(path) for path in paths],key=lambda item:(item[0]['host']['architecture'],item[0]['repetition'],item[0]['shard_index']))
    result=aggregate([report for report,_ in loaded],mode,load_pins(root/'benchmarks/released-versions.json'))
    if not result['reference_valid']:raise ValueError('Invalid reference cases: retain diagnostics; cannot publish a validated 1000-case cohort')
    directory=root/DIRECTORY;directory.mkdir(parents=True,exist_ok=True);entries=[];archives={}
    for report,raw in loaded:
        fingerprint=sha256(raw);arch=report['host']['architecture']
        name=f'v2-{mode}-{arch}-r{report["repetition"]}-s{report["shard_index"]:02d}-{fingerprint[:12]}.raw.json.gz'
        path=directory/name
        if path.exists():
            _,existing=load(path)
            if existing!=raw:raise ValueError('V2 raw archive fingerprint collision')
            archive=path.read_bytes()
        else:archive=gzip.compress(raw,mtime=0)
        archives[name]=archive
        entries.append(dict(file=name,sha256=sha256(archive),raw_sha256=fingerprint))
    index=dict(schema_version=1,kind='v2-complete-cohort',mode=mode,reports=entries,result=result)
    target=directory/(mode+'-index.json');new=encoded(index)
    if target.exists() and target.read_bytes()!=new:
        old=target.read_bytes();history=directory/f'history-{mode}-{sha256(old)[:12]}-index.json'
        if history.exists() and history.read_bytes()!=old:raise ValueError('V2 history fingerprint collision')
        history.write_bytes(old)
    for name,archive in archives.items():(directory/name).write_bytes(archive)
    target.write_bytes(new);sync(root);return result


def main():
    parser=argparse.ArgumentParser(description=__doc__);action=parser.add_mutually_exclusive_group(required=True)
    action.add_argument('--record',type=Path,nargs='+');action.add_argument('--check',action='store_true')
    action.add_argument('--init',action='store_true');parser.add_argument('--mode',choices=['preflight','measure']);args=parser.parse_args()
    if bool(args.record)!=bool(args.mode):parser.error('--mode is required only with --record')
    if args.record:record(args.record,args.mode)
    elif args.init:sync()
    check();print('V2 archives, complete coverage, outcomes and generated documentation are consistent')


if __name__=='__main__':main()
