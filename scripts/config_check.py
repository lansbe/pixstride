#!/usr/bin/env python3
"""Synthetic secret-manager bootstrap, independent signatures and fail-closed checks."""
import argparse
import base64
import hashlib
import hmac
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import re
import subprocess
import tempfile
import threading
from urllib.parse import parse_qs
from compatibility import environment


def crc32c(data):
    value = 0xffffffff
    for byte in data:
        value ^= byte
        for _ in range(8):
            value = (value >> 1) ^ (0x82f63b78 if value & 1 else 0)
    return value ^ 0xffffffff


def check(binary):
    binary = binary.resolve()
    capabilities = json.loads(subprocess.check_output([str(binary), 'capabilities'], env=environment()))
    if not capabilities.get('cloud_config'):
        print('Cloud configuration is not compiled')
        return
    seen, errors = [], []
    corrupt = False

    class Secrets(BaseHTTPRequestHandler):
        def reply(self, data, status=200):
            data = json.dumps(data).encode()
            self.send_response(status)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_POST(self):
            try:
                body = self.rfile.read(int(self.headers['Content-Length']))
                assert len(body) < 65536
                headers = {k.lower(): v for k, v in self.headers.items()}
                if self.path == '/oauth':
                    fields = json.loads(body) if headers.get('content-type','').startswith('application/json') else parse_qs(body.decode())
                    fields = {k: [v] if isinstance(v,str) else v for k,v in fields.items()}
                    assert fields['grant_type'] == ['refresh_token'] and fields['refresh_token'] == ['synthetic-refresh-token']
                    assert fields['client_id'] == ['synthetic-google-client'] and fields['client_secret'] == ['synthetic-google-client-secret']
                    seen.append((self.path, {}))
                    self.reply({'access_token':'synthetic-google-bootstrap', 'expires_in':3600, 'token_type':'Bearer'})
                    return
                match = re.fullmatch(r'AWS4-HMAC-SHA256 Credential=([^/]+)/([^,]+), SignedHeaders=([^,]+), Signature=([a-f0-9]{64})', headers['authorization'])
                assert match
                access, scope, signed, signature = match.groups()
                assert access == 'synthetic-bootstrap-access' and headers['x-amz-security-token'] == 'synthetic-bootstrap-token'
                date, region, service, terminator = scope.split('/')
                assert region == 'us-west-1' and service in ('secretsmanager', 'ssm') and terminator == 'aws4_request'
                digest = hashlib.sha256(body).hexdigest()
                assert headers['x-amz-content-sha256'] == digest
                canonical_headers = ''.join(k + ':' + ' '.join(headers[k].split()) + '\n' for k in signed.split(';'))
                canonical = 'POST\n' + self.path + '\n\n' + canonical_headers + '\n' + signed + '\n' + digest
                message = 'AWS4-HMAC-SHA256\n' + headers['x-amz-date'] + '\n' + scope + '\n' + hashlib.sha256(canonical.encode()).hexdigest()
                key = b'AWS4synthetic-bootstrap-secret'
                for value in (date, region, service, terminator):
                    key = hmac.new(key, value.encode(), hashlib.sha256).digest()
                assert hmac.new(key, message.encode(), hashlib.sha256).hexdigest() == signature
                request = json.loads(body)
                seen.append((self.path, request))
                if self.path == '/aws':
                    assert request['SecretId'] == 'synthetic-config'
                    assert request.get('VersionStage') == 'AWSCURRENT'
                    doc = ('PIXSTRIDE_KEY=' + '42'*32 + '\nPIXSTRIDE_SALT=' + '37'*16 + '\n'
                           'PIXSTRIDE_ENV_AWS_SSM_PARAMETERS_PATH=/synthetic\n'
                           'PIXSTRIDE_ENV_GCP_SECRET_ID=configuration\nPIXSTRIDE_ENV_GCP_SECRET_PROJECT_ID=synthetic-project\n')
                    self.reply({'SecretString': doc})
                else:
                    assert self.path == '/ssm' and request['WithDecryption'] is True and request['Path'] == '/synthetic'
                    if request.get('NextToken'):
                        assert request['NextToken'] == 'synthetic-page-two'
                        self.reply({'Parameters': [{'Name':'/synthetic/PIXSTRIDE_TTL', 'Value':'12'}]})
                    else:
                        self.reply({'Parameters': [{'Name':'/synthetic/PIXSTRIDE_KEY', 'Value':'43'*32}], 'NextToken':'synthetic-page-two'})
            except Exception as error:
                errors.append(str(error))
                self.reply({}, 400)

        def do_GET(self):
            try:
                assert self.headers['Authorization'] == 'Bearer synthetic-google-bootstrap'
                assert self.path == '/gcp/v1/projects/synthetic-project/secrets/configuration/versions/latest:access'
                seen.append((self.path, {}))
                data = ('PIXSTRIDE_KEY=' + '44'*32 + '\n').encode()
                self.reply({'payload': {'data':base64.b64encode(data).decode(), 'dataCrc32c':str(crc32c(data) ^ int(corrupt))}})
            except Exception as error:
                errors.append(str(error))
                self.reply({}, 400)

        def log_message(self, *_):
            pass

    origin = ThreadingHTTPServer(('127.0.0.1', 0), Secrets)
    thread = threading.Thread(target=origin.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix='pixstride-config-check-') as temporary:
            root = Path(temporary)
            secret = root/'aws-secret'
            secret.write_text('synthetic-bootstrap-secret\n')
            local = root/'local.env'
            local.write_text('PIXSTRIDE_KEY='+'45'*32+'\n')
            base = f'http://127.0.0.1:{origin.server_port}'
            variables = {**environment(), 'PIXSTRIDE_ENV_AWS_SECRET_ID':'synthetic-config',
                         'PIXSTRIDE_ENV_AWS_SECRET_VERSION_STAGE':'AWSCURRENT', 'PIXSTRIDE_ENV_AWS_SECRET_ENDPOINT':base+'/aws',
                         'PIXSTRIDE_ENV_AWS_SSM_PARAMETERS_ENDPOINT':base+'/ssm', 'PIXSTRIDE_ENV_GCP_ENDPOINT':base+'/gcp/',
                         'PIXSTRIDE_ENV_GCP_ACCESS_TOKEN':'synthetic-google-bootstrap', 'AWS_ACCESS_KEY_ID':'synthetic-bootstrap-access',
                         'AWS_SECRET_ACCESS_KEY_FILE':str(secret), 'AWS_SESSION_TOKEN':'synthetic-bootstrap-token'}
            variables.pop('AWS_SECRET_ACCESS_KEY', None)
            path = '/rs:fit:2:2/plain/https://example.test/image.png@png'
            def run(extra=None):
                result = subprocess.run([str(binary), 'sign', path], env={**variables, **(extra or {})}, capture_output=True, timeout=20)
                assert all(s not in result.stderr for s in (b'synthetic-bootstrap-secret', b'synthetic-google-bootstrap', b'PIXSTRIDE_KEY='))
                return result
            def expected(key):
                return '/' + base64.urlsafe_b64encode(hmac.new(bytes.fromhex(key*32),bytes.fromhex('37'*16)+path.encode(),hashlib.sha256).digest()).rstrip(b'=').decode() + path
            result = run()
            assert result.returncode == 0, result.stderr.decode()
            assert result.stdout.decode().strip() == expected('44')
            assert len(seen) == 4 and not errors, (seen, errors)
            result = run({'PIXSTRIDE_ENV_LOCAL_FILE_PATH':str(local)})
            assert result.returncode == 0 and result.stdout.decode().strip() == expected('45')
            google = root/'google.json'
            google.write_text(json.dumps({'type':'authorized_user', 'client_id':'synthetic-google-client',
                                         'client_secret':'synthetic-google-client-secret', 'refresh_token':'synthetic-refresh-token'}))
            saved_token = variables.pop('PIXSTRIDE_ENV_GCP_ACCESS_TOKEN')
            variables['PIXSTRIDE_ENV_GCP_TOKEN_ENDPOINT'] = base+'/oauth'
            result = run({'PIXSTRIDE_ENV_GCP_KEY_FILE':str(google)})
            assert result.returncode == 0 and result.stdout.decode().strip() == expected('44'), result.stderr.decode()
            result = run({'GOOGLE_APPLICATION_CREDENTIALS':str(google)})
            assert result.returncode == 0 and result.stdout.decode().strip() == expected('44'), result.stderr.decode()
            assert sum(path == '/oauth' for path, _ in seen) == 2
            variables['PIXSTRIDE_ENV_GCP_ACCESS_TOKEN'] = saved_token
            corrupt = True
            assert run().returncode != 0, 'Corrupt GCP secret checksum was accepted'
            corrupt = False
            local.write_text('LD_PRELOAD=/untrusted\n')
            assert run({'PIXSTRIDE_ENV_LOCAL_FILE_PATH':str(local)}).returncode != 0
            local.write_bytes(b'x'*65537)
            assert run({'PIXSTRIDE_ENV_LOCAL_FILE_PATH':str(local)}).returncode != 0
            assert not errors, errors
    finally:
        origin.shutdown()
        origin.server_close()
        thread.join()
    print('AWS Secrets Manager/SSM SigV4, pagination, GCP CRC32C, staged precedence, credential files and failure privacy passed')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    check(parser.parse_args().binary)
