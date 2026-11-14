#!/usr/bin/env python3
"""Verify OTLP and vendor wire contracts against an isolated local collector."""
import argparse
import hashlib
import hmac
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path
import re
import subprocess
import tempfile
import threading
import time
from urllib.parse import parse_qs, urlsplit
from image_fixtures import png
from server_check import request, server
from compatibility import environment


def verify_cloudwatch(path, headers, body):
    auth = headers['authorization']
    match = re.fullmatch(r'AWS4-HMAC-SHA256 Credential=([^/]+)/([^,]+), SignedHeaders=([^,]+), Signature=([a-f0-9]{64})', auth)
    assert match, 'Malformed SigV4 authorization'
    access, scope, signed, signature = match.groups()
    assert access == 'synthetic-access'
    date, region, service, terminator = scope.split('/')
    assert region == 'us-west-1' and service == 'monitoring' and terminator == 'aws4_request'
    canonical_headers = ''.join(name + ':' + ' '.join(headers[name].split()) + '\n' for name in signed.split(';'))
    canonical = 'POST\n' + path + '\n\n' + canonical_headers + '\n' + signed + '\n' + hashlib.sha256(body).hexdigest()
    message = 'AWS4-HMAC-SHA256\n' + headers['x-amz-date'] + '\n' + scope + '\n' + hashlib.sha256(canonical.encode()).hexdigest()
    key = b'AWS4synthetic-cloudwatch-secret'
    for item in (date, region, service, terminator):
        key = hmac.new(key, item.encode(), hashlib.sha256).digest()
    assert hmac.new(key, message.encode(), hashlib.sha256).hexdigest() == signature, 'Invalid SigV4 signature'
    values = parse_qs(body.decode())
    assert values['Action'] == ['PutMetricData'] and values['Namespace'] == ['synthetic']
    assert values['MetricData.member.1.MetricName'] == ['Requests']


