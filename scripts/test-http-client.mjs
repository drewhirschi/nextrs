import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';

const root = resolve(process.argv[2]);
const require = createRequire(resolve(root, 'package.json'));
const { QueryClient } = require('@tanstack/react-query');
const client = await import(pathToFileURL(resolve(root, '.nextrs/client/dist/http-client.js')));
const { HttpError } = await import(pathToFileURL(resolve(root, '.nextrs/client/dist/index.js')));
const reactQuery = await import(pathToFileURL(resolve(root, '.nextrs/client/dist/react-query.js')));
assert.equal(reactQuery.HttpError, HttpError);
const server = createServer((req, res) => {
  const status = req.url.startsWith("/api/") ? 401 : Number(req.url.slice(1));
  res.writeHead(status, { 'content-type': status === 502 ? 'text/plain' : 'application/json', 'x-test': 'retained' });
  res.end(status === 502 ? 'upstream unavailable' : status === 503 || status === 201 ? '{invalid' : JSON.stringify({ message: status === 200 ? 'ok' : 'denied' }));
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;
const originalFetch = globalThis.fetch;
globalThis.fetch = (url, options) => originalFetch(new URL(url, base), options);
const query = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } });
try {
  const success = await client.httpClient(`${base}/200`);
  assert.equal(success.status, 200);
  assert.deepEqual(success.data, { message: 'ok' });
  assert.equal(success.headers.get('x-test'), 'retained');
  for (const status of [204, 205]) {
    assert.equal((await client.httpClient(`${base}/${status}`)).data, undefined);
  }
  for (const [status, data] of [[401, { message: 'denied' }], [422, { message: 'denied' }], [502, 'upstream unavailable'], [503, '{invalid'], [304, undefined]]) {
    await assert.rejects(client.httpClient(`${base}/${status}`), error => {
      assert(error instanceof HttpError);
      assert.equal(error.status, status);
      assert.deepEqual(error.data, data);
      assert.equal(error.headers.get('x-test'), 'retained');
      return true;
    });
  }
  await assert.rejects(client.httpClient(`${base}/201`), SyntaxError);
  await assert.rejects(query.fetchQuery({ queryKey: ['failure'], queryFn: () => client.httpClient(`${base}/401`) }), HttpError);
  assert.equal(query.getQueryState(['failure']).status, 'error');
  const generatedOptions = reactQuery.getGetApiPingQueryOptions?.()
    ?? reactQuery.getGetApiTodosQueryOptions();
  await assert.rejects(query.fetchQuery(generatedOptions), HttpError);
  assert.equal(query.getQueryState(generatedOptions.queryKey).status, 'error');
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(client.httpClient(`${base}/200`, { signal: controller.signal }), { name: 'AbortError' });
} finally {
  globalThis.fetch = originalFetch;
  query.clear();
  await new Promise(resolve => server.close(resolve));
}
await assert.rejects(client.httpClient(`${base}/200`), error => error instanceof TypeError && !(error instanceof HttpError));
console.log(`HTTP client contracts passed: ${root}`);
