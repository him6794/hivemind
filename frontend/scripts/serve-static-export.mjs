// Serves the official site's static export (`next build` with output: "export") the way Cloudflare Pages does.
// `/docs/loops` resolves to `docs/loops.html`, and an unknown path gets `404.html`. Requests to `/api/*` are
// forwarded to the website backend under the same rules as functions/api/[[path]].js, and they answer 503 until
// both API_ORIGIN and PROXY_SHARED_SECRET are set. The UI tests, the release smoke harness, and the Docker image
// all use this server.
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const CONTENT_TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.webmanifest': 'application/manifest+json',
};

// Match public/_headers when serving outside Cloudflare Pages.
const RESPONSE_POLICY = {
  'X-Content-Type-Options': 'nosniff',
  'X-Frame-Options': 'DENY',
  'Referrer-Policy': 'strict-origin-when-cross-origin',
  'Permissions-Policy': 'camera=(), microphone=(), geolocation=(), payment=()',
  'Content-Security-Policy': "frame-ancestors 'none'; base-uri 'self'; form-action 'self'",
};

const FORWARDED_METHODS = new Set(['GET', 'POST']);
// Headers that describe one connection, or that the proxy sets itself. They are never copied.
const HOP_BY_HOP = new Set([
  'connection',
  'content-encoding',
  'content-length',
  'host',
  'keep-alive',
  'proxy-authenticate',
  'proxy-authorization',
  'te',
  'trailer',
  'transfer-encoding',
  'upgrade',
]);
const PROXY_SECRET_HEADER = 'x-hivemind-proxy-secret';
// The account routes accept far less than this. The cap keeps a large upload from being buffered.
const MAX_PROXY_BODY_BYTES = 64 * 1024;

// Only https is accepted. Plain http is allowed only for a loopback backend. Anything else returns null.
export function configuredOrigin(env) {
  const origin = String(env.API_ORIGIN ?? '').trim().replace(/\/+$/, '');
  if (/^https:\/\/[^/\s]+$/.test(origin) || /^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?$/.test(origin)) {
    return origin;
  }
  return null;
}

function isInside(root, file) {
  return file === root || file.startsWith(root + path.sep);
}

async function isFile(file) {
  try {
    return (await fs.stat(file)).isFile();
  } catch {
    return false;
  }
}

// Finds the file for a request path: the path itself, then `<path>.html`, then `<path>/index.html`.
// A path that resolves outside the export directory never matches.
async function staticFileFor(root, pathname) {
  let decoded;
  try {
    decoded = decodeURIComponent(pathname);
  } catch {
    return null;
  }
  const relative = path.posix.normalize(`/${decoded}`).replace(/^\/+|\/+$/g, '');
  const candidates =
    relative === ''
      ? [path.join(root, 'index.html')]
      : [path.join(root, relative), path.join(root, `${relative}.html`), path.join(root, relative, 'index.html')];
  for (const candidate of candidates) {
    if (!isInside(root, candidate)) return null;
    if (await isFile(candidate)) return candidate;
  }
  return null;
}

function sendJson(response, status, body, headers = {}) {
  const payload = JSON.stringify(body);
  response.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Cache-Control': 'no-store',
    'Content-Length': Buffer.byteLength(payload),
    ...headers,
  });
  response.end(payload);
}

function sendError(response, status, code, message, headers = {}) {
  sendJson(response, status, { success: false, code, message, status_message: message }, headers);
}

async function sendFile(request, response, file, status) {
  const body = await fs.readFile(file);
  response.writeHead(status, {
    'Content-Type': CONTENT_TYPES[path.extname(file)] ?? 'application/octet-stream',
    'Content-Length': body.length,
    'Cache-Control': 'no-cache',
  });
  response.end(request.method === 'HEAD' ? undefined : body);
}

class BodyTooLargeError extends Error {}

function readBody(request, limit) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let total = 0;
    request.on('data', (chunk) => {
      total += chunk.length;
      if (total > limit) {
        request.pause();
        reject(new BodyTooLargeError('body too large'));
        return;
      }
      chunks.push(chunk);
    });
    request.on('end', () => resolve(Buffer.concat(chunks)));
    request.on('error', reject);
  });
}