def check(binary):
    received, errors = [], []
    class Collector(BaseHTTPRequestHandler):
        def do_POST(self):
            try:
                length = int(self.headers['Content-Length'])
                assert 0 < length < 16384
                body = self.rfile.read(length)
                headers = {k.lower(): v for k, v in self.headers.items()}
                path = urlsplit(self.path).path
                if path == '/cloudwatch':
                    verify_cloudwatch(path, headers, body)
                    data = parse_qs(body.decode())
                elif path == '/api/123/envelope/':
                    lines = body.splitlines()
                    envelope, item, data = (json.loads(line) for line in lines)
                    assert item['type'] == 'event' and item['length'] == len(lines[2])
                    assert data['event_id'] == envelope['event_id']
                    assert 'sentry_key=synthetic' in headers['x-sentry-auth']
                else:
                    data = json.loads(body)
                    if path.startswith('/nr/'):
                        assert headers['api-key'] == 'synthetic-newrelic'
                    if path.startswith('/dd/'):
                        assert headers['dd-api-key'] == 'synthetic-datadog'
                    if path == '/honeybadger':
                        assert headers['x-api-key'] == 'synthetic-honeybadger'
                        assert data['error']['class'] == 'PixstrideError'
                    if path == '/bugsnag':
                        assert headers['bugsnag-payload-version'] == '5'
                        assert data['apiKey'] == 'synthetic-bugsnag' and data['events'][0]['exceptions']
                    if path.startswith('/airbrake/'):
                        assert parse_qs(urlsplit(self.path).query)['key'] == ['synthetic-airbrake']
                        assert data['errors'][0]['type'] == 'PixstrideError'
                assert all(value not in body for value in (b'synthetic-secret', b'local://', b'sample.png', b'private-source'))
                received.append((path, data))
                self.send_response(200)
            except Exception as error:
                errors.append(str(error))
                self.send_response(400)
            self.send_header('Content-Length', '2')
            self.end_headers()
            self.wfile.write(b'{}')
        def log_message(self, *_):
            pass
    collector = HTTPServer(('127.0.0.1', 0), Collector)
    worker = threading.Thread(target=collector.serve_forever, daemon=True)
    worker.start()
    base = f'http://127.0.0.1:{collector.server_port}'
    settings = dict(PIXSTRIDE_SECRET='synthetic-secret', PIXSTRIDE_OTLP_PROPAGATE_EXTERNAL='1',
                    PIXSTRIDE_OTLP_ENDPOINT=base+'/v1/traces', PIXSTRIDE_OTLP_METRICS_ENDPOINT=base+'/v1/metrics',
                    PIXSTRIDE_OTLP_LOGS_ENDPOINT=base+'/v1/logs',
                    PIXSTRIDE_NEW_RELIC_KEY='synthetic-newrelic', PIXSTRIDE_NEW_RELIC_ENDPOINT=base+'/nr',
                    PIXSTRIDE_DATADOG_KEY='synthetic-datadog', PIXSTRIDE_DATADOG_ENDPOINT=base+'/dd',
                    PIXSTRIDE_SENTRY_DSN=f'http://synthetic@127.0.0.1:{collector.server_port}/123',
                    PIXSTRIDE_HONEYBADGER_KEY='synthetic-honeybadger', PIXSTRIDE_HONEYBADGER_ENDPOINT=base+'/honeybadger',
                    PIXSTRIDE_BUGSNAG_KEY='synthetic-bugsnag', PIXSTRIDE_BUGSNAG_ENDPOINT=base+'/bugsnag',
                    PIXSTRIDE_AIRBRAKE_PROJECT_ID='123', PIXSTRIDE_AIRBRAKE_PROJECT_KEY='synthetic-airbrake', PIXSTRIDE_AIRBRAKE_ENDPOINT=base+'/airbrake',
                    PIXSTRIDE_CLOUD_WATCH_SERVICE_NAME='synthetic-service', PIXSTRIDE_CLOUD_WATCH_NAMESPACE='synthetic',PIXSTRIDE_CLOUD_WATCH_REGION='us-west-1',
                    PIXSTRIDE_CLOUD_WATCH_ENDPOINT=base+'/cloudwatch', PIXSTRIDE_CLOUD_WATCH_ACCESS_KEY_ID='synthetic-access',
                    PIXSTRIDE_CLOUD_WATCH_SECRET_ACCESS_KEY='synthetic-cloudwatch-secret')
    expected={'/v1/traces','/v1/metrics','/v1/logs','/nr/v1/traces','/nr/v1/metrics','/nr/v1/logs',
              '/dd/v1/traces','/dd/v1/metrics','/dd/v1/logs','/api/123/envelope/','/honeybadger','/bugsnag',
              '/airbrake/api/v3/projects/123/notices','/cloudwatch'}
    sampled = '0123456789abcdef0123456789abcdef'
    unsampled = '1123456789abcdef0123456789abcdef'
    try:
        with tempfile.TemporaryDirectory(prefix='pixstride-telemetry-check-') as temp:
            root = Path(temp)
            (root/'sample.png').write_bytes(png(16,16))
            with server(binary.resolve(), root, settings) as (port, _, _):
                auth = {'Authorization':'Bearer synthetic-secret'}
                assert request(port,'/unsafe/plain/local:///sample.png@png',headers={**auth,'traceparent':f'00-{sampled}-0123456789abcdef-01'})[0]==200
                failed = '/unsafe/plain/http://127.0.0.1/private-source.png@png'
                assert request(port,failed,headers=auth)[0]==502
                assert request(port,failed,headers={**auth,'traceparent':f'00-{unsampled}-0123456789abcdef-00'})[0]==502
                deadline = time.monotonic()+8
                while time.monotonic()<deadline and not expected.issubset({path for path,_ in received}):
                    time.sleep(0.05)
                assert not errors, errors
                assert expected.issubset({path for path,_ in received}), expected-{path for path,_ in received}
                traces = [span for path,data in received if path.endswith('/traces') for resource in data['resourceSpans'] for scope in resource['scopeSpans'] for span in scope['spans']]
                assert any(s['traceId']==sampled and s['parentSpanId']=='0123456789abcdef' for s in traces)
                assert all(s['traceId']!=unsampled for s in traces), 'Unsampled parent exported a trace'
                assert request(port,'/metrics',headers=auth)[2].find(b'pixstride_telemetry_failed_total 0')>=0
            capabilities=json.loads(subprocess.check_output([str(binary.resolve()),'capabilities'],env=environment()))
            if capabilities.get('cloud_sources'):
                credentials=root/'credentials'
                credentials.write_text('[synthetic]\naws_access_key_id=synthetic-access\naws_secret_access_key=synthetic-cloudwatch-secret\n')
                config=root/'config';config.write_text('')
                profile={k:v for k,v in settings.items() if k.startswith('PIXSTRIDE_CLOUD_WATCH_') and not k.endswith(('ACCESS_KEY_ID','SECRET_ACCESS_KEY'))}
                profile.update(AWS_PROFILE='synthetic',AWS_SHARED_CREDENTIALS_FILE=str(credentials),AWS_CONFIG_FILE=str(config))
                for name in ('AWS_ACCESS_KEY_ID','AWS_SECRET_ACCESS_KEY','AWS_SESSION_TOKEN'):
                    profile[name]=None;profile[name+'_FILE']=None
                previous=sum(path=='/cloudwatch' for path,_ in received)
                with server(binary.resolve(),root,profile) as (port,_,_):
                    assert request(port,'/unsafe/plain/local:///sample.png@png')[0]==200
                    deadline=time.monotonic()+8
                    while time.monotonic()<deadline and sum(path=='/cloudwatch' for path,_ in received)<=previous:time.sleep(0.05)
                    assert not errors and sum(path=='/cloudwatch' for path,_ in received)>previous,errors
        print('OTLP traces/metrics/logs, parent sampling, four notice APIs, Datadog/New Relic and CloudWatch SigV4/SDK profiles passed')
    finally:
        collector.shutdown()
        collector.server_close()
        worker.join()


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    check(parser.parse_args().binary)
