"""Verify exact published OCI index/platform bytes independently of harness source."""
import hashlib
import json
import re

from release_policy import version_key

MAX_MANIFEST = 2_000_000


def identity(pins):
    try:
        result = {key: pins[key] for key in ('version', 'revision', 'image')}
        version_key(result['version'])
        if (not re.fullmatch(r'[0-9a-f]{40}', result['revision'])
                or not re.fullmatch(r'[^\s@]+@sha256:[0-9a-f]{64}', result['image'])):
            raise ValueError('Invalid immutable release identity')
        return result
    except (KeyError, TypeError) as error:
        raise ValueError('Missing/invalid published release identity') from error


def manifest(raw, digest):
    if isinstance(raw, str): raw = raw.encode()
    if not isinstance(raw, bytes) or len(raw) > MAX_MANIFEST:
        raise ValueError('Invalid/oversized OCI manifest')
    # buildx can append one CLI newline. Never reserialize/normalize OCI bytes.
    candidates = [raw, raw[:-1]] if raw.endswith(b'\n') else [raw]
    for value in candidates:
        if 'sha256:' + hashlib.sha256(value).hexdigest() == digest:
            try:
                text = value.decode()
                parsed = json.loads(text)
                if parsed['schemaVersion'] != 2: raise ValueError('Unsupported OCI schema')
                return text, parsed
            except (UnicodeError, json.JSONDecodeError, KeyError, TypeError) as error:
                raise ValueError('Invalid OCI manifest JSON') from error
    raise ValueError('Published OCI manifest digest mismatch')


def platform(index, image):
    matches = [entry for entry in index['manifests']
               if entry.get('platform', {}).get('architecture') == image['architecture']
               and entry.get('platform', {}).get('os') == image['os']
               and entry.get('annotations', {}).get('vnd.docker.reference.type') != 'attestation-manifest']
    if len(matches) != 1 or not re.fullmatch(r'sha256:[0-9a-f]{64}', matches[0]['digest']):
        raise ValueError('Missing/ambiguous native release platform')
    return matches[0]['digest']


def validate_release_image(image, expected=None):
    try:
        proof = image['release']
        actual = identity({**proof, 'image': image['reference']})
        if image.get('candidate') or (expected is not None and actual != identity(expected)):
            raise ValueError('Published release cannot use candidate/different image provenance')
        if image.get('capabilities', {}).get('version') != actual['version']:
            raise ValueError('Runtime capability version differs from release')
        if image['os'] != 'linux' or image['architecture'] not in ('amd64', 'arm64'):
            raise ValueError('Published comparison requires native Linux AMD64/ARM64')
        _, index = manifest(proof['index_manifest'], image['reference'].rsplit('@', 1)[1])
        annotations = index['annotations']
        if (annotations['org.opencontainers.image.version'] != actual['version']
                or annotations['org.opencontainers.image.revision'] != actual['revision']):
            raise ValueError('OCI index version/revision differs from published release')
        _, child = manifest(proof['platform_manifest'], platform(index, image))
        if (not re.fullmatch(r'sha256:[0-9a-f]{64}', image['image_id'])
                or child['config']['digest'] != image['image_id']):
            raise ValueError('Native image config does not belong to the pinned platform manifest')
        return actual
    except (KeyError, TypeError, IndexError) as error:
        raise ValueError('Missing/invalid published image proof') from error


def capture_release(docker, image, pins):
    expected = identity(pins)
    if image['reference'] != expected['image']:
        raise ValueError('Published image differs from selected release pins')
    index_text, index = manifest(docker.manifest(image['reference']), image['reference'].rsplit('@', 1)[1])
    child_ref = image['reference'].split('@')[0] + '@' + platform(index, image)
    child_text, _ = manifest(docker.manifest(child_ref), platform(index, image))
    proof = dict(version=expected['version'], revision=expected['revision'],
                 index_manifest=index_text, platform_manifest=child_text)
    validate_release_image({**image, 'release': proof}, expected)
    image['release'] = proof
