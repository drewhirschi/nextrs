#!/usr/bin/env python3
"""Build real separate processes and verify dependency, asset and HTTP isolation."""
import json
import os
from pathlib import Path
import socket
import sys
import subprocess
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
VERCEL = '--vercel' in sys.argv
SKIP_BUILD = '--skip-build' in sys.argv
BUILD_FLAGS = ['--vercel'] if VERCEL else ['--dev']
APP = ROOT / 'examples/react-todos'

def output_for(app):
    return app / ('.vercel/output' if VERCEL else '.nextrs/bundles')

def directory_for(output, name):
    return output / (f'functions/__nextrs_functions/{name}.func' if VERCEL else name)

HTTP = urllib.request.build_opener(urllib.request.ProxyHandler({}))

def run(*args, cwd=ROOT, env=None):
    subprocess.run(args, cwd=cwd, env=env, check=True)

def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]

def request(base, path, method='GET', data=None, headers=None):
    req = urllib.request.Request(base + path, method=method, data=data, headers=headers or {})
    try:
        with HTTP.open(req, timeout=5) as response:
            return response.status, response.headers, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.headers, error.read()

if not SKIP_BUILD:
    run('cargo', 'run', '--locked', '-p', 'cargo-nextrs', '--bin', 'nextrs', '--',
        'bundles', 'build', *BUILD_FLAGS, '--root', str(APP))
output = output_for(APP)
plan = json.loads((output / 'bundle-manifest.json').read_text())
assert plan['bundles']['exports']['routes'] == ['/api/exports']
assert '/api/exports' not in plan['bundles']['default']['routes']
assert (directory_for(output, 'exports') / 'resources/exports/README.txt').is_file()
assert not (directory_for(output, 'default') / 'resources').exists()
# Same normal dependency graph selected by the default build: csv must be absent.
tree = subprocess.check_output(['cargo', 'tree', '-p', 'react-todos', '--no-default-features', '--edges', 'normal', '--prefix', 'none'], cwd=ROOT, text=True)
assert not any(line.startswith('csv ') for line in tree.splitlines())
# Check the positive dependency selection too, so a broken feature flag cannot pass.
export_tree = subprocess.check_output(['cargo', 'tree', '-p', 'react-todos', '--no-default-features', '--features', 'exports', '--edges', 'normal', '--prefix', 'none'], cwd=ROOT, text=True)
assert any(line.startswith('csv ') for line in export_tree.splitlines())
assert any(line.startswith('csv-core ') for line in export_tree.splitlines())
assert not any(line.startswith('csv-core ') for line in tree.splitlines())
for name in ('default', 'exports'):
    directory = directory_for(output, name)
    expected = {'executable'} | ({'.vc-config.json'} if VERCEL else set())
    if name == 'exports':
        expected.add('resources/exports/README.txt')
        assert (directory / 'resources/exports/README.txt').read_bytes() == (APP / 'resources/exports/README.txt').read_bytes()
    actual = {str(p.relative_to(directory)) for p in directory.rglob('*') if p.is_file()}
    assert actual == expected, (name, actual, expected)
    # Positive and negative control for this endpoint's distinctive compiled payload.
    assert (b'Try a separate server bundle' in (directory / 'executable').read_bytes()) == (name == 'exports')
    print(f'{name}: {(directory / "executable").stat().st_size} executable bytes; files={sorted(actual)}')
if VERCEL:
    assert not (output / 'static/resources').exists()
processes = []
try:
    bases = {}
    for name in ('default', 'exports'):
        port = free_port()
        env = {**os.environ, 'PORT': str(port), 'VERCEL_DEV_PORT': str(port), 'NEXTRS_PUBLIC_DIR': str(APP / 'public')}
        directory = directory_for(output, name)
        child = subprocess.Popen([str(directory / 'executable')], cwd=directory, env=env, stdout=subprocess.DEVNULL)
        processes.append(child)
        base = f'http://127.0.0.1:{port}'
        for attempt in range(100):
            if child.poll() is not None:
                raise AssertionError(f'{name} exited: {child.returncode}')
            try:
                if request(base, '/__nx/health')[0] == 200:
                    break
            except OSError:
                time.sleep(.1)
        else:
            raise AssertionError(f'{name} did not start')
        bases[name] = base
    status, headers, body = request(bases['exports'], '/api/exports?download=1', headers={'Cookie': 'sample=1'})
    assert status == 200 and headers['Content-Type'].startswith('text/csv')
    assert b'Try a separate server bundle' in body
    assert request(bases['exports'], '/api/exports', method='HEAD')[2] == b''
    assert request(bases['exports'], '/api/exports', method='POST', data=b'example')[0] == 405
    assert request(bases['default'], '/api/exports')[0] == 404
    assert request(bases['exports'], '/api/todos')[0] == 404
    assert request(bases['default'], '/api/todos')[0] == 200
    assert request(bases['default'], '/')[0] == 200
    assert request(bases['exports'], '/')[0] == 404
