#!/usr/bin/env python3
"""Exercise the native runtime image with its deployment restrictions enabled."""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid


def check(args):
    def docker(*commands):
        return subprocess.check_output([args.docker, *commands], stderr=subprocess.STDOUT).decode().strip()
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    def get(path, authorized=True):
        headers = {'Host': 'localhost:8080'}
        if authorized:
            headers['Authorization'] = 'Bearer synthetic-secret'
        request = urllib.request.Request(f'http://127.0.0.1:{port}' + path, headers=headers)
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.status, response.headers, response.read()
    name = 'pixstride-check-' + uuid.uuid4().hex[:12]
    with tempfile.TemporaryDirectory(prefix='pixstride-container-') as directory:
        root = Path(directory)
        root.chmod(0o755)  # The image's non-root uid reads only this public fixture.
        (root / 'sample.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48"><rect width="64" height="48" fill="#247ac0"/></svg>')
        hosts = '127.0.0.1'
        if args.remote_url:
            remote = urllib.parse.urlsplit(args.remote_url)
            if remote.scheme != 'https' or not remote.hostname or remote.username or remote.password:
                raise ValueError('The optional remote fixture must be a public HTTPS URL')
            hosts += ',' + remote.hostname
        try:
            docker('run', '-d', '--name', name, '--read-only', '--cap-drop=ALL',
                   '--security-opt=no-new-privileges', '--memory=512m', '--pids-limit=64', '--cpus=2',
                   '-p', f'127.0.0.1:{port}:8080', '-v', str(root) + ':/images:ro',
                   '-e', 'PIXSTRIDE_ALLOW_UNSAFE=1', '-e', 'PIXSTRIDE_ENGINE=native',
                   '-e', 'PIXSTRIDE_LOCAL_ROOT=/images', '-e', 'PIXSTRIDE_ALLOWED_HOSTS=' + hosts,
                   '-e', 'PIXSTRIDE_SECRET=synthetic-secret', args.image)
            for _ in range(100):
                try:
                    if get('/health')[0] == 200:
                        break
                except (OSError, urllib.error.URLError):
                    time.sleep(0.1)
            else:
                raise AssertionError('Container readiness failed')
            for fmt, mime in [('png', 'image/png'), ('avif', 'image/avif'), ('heic', 'image/heic'), ('jxl', 'image/jxl')]:
                status, headers, body = get('/unsafe/rs:fit:32:24/plain/local:///sample.svg@' + fmt)
                assert status == 200 and headers['Content-Type'] == mime and len(body) > 20, fmt
                assert headers['X-Image-Width'] == '32' and headers['X-Image-Height'] == '24'
            for path, authorized, expected in [('/health', False, 401), ('/unsafe/plain/http://127.0.0.1/fixture@png', True, 502)]:
                try:
                    get(path, authorized)
                    raise AssertionError('Restricted request was accepted')
                except urllib.error.HTTPError as error:
                    assert error.code == expected
            docker('exec', name, 'pixstride', 'healthcheck')
            assert b'pixstride_requests_total' in get('/metrics')[2]
            capabilities = json.loads(docker('exec', name, 'pixstride', 'capabilities'))
            assert capabilities['native_codecs'] and capabilities['cloud_sources']
            if args.remote_url:
                encoded = urllib.parse.quote(args.remote_url, safe=':/')
                assert get('/unsafe/rs:fit:32:24/plain/' + encoded + '@png')[2].startswith(b'\x89PNG')
            print('Container restrictions, native PNG/AVIF/HEIC/JXL, authorization, SSRF refusal, health and metrics passed')
        finally:
            subprocess.run([args.docker, 'rm', '-f', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--image', default='pixstride:check', help='Use a local tag or the exact validated manifest reference')
    parser.add_argument('--docker', default='docker', help='Docker CLI executable')
    parser.add_argument('--remote-url', help='Optional public HTTPS image fixture; excluded from mandatory CI')
    check(parser.parse_args())
