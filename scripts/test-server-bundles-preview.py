#!/usr/bin/env python3
"""Exercise a deployed Todo preview using Vercel CLI's authenticated transport.

Usage: python3 scripts/test-server-bundles-preview.py https://preview.vercel.app
Requires the example to be linked and the CLI to be authenticated. Does not
change deployment protection or access credential files.
"""
import csv
import io
import json
import re
from pathlib import Path
import subprocess
import sys
import tempfile

APP = Path(__file__).resolve().parents[1] / 'examples/react-todos'
DEPLOYMENT = sys.argv[1]

def request(path, method='GET', data=None):
    with tempfile.TemporaryDirectory() as tmp:
        body = Path(tmp) / 'body'
        headers = Path(tmp) / 'headers'
        args = ['vercel', 'curl', path, '--deployment', DEPLOYMENT, '--',
                '--silent', '--show-error', '--max-time', '45',
                '--output', str(body), '--dump-header', str(headers), '--write-out', '%{http_code}']
        args += ['--head'] if method == 'HEAD' else ['--request', method]
        if data is not None:
            args += ['--header', 'Content-Type: application/json', '--data', json.dumps(data)]
        result = subprocess.run(args, cwd=APP, check=True, capture_output=True, text=True)
        status = int(result.stdout.strip())
        parsed = {}
        for line in headers.read_text().splitlines():
            if ': ' in line:
                key, value = line.split(': ', 1)
                parsed[key.lower()] = value
        content = body.read_bytes()
        print(f'{method} {path}: {status}', flush=True)
        return status, parsed, content

status, headers, body = request('/api/exports?download=1')
assert status == 200 and headers['content-type'].startswith('text/csv')
assert headers['content-disposition'] == 'attachment; filename=todos.csv'
assert list(csv.reader(io.StringIO(body.decode()))) == [
    ['id', 'title', 'completed'], ['1', 'Try a separate server bundle', 'false']]
assert request('/api/exports', 'HEAD')[0] == 200
assert request('/api/exports', 'POST', {'test': True})[0] == 405
status, _, body = request('/api/todos?status=open')
assert status == 200 and isinstance(json.loads(body), list)
assert all(not todo['done'] for todo in json.loads(body))
status, _, body = request('/api/todos/1')
assert status == 200 and json.loads(body)['id'] == 1
assets = set()
for path in ['/', '/about', '/todos/1', '/todos/1/test']:
    status, headers, body = request(path)
    assert status == 200 and 'text/html' in headers['content-type']
    assert b'Deployment is building' not in body
    assets.update(re.findall(r'(?:src|href)="(/(?:dist/[^"]+|style\.css))"', body.decode()))
assert any(path.endswith('.js') for path in assets), 'no frontend JavaScript in deployed HTML'
for path in sorted(assets):
    status, headers, body = request(path)
    assert status == 200 and body
    assert ('javascript' if path.endswith('.js') else 'css') in headers['content-type']
for path in ['/resources/exports/README.txt', '/bundle-manifest.json',
             '/bundle-artifacts.json', '/__nextrs_functions/exports',
             '/__nextrs_functions/default', '/api/exports/unknown', '/unknown']:
    assert request(path)[0] == 404
print('Live Vercel Todo preview passed: CSV, headers, HEAD/405, Todo API, dynamic routes, pages, private paths.')
