#!/usr/bin/env python3
"""Exercise real native child processes with public synthetic fixtures only."""
import argparse
import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile
import zlib

from image_fixtures import png, read_png, animated_gif, read_gif, read_webp_animation, exif, rgb_icc


def run(binary, root, source, options='', output='png', success=True, env=None):
    path = '/unsafe/' + (options + '/' if options else '') + 'plain/local:///fixture@' + output
    target = root / ('out.' + output)
    variables = {k: v for k, v in os.environ.items() if not k.startswith(('PIXSTRIDE_', 'IMGPROXY_'))}
    variables.update(PIXSTRIDE_ENGINE='native')
    variables.update(env or {})
    result = subprocess.run([str(binary), 'process', str(source), str(target), path], env=variables,
                            capture_output=True, timeout=30)
    if success != (result.returncode == 0):
        raise AssertionError(f'{options}@{output}: unexpected exit status {result.returncode}: {result.stderr.decode(errors="replace")}')
    return target.read_bytes() if success else b''


def metadata_text(data):
    texts=[]
    for kind,body in read_png(data)[2]:
        if kind==b'iTXt':
            _,rest=body.split(b'\0',1)
            compressed=rest[0]
            _,rest=rest[2:].split(b'\0',1)
            _,text=rest.split(b'\0',1)
            texts.append(zlib.decompress(text) if compressed else text)
        elif kind==b'zTXt':
            _,rest=body.split(b'\0',1)
            texts.append(zlib.decompress(rest[1:]))
        elif kind==b'tEXt':
            texts.append(body)
    return b'\n'.join(texts)


