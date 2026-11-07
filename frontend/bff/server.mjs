// Process entry for the website backend. Run it beside Nodepool; the reverse proxy
// in front of it forwards the account API (/api/*, see ACCOUNT_API_PATHS) here, and
// /health is for host checks.
import http from 'node:http';
import { timingSafeEqual } from 'node:crypto';
import { ACCOUNT_API_PATHS, handleAccountRequest } from './account-api.mjs';

const HOST = process.env.HOST ?? '127.0.0.1';
const PORT = Number(process.env.PORT ?? 3100);

// The backend is reachable through a public hostname, so it accepts account requests
// only from the Pages function, which holds this secret and sends it on every call.
// The secret is required and must be long, so the server never starts without it.
const PROXY_SECRET = (process.env.PROXY_SHARED_SECRET ?? '').trim();
if (PROXY_SECRET.length < 32) {
  console.error('PROXY_SHARED_SECRET must be set to at least 32 characters; refusing to start.');
  process.exit(1);
}
const PROXY_SECRET_BYTES = Buffer.from(PROXY_SECRET, 'utf8');

function fromPagesFunction(headers) {
  const given = Buffer.from(String(headers['x-hivemind-proxy-secret'] ?? ''), 'utf8');
  return given.length === PROXY_SECRET_BYTES.length && timingSafeEqual(given, PROXY_SECRET_BYTES);
}

const ACCOUNT_PATHS = new Set(ACCOUNT_API_PATHS);
// The account routes accept far less than this (MAX_BODY_BYTES in account-api.mjs).
// The cap keeps a large upload from being buffered before any route checks it.
const MAX_BUFFERED_BYTES = 64 * 1024;

class BodyTooLargeError extends Error {}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let total = 0;
    req.on('data', (chunk) => {
      total += chunk.length;
      if (total > MAX_BUFFERED_BYTES) {
        req.pause();
        reject(new BodyTooLargeError('body too large'));
        return;
      }
      chunks.push(chunk);
    });
    req.on('end', () => resolve(Buffer.concat(chunks)));
    req.on('error', reject);
  });
}

function toHeaders(nodeHeaders) {
  const headers = new Headers();
  for (const [name, value] of Object.entries(nodeHeaders)) {
    if (value === undefined) continue;
    for (const item of Array.isArray(value) ? value : [value]) {
      headers.append(name, item);
    }
  }
  return headers;
}

async function send(res, response) {
  const body = Buffer.from(await response.arrayBuffer());
  res.writeHead(response.status, Object.fromEntries(response.headers));
  res.end(body);
}

async function handle(req, res) {
  const url = new URL(req.url ?? '/', 'http://localhost');

  if (req.method === 'GET' && url.pathname === '/health') {
    res.writeHead(200, { 'Content-Type': 'text/plain; charset=utf-8' });
    res.end('OK');
    return;
  }
  // Checked before the body is read, so a caller without the secret cannot make the server buffer anything.
  if (!fromPagesFunction(req.headers)) {
    await send(res, Response.json({ success: false, code: 'forbidden', message: 'Forbidden.', status_message: 'Forbidden.' }, { status: 403 }));
    return;
  }
  if (!ACCOUNT_PATHS.has(url.pathname)) {
    await send(res, Response.json({ success: false, message: 'Not found' }, { status: 404 }));
    return;
  }

  const hasBody = req.method !== 'GET' && req.method !== 'HEAD';
  let body;
  try {
    body = hasBody ? await readBody(req) : undefined;
  } catch (error) {
    if (error instanceof BodyTooLargeError) {
      // Close the connection after the refusal, so the rest of the upload is dropped.
      res.writeHead(413, { 'Content-Type': 'application/json', Connection: 'close' });
      res.end(
        JSON.stringify({
          success: false,
          code: 'request_too_large',
          message: 'The request is too large.',
          status_message: 'The request is too large.',
        })
      );
      res.on('finish', () => req.socket.destroy());
      return;
    }
    throw error;
  }

  const request = new Request(url, { method: req.method, headers: toHeaders(req.headers), body });
  await send(res, await handleAccountRequest(request));
}

const server = http.createServer((req, res) => {
  handle(req, res).catch((error) => {
    // Log the path and error name only. Request bodies can hold passwords.
    console.error('website backend request failed', { path: req.url?.split('?')[0], name: error?.name });
    if (!res.headersSent) {
      const message = 'Account service is unavailable.';
      res.writeHead(500, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ success: false, code: 'service_unavailable', message, status_message: message }));
    }
  });
});

server.requestTimeout = 30_000;

server.listen(PORT, HOST, () => {
  console.log(`website backend listening on ${HOST}:${PORT}`);
});

process.on('SIGTERM', () => {
  server.close(() => process.exit(0));
});
