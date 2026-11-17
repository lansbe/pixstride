#!/usr/bin/env python3
"""Frozen v2 workloads; nominal work order is independent of either service's timings."""
import argparse
import json
from pathlib import Path

from benchmark_fixtures import ROOT

SUITE = 'pixstride-v2-1000'
CATALOG = ROOT / 'benchmarks/cases-v2.json'
EDGES = (256,384,512,768,1024,1536,2048,2560,3072,4096)
KINDS = (
    ('photo-jpeg','jpg',4,3), ('earth-progressive','jpg',1,1),
    ('diagram-rgb','png',4,3), ('gradient-alpha','png',3,4),
    ('indexed-palette','png',1,1), ('gray-ramp','png',1,2),
    ('texture-lossless','webp',2,1), ('panorama-lossy','webp',4,1),
    ('photo-tiff','tiff',4,3), ('vector-curves','svg',4,3),
    ('poster-bmp','bmp',1,4), ('static-gif','gif',1,1),
)
FAMILIES = ('fit','fill','force','crop','rotate','flip','dpr','zoom','canvas','padding',
            'blur','sharpen','double-filter','offset-crop','fractional-crop','smart-crop',
            'alpha-background','pixelate','avif-convert','long-chain')
FORMATS = ('jpg','png','webp','bmp','avif')
ENCODER_WORK = {'jpg':2,'png':3,'webp':5,'bmp':1,'avif':15}
GRAVITIES = ('ce','noea','soea','sowe','nowe')
COLORS = ('ffffff','17324d','d8e5a2','ef763f','080b12')


