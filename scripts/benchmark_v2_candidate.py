#!/usr/bin/env python3
"""Private, unreleased V2 native build manifests and explicit candidate pins."""
import argparse
import copy
import json
from pathlib import Path
import re
import tomllib

from benchmark_fixtures import ROOT,PILLOW_VERSION
from benchmark_runtime import Docker,architecture,candidate_source,require_native_image,validate_candidate_image
from release_policy import version_key

FIELDS={'revision':40,'tree':40,'dockerfile_sha256':64,'cargo_lock_sha256':64}


def source_identity(source):
    if (not isinstance(source,dict) or set(source)!=set(FIELDS)
            or any(not isinstance(source[k],str) or not re.fullmatch('[0-9a-f]{'+str(n)+'}',source[k])
                   for k,n in FIELDS.items())):
        raise ValueError('Missing/invalid complete V2 candidate source identity')
    return dict(source)


def candidate_identity(pins):
    try:
        if pins['published'] is not False or 'image' in pins or 'release' in pins:
            raise ValueError('Candidate pins must explicitly identify an unreleased source')
        version_key(pins['version']);source=source_identity(pins['candidate'])
        if (not isinstance(pins['images'],dict) or set(pins['images'])!={'amd64','arm64'}
                or any(not isinstance(value,str) or not re.fullmatch(r'sha256:[0-9a-f]{64}',value)
                       for value in pins['images'].values())):
            raise ValueError('Candidate pins require one immutable image ID for each native architecture')
        return dict(published=False,version=pins['version'],candidate=source,images=dict(pins['images']))
    except (KeyError,TypeError,AttributeError) as error:
        raise ValueError('Missing/invalid V2 candidate pins') from error


def validate_image(image,pins,revision,arch):
    expected=candidate_identity(pins)
    validate_candidate_image(image,revision)
    require_native_image(image['architecture'],image['os'],arch)
    if (image['candidate']!=expected['candidate'] or revision!=expected['candidate']['revision']
            or image['image_id']!=expected['images'][arch] or image.get('release') is not None
            or image.get('capabilities',{}).get('version')!=expected['version']):
        raise ValueError('V2 candidate image/runtime/source differs from explicit candidate pins')
    return image


def build_manifest(docker,image_id,revision,arch,repository=ROOT):
    """Inspect an already built local image; never pull or publish candidate software."""
    source=candidate_source(revision,repository)
    if architecture()!=arch:raise ValueError('Candidate manifest requires its native build host')
    image=docker.image(image_id,revision)
    if image['candidate']!=source:raise ValueError('Candidate build source differs from inspected checkout')
    require_native_image(image['architecture'],image['os'],arch)
    version=tomllib.loads((repository/'Cargo.toml').read_text())['package']['version']
    version_key(version)
    return dict(schema_version=1,published=False,architecture=arch,version=version,
                candidate=source,image=image)


def combine(manifests,versions,source=None):
    """Join the two native manifests into generated pins outside the checkout."""
    expected_source=source_identity(source) if source is not None else None
    if len(manifests)!=2:raise ValueError('Exactly two native candidate build manifests are required')
    by_arch={}
    for manifest in manifests:
        try:
            arch=manifest['architecture'];image=manifest['image'];current=source_identity(manifest['candidate'])
            if (manifest['schema_version']!=1 or manifest['published'] is not False or arch in by_arch
                    or arch not in ('amd64','arm64') or current!=image['candidate'] or image.get('release') is not None
                    or (expected_source is not None and current!=expected_source)):
                raise ValueError('Candidate native manifests differ from the clean harness source')
            validate_candidate_image(image,current['revision']);require_native_image(image['architecture'],image['os'],arch)
            version_key(manifest['version']);by_arch[arch]=manifest
        except (KeyError,TypeError,AttributeError) as error:raise ValueError('Malformed native candidate manifest') from error
    first=by_arch['amd64'];second=by_arch['arm64']
    if first['candidate']!=second['candidate'] or first['version']!=second['version']:
        raise ValueError('Native candidate build manifests have different source/package versions')
    if versions['pillow']!=PILLOW_VERSION:raise ValueError('Candidate decoder must retain the fixed Pillow version')
    pins=dict(schema_version=1,pixstride=dict(published=False,version=first['version'],candidate=first['candidate'],
        images={arch:manifest['image']['image_id'] for arch,manifest in by_arch.items()}),
        imgproxy=copy.deepcopy(versions['imgproxy']),pillow=versions['pillow'])
    candidate_identity(pins['pixstride']);return pins


def matrix(mode):
    if mode not in ('preflight','measure'):raise ValueError('Unknown V2 candidate mode')
    return {'include':[dict(arch=arch,runner=runner,repetition=rep,shard=shard,mode=mode)
        for arch,runner in [('amd64','ubuntu-24.04'),('arm64','ubuntu-24.04-arm')]
        for rep in range(1,4 if mode=='measure' else 2) for shard in range(40)]}


def main():
    parser=argparse.ArgumentParser(description=__doc__);commands=parser.add_subparsers(dest='command',required=True)
    build=commands.add_parser('manifest');build.add_argument('--image',required=True)
    build.add_argument('--revision',required=True);build.add_argument('--architecture',choices=['amd64','arm64'],required=True)
    build.add_argument('--output',type=Path,required=True)
    join=commands.add_parser('pins');join.add_argument('--manifests',type=Path,nargs='+',required=True)
    join.add_argument('--versions',type=Path,default=ROOT/'benchmarks/versions-v2.json')
    join.add_argument('--revision',required=True);join.add_argument('--output',type=Path,required=True)
    select=commands.add_parser('matrix');select.add_argument('--mode',choices=['preflight','measure'],required=True)
    args=parser.parse_args()
    if args.command=='matrix':print(json.dumps(matrix(args.mode),separators=(',',':')));return
    if args.command=='manifest':result=build_manifest(Docker(),args.image,args.revision,args.architecture)
    else:result=combine([json.loads(p.read_text()) for p in args.manifests],json.loads(args.versions.read_text()),candidate_source(args.revision))
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2)+'\n')


if __name__=='__main__':main()