async function forwardAccountRequest(request, response, url, env) {
  if (!FORWARDED_METHODS.has(request.method)) {
    sendError(response, 405, 'method_not_allowed', 'Method not allowed.', { Allow: 'GET, POST' });
    return;
  }
  const origin = configuredOrigin(env);
  const secret = String(env.PROXY_SHARED_SECRET ?? '').trim();
  if (!origin || !secret) {
    sendError(response, 503, 'service_unavailable', 'Account service is unavailable.');
    return;
  }

  let body;
  if (request.method === 'POST') {
    try {
      body = await readBody(request, MAX_PROXY_BODY_BYTES);
    } catch (error) {
      if (!(error instanceof BodyTooLargeError)) throw error;
      sendError(response, 413, 'request_too_large', 'The request is too large.', { Connection: 'close' });
      return;
    }
  }

  // The client's address comes from this socket, never from a header the client sent.
  const headers = new Headers();
  for (const [name, value] of Object.entries(request.headers)) {
    if (value === undefined || HOP_BY_HOP.has(name) || name === 'x-real-ip' || name === PROXY_SECRET_HEADER) continue;
    for (const item of Array.isArray(value) ? value : [value]) headers.append(name, item);
  }
  if (request.socket.remoteAddress) headers.set('x-real-ip', request.socket.remoteAddress);
  headers.set(PROXY_SECRET_HEADER, secret);

  let upstream;
  try {
    upstream = await fetch(`${origin}${url.pathname}${url.search}`, {
      method: request.method,
      headers,
      body,
      redirect: 'manual',
    });
  } catch {
    sendError(response, 502, 'service_unavailable', 'Account service is unavailable.');
    return;
  }

  // Session cookies come back as separate Set-Cookie lines, so they are copied one by one.
  const outgoing = {};
  for (const [name, value] of upstream.headers) {
    if (!HOP_BY_HOP.has(name) && name !== 'set-cookie') outgoing[name] = value;
  }
  const cookies = upstream.headers.getSetCookie();
  if (cookies.length > 0) outgoing['set-cookie'] = cookies;
  outgoing['cache-control'] = 'no-store';
  const payload = Buffer.from(await upstream.arrayBuffer());
  outgoing['content-length'] = payload.length;
  response.writeHead(upstream.status, outgoing);
  response.end(payload);
}

async function handleRequest(request, response, options) {
  const url = new URL(request.url ?? '/', 'http://localhost');
  if (url.pathname === '/api' || url.pathname.startsWith('/api/')) {
    await forwardAccountRequest(request, response, url, options.env);
    return;
  }
  if (request.method !== 'GET' && request.method !== 'HEAD') {
    sendError(response, 405, 'method_not_allowed', 'Method not allowed.', { Allow: 'GET, HEAD' });
    return;
  }
  const file = await staticFileFor(options.root, url.pathname);
  if (file) {
    await sendFile(request, response, file, 200);
    return;
  }
  const notFound = path.join(options.root, '404.html');
  if (await isFile(notFound)) {
    await sendFile(request, response, notFound, 404);
    return;
  }
  sendError(response, 404, 'not_found', 'Not found.');
}

export function createStaticExportServer({ directory, env = process.env }) {
  const options = { root: path.resolve(directory), env };
  const server = http.createServer((request, response) => {
    for (const [name, value] of Object.entries(RESPONSE_POLICY)) response.setHeader(name, value);
    handleRequest(request, response, options).catch((error) => {
      // Log the path and error name only. Request bodies can hold passwords.
      console.error('static export request failed', { path: request.url?.split('?')[0], name: error?.name });
      if (response.headersSent) {
        response.destroy();
      } else {
        sendError(response, 500, 'internal_error', 'Internal server error.');
      }
    });
  });
  server.requestTimeout = 30_000;
  return server;
}

export function parseArgs(argv, env = process.env) {
  const options = {
    directory: 'out',
    host: env.HOST || '127.0.0.1',
    port: Number(env.PORT || 3000),
  };
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (value === undefined) throw new Error(`missing value for ${flag}`);
    if (flag === '--directory') options.directory = value;
    else if (flag === '--host') options.host = value;
    else if (flag === '--port') options.port = Number(value);
    else throw new Error(`unknown option ${flag}`);
  }
  if (!Number.isInteger(options.port) || options.port < 0 || options.port > 65535) {
    throw new Error(`invalid port ${options.port}`);
  }
  return options;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const root = path.resolve(options.directory);
  // Refuse to start without an export, so a missing `npm run build` cannot pass as a server full of 404s.
  if (!(await isFile(path.join(root, 'index.html')))) {
    throw new Error(`No static export at ${root}. Run "npm run build" first.`);
  }
  const server = createStaticExportServer({ directory: root });
  server.listen(options.port, options.host, () => {
    console.log(`static export listening on ${options.host}:${options.port}`);
  });
  process.on('SIGTERM', () => server.close(() => process.exit(0)));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
