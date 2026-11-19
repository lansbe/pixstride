"""Published image proof binds OCI bytes, source and native config independently of the harness."""
import copy
import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
from benchmark_release import capture_release, validate_release_image
from benchmark_report import validate_report
from test_benchmark_report import measured_report


def release_fixture(architecture='amd64'):
    revision = '9' * 40
    manifests = {}
    for arch, config in [('amd64', '3'), ('arm64', '4')]:
        manifests[arch] = json.dumps({'schemaVersion': 2,
            'mediaType': 'application/vnd.oci.image.manifest.v1+json',
            'config': {'digest': 'sha256:' + config * 64}, 'layers': []}, sort_keys=True)
    digest = lambda raw: 'sha256:' + hashlib.sha256(raw.encode()).hexdigest()
    index = json.dumps({'schemaVersion': 2,
        'mediaType': 'application/vnd.oci.image.index.v1+json',
        'annotations': {'org.opencontainers.image.version': '0.4.0',
                        'org.opencontainers.image.revision': revision},
        'manifests': [{'digest': digest(raw), 'platform': {'os': 'linux', 'architecture': arch}}
                      for arch, raw in manifests.items()]}, sort_keys=True)
    pins = dict(version='0.4.0', revision=revision, published=True,
                image='ghcr.io/lansbe/pixstride:0.4.0@' + digest(index))
    image = dict(reference=pins['image'], architecture=architecture, os='linux',
                 image_id='sha256:' + ('3' if architecture == 'amd64' else '4') * 64,
                 capabilities={'version': '0.4.0'},
                 release=dict(version='0.4.0', revision=revision,
                              index_manifest=index, platform_manifest=manifests[architecture]))
    return pins, image


class ReleaseImageTests(unittest.TestCase):
    def test_real_published_proof_allows_a_different_committed_harness_revision(self):
        for arch in ('amd64', 'arm64'):
            pins, image = release_fixture(arch)
            self.assertEqual(validate_release_image(image, pins),
                             {k: pins[k] for k in ('version', 'revision', 'image')})
            report = measured_report(arch)
            report['images']['pixstride'] = image
            self.assertNotEqual(report['harness_revision'], pins['revision'])
            validate_report(report, require_client_isolation=True)

    def test_capture_records_verified_raw_manifests_and_tolerates_only_cli_newline(self):
        pins, image = release_fixture()
        proof = image.pop('release')
        class Registry:
            calls = []
            def manifest(self, reference):
                self.calls.append(reference)
                return (proof['index_manifest'] if len(self.calls) == 1 else proof['platform_manifest']).encode() + b'\n'
        registry = Registry()
        capture_release(registry, image, pins)
        self.assertEqual(image['release'], proof)
        self.assertEqual(len(registry.calls), 2)
        validate_release_image(image, pins)

    def test_wrong_or_missing_source_config_platform_version_and_digest_fail(self):
        for bad in ('missing', 'candidate', 'revision', 'version', 'config', 'platform', 'index', 'manifest', 'image'):
            pins, image = release_fixture()
            if bad == 'missing': image.pop('release')
            elif bad == 'candidate': image['candidate'] = {'revision': 'b' * 40}
            elif bad == 'revision': image['release']['revision'] = 'a' * 40
            elif bad == 'version': image['capabilities']['version'] = '0.3.0'
            elif bad == 'config': image['image_id'] = 'sha256:' + '5' * 64
            elif bad == 'platform': image['architecture'] = 'arm64'
            elif bad == 'image': pins['image'] = 'other@sha256:' + 'a' * 64
            else: image['release']['index_manifest' if bad == 'index' else 'platform_manifest'] += ' '
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                validate_release_image(image, pins)

    def test_ordinary_report_validation_also_rejects_tampered_published_proof(self):
        _, image = release_fixture()
        report = measured_report()
        report['images']['pixstride'] = copy.deepcopy(image)
        report['images']['pixstride']['image_id'] = 'sha256:' + '5' * 64
        with self.assertRaises(ValueError): validate_report(report)
