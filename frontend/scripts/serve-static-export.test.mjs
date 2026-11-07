import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { configuredOrigin, createStaticExportServer, parseArgs } from './serve-static-export.mjs';

const PROXY_SECRET = 'proxy-secret-for-tests-0123456789abcdef';

async function listen(server) {
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return `http://127.0.0.1:${server.address().port}`;
}

async function close(server) {
  await new Promise((resolve) => server.close(resolve));
}

async function makeExport(t) {
  const parent = await fs.mkdtemp(path.join(os.tmpdir(), 'hivemind-static-'));
  t.after(() => fs.rm(parent, { recursive: true, force: true }));
  const directory = path.join(parent, 'out');
  await fs.mkdir(path.join(directory, 'docs'), { recursive: true });
  await fs.writeFile(path.join(directory, 'index.html'), '<title>Hivemind</title>');
  await fs.writeFile(path.join(directory, 'docs', 'loops.html'), '<h1>Loops</h1>');
  await fs.writeFile(path.join(directory, '404.html'), '<h1>Missing page</h1>');
  // Sits beside the export, so a traversal that escaped the export would reveal it.
  await fs.writeFile(path.join(parent, 'secret.txt'), 'outside the export');
  return directory;
}

// Sends the path exactly as written. fetch would normalize `..` segments first.
function rawRequest(base, { method = 'GET', path: requestPath, headers = {}, body } = {}) {
  const url = new URL(base);
  return new Promise((resolve, reject) => {
    const request = http.request(
      { host: url.hostname, port: url.port, method, path: requestPath, headers },
      (response) => {
        const chunks = [];
        response.on('data', (chunk) => chunks.push(chunk));
        response.on('end', () =>
          resolve({
            status: response.statusCode,
            headers: response.headers,
            body: Buffer.concat(chunks).toString('utf8'),
          }),
        );
      },
    );
    request.on('error', reject);
    request.end(body);
  });
}

test('serves the export with clean URLs the way Pages does', async (t) => {
  const directory = await makeExport(t);
  const server = createStaticExportServer({ directory, env: {} });
  t.after(() => close(server));
  const base = await listen(server);

  const home = await rawRequest(base, { path: '/' });
  assert.equal(home.status, 200);
  assert.match(home.body, /Hivemind/);

  for (const route of ['/docs/loops', '/docs/loops/']) {
    const page = await rawRequest(base, { path: route });
    assert.equal(page.status, 200, route);
    assert.equal(page.body, '<h1>Loops</h1>', route);
    assert.match(page.headers['content-type'], /^text\/html/);
  }

  const missing = await rawRequest(base, { path: '/no/such/page' });
  assert.equal(missing.status, 404);
  assert.equal(missing.body, '<h1>Missing page</h1>');
});

test('never serves a file outside the export directory', async (t) => {
  const directory = await makeExport(t);
  const server = createStaticExportServer({ directory, env: {} });
  t.after(() => close(server));
  const base = await listen(server);

  for (const attempt of ['/..%2Fsecret.txt', '/%2e%2e/secret.txt', '/..%5Csecret.txt', '/docs/..%2F..%2Fsecret.txt']) {
    const response = await rawRequest(base, { path: attempt });
    assert.notEqual(response.body, 'outside the export', attempt);
    assert.notEqual(response.status, 200, attempt);
  }
});

test('answers account calls with 503 until the origin and the proxy secret are both configured', async (t) => {
  const directory = await makeExport(t);
  for (const env of [{}, { API_ORIGIN: 'https://api.example.test' }, { PROXY_SHARED_SECRET: PROXY_SECRET }]) {
    const server = createStaticExportServer({ directory, env });
    const base = await listen(server);
    const response = await rawRequest(base, { path: '/api/account' });
    await close(server);
    assert.equal(response.status, 503, JSON.stringify(env));
    assert.equal(JSON.parse(response.body).code, 'service_unavailable');
  }
});

