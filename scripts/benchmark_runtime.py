"""Docker lifecycle and host accounting for one sequential Linux workload."""
import base64
import hashlib
import hmac
import http.client
import io
import json
from pathlib import Path
import platform
import re
import socket
import subprocess
import tempfile
import time

from benchmark_fixtures import MIMES, ROOT, sha256
from benchmark_metrics import cgroup_location

SECRET = 'synthetic-benchmark-secret'
KEY, SALT = bytes.fromhex('42' * 32), bytes.fromhex('37' * 16)
ENGINES = ('pixstride_default', 'pixstride_native', 'imgproxy')
MEMORY_BYTES = 1024 ** 3


def candidate_source(revision, repository=ROOT):
    """Describe a committed checkout, independently of its platform image ID."""
    if not re.fullmatch(r'[0-9a-f]{40}', revision):
        raise ValueError('Candidate revision must be a full Git commit SHA')
    def git(*args):
        return subprocess.check_output(['git', '-C', str(repository), *args], text=True).strip()
    if git('rev-parse', 'HEAD') != revision:
        raise ValueError('Candidate revision does not match the checkout')
    if (subprocess.run(['git', '-C', str(repository), 'diff', '--quiet', 'HEAD']).returncode
            or git('ls-files', '--others', '--exclude-standard')):
        raise ValueError('Candidate images require a clean committed source checkout')
    return {'revision': revision, 'tree': git('rev-parse', 'HEAD^{tree}'),
            'dockerfile_sha256': sha256((repository / 'Dockerfile').read_bytes()),
            'cargo_lock_sha256': sha256((repository / 'Cargo.lock').read_bytes())}


def validate_candidate_image(image, revision):
    source = image.get('candidate') or {}
    if (not re.fullmatch(r'sha256:[0-9a-f]{64}', image.get('reference', ''))
            or image.get('image_id') != image['reference']):
        raise ValueError('Candidate images must use their inspected immutable local image ID')
    for field, length in [('revision', 40), ('tree', 40), ('dockerfile_sha256', 64), ('cargo_lock_sha256', 64)]:
        if not re.fullmatch(r'[0-9a-f]{' + str(length) + '}', source.get(field, '')):
            raise ValueError('Candidate source provenance is missing or malformed')
    if (source['revision'] != revision or
            image.get('labels', {}).get('org.opencontainers.image.revision') != revision):
        raise ValueError('Candidate image, source checkout and harness must have the same revision')


def architecture(value=None):
    value = value or platform.machine()
    try:
        return {'x86_64': 'amd64', 'amd64': 'amd64', 'aarch64': 'arm64', 'arm64': 'arm64'}[value.lower()]
    except KeyError as error:
        raise ValueError('Only native Linux AMD64 and ARM64 are qualified') from error


def require_native_image(image_architecture, image_os, host_architecture=None):
    if image_os != 'linux' or architecture(image_architecture) != architecture(host_architecture):
        raise ValueError('Refuse a non-Linux or cross-architecture/emulated image')


def signed_path(case):
    path = '/' + (case['options'] + '/' if case['options'] else '')
    path += 'plain/local:///' + case['source'] + '@' + case['format']
    signature = base64.urlsafe_b64encode(hmac.new(KEY, SALT + path.encode(), hashlib.sha256).digest()).decode().rstrip('=')
    return '/' + signature + path


def validate_response(case, status, headers, body, expected_hash=None):
    if status != 200:
        raise ValueError(f'Expected HTTP 200, received {status}: ' + body[:160].decode(errors='replace'))
    mime = headers.get('content-type', '').split(';')[0].strip().lower()
    if mime != MIMES[case['format']]:
        raise ValueError('Output MIME differs from the requested format')
    digest = sha256(body)
    if expected_hash is not None:
        if digest != expected_hash:
            raise ValueError('A timed response differs from the decoded preflight response')
    else:
        from PIL import Image
        with Image.open(io.BytesIO(body)) as image:
            image.load()
            if list(image.size) != case['dimensions']:
                raise ValueError(f'Wrong dimensions: expected {case["dimensions"]}, got {list(image.size)}')
            if image.format != {'jpg': 'JPEG', 'png': 'PNG', 'webp': 'WEBP', 'bmp': 'BMP'}[case['format']]:
                raise ValueError('Encoded format does not match the HTTP media type')
    return {'bytes': len(body), 'sha256': digest, 'mime': mime, 'dimensions': case['dimensions']}


