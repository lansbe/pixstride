#!/usr/bin/env python3
"""Compare synthetic images against a pinned imgproxy executable; no private corpus."""
import argparse
import binascii
import hashlib
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import zlib
from image_fixtures import read_png, png as fixture_png

ROOT = Path(__file__).resolve().parents[1]


def png(width=64, height=40):
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', binascii.crc32(kind + body))
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            rows.extend(((x * 3 + y) % 256, (y * 5 + x) % 256, 180 if x < width // 3 else 40))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


def get(port, path, authorized=True):
    request = urllib.request.Request(f'http://127.0.0.1:{port}{path}', headers={'Host': f'localhost:{port}', **({'Authorization':'Bearer synthetic-secret'} if authorized else {})})
    try:
        with urllib.request.urlopen(request, timeout=30) as r:
            return r.status, r.headers.get('Content-Type', ''), r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.headers.get('Content-Type', ''), e.read()


def environment():
    return {k: v for k, v in os.environ.items() if not k.startswith(('PIXSTRIDE_', 'IMGPROXY_'))}


def start(binary, directory, prefix, log):
    port = free_port()
    env = environment()
    if prefix == 'PIXSTRIDE':
        env.update(PIXSTRIDE_BIND='127.0.0.1', PIXSTRIDE_PORT=str(port), PIXSTRIDE_ALLOW_UNSAFE='1',
                   PIXSTRIDE_LOCAL_ROOT=str(directory), PIXSTRIDE_ENGINE='native',PIXSTRIDE_WATERMARK_PATH=str(directory/'watermark.png'))
    else:
        env.update(IMGPROXY_BIND=f'127.0.0.1:{port}', IMGPROXY_LOCAL_FILESYSTEM_ROOT=str(directory),
                   IMGPROXY_ALLOWED_SOURCES='local://', IMGPROXY_LOG_LEVEL='error',IMGPROXY_ALLOW_SECURITY_OPTIONS='true',IMGPROXY_WATERMARK_PATH=str(directory/'watermark.png'))
    env.update({prefix+'_KEY':'42'*32,prefix+'_SALT':'37'*16,prefix+'_TRUSTED_SIGNATURES':'unsafe',prefix+'_SECRET':'synthetic-secret'})
    process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
    try:
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f'{prefix} exited before readiness')
            try:
                if get(port, '/health')[0] == 200:
                    return process, port
            except (OSError, urllib.error.URLError):
                pass
            time.sleep(0.05)
        raise RuntimeError(f'{prefix} did not become ready')
    except BaseException:
        process.kill()
        process.wait()
        raise


def pixels(binary, encoded, extension, root, name):
    source, output = root / f'{name}.{extension}', root / f'{name}.png'
    source.write_bytes(encoded)
    result = subprocess.run([str(binary), 'process', str(source), str(output),
                             '/unsafe/plain/local:///fixture@png'], env={**environment(), 'PIXSTRIDE_ENGINE': 'native'},
                            capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError(f'Independent output decoding failed for {name}')
    (width,height,channels,depth),values,_=read_png(output.read_bytes())
    if depth!=8:
        raise ValueError('Expected eight-bit comparison samples')
    rgba=bytearray()
    for at in range(0,len(values),channels):
        color=values[at:at+channels]
        rgba.extend(([color[0]]*3 if channels<3 else color[:3])+([color[-1]] if channels in (2,4) else [255]))
    return (width,height),rgba


def compare(args):
    cases = json.loads((ROOT / '.github/checks/compatibility/cases.json').read_text())
    reference = json.loads((ROOT / '.github/checks/compatibility/reference.json').read_text())
    binary, upstream = args.binary.resolve(), args.reference_binary.resolve()
    report = {'reference': reference, 'reference_executable_sha256':hashlib.sha256(upstream.read_bytes()).hexdigest(), 'cases': []}
    with tempfile.TemporaryDirectory(prefix='pixstride-compatibility-') as temp:
        directory = Path(temp)
        (directory / 'fixture.png').write_bytes(png())
        (directory / 'watermark.png').write_bytes(png(8,4))
        (directory / 'alpha.png').write_bytes(fixture_png(32,24,channels=4))
        processes = []
        with (directory / 'server.log').open('wb') as log:
            try:
                pix, port = start(binary, directory, 'PIXSTRIDE', log)
                processes.append(pix)
                ref, ref_port = start(upstream, directory, 'IMGPROXY', log)
                processes.append(ref)
                for case in cases:
                    path = '/' + case.get('signature','unsafe') + '/' + (case['options'] + '/' if case['options'] else '') + 'plain/local:///'+case.get('source','fixture.png')+'@' + case['format']
                    actual, expected = get(port, path,case.get('authorized',True)), get(ref_port, path,case.get('authorized',True))
                    entry = {'name': case['name'], 'status': actual[0], 'reference_status': expected[0], 'pass': False}
                    if actual[0] != 200:
                        entry['error'] = actual[2].decode('utf-8', errors='replace')[:256]
                    if actual[0] == expected[0] == 200:
                        ad, ab = pixels(binary, actual[2], case['format'], directory, 'actual')
                        ed, eb = pixels(binary, expected[2], case['format'], directory, 'expected')
                        entry.update(dimensions=ad, reference_dimensions=ed, bytes=len(actual[2]), reference_bytes=len(expected[2]))
                        if ad == ed and len(ab) == len(eb):
                            errors = [abs(a-b) for a, b in zip(ab, eb)]
                            maximum, mean = max(errors, default=0), sum(errors) / max(len(errors), 1)
                            entry.update(max_error=maximum, mean_error=mean,
                                         **{'pass': actual[1].split(';')[0] == expected[1].split(';')[0]
                                            and maximum <= case['max_error'] and mean <= case['mean_error']})
                    if case.get('status',200) != 200:
                        entry['pass'] = actual[0] == expected[0] == case['status']
                    report['cases'].append(entry)
            finally:
                for process in processes:
                    process.terminate()
                for process in processes:
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
    args.report.write_text(json.dumps(report, indent=2) + '\n')
    failures = [c['name'] for c in report['cases'] if not c['pass']]
    print(f"{len(cases)-len(failures)}/{len(cases)} comparisons passed")
    if failures:
        print('Differences: ' + ', '.join(failures))
        raise SystemExit(1)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--reference-binary', type=Path, required=True,
                        help='An executable built from reference.json/source_commit')
    parser.add_argument('--report', type=Path, required=True, help='Private, untracked output JSON')
    compare(parser.parse_args())