finally:
    for child in processes:
        child.terminate()
    for child in processes:
        child.wait(timeout=10)
print('Separate server bundle smoke passed: two processes, dependency/asset exclusion, route ownership, HEAD and 405.')

# Exercise inherited authorization, dynamic params, query/body/header forwarding
# and streaming through the exact generated routing table. This is a local
# adapter for the platform rules, not a claim of a live Vercel deployment.
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import re
import threading
from urllib.parse import urlsplit
FIXTURE = ROOT / 'crates/cargo-nextrs/tests/fixtures/server-bundles'
if not SKIP_BUILD:
    run('cargo', 'run', '--locked', '-p', 'cargo-nextrs', '--bin', 'nextrs', '--',
        'bundles', 'build', *BUILD_FLAGS, *(['--bin', 'fixture-vercel'] if VERCEL else []), '--root', str(FIXTURE))
fixture_output = output_for(FIXTURE)
rules = json.loads((fixture_output / 'routing.json').read_text())
backends = {}
children = []
class Proxy(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass
    def do_GET(self):
        public_path = urlsplit(self.path).path
        for rule in rules:
            if 'src' in rule and re.fullmatch(rule['src'], public_path):
                if 'status' in rule:
                    self.send_response(rule['status']); self.end_headers(); return
                if 'dest' in rule:
                    name = rule['dest'].rsplit('/', 1)[1]
                    break
        else:
            raise AssertionError('no generated fallback')
        connection = http.client.HTTPConnection('127.0.0.1', backends[name], timeout=5)
        body = self.rfile.read(int(self.headers.get('content-length', 0)))
        connection.request(self.command, self.path, body=body, headers=dict(self.headers))
        response = connection.getresponse()
        self.send_response(response.status)
        for key, value in response.getheaders():
            if key.lower() not in ('transfer-encoding', 'connection'):
                self.send_header(key, value)
        self.end_headers()
        if self.command != 'HEAD':
            while chunk := response.read1(65536):
                self.wfile.write(chunk); self.wfile.flush()
        connection.close()
    do_POST = do_GET
    do_HEAD = do_GET
    do_OPTIONS = do_GET
proxy = ThreadingHTTPServer(('127.0.0.1', 0), Proxy)
threading.Thread(target=proxy.serve_forever, daemon=True).start()
try:
    for name in ('default', 'heavy'):
        port = free_port(); backends[name] = port
        child = subprocess.Popen([str(directory_for(fixture_output, name) / 'executable')], env={**os.environ, 'PORT': str(port), 'VERCEL_DEV_PORT': str(port)})
        children.append(child)
        for _ in range(100):
            try:
                if request(f'http://127.0.0.1:{port}', '/__nx/health')[0] == 200: break
            except OSError: time.sleep(.1)
        else: raise AssertionError('fixture did not start')
    base = f'http://127.0.0.1:{proxy.server_port}'
    assert request(base, '/api/heavy/42')[0] == 401
    auth = {'Authorization': 'Bearer test', 'Cookie': 'session=abc'}
    status, headers, body = request(base, '/api/heavy/42?q=a%20b', 'POST', b'\x00binary\xff', auth)
    assert status == 201 and body == b'\x00binary\xff'
    assert headers['x-id'] == '42' and headers['x-query'] == 'a b'
    assert headers['x-cookie'] == 'session=abc' and headers['x-guard-ran'] == 'yes'
    assert headers['set-cookie'] == 'result=ok; Path=/; HttpOnly'
    assert request(base, '/api/light', headers=auth)[2] == b'light'
    assert request(base, '/api/light', method='POST', data=b'x', headers=auth)[0] == 405
    assert request(base, '/api/heavy/42', method='OPTIONS', headers=auth)[0] == 405
    assert request(base, '/__nextrs_functions/heavy', headers=auth)[0] == 404
    assert request(base, '/unknown', headers=auth)[0] == 404
    with HTTP.open(urllib.request.Request(base + '/api/heavy/42', headers=auth), timeout=5) as stream:
        assert stream.readline() == b'first\n'
        start = time.monotonic()
        assert stream.readline() == b'second\n'
        assert time.monotonic() - start >= .2, 'stream was buffered'
finally:
    proxy.shutdown(); proxy.server_close()
    for child in children: child.terminate()
    for child in children: child.wait(timeout=10)
print('Generated-routing smoke passed: authorization, dynamic paths, query, binary body, cookies, status, response headers and streaming.')
