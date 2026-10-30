// Same-origin entry for the account API. The browser calls /api/* on the site's own
// domain; this function forwards each call, with the same path and query, to the
// website backend named by the API_ORIGIN variable. Passwords pass through here, so
// the function refuses to forward anywhere it has not been told to go.
const FORWARDED_METHODS = new Set(['GET', 'POST']);

// Headers that describe a single connection and must not be forwarded.
const HOP_BY_HOP = new Set([
  'connection',
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

function jsonError(status, code, message, headers = {}) {
  return Response.json(
    { success: false, code, message, status_message: message },
    { status, headers: { 'Cache-Control': 'no-store', ...headers } },
  );
}

function withoutHopByHop(headers) {
  const kept = new Headers();
  for (const [name, value] of headers) {
    if (!HOP_BY_HOP.has(name.toLowerCase())) kept.set(name, value);
  }
  return kept;
}

// The backend accepts account requests only from this function. Each call carries the
// PROXY_SHARED_SECRET value; without it the function does not forward anything.
const PROXY_SECRET_HEADER = 'X-Hivemind-Proxy-Secret';

// Only https is accepted. Plain http is allowed for a loopback backend during local
// development. Anything else, including an unset variable, returns null.
function configuredOrigin(env) {
  const origin = String(env.API_ORIGIN ?? '').trim().replace(/\/+$/, '');
  if (/^https:\/\/[^/\s]+$/.test(origin) || /^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?$/.test(origin)) {
    return origin;
  }
  return null;
}

export async function onRequest({ request, env }) {
  if (!FORWARDED_METHODS.has(request.method)) {
    return jsonError(405, 'method_not_allowed', 'Method not allowed.', { Allow: 'GET, POST' });
  }

  const origin = configuredOrigin(env);
  const secret = String(env.PROXY_SHARED_SECRET ?? '').trim();
  if (!origin || !secret) {
    return jsonError(503, 'service_unavailable', 'Account service is unavailable.');
  }

  const url = new URL(request.url);

  // The visitor's address comes from Cloudflare, never from the client. Overwrite
  // any value the client sent, so the backend can trust X-Real-IP from this hop.
  const headers = withoutHopByHop(request.headers);
  const clientAddress = request.headers.get('CF-Connecting-IP');
  if (clientAddress) {
    headers.set('X-Real-IP', clientAddress);
  } else {
    headers.delete('X-Real-IP');
  }
  // Replaces any value the client sent under the same name.
  headers.set(PROXY_SECRET_HEADER, secret);

  try {
    const upstream = await fetch(`${origin}${url.pathname}${url.search}`, {
      method: request.method,
      headers,
      body: request.method === 'POST' ? request.body : undefined,
      redirect: 'manual',
    });
    const responseHeaders = withoutHopByHop(upstream.headers);
    responseHeaders.set('Cache-Control', 'no-store');
    return new Response(upstream.body, {
      status: upstream.status,
      statusText: upstream.statusText,
      headers: responseHeaders,
    });
  } catch {
    return jsonError(502, 'service_unavailable', 'Account service is unavailable.');
  }
}
