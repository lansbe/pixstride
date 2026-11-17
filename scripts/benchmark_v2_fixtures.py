#!/usr/bin/env python3
"""Prepare one versioned corpus; input generation/decoding is outside service timings."""
import argparse
import json
from pathlib import Path
import random
import xml.etree.ElementTree as ET

from benchmark_fixtures import (PILLOW_VERSION,SOURCES,corpus_hash,download,normalized_photo,
                                sha256,verify_fixtures)
from benchmark_v2_catalog import CATALOG,SUITE,fixture_specs,verify_catalog

FORMATS={'jpg':'JPEG','png':'PNG','webp':'WEBP','tiff':'TIFF','bmp':'BMP','gif':'GIF'}


def require_decoders():
    import PIL
    from PIL import Image
    if PIL.__version__!=PILLOW_VERSION:raise ValueError('Install Pillow=='+PILLOW_VERSION)
    Image.init()
    if 'AVIF' not in Image.OPEN or 'AVIF' not in Image.SAVE:
        raise ValueError('V2 requires the Pillow AVIF encoder/independent decoder')


def build_image(spec,photos):
    from PIL import Image,ImageDraw,ImageOps
    w,h=spec['dimensions'];kind=spec['kind'];scale=spec['scale']
    if kind in ('photo-jpeg','earth-progressive','panorama-lossy','photo-tiff'):
        key='earth.jpg' if kind in ('earth-progressive','photo-tiff') else 'landscape.jpg'
        image=ImageOps.fit(photos[key],(w,h),Image.Resampling.LANCZOS)
        image.info.clear();return image
    if kind=='texture-lossless':
        data=random.Random(0x51D300+scale).randbytes(w*h*3)
        return Image.frombytes('RGB',(w,h),data)
    if kind in ('indexed-palette','static-gif'):
        seed=0xA1800+scale+(1000 if kind=='static-gif' else 0)
        image=Image.frombytes('P',(w,h),random.Random(seed).randbytes(w*h))
        image.putpalette([value for i in range(256) for value in (i,(i*67)%256,(i*113)%256)])
        return image
    if kind=='gray-ramp':return Image.linear_gradient('L').resize((w,h))
    if kind=='gradient-alpha':
        alpha=Image.linear_gradient('L').resize((w,h))
        image=Image.new('RGBA',(w,h),(27,165,231,0));image.putalpha(alpha)
        draw=ImageDraw.Draw(image)
        for i in range(32+scale*8):
            x,y=(i*149)%w,(i*97)%h;r=max(2,w//32)
            draw.ellipse((x,y,x+r,y+r),fill=((i*53)%256,(i*97)%256,190,32+i*7%224))
        return image
    image=Image.new('RGB',(w,h),'#e9eef4');draw=ImageDraw.Draw(image)
    for i in range(64+scale*32):
        x,y=(i*157)%w,(i*83)%h;color=((i*41)%256,(i*67)%256,(i*103)%256)
        draw.rectangle((x,y,x+max(1,w//19),y+max(1,h//23)),fill=color)
        draw.line((0,y,w,h-y),fill=color,width=1+i%3)
    return image


def svg(spec):
    w,h=spec['dimensions'];paths=[]
    for i in range(80+spec['scale']*80):
        x,y=(i*73)%w,(i*61)%h
        paths.append(f'<path d="M{x} {y} Q{(x+w//7)%w} {(y+h//3)%h} {(x+w//2)%w} {(y+h//2)%h} Z" '
                     f'fill="#{i*53271%16777216:06x}" fill-opacity="0.6"/>')
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">'
            '<defs><linearGradient id="g"><stop stop-color="#e0f1fc"/><stop offset="1" stop-color="#4d276c"/>'
            '</linearGradient></defs><rect width="100%" height="100%" fill="url(#g)"/>'
            +''.join(paths)+'</svg>')


def inspect_fixture(path,spec):
    from PIL import Image
    path=Path(path)
    if path.is_symlink() or not path.is_file() or path.stat().st_size>64*1024*1024:
        raise ValueError('V2 fixture must be a bounded regular file')
    raw=path.read_bytes()
    if spec['format']=='svg':
        if raw!=svg(spec).encode():raise ValueError('V2 vector source differs from its deterministic recipe')
        node=ET.fromstring(raw);dims=[int(node.attrib['width']),int(node.attrib['height'])]
    else:
        with Image.open(path) as image:
            dims=list(image.size)
            if image.format!=FORMATS[spec['format']] or getattr(image,'n_frames',1)!=1:
                raise ValueError('Wrong/static-only V2 input representation')
    if dims!=spec['dimensions']:raise ValueError('V2 source dimensions differ from declared workload')
    return dict(bytes=len(raw),sha256=sha256(raw),dimensions=dims,kind=spec['kind'],
                scale=spec['scale'],format=spec['format'])


def write_fixture(spec,root,photos):
    path=Path(root)/spec['name']
    if spec['format']=='svg':path.write_text(svg(spec))
    else:
        with build_image(spec,photos) as image:
            options={}
            if spec['format']=='jpg':options=dict(quality=90,subsampling=0,progressive=spec['kind']=='earth-progressive')
            elif spec['format']=='png':options=dict(compress_level=6)
            elif spec['format']=='webp':options=dict(quality=85,lossless=spec['kind']=='texture-lossless',method=4)
            elif spec['format']=='tiff':options=dict(compression='raw')
            elif spec['format']=='gif':options=dict(optimize=False)
            image.save(path,format=FORMATS[spec['format']],**options)
    path.chmod(0o644)
    return inspect_fixture(path,spec)


def verify(root):
    require_decoders();manifest=verify_fixtures(root)
    if (manifest.get('suite_id')!=SUITE or manifest.get('catalog_sha256')!=sha256(CATALOG.read_bytes())
            or manifest.get('generator',{}).get('pillow')!=PILLOW_VERSION
            or set(manifest['files'])!={s['name'] for s in fixture_specs()}):
        raise ValueError('V2 corpus/version/catalog binding differs')
    for spec in fixture_specs():
        if inspect_fixture(Path(root)/spec['name'],spec)!=manifest['files'][spec['name']]:
            raise ValueError('V2 source metadata/hash mismatch')
    verify_catalog(CATALOG,manifest)
    return manifest


def prepare(root,cache):
    import PIL
    from PIL import features
    require_decoders();verify_catalog()
    root,cache=Path(root),Path(cache);root.mkdir(parents=True,exist_ok=True);cache.mkdir(parents=True,exist_ok=True)
    photos={s['name']:normalized_photo(download(s,cache)) for s in SOURCES}
    manifest=dict(schema_version=1,suite_id=SUITE,catalog_sha256=sha256(CATALOG.read_bytes()),
                  sources=SOURCES,files={},generator=dict(pillow=PIL.__version__,
                  jpeg=features.version_codec('jpg'),zlib=features.version_codec('zlib'),webp=features.version('webp')))
    try:
        for i,spec in enumerate(fixture_specs(),1):
            print(f'Fixture {i}/120 {spec["name"]}',flush=True)
            manifest['files'][spec['name']]=write_fixture(spec,root,photos)
    finally:
        for image in photos.values():image.close()
    manifest['corpus_sha256']=corpus_hash(manifest)
    (root/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    attribution='# Benchmark v2 corpus\n\nPhotos converted to sRGB and cropped/resized/re-encoded without inherited metadata. '
    attribution+='Static synthetic patterns use fixed recipes/seeds; no animation, HDR or high-precision claim.\n\n'
    for source in SOURCES:attribution+=f'- {source["credit"]}: {source["page"]}; {source["license"]}, {source["license_url"]}\n'
    attribution+='\nFronalpstock derivatives: CC BY-SA 3.0. Synthetic inputs: MIT. Prepared media stay outside the repository.\n'
    (root/'ATTRIBUTION.md').write_text(attribution);root.chmod(0o755)
    verify(root);print('V2 shared corpus '+manifest['corpus_sha256']);return manifest


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--cache',type=Path);parser.add_argument('--check',action='store_true');args=parser.parse_args()
    if args.check:verify(args.output)
    elif args.cache:prepare(args.output,args.cache)
    else:parser.error('--cache is required for preparation')


if __name__=='__main__':main()