def counter_delta(before, after):
    if after['oom_kill'] != before['oom_kill']:
        raise ValueError('Container reported an OOM kill; benchmark is invalid')
    fields = ('cpu_usec', 'user_usec', 'system_usec', 'nr_throttled', 'throttled_usec')
    result = {name: after[name] - before[name] for name in fields}
    if any(value < 0 for value in result.values()):
        raise ValueError('Container accounting reset during a batch')
    return result


def request(port, path):
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=60)
    started = time.perf_counter_ns()
    try:
        connection.request('GET', path, headers={'Host': 'localhost:8080',
                           'Authorization': 'Bearer ' + SECRET, 'Connection': 'close'})
        response = connection.getresponse()
        body = response.read(64 * 1024 * 1024 + 1)
        elapsed = time.perf_counter_ns() - started
        if len(body) > 64 * 1024 * 1024:
            raise ValueError('Benchmark response exceeds the source/output ceiling')
        return response.status, {key.lower(): value for key, value in response.getheaders()}, body, elapsed
    finally:
        connection.close()


def _free_port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


class Docker:
    def __init__(self, executable='docker'):
        self.executable = executable

    def run(self, *arguments, timeout=180):
        result = subprocess.run([self.executable, *map(str, arguments)], capture_output=True,
                                text=True, timeout=timeout)
        if result.returncode:
            raise RuntimeError('Docker ' + arguments[0] + ' failed: ' + result.stderr[-2000:])
        return result.stdout.strip()

    def inspect(self, name):
        return json.loads(self.run('inspect', name))[0]

    def manifest(self, reference):
        # Preserve exact registry bytes; ordinary run() strips CLI whitespace.
        with tempfile.TemporaryFile() as output:
            result = subprocess.run([self.executable, 'buildx', 'imagetools', 'inspect', '--raw', reference],
                                    stdout=output, stderr=subprocess.PIPE, timeout=180)
            if result.returncode:
                raise RuntimeError('OCI inspection failed: ' + result.stderr[-2000:].decode(errors='replace'))
            output.seek(0)
            raw = output.read(2_000_001)
            if len(raw) > 2_000_000: raise ValueError('Oversized OCI manifest')
            return raw

    def image(self, reference, candidate_revision=None):
        if candidate_revision:
            if not re.fullmatch(r'sha256:[0-9a-f]{64}', reference):
                raise ValueError('Candidate images must be addressed by immutable local image ID')
            source = candidate_source(candidate_revision)
        else:
            if not re.search(r'@sha256:[0-9a-f]{64}$', reference):
                raise ValueError('Benchmark images must include immutable sha256 digests')
            self.run('pull', reference, timeout=600)
        data = json.loads(self.run('image', 'inspect', reference))[0]
        require_native_image(data['Architecture'], data['Os'])
        image = {'reference': reference, 'image_id': data['Id'], 'architecture': data['Architecture'],
                'os': data['Os'], 'repo_digests': data.get('RepoDigests', []),
                'labels': {key: value for key, value in (data['Config'].get('Labels') or {}).items()
                           if key.startswith('org.opencontainers.image.') and key.rsplit('.', 1)[-1] in
                           {'version', 'revision', 'source', 'created'}}}
        if candidate_revision:
            image['candidate'] = source
            validate_candidate_image(image, candidate_revision)
        return image

    def probe(self, reference, arguments, owner, product):
        name = f'pixstride-bench-{owner}-probe-{product}'
        try:
            return self.run('run', '--rm', '--name', name, '--network=none',
                            '--label', 'io.pixstride.benchmark.run=' + owner, reference, *arguments)
        finally:
            cleanup_container(self, name, owner)


