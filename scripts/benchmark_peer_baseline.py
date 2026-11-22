"""Read the published V2 timings and recover missing reference pixels once, outside timing."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import uuid

from benchmark_fixtures import ROOT
from benchmark_runtime import Docker, Server, architecture, request, signed_path
from benchmark_v2_catalog import verify_catalog
from benchmark_v2_collect import validate_response
from benchmark_v2_fixtures import verify

BASE = ROOT / 'benchmarks/results/v2'
LANES = 10


def baseline(arch):
    index = json.loads((BASE / 'measure-index.json').read_text())
    result = index['result']
    samples = {}
    for item in index['reports']:
        if not item['file'].startswith(f'v2-measure-{arch}-'):
            continue
        compressed = (BASE / item['file']).read_bytes()
        if hashlib.sha256(compressed).hexdigest() != item['sha256']:
            raise ValueError('Published archive hash mismatch')
        report = json.loads(gzip.decompress(compressed))
        if report['fixtures']['corpus_sha256'] != result['corpus_sha256']:
            raise ValueError('Published corpus mismatch')
        for row in report['cases']:
            engines = row['engines']
            samples.setdefault(row['id'], []).append(dict(
                repetition=report['repetition'], cpu_model=report['host']['cpu_model'],
                reference=engines['imgproxy']['output']['sha256'],
                **{mode: dict(engines[mode]['summary'], bytes=engines[mode]['output']['bytes'],
                                 output_sha256=engines[mode]['output']['sha256'])
                   for mode in ('pixstride_default', 'pixstride_native', 'imgproxy')}))
    if len(samples) != 1000 or any(sorted(x['repetition'] for x in rows) != [1, 2, 3]
                                  for rows in samples.values()):
        raise ValueError('Incomplete historical cohort')
    return result, samples


def lane(case):
    return (case['rank'] - 1) % LANES


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result, samples = baseline(architecture())
    manifest = verify(args.fixtures)
    if (manifest['corpus_sha256'], manifest['catalog_sha256']) != (result['corpus_sha256'], result['catalog_sha256']):
        raise ValueError('Inputs differ from the published cohort')
    docker = Docker()
    image = docker.image(result['pins']['imgproxy']['image'])
    cases = verify_catalog(manifest=manifest)
    # The archives contain only hashes and statistics. One untimed render per case
    # recovers the reference body, shared by all five peers and verified against history.
    with Server(docker, 'imgproxy', image['reference'], args.fixtures, sorted(os.sched_getaffinity(0))[:2],
                uuid.uuid4().hex[:12], 0) as server:
        for case in cases:
            status, headers, body, _ = request(server.port, signed_path(case))
            output = validate_response(case, status, headers, body)
            expected = {sample['reference'] for sample in samples[case['id']]}
            if output['sha256'] not in expected:
                raise ValueError('Reference differs from all published hashes: ' + case['id'])
            dest = args.output / str(lane(case))
            dest.mkdir(parents=True, exist_ok=True)
            (dest / case['id']).write_bytes(body)
            if case['rank'] % 100 == 0:
                print('Reference bodies verified:', case['rank'], flush=True)


if __name__ == '__main__':
    main()
