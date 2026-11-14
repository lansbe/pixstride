#!/usr/bin/env python3
"""Deterministic mutation smoke tests; this is not coverage-guided fuzzing."""
import argparse
import json
import os
from pathlib import Path
import random
import subprocess
import tempfile
import time
from image_fixtures import png, animated_gif
from compatibility import environment


def check(args):
    binary = args.binary.resolve()
    variables = {**environment(), 'PIXSTRIDE_ENGINE': 'native', 'PIXSTRIDE_PROCESS_TIMEOUT': '2'}
    generator = random.Random(0x504958)
    seeds = [png(8, 8), animated_gif(), b'<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><path d="M0 0L8 8"/></svg>']
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='pixstride-mutations-') as temp:
        root = Path(temp)
        for index in range(args.cases):
            seed = seeds[index % len(seeds)]
            data = bytearray(seed)
            mode = index % 4
            if mode == 0:
                data = data[:generator.randrange(len(data))]
            elif mode == 1:
                for _ in range(1 + index % 8):
                    position = generator.randrange(len(data))
                    data[position] ^= generator.randrange(1, 256)
            elif mode == 2:
                position = generator.randrange(len(data))
                data[position:position] = generator.randbytes(1 + index % 32)
            else:
                data.extend(generator.randbytes(index % 128))
            source, target = root / 'mutation', root / 'output.png'
            source.write_bytes(data)
            result = subprocess.run([str(binary), 'process', str(source), str(target),
                                     '/unsafe/maf:2/plain/local:///mutation@png'],
                                    env=variables, capture_output=True, timeout=5)
            assert result.returncode in (0, 1), f'Parent crashed on generated case {index}'
            assert b'native worker crashed' not in result.stderr, f'Native crash on generated case {index}'
            assert len(result.stdout) < 65536 and len(result.stderr) < 65536
            if result.returncode == 0:
                assert target.stat().st_size <= 64 * 1024 * 1024
    report = {'generated_cases': args.cases, 'seed': '0x504958', 'elapsed_seconds': time.monotonic() - started,
              'kind': 'bounded deterministic mutation smoke; no coverage claim'}
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + '\n')
    print(f'{args.cases} mutation cases completed without a parent crash or harness timeout')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--cases', type=int, default=150)
    parser.add_argument('--report', type=Path)
    arguments = parser.parse_args()
    if not 1 <= arguments.cases <= 10000:
        parser.error('--cases must be 1..10000')
    check(arguments)
