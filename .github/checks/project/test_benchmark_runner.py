"""Interrupted measurements retain completed batches and clean metadata probes."""
import contextlib
import importlib.util
import io
import json
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'scripts'))
import benchmark
from benchmark_runtime import Docker, ENGINES


class BenchmarkRunnerTests(unittest.TestCase):
    @unittest.skipUnless(importlib.util.find_spec('PIL'), 'Pillow is installed by the benchmark workflow')
    def test_http_failure_after_first_batch_keeps_partial_case_and_context(self):
        from PIL import Image
        encoded = io.BytesIO()
        Image.new('RGB', (1, 1), 'red').save(encoded, format='PNG')
        body = encoded.getvalue()
        class Handler(BaseHTTPRequestHandler):
            calls = 0
            def do_GET(self):
                type(self).calls += 1
                status = 500 if type(self).calls == 7 else 200
                self.send_response(status)
                self.send_header('Content-Type', 'image/png')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            def log_message(self, *_):
                pass
        server = HTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=lambda: server.serve_forever(poll_interval=.01), daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory() as directory:
                group = Path(directory)
                (group / 'cpu.stat').write_text('usage_usec 1000\nuser_usec 800\nsystem_usec 200\n')
                (group / 'memory.peak').write_text('1024\n')
                (group / 'memory.events').write_text('oom_kill 0\n')
                # Docker is the external boundary; HTTP parsing/counters/collection stay real.
                factory = lambda *_: contextlib.nullcontext(SimpleNamespace(port=server.server_port, group=group))
                progress = {}
                case = dict(id='fixture', label='Fixture', source='input.png', options='', format='png', dimensions=[1, 1])
                protocol = dict(warmups=0, batches=2, requests_per_batch=1, cpu_tail_seconds=0)
                images = {product: {'reference': 'synthetic'} for product in ['pixstride', 'imgproxy']}
                with patch('benchmark.Server', side_effect=factory), self.assertRaisesRegex(ValueError, '500'):
                    benchmark.case_measurement(case, None, images, group, [0, 1], 'fixture', 0, protocol, progress=progress)
                saved = json.loads(json.dumps(progress))
                self.assertEqual(saved['status'], 'failed')
                self.assertEqual(saved['failure']['engine'], 'pixstride_default')
                self.assertEqual(saved['failure']['batch'], 1)
                for engine in ENGINES:
                    self.assertEqual(len(saved['engines'][engine]['batches']), 1)
                    self.assertEqual(len(saved['engines'][engine]['batches'][0]['latencies_ns']), 1)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)

    def test_timed_out_metadata_probe_cleans_only_its_owned_container(self):
        class BlockingDocker(Docker):
            alive = False
            def run(self, *args, **_):
                if args[0] == 'run':
                    self.alive = True
                    self.name = args[args.index('--name') + 1]
                    label = args[args.index('--label') + 1]
                    self.labels = dict([label.split('=', 1)])
                    raise subprocess.TimeoutExpired('docker', .01)
                if args[0] == 'inspect':
                    return json.dumps([{'Config': {'Labels': self.labels}}])
                if args[0] == 'rm':
                    if args[-1] != self.name:
                        raise AssertionError('Cleanup targeted another container')
                    self.alive = False
                    return ''
                raise AssertionError(args)
        docker = BlockingDocker()
        with self.assertRaises(subprocess.TimeoutExpired):
            docker.probe('synthetic', ['imgproxy', 'version'], 'fixture-owner', 'imgproxy')
        self.assertFalse(docker.alive)


if __name__ == '__main__':
    unittest.main()
