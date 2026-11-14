"""Exercise real loopback transports using explicit administrator opt-ins."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from email.utils import formatdate
import gzip
import socket
import threading
import time
from image_fixtures import png


def check(binary, root, server, request, capabilities):
    captured = []
    data = png(8, 6)

    class Origin(BaseHTTPRequestHandler):
        def do_GET(self):
            captured.append((self.path, dict(self.headers)))
            if self.path == '/start':
                self.send_response(302)
                self.send_header('Location', '/dir/image')
                self.send_header('Set-Cookie', 'redirect=synthetic; Path=/dir')
                self.send_header('Set-Cookie', 'secure=hidden; Secure')
                self.send_header('Content-Length', '0')
                self.end_headers()
                return
            if self.path == '/cross':
                self.send_response(302)
                self.send_header('Location', f'http://[::ffff:127.0.0.1]:{self.server.server_port}/dir/image')
                self.send_header('Content-Length', '0')
                self.end_headers()
                return
            wire=gzip.compress(data) if self.path.startswith('/gzip') else data
            if self.path=='/gzip-bad':wire=wire[:-1]+bytes([wire[-1]^1])
            self.send_response(200)
            self.send_header('Content-Type', 'image/png')
            self.send_header('Content-Length', str(len(wire)))
            if self.path.startswith('/gzip'):self.send_header('Content-Encoding','gzip')
            if self.path == '/expires':
                self.send_header('Expires', formatdate(time.time() + 300, usegmt=True))
            else:
                self.send_header('Cache-Control', 'public, max-age=120')
            self.end_headers()
            self.wfile.write(wire)

        def log_message(self, *_):
            pass

    origin = ThreadingHTTPServer(('127.0.0.1', 0), Origin)
    thread = threading.Thread(target=origin.serve_forever, daemon=True)
    thread.start()
    try:
        source = f'http://127.0.0.1:{origin.server_port}'
        policy = dict(PIXSTRIDE_ALLOWED_HOSTS='127.0.0.1,::ffff:127.0.0.1',
                      PIXSTRIDE_ALLOWED_SOURCE_PORTS=str(origin.server_port))
        with server(binary, root, policy) as (port, _, _):
            assert request(port, f'/unsafe/plain/{source}/image@png')[0] == 502
            assert not captured, 'Default policy connected to loopback'
        policy.update(PIXSTRIDE_ALLOW_LOOPBACK_SOURCE_ADDRESSES='1',
                      PIXSTRIDE_USER_AGENT='synthetic/%current_version')
        with server(binary, root, policy) as (port, _, _):
            assert request(port, f'/unsafe/plain/{source}/start@png')[0] == 200
            assert len(captured) == 2
            assert all(v['User-Agent'].startswith('synthetic/') for _, v in captured)
            assert all('Authorization' not in v and 'Cookie' not in v for _, v in captured)
            decoded=request(port,f'/unsafe/raw:1/plain/{source}/gzip@png')
            assert decoded[0]==200 and decoded[2]==data
            assert int(decoded[1]['x-upstream-bytes'])==len(gzip.compress(data))
            assert request(port,f'/unsafe/raw:1/plain/{source}/gzip-bad@png')[0]==502
        captured.clear()
        with server(binary, root, {**policy, 'PIXSTRIDE_MAX_REDIRECTS': '0'}) as (port, _, _):
            assert request(port, f'/unsafe/plain/{source}/start@png')[0] == 502
            assert len(captured) == 1
        with server(binary, root, {**policy, 'PIXSTRIDE_CACHE_CONTROL_PASSTHROUGH': '1', 'PIXSTRIDE_TTL': '30'}) as (port, _, _):
            inherited = request(port, f'/unsafe/plain/{source}/image@png')
            assert inherited[0] == 200 and inherited[1]['cache-control'] == 'public, max-age=120'
            assert "default-src 'none'" in inherited[1]['content-security-policy']
            expires = request(port, f'/unsafe/plain/{source}/expires@png')
            assert expires[0] == 200 and 280 <= int(expires[1]['cache-control'].split('=')[1]) <= 300
            capped = request(port, f'/unsafe/exp:{int(time.time())+60}/plain/{source}/image@png')
            assert capped[0] == 200 and capped[1]['cache-control'] == 'public, max-age=30'
        if capabilities.get('source_cookies'):
            captured.clear()
            with server(binary, root, {**policy, 'PIXSTRIDE_COOKIE_PASSTHROUGH': '1',
                                     'PIXSTRIDE_COOKIE_BASE_URL': source, 'PIXSTRIDE_TTL': '3600',
                                     'PIXSTRIDE_CACHE_CONTROL_PASSTHROUGH': '1'}) as (port, _, _):
                result = request(port, f'/unsafe/plain/{source}/start@png', headers={'Cookie': 'session=synthetic'})
                assert result[0] == 200 and result[1]['cache-control'] == 'private, no-store'
                assert 'Cookie' in result[1]['vary']
                assert captured[0][1].get('Cookie') == 'session=synthetic'
                redirected = captured[1][1]['Cookie']
                assert 'session=synthetic' in redirected and 'redirect=synthetic' in redirected and 'secure=' not in redirected, redirected
                captured.clear()
                assert request(port, f'/unsafe/plain/{source}/image@png')[0] == 200
                assert 'Cookie' not in captured[0][1], 'Pooled connection leaked a previous request cookie'
                captured.clear()
                cross = request(port, f'/unsafe/plain/{source}/cross@png', headers={'Cookie': 'session=synthetic'})
                assert cross[0] == 200, (cross, captured)
                assert 'Cookie' not in captured[-1][1], 'Cookie crossed its host scope'
    finally:
        origin.shutdown()
        origin.server_close()
        thread.join()
    class IPv6Origin(ThreadingHTTPServer):
        address_family = socket.AF_INET6
    origin6 = IPv6Origin(('::1', 0), Origin)
    thread6 = threading.Thread(target=origin6.serve_forever, daemon=True)
    thread6.start()
    try:
        with server(binary, root, {'PIXSTRIDE_ALLOWED_HOSTS': '::1',
                                  'PIXSTRIDE_ALLOWED_SOURCE_PORTS': str(origin6.server_port),
                                  'PIXSTRIDE_ALLOW_LOOPBACK_SOURCE_ADDRESSES': '1'}) as (port, _, _):
            assert request(port, f'/unsafe/plain/http://[::1]:{origin6.server_port}/image@png')[0] == 200
    finally:
        origin6.shutdown()
        origin6.server_close()
        thread6.join()
    return ['source-ipv4-ipv6-opt-ins-redirects-user-agent-and-cookie-isolation',
            'origin-cache-passthrough-expiration-and-content-security-policy',
            'bounded-source-gzip-decoding-wire-accounting-and-corrupt-trailer-rejection']