def cleanup_container(docker, name, owner, diagnostics=False):
    try:
        info = docker.inspect(name)
    except RuntimeError:
        return  # --rm already removed a completed probe, or creation never succeeded.
    labels = info.get('Config', {}).get('Labels') or {}
    if labels.get('io.pixstride.benchmark.run') != owner:
        raise RuntimeError('Refuse to clean a container owned by another run')
    if diagnostics:
        try:
            print(docker.run('logs', '--tail', '20', name), flush=True)
        except RuntimeError:
            pass
    docker.run('rm', '-f', name)


class Server:
    def __init__(self, docker, engine, image, fixtures, cpus, owner, index):
        self.docker, self.engine, self.image = docker, engine, image
        self.fixtures, self.cpus, self.owner = Path(fixtures).resolve(), cpus, owner
        self.name = f'pixstride-bench-{owner}-{index}-{engine}'
        self.port = _free_port()
        self.group = None

    def __enter__(self):
        variables = {'GOMAXPROCS': '2', 'VIPS_CONCURRENCY': '1', 'MALLOC_ARENA_MAX': '2'}
        if self.engine == 'imgproxy':
            variables.update(IMGPROXY_BIND='0.0.0.0:8080', IMGPROXY_LOCAL_FILESYSTEM_ROOT='/images',
                             IMGPROXY_ALLOWED_SOURCES='local://', IMGPROXY_KEY=KEY.hex(),
                             IMGPROXY_SALT=SALT.hex(), IMGPROXY_SECRET=SECRET, IMGPROXY_LOG_LEVEL='error',
                             IMGPROXY_MAX_CLIENTS='1')
        else:
            variables.update(PIXSTRIDE_BIND='0.0.0.0', PIXSTRIDE_PORT='8080',
                             PIXSTRIDE_LOCAL_ROOT='/images', PIXSTRIDE_KEY=KEY.hex(),
                             PIXSTRIDE_SALT=SALT.hex(), PIXSTRIDE_SECRET=SECRET,
                             PIXSTRIDE_ALLOWED_SOURCES='local://*', PIXSTRIDE_NATIVE_CONCURRENCY='1')
            if self.engine == 'pixstride_native':
                variables['PIXSTRIDE_ENGINE'] = 'native'
        command = ['run', '-d', '--name', self.name, '--label', 'io.pixstride.benchmark.run=' + self.owner,
                   '--read-only', '--cap-drop=ALL', '--security-opt=no-new-privileges', '--no-healthcheck',
                   '--memory=' + str(MEMORY_BYTES), '--memory-swap=' + str(MEMORY_BYTES),
                   '--pids-limit=128', '--cpus=2', '--cpuset-cpus=' + ','.join(map(str, self.cpus)),
                   '-p', f'127.0.0.1:{self.port}:8080', '-v', str(self.fixtures) + ':/images:ro']
        for key, value in variables.items():
            command.extend(['-e', key + '=' + value])
        try:
            self.docker.run(*command, self.image)
            inspected = self.docker.inspect(self.name)
            self.group = cgroup_location(inspected['State']['Pid'], container_id=inspected['Id'])
            host = inspected['HostConfig']
            if host['Memory'] != MEMORY_BYTES or host['NanoCpus'] != 2_000_000_000 or host['PidsLimit'] != 128:
                raise ValueError('Docker did not apply the benchmark limits')
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                try:
                    status, _, _, _ = request(self.port, '/health')
                    if status == 200:
                        return self
                except (OSError, http.client.HTTPException):
                    pass
                if not self.docker.inspect(self.name)['State']['Running']:
                    raise RuntimeError('Server exited before readiness')
                time.sleep(.1)
            raise RuntimeError('Server readiness deadline exceeded')
        except BaseException as error:
            self.__exit__(type(error), error, error.__traceback__)
            raise

    def __exit__(self, kind, value, traceback):
        cleanup_container(self.docker, self.name, self.owner, diagnostics=kind is not None)
