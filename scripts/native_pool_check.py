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
        (root / 'large-bad.png').write_bytes(b'\x89PNG\r\n\x1a\n' + bytes(4 * 1024 * 1024))
        red = '/unsafe/plain/local:///red.png@png'
        blue = '/unsafe/plain/local:///blue.png@png'
        with server(binary, root, {'PIXSTRIDE_ENGINE': 'auto'}) as (port, process, _):
            identity = request(port, red)
            assert identity[0] == 200 and identity[2] == (root / 'red.png').read_bytes()
            assert not children(process.pid), 'Validated opaque PNG identity should keep its builtin path'
            transformed = request(port, '/unsafe/rs:fit:16:12/plain/local:///red.png@png')
            assert transformed[0] == 200 and read_png(transformed[2])[0][:2] == (16, 12)
            assert len(children(process.pid)) == 1, 'Automatic routing should use the reusable native engine for transforms'
        with server(binary, root, {'PIXSTRIDE_ENGINE': 'builtin'}) as (port, process, _):
            result = request(port, '/unsafe/rs:fit:16:12/plain/local:///blue.png@png')
            assert result[0] == 200 and read_png(result[2])[1][:3] == [0, 0, 255]
            assert not children(process.pid), 'Legacy builtin routing should remain explicitly available'
        with server(binary, root, {'PIXSTRIDE_PROCESS_TIMEOUT': '1'}) as (port, process, _):
            expected = request(port, red)
            assert expected[0] == 200, expected[:2]
            initial = children(process.pid)
            assert len(initial) == 1, 'A completed native request must leave one reusable isolated worker'
            original = next(iter(initial))
            status_path = Path('/proc') / str(original) / 'status'
            if status_path.exists():
                fields = dict(line.split(':', 1) for line in status_path.read_text().splitlines() if ':' in line)
                assert fields.get('THP_enabled', '').strip() == '0', 'Native worker did not request base pages'
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
            stopped = next(iter(children(process.pid)))
            os.kill(stopped, signal.SIGSTOP)
            started = time.monotonic()
            assert request(port, '/unsafe/plain/local:///large-bad.png@png')[0] != 200
            assert time.monotonic() - started < 4, 'A full input pipe escaped the exchange deadline'
            assert gone(stopped), 'Write-blocked worker was not killed and reaped'
            assert request(port, red)[2] == expected[2], 'Write timeout poisoned the next request'
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
            admission_retries = []
            def image(index):
                # HTTP admission deliberately returns 503 under backpressure.
                # Keep this test about native isolation, and fail every codec
                # error instead of confusing an admissible 503 with corruption.
                for _ in range(40):
                    result = request(port, red if index % 2 else blue)
                    if result[0] != 503:
                        assert result[0] == 200, (index, result[0], result[2][:160])
                        return result
                    admission_retries.append(index)
                    time.sleep(.01)
                raise AssertionError('Concurrent request could not obtain bounded HTTP admission')
            with ThreadPoolExecutor(max_workers=4) as clients:
                results = list(clients.map(image, range(40)))
            assert all(result[0] == 200 for result in results)
            for index, result in enumerate(results):
                assert read_png(result[2])[1][:3] == ([255, 0, 0] if index % 2 else [0, 0, 255])
            assert 1 <= len(children(process.pid)) <= 2, 'Native concurrency escaped its configured bound'
            if admission_retries:
                print(f'Retried {len(admission_retries)} documented HTTP admission 503 responses during native concurrency check')

        (root / 'large.png').write_bytes(png(1280, 720, channels=3))
        with server(binary, root, {'PIXSTRIDE_PROCESS_TIMEOUT': '1', 'PIXSTRIDE_PNG_INTERLACED': '1'}) as (port, process, _):
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