def fixture_specs():
    result=[]
    for scale,edge in enumerate(EDGES,1):
        for kind,ext,aspect_w,aspect_h in KINDS:
            size=[edge,edge*aspect_h//aspect_w] if aspect_w>=aspect_h else [edge*aspect_w//aspect_h,edge]
            result.append(dict(name=f'v2-{kind}-{edge}.{ext}',kind=kind,scale=scale,
                               dimensions=size,format=ext))
    return result


def recipe(family,variant,spec):
    """Use literal geometry, unique option keys, and only documented OSS transforms."""
    w,h=spec['dimensions'];v=variant
    tw=max(16,w//(2+v%2));th=max(16,h//(2+(v+FAMILIES.index(family))%2))
    n=max(16,min(w,h)//(2+v%2));p=max(1,spec['scale']+v)
    output=FORMATS[(FAMILIES.index(family)+v)%len(FORMATS)]
    gravity,color=GRAVITIES[v],COLORS[v]
    sigma=f'{1+(spec["scale"]-1)//3+v*.25:g}'
    dims=[tw,th];passes=2;interpolated=True
    options=[f'rs:force:{tw}:{th}']
    if family=='fit':
        dims=[w//2,h//2];options=[f'rs:fit:{dims[0]}:{dims[1]}']
    elif family=='fill':
        dims=[n,n+8*v];options=[f'rs:fill:{dims[0]}:{dims[1]}',f'g:{gravity}'];passes=3
    elif family=='crop':
        options.insert(0,f'c:{w//2}:{h//2}:{gravity}');passes=3
    elif family=='rotate':
        angle=(90,180,270,90,270)[v]
        dims=[h,w] if angle in (90,270) else [w,h]
        options=[f'rot:{angle}'];output='png' if v%2==0 else 'bmp';interpolated=False
    elif family=='flip':
        dims=[w,h];options=[f'fl:{int(v%2==0)}:{int(v%2==1 or v==4)}']
        output='png' if v%2==0 else 'bmp';interpolated=False
    elif family=='dpr':
        options.append('dpr:2');dims=[tw*2,th*2]
    elif family=='zoom':
        options.append('z:2:1');dims=[tw*2,th]
    elif family=='canvas':
        options=[f'rs:fit:{n}:{n}',f'ex:1:{gravity}',f'bg:{color}'];dims=[n,n];passes=4
    elif family=='padding':
        options.extend([f'pd:{p}:{2*p}:{3*p}:{4*p}',f'bg:{color}'])
        dims=[tw+6*p,th+4*p];passes=4
    elif family=='blur':options.append('bl:'+sigma);passes=4
    elif family=='sharpen':options.append('sh:'+sigma);passes=4
    elif family=='double-filter':options.extend(['bl:'+sigma,'sh:1']);passes=6
    elif family=='offset-crop':
        options=[f'c:{w//2}:{h//2}:soea:0.1:0.1',f'rs:fill:{n}:{n}',f'g:{gravity}']
        dims=[n,n];passes=4
    elif family=='fractional-crop':options.insert(0,'c:0.75:0.5:'+gravity);passes=3
    elif family=='smart-crop':
        options=[f'rs:fill:{n}:{n+8*v}','g:sm'];dims=[n,n+8*v];passes=5
    elif family=='alpha-background':options.append('bg:'+color);passes=3
    elif family=='pixelate':options.append('pix:'+str(2**(v+1)));passes=4
    elif family=='avif-convert':output='avif'
    elif family=='long-chain':
        options.extend(['rot:90',f'fl:1:{v%2}','bl:'+sigma,'sh:1',f'pd:{p}','bg:'+color])
        dims=[tw+2*p,th+2*p];passes=9
    if output in ('jpg','webp','avif'):options.append('q:'+str((65,75,85,90,95)[v]))
    vector=spec['kind']=='vector-curves'
    filtered=family in ('blur','sharpen','double-filter','pixelate','long-chain')
    lossy=output in ('jpg','webp','avif')
    exact=(not interpolated and not vector and spec['format'] in ('png','bmp','tiff','gif')
           and not lossy)
    mae,rmse=(6,12) if lossy else ((4,10) if filtered or vector else (2,5))
    quality=dict(mae=0 if exact else mae,rmse=0 if exact else rmse,
                 alpha_mae=0 if exact else 1,alpha_rmse=0 if exact else 3,exact=exact)
    work=w*h+dims[0]*dims[1]*(passes+ENCODER_WORK[output])
    return dict(options='/'.join(options),format=output,dimensions=dims,pipeline_passes=passes,
                work_units=work,quality=quality)


def catalog():
    specs={(s['scale'],s['kind']):s for s in fixture_specs()};cases=[]
    for scale in range(1,11):
        for f,family in enumerate(FAMILIES):
            for v in range(5):
                kind=KINDS[(5*f+v)%len(KINDS)][0];spec=specs[scale,kind]
                cases.append(dict(family=family,variant=v+1,scale=scale,input_kind=kind,
                                  source=spec['name'],source_dimensions=spec['dimensions'],
                                  **recipe(family,v,spec)))
    cases.sort(key=lambda c:(c['work_units'],c['scale'],c['family'],c['variant']))
    for rank,c in enumerate(cases,1):
        c.update(id=f'v2-{rank:04d}-{c["family"]}-{c["input_kind"]}-s{c["scale"]}',
                 label=f'{c["family"]}: {c["input_kind"]}, scale {c["scale"]}, variant {c["variant"]}',
                 rank=rank,tier=(rank-1)//100+1)
    return dict(schema_version=2,suite_id=SUITE,version='2.0.0',case_count=1000,
                difficulty_model=dict(formula='input_pixels + output_pixels * (pipeline_passes + encoder_weight)',
                                      encoder_weights=ENCODER_WORK,meaning='nominal work, not measured runtime'),
                cases=cases)


def encoded(document):
    return (json.dumps(document,indent=2,sort_keys=True)+'\n').encode()


def verify_catalog(path=CATALOG,manifest=None):
    document=json.loads(Path(path).read_text())
    if document!=catalog():raise ValueError('V2 catalog drift: regenerate/review the frozen 1000 workloads')
    cases=document['cases']
    if manifest is not None:
        for spec in fixture_specs():
            if manifest.get('files',{}).get(spec['name'],{}).get('dimensions')!=spec['dimensions']:
                raise ValueError('V2 fixture dimensions/coverage differ from the catalog')
    return cases


def select_shard(cases,index,count=40):
    if type(index) is not int or type(count) is not int or count!=40 or not 0<=index<count:
        raise ValueError('V2 requires shard index 0..39 and exactly 40 shards')
    if len(cases)!=1000:raise ValueError('Shard selection requires the complete 1000-case catalog')
    return cases[index*25:(index+1)*25]


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    mode=parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--write',action='store_true');mode.add_argument('--check',action='store_true')
    args=parser.parse_args()
    if args.write:CATALOG.write_bytes(encoded(catalog()))
    verify_catalog()
    print('V2 catalog: 1000 distinct workloads, 20 families, 12 input representations, 10 difficulty tiers')


if __name__=='__main__':main()
