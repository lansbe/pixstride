#!/usr/bin/env python3
"""Synthetic HTTP/Unix/OTLP integration and bounded soak checks; no external service."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
import http.client
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
from image_fixtures import png
from compatibility import environment, free_port


@contextmanager
def server(binary, root, extra=None):
    port = free_port()
    variables = environment()
    variables.update(PIXSTRIDE_BIND='127.0.0.1', PIXSTRIDE_PORT=str(port), PIXSTRIDE_ALLOW_UNSAFE='1',
                     PIXSTRIDE_LOCAL_ROOT=str(root), PIXSTRIDE_ENGINE='native')
    variables.update(extra or {})
    with (root / 'server.log').open('wb') as log:
        process = subprocess.Popen([str(binary)], env=variables, stdout=log, stderr=log)
        try:
            for _ in range(300):
                if process.poll() is not None:
                    raise AssertionError('Server exited before readiness')
                try:
                    health = variables.get('PIXSTRIDE_PATH_PREFIX','').rstrip('/') + variables.get('PIXSTRIDE_HEALTH_CHECK_PATH','/health')
                    result = request(port, health, headers={'Authorization': 'Bearer synthetic-secret'}, unix=variables.get('PIXSTRIDE_UNIX_SOCKET'))
                    if result[0] == 200:
                        break
                except (OSError, http.client.HTTPException):
                    pass
                time.sleep(0.05)
            else:
                raise AssertionError('Readiness deadline exceeded')
            yield port, process, variables
        finally:
            process.terminate()
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
                raise AssertionError('Graceful shutdown exceeded 15 seconds')


def request(port, path, method='GET', headers=None, unix=None):
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=20)
    if unix:
        connection.sock = socket.socket(socket.AF_UNIX)
        connection.sock.settimeout(20)
        connection.sock.connect(unix)
    connection.request(method, path, headers={'Host': f'localhost:{port}', **(headers or {})})
    response = connection.getresponse()
    result = response.status, {k.lower(): v for k, v in response.getheaders()}, response.read()
    connection.close()
    return result


def check(args):
    binary = args.binary.resolve()
    capabilities = json.loads(subprocess.check_output([str(binary), 'capabilities'], env=environment()))
    spans = []
    class Collector(BaseHTTPRequestHandler):
        def do_POST(self):
            assert self.path == '/v1/traces'
            length = int(self.headers['Content-Length'])
            assert length < 16384
            spans.append(json.loads(self.rfile.read(length)))
            self.send_response(200)
            self.send_header('Content-Length', '2')
            self.end_headers()
            self.wfile.write(b'{}')
        def log_message(self, *_):
            pass
    collector = HTTPServer(('127.0.0.1', 0), Collector)
    collector_thread = threading.Thread(target=collector.serve_forever, daemon=True)
    collector_thread.start()
    passed = []
    try:
        with tempfile.TemporaryDirectory(prefix='pixstride-server-check-') as temp:
            root = Path(temp)
            (root / 'nested').mkdir()
            (root / 'nested' / 'sample.png').write_bytes(png(32, 24, channels=4))
            settings = dict(PIXSTRIDE_SECRET='synthetic-secret', PIXSTRIDE_USE_ETAG='1',
                            PIXSTRIDE_USE_LAST_MODIFIED='1', PIXSTRIDE_TTL='3600', PIXSTRIDE_AUTO_WEBP='1',
                            PIXSTRIDE_PRESETS='thumb=rs:fit:16:12', PIXSTRIDE_ALLOW_ORIGIN='https://example.test')
            if capabilities.get('telemetry'):
                settings['PIXSTRIDE_OTLP_ENDPOINT'] = f'http://127.0.0.1:{collector.server_port}/v1/traces'
            auth = {'Authorization': 'Bearer synthetic-secret'}
            target = '/unsafe/pr:thumb/plain/local:///nested/sample.png'
            with server(binary, root, settings) as (port, process, variables):
                assert request(port, '/health')[0] == 401
                preflight=request(port,target,'OPTIONS',{'Origin':'https://example.test','Access-Control-Request-Method':'GET','Access-Control-Request-Headers':'authorization'})
                assert preflight[0]==204 and preflight[2]==b'' and 'authorization' in preflight[1]['access-control-allow-headers'].lower()
                assert request(port,target,'OPTIONS',{'Origin':'https://denied.test','Access-Control-Request-Method':'GET'})[0]==403
                assert request(port, target, 'HEAD')[2] == b''
                image = request(port, target, headers={**auth, 'Accept': 'image/webp'})
                assert image[0] == 200 and image[1]['content-type'] == 'image/webp', image[:2]
                assert image[1]['vary'] == 'Accept' and image[1]['x-image-width'] == '16'
                assert image[1]['access-control-allow-origin'] == 'https://example.test'
                assert request(port, target, headers={**auth, 'Accept': 'image/webp;q=0'})[1]['content-type'] == 'image/png'
                etag = image[1]['etag']
                assert request(port, target, headers={**auth, 'Accept': 'image/webp', 'If-None-Match': etag})[0] == 304
                head = request(port, target, 'HEAD', {**auth, 'Accept': 'image/webp'})
                assert head[0] == 200 and head[2] == b'' and int(head[1]['content-length']) == len(image[2])
                expiry = int(time.time()) + 30
                timed = request(port, f'/unsafe/exp:{expiry}/plain/local:///nested/sample.png@png', headers=auth)
                assert timed[0] == 200 and 0 < int(timed[1]['cache-control'].split('=')[1]) <= 30
                assert request(port, '/unsafe/exp:1/plain/local:///missing@png', headers=auth)[0] == 404
                result = subprocess.run([str(binary), 'healthcheck'], env=variables, capture_output=True, timeout=5)
                assert result.returncode == 0, result.stderr.decode(errors='replace')
                passed.append('http-auth-negotiation-validators-presets-expiry-head-health')
                with ThreadPoolExecutor(max_workers=4) as workers:
                    results = list(workers.map(lambda _: request(port, target, headers={**auth, 'Accept': 'image/webp'}), range(args.requests)))
                assert all(r[0] in (200, 503) for r in results)
                successes = [r for r in results if r[0] == 200]
                assert len(successes) >= args.requests // 2 and all(r[2] == image[2] for r in successes)
                metrics = request(port, '/metrics', headers=auth)[2].decode()
                assert 'pixstride_requests_total' in metrics
                assert all(value not in metrics for value in ('nested', 'sample.png', 'synthetic-secret', 'local://'))
                assert process.poll() is None
                passed.append('bounded-concurrent-soak-and-metric-privacy')
            fallback = root / 'fallback.png'
            fallback.write_bytes(png(20,12))
            metrics_port = free_port()
            policy = dict(PIXSTRIDE_SECRET='synthetic-secret',PIXSTRIDE_FALLBACK_IMAGE_PATH=str(fallback),
                          PIXSTRIDE_FALLBACK_IMAGE_TTL='30',PIXSTRIDE_FALLBACK_IMAGE_HTTP_CODE='404',
                          PIXSTRIDE_ENABLE_CLIENT_HINTS='1',PIXSTRIDE_ENFORCE_WEBP='1',
                          PIXSTRIDE_PATH_PREFIX='/images',PIXSTRIDE_HEALTH_CHECK_PATH='/ready',
                          PIXSTRIDE_PROMETHEUS_BIND=f'127.0.0.1:{metrics_port}',PIXSTRIDE_PROMETHEUS_NAMESPACE='synthetic')
            with server(binary,root,policy) as (port,_,variables):
                assert request(port,'/images/ready')[0]==401
                assert request(port,'/images/ready',headers=auth)[0]==200
                assert request(port,'/ready',headers=auth)[0]==400
                assert subprocess.run([str(binary),'healthcheck'],env=variables,capture_output=True,timeout=5).returncode==0
                missing='/images/unsafe/rs:fit:10:10/plain/local:///missing.png@png'
                image=request(port,missing,headers=auth)
                assert image[0]==404 and image[1]['content-type']=='image/png'
                assert image[1]['cache-control']=='public, max-age=30' and image[2].startswith(b'\x89PNG')
                assert request(port,missing)[0]==401, 'Fallback bypassed authorization'
                assert request(port,'/images/invalid/plain/local:///missing.png@png',headers=auth)[0]==400, 'Fallback bypassed URL authentication'
                hinted=request(port,'/images/unsafe/plain/local:///nested/sample.png@png',headers={**auth,'Accept':'image/webp','Sec-CH-DPR':'2','Sec-CH-Width':'16'})
                assert hinted[0]==200 and hinted[1]['content-type']=='image/webp'
                assert hinted[1]['x-image-width']=='16' and hinted[1]['content-dpr']=='2'
                assert 'Sec-CH-DPR' in hinted[1]['vary'] and 'Accept' in hinted[1]['vary']
                explicit=request(port,'/images/unsafe/w:12/dpr:1/plain/local:///nested/sample.png@png',headers={**auth,'Sec-CH-DPR':'2','Sec-CH-Width':'16'})
                assert explicit[1]['x-image-width']=='12' and explicit[1]['content-dpr']=='1', 'Hints overrode explicit URL options'
                assert request(metrics_port,'/metrics')[0]==401
                metric=request(metrics_port,'/metrics',headers=auth)
                assert metric[0]==200 and b'synthetic_requests_total' in metric[2]
                assert request(metrics_port,'/health',headers=auth)[0]==404
            passed.append('fallback-auth-client-hints-enforced-format-prefix-health-and-separate-metrics')
            unix = root / 'service.sock'
            with server(binary, root, {'PIXSTRIDE_UNIX_SOCKET': str(unix)}) as (port, _, variables):
                assert request(port, '/health', unix=str(unix))[0] == 200
                assert subprocess.run([str(binary), 'healthcheck'], env=variables, capture_output=True, timeout=5).returncode == 0
            assert not unix.exists(), 'Unix socket not cleaned on shutdown'
            passed.append('unix-health-and-graceful-cleanup')
            with server(binary,root,{'PIXSTRIDE_CONCURRENCY':'2','PIXSTRIDE_MEMORY_BUDGET_MIB':'3328'}) as (port,_,_):
                with ThreadPoolExecutor(max_workers=2) as workers:
                    parallel=list(workers.map(lambda _:request(port,'/unsafe/plain/local:///nested/sample.png@png'),range(20)))
                assert all(r[0]==200 for r in parallel)
                assert len({r[2] for r in parallel})==1
            passed.append('native-concurrency-with-explicit-memory-reservations')
            for extra in ({'PIXSTRIDE_ENGINE': 'invalid'}, {'PIXSTRIDE_CONCURRENCY': '2'}, {'PIXSTRIDE_PROCESS_TIMEOUT': '0'}, {'PIXSTRIDE_ENGINE':'native','PIXSTRIDE_CONCURRENCY':'2','PIXSTRIDE_MEMORY_BUDGET_MIB':'3327'}, {'PIXSTRIDE_MAX_CLIENTS':'17'}, {'PIXSTRIDE_MAX_CLIENTS':'5'}):
                variables = {**environment(), 'PIXSTRIDE_ALLOW_UNSAFE': '1', **extra}
                assert subprocess.run([str(binary)], env=variables, capture_output=True, timeout=5).returncode != 0
            passed.append('invalid-policy-fails-before-listening')
        if capabilities.get('telemetry'):
            assert spans, 'OTLP exporter sent no spans'
            for payload in spans:
                encoded = json.dumps(payload)
                assert all(value not in encoded for value in ('synthetic-secret', 'local://', 'sample.png'))
                span = payload['resourceSpans'][0]['scopeSpans'][0]['spans'][0]
                assert len(span['traceId']) == 32 and len(span['spanId']) == 16
                assert int(span['endTimeUnixNano']) >= int(span['startTimeUnixNano'])
            passed.append('otlp-http-json-and-attribute-privacy')
    finally:
        collector.shutdown()
        collector.server_close()
        collector_thread.join()
    print(f'{len(passed)} server checks passed ({args.requests} soak requests)')
    if args.report:
        args.report.write_text(json.dumps({'passed': passed, 'soak_requests': args.requests}, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--requests', type=int, default=100)
    parser.add_argument('--report', type=Path)
    arguments = parser.parse_args()
    if not 1 <= arguments.requests <= 10000:
        parser.error('--requests must be 1..10000')
    check(arguments)