def check(args):
    binary = args.binary.resolve()
    passed = []
    with tempfile.TemporaryDirectory(prefix='pixstride-native-check-') as temp:
        root = Path(temp)
        source = root / 'fixture.png'
        source.write_bytes(png(32, 24))
        for fmt in ('png', 'jpg', 'webp', 'avif', 'heic', 'jxl', 'gif', 'tiff', 'bmp', 'ico'):
            encoded = run(binary, root, source, output=fmt)
            intermediate = root / ('roundtrip.' + fmt)
            intermediate.write_bytes(encoded)
            decoded = run(binary, root, intermediate)
            assert read_png(decoded)[0][:2] == (32, 24), fmt
            passed.append('roundtrip-' + fmt)
        gray = root / 'gray16.png'
        values = [x * 2053 for x in range(16)]
        gray.write_bytes(png(16, 2, channels=1, depth=16, rows=[values, values]))
        preserved = read_png(run(binary, root, gray, options='ph:1'))
        assert preserved[0] == (16, 2, 1, 16), preserved[0]
        assert preserved[1] == values * 2, '16-bit samples changed on identity'
        assert read_png(run(binary, root, gray, options='ph:0'))[0][3] == 8
        passed.append('gray16-identity')
        color = root / 'rgb16.png'
        color.write_bytes(png(16, 8, depth=16))
        original = read_png(color.read_bytes())
        retained = read_png(run(binary, root, color, options='ph:1'))
        assert retained[:2] == original[:2], 'RGB16 identity changed precision or samples'
        assert read_png(run(binary, root, color, options='ph:1/rs:fit:8:4'))[0][3] == 16
        passed.append('rgb16-resize')
        alpha = root / 'rgba16.png'
        alpha.write_bytes(png(16, 8, channels=4, depth=16, rows=[[0,0,0,0]*16 for _ in range(8)]))
        flattened = read_png(run(binary, root, alpha, options='ph:1/bg:ffffff'))
        assert flattened[0] == (16,8,3,16) and set(flattened[1]) == {65535}, 'HDR background lost range'
        passed.append('rgba16-background')
        gray_alpha=root/'gray-alpha.png'
        gray_alpha.write_bytes(png(4,4,channels=2,depth=16,rows=[[0,0]*4]*4))
        red=read_png(run(binary,root,gray_alpha,options='ph:1/bg:ff0000'))
        assert red[0]==(4,4,3,16) and red[1]==[65535,0,0]*16, 'Colored grayscale background lost hue'
        logo = root / 'logo.png'
        logo.write_bytes(png(2,2,channels=4,rows=[[255,0,0,255]*2]*2))
        marked = read_png(run(binary, root, color, options='ph:1/wm:1', env={'PIXSTRIDE_WATERMARK_PATH':str(logo)}))
        assert marked[0][3] == 16 and 65535 in marked[1], 'HDR watermark lost precision'
        inline = read_png(run(binary,root,color,options='ph:1/wm:1',env={'PIXSTRIDE_WATERMARK_DATA':base64.b64encode(logo.read_bytes()).decode()}))
        assert inline[:2] == marked[:2], 'Base64 watermark differs from file input'
        run(binary, root, color, options='ph:1', output='jpg', success=False)
        passed.append('hdr-watermark-and-explicit-format-boundary')
        animation = root / 'animation.gif'
        animation.write_bytes(animated_gif())
        _, delays, loop = read_gif(run(binary, root, animation, options='maf:2', output='gif'))
        assert delays == [50, 100] and loop == 2, (delays, loop)
        delays, loop = read_webp_animation(run(binary, root, animation, options='maf:2/q:100', output='webp'))
        assert delays == [50, 100] and loop == 3, (delays, loop)
        assert len(read_gif(run(binary, root, animation, output='gif'))[1]) == 1
        passed.append('animation-timing-and-loops')
        xmp = b'<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" dc:rights="Public copyright" dc:description="private-location"/></rdf:RDF>'
        source.write_bytes(png(32, 24, extra=[(b'iTXt', b'XML:com.adobe.xmp\0\0\0\0\0' + xmp)]))
        retained = metadata_text(run(binary, root, source, options='kcr:1'))
        assert b'Public copyright' in retained and b'private-location' not in retained
        assert b'Public copyright' not in metadata_text(run(binary, root, source, options='kcr:0'))
        assert b'private-location' in metadata_text(run(binary, root, source, options='sm:0'))
        passed.append('copyright-privacy')
        source.write_bytes(png(32,24,extra=[(b'eXIf',exif())]))
        kept = read_png(run(binary,root,source,options='kcr:1'))
        assert kept[0][:2] == (24,32), 'EXIF orientation not applied'
        tags = b''.join(body for kind,body in kept[2] if kind==b'eXIf')
        assert b'fixture-copyright' in tags and b'fixture-artist' in tags and b'private-description' not in tags
        assert read_png(run(binary,root,source,options='ar:0'))[0][:2] == (32,24)
        passed.append('exif-orientation-and-copyright-privacy')
        profile=rgb_icc()
        source.write_bytes(png(32,24,extra=[(b'iCCP',b'synthetic\0\0'+zlib.compress(profile))]))
        kept=read_png(run(binary,root,source,options='scp:0'))
        icc=next(body for kind,body in kept[2] if kind==b'iCCP')
        assert zlib.decompress(icc.split(b'\0',1)[1][1:])==profile
        assert kept[1]==read_png(source.read_bytes())[1], 'Profile retention changed source samples'
        converted=read_png(run(binary,root,source,options='scp:1'))
        assert not any(kind==b'iCCP' for kind,_ in converted[2])
        assert converted[1]!=kept[1], 'ICC conversion did not change gamma-encoded samples'
        passed.append('icc-retention-and-colour-conversion')
        source.write_bytes(png(64,32))
        palette = run(binary,root,source,options='q:75',env={'PIXSTRIDE_PNG_QUANTIZE':'1'})
        assert palette[25] == 3 and palette[24] <= 8, 'PNG quantization did not produce a palette'
        intermediate=root/'palette.png';intermediate.write_bytes(palette)
        assert read_png(run(binary,root,intermediate))[0][:2] == (64,32)
        low=len(run(binary,root,source,options='q:10',output='jpg'))
        high=len(run(binary,root,source,options='q:80',output='jpg'))
        limit=(low+high)//2
        bounded=run(binary,root,source,options=f'q:80/mb:{limit}',output='jpg')
        assert 0<len(bounded)<=limit
        passed.append('png-quantization-and-bounded-quality-search')
        svg = root / 'fixture.svg'
        svg.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24"><defs><linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient></defs><rect width="32" height="24" fill="url(#g)"/></svg>')
        assert read_png(run(binary, root, svg))[0][:2] == (32, 24)
        for content in ('<g transform="translate(4 4)"><path d="M0 0H16V12H0Z" fill="red"/></g>', '<defs><mask id="m"><rect width="16" height="24" fill="white"/></mask></defs><rect width="32" height="24" fill="blue" mask="url(#m)"/>', '<text x="2" y="20" font-family="sans-serif" font-size="18" fill="green">X</text>'):
            svg.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24">'+content+'</svg>')
            raster=read_png(run(binary,root,svg))
            assert raster[0][:2]==(32,24) and any(raster[1]), 'Static SVG feature rendered empty'
        for content in ('<script/>', '<image href="file:///etc/passwd"/>', '<style>@import "https://example.invalid/x.css";</style>', '<style>rect{}<!--split-->@import "https://example.invalid/x.css";</style>'):
            svg.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24">' + content + '</svg>')
            run(binary, root, svg, success=False)
        passed.append('static-svg-boundary')
        source.write_bytes(png(32, 24))
        run(binary, root, source, options='mb:1', output='jpg', success=False)
        run(binary, root, source, options='dpr:100000/el:1', success=False)
        corrupt = bytearray(source.read_bytes())
        corrupt[29] ^= 1
        source.write_bytes(corrupt)
        run(binary, root, source, success=False)
        passed.append('resource-and-malformed-input-rejection')
    print(f'{len(passed)} native checks passed: ' + ', '.join(passed))
    if args.report:
        args.report.write_text(json.dumps({'passed': passed}, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--report', type=Path)
    check(parser.parse_args())
