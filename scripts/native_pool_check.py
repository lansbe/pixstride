#!/usr/bin/env python3
"""Exercise worker reuse, isolation, failure recovery and resource limits."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

from image_fixtures import png, read_png
from server_check import request, server


def children(parent):
    tasks = Path('/proc') / str(parent) / 'task'
    if tasks.is_dir():
        owned = set()
        for path in tasks.glob('*/children'):
            try:
                owned.update(int(value) for value in path.read_text().split())
            except FileNotFoundError:
                pass  # An HTTP worker thread can exit during inspection.
        return owned
    result = subprocess.run(['pgrep', '-P', str(parent)], capture_output=True, text=True)
    if result.returncode not in (0, 1):
        raise AssertionError('Cannot inspect owned worker processes')
    return {int(value) for value in result.stdout.split()}


def gone(pid):
    try:
        os.kill(pid, 0)
        return False
    except ProcessLookupError:
        return True


def cpu_seconds(pid):
    stat = Path('/proc') / str(pid) / 'stat'
    if stat.exists():
        fields = stat.read_text().rsplit(')', 1)[1].split()
        return (int(fields[11]) + int(fields[12])) / os.sysconf('SC_CLK_TCK')
    value = subprocess.check_output(['ps', '-p', str(pid), '-o', 'time='], text=True).strip()
    days, value = value.split('-', 1) if '-' in value else ('0', value)
    seconds = 0.0
    for part in value.split(':'):
        seconds = seconds * 60 + float(part)
    return int(days) * 86400 + seconds


def check(binary):
    binary = binary.resolve()
    with tempfile.TemporaryDirectory(prefix='pixstride-pool-check-') as temp:
        root = Path(temp)
        for name, color in [('red', [255, 0, 0]), ('blue', [0, 0, 255])]:
            (root / (name + '.png')).write_bytes(png(32, 24, channels=3, rows=[color * 32] * 24))
        (root / 'bad.png').write_bytes(b'\x89PNG\r\n\x1a\ntruncated')
        red = '/unsafe/plain/local:///red.png@png'
        blue = '/unsafe/plain/local:///blue.png@png'
        with server(binary, root, {'PIXSTRIDE_PROCESS_TIMEOUT': '1'}) as (port, process, _):
            expected = request(port, red)
            assert expected[0] == 200, expected[:2]
            initial = children(process.pid)
            assert len(initial) == 1, 'A completed native request must leave one reusable isolated worker'
            original = next(iter(initial))
            for path, color in [(blue, [0, 0, 255]), (red, [255, 0, 0])] * 4:
                result = request(port, path)
                assert result[0] == 200 and read_png(result[2])[1][:3] == color
                assert children(process.pid) == initial, 'Sequential images must reuse the same child'
            jpeg = request(port, red[:-3] + 'jpg')
            assert jpeg[0] == 200 and jpeg[2].startswith(b'\xff\xd8')
            assert request(port, red)[2] == expected[2], 'Format/options leaked between worker requests'

            bad = request(port, '/unsafe/plain/local:///bad.png@png')
            assert bad[0] != 200
            assert request(port, red)[2] == expected[2], 'Invalid input poisoned the next request'
            assert len(children(process.pid)) == 1
            assert gone(original), 'Failed workers must be reaped'

            killed = next(iter(children(process.pid)))
            os.kill(killed, signal.SIGKILL)
            time.sleep(.05)
            assert request(port, blue)[0] == 200, 'An idle crash must be replaced before reuse'
            assert gone(killed)

            stopped = next(iter(children(process.pid)))
            os.kill(stopped, signal.SIGSTOP)
            started = time.monotonic()
            assert request(port, red)[0] != 200, 'Hung worker unexpectedly completed a request'
            assert time.monotonic() - started < 4, 'Native wall-clock deadline was not enforced'
            assert gone(stopped), 'Timed-out worker was not killed and reaped'
            assert request(port, red)[2] == expected[2], 'Timeout poisoned the next request'
            idle = children(process.pid)
        deadline = time.monotonic() + 3
        while any(not gone(pid) for pid in idle) and time.monotonic() < deadline:
            time.sleep(.02)
        assert all(gone(pid) for pid in idle), 'Server shutdown left native children behind'

        with server(binary, root) as (port, process, _):
            seen = set()
            for index in range(270):
                result = request(port, red)
                assert result[0] == 200 and result[2] == expected[2]
                owned = children(process.pid)
                assert len(owned) <= 1, f'Idle pool accumulated extra children at request {index + 1}: {owned}'
                seen.update(owned)
            assert 2 <= len(seen) <= 3, 'Workers must recycle at a bounded request count'

        with server(binary, root, {'PIXSTRIDE_CONCURRENCY': '2', 'PIXSTRIDE_MEMORY_BUDGET_MIB': '4096'}) as (port, process, _):
            def image(index):
                return request(port, red if index % 2 else blue)
            with ThreadPoolExecutor(max_workers=4) as clients:
                results = list(clients.map(image, range(40)))
            assert all(result[0] == 200 for result in results)
            for index, result in enumerate(results):
                assert read_png(result[2])[1][:3] == ([255, 0, 0] if index % 2 else [0, 0, 255])
            assert 1 <= len(children(process.pid)) <= 2, 'Native concurrency escaped its configured bound'

        (root / 'large.png').write_bytes(png(1280, 720, channels=3))
        with server(binary, root, {'PIXSTRIDE_PROCESS_TIMEOUT': '1'}) as (port, process, _):
            target = '/unsafe/plain/local:///large.png@png'
            assert request(port, target)[0] == 200
            original = next(iter(children(process.pid)))
            for _ in range(200):
                assert request(port, target)[0] == 200, 'Cumulative CPU was mistaken for one image CPU'
                assert children(process.pid) == {original}
                if cpu_seconds(original) > 2.2:
                    break
            else:
                raise AssertionError('CPU renewal check did not exercise more than the original one-image budget')
    print('Native worker reuse, framing isolation, crash/deadline recovery, recycling, concurrency, CPU renewal and shutdown passed')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    check(parser.parse_args().binary)