test('refuses to forward to a non-loopback plain http origin', () => {
  assert.equal(configuredOrigin({ API_ORIGIN: 'http://10.0.0.5:3100' }), null);
  assert.equal(configuredOrigin({ API_ORIGIN: 'https://api.example.test/' }), 'https://api.example.test');
  assert.equal(configuredOrigin({ API_ORIGIN: 'http://127.0.0.1:3100/' }), 'http://127.0.0.1:3100');
  assert.equal(configuredOrigin({}), null);
});

test('forwards account calls with the proxy secret and passes session cookies back', async (t) => {
  const directory = await makeExport(t);
  const upstreamRequests = [];
  const upstream = http.createServer((request, response) => {
    let body = '';
    request.on('data', (chunk) => {
      body += chunk;
    });
    request.on('end', () => {
      upstreamRequests.push({
        method: request.method,
        url: request.url,
        secret: request.headers['x-hivemind-proxy-secret'],
        realIp: request.headers['x-real-ip'],
        body,
      });
      response.writeHead(200, {
        'content-type': 'application/json; charset=utf-8',
        'set-cookie': ['hivemind_session=abc; Path=/api; HttpOnly', 'theme=dark; Path=/'],
      });
      response.end(JSON.stringify({ success: true }));
    });
  });
  const upstreamBase = await listen(upstream);
  t.after(() => close(upstream));

  const server = createStaticExportServer({
    directory,
    env: { API_ORIGIN: upstreamBase, PROXY_SHARED_SECRET: PROXY_SECRET },
  });
  t.after(() => close(server));
  const base = await listen(server);

  const response = await rawRequest(base, {
    method: 'POST',
    path: '/api/login?source=site',
    headers: {
      'content-type': 'application/json',
      'x-hivemind-proxy-secret': 'spoofed-by-the-visitor',
      'x-real-ip': '203.0.113.9',
    },
    body: '{"username":"alice"}',
  });

  assert.equal(response.status, 200);
  assert.equal(response.headers['cache-control'], 'no-store');
  assert.deepEqual(response.headers['set-cookie'], ['hivemind_session=abc; Path=/api; HttpOnly', 'theme=dark; Path=/']);
  assert.equal(upstreamRequests.length, 1);
  assert.equal(upstreamRequests[0].method, 'POST');
  assert.equal(upstreamRequests[0].url, '/api/login?source=site');
  assert.equal(upstreamRequests[0].secret, PROXY_SECRET);
  assert.notEqual(upstreamRequests[0].realIp, '203.0.113.9');
  assert.equal(upstreamRequests[0].body, '{"username":"alice"}');
});

test('refuses methods the account API does not use', async (t) => {
  const directory = await makeExport(t);
  const server = createStaticExportServer({
    directory,
    env: { API_ORIGIN: 'http://127.0.0.1:9', PROXY_SHARED_SECRET: PROXY_SECRET },
  });
  t.after(() => close(server));
  const base = await listen(server);

  const response = await rawRequest(base, { method: 'PUT', path: '/api/account' });
  assert.equal(response.status, 405);
  assert.equal(response.headers.allow, 'GET, POST');
});

test('answers HEAD for pages without a body', async (t) => {
  const directory = await makeExport(t);
  const server = createStaticExportServer({ directory, env: {} });
  t.after(() => close(server));
  const base = await listen(server);

  const response = await rawRequest(base, { method: 'HEAD', path: '/docs/loops' });
  assert.equal(response.status, 200);
  assert.equal(response.body, '');
});

test('parses the command line and rejects unusable values', () => {
  assert.deepEqual(parseArgs(['--directory', 'out', '--port', '4175'], {}), {
    directory: 'out',
    host: '127.0.0.1',
    port: 4175,
  });
  assert.deepEqual(parseArgs([], { HOST: '0.0.0.0', PORT: '3000' }), { directory: 'out', host: '0.0.0.0', port: 3000 });
  assert.throws(() => parseArgs(['--port']), /missing value/);
  assert.throws(() => parseArgs(['--port', '70000']), /invalid port/);
  assert.throws(() => parseArgs(['--unknown', 'x']), /unknown option/);
});
