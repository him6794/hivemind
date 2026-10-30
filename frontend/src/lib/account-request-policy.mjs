// Request parsing and client identification for the website account proxy.

export class RequestBodyError extends Error {
  constructor(reason) {
    super(reason);
    this.name = 'RequestBodyError';
    this.reason = reason;
  }
}

/**
 * Reads a JSON object from the request body and refuses bodies over `maxBytes`.
 * The limit is enforced while reading, so a missing or wrong Content-Length
 * cannot make the server buffer an unbounded body.
 */
export async function readJsonObject(request, maxBytes) {
  const declared = request.headers.get('content-length');
  if (declared !== null && Number(declared) > maxBytes) {
    throw new RequestBodyError('too_large');
  }
  if (!request.body) {
    throw new RequestBodyError('invalid');
  }

  const reader = request.body.getReader();
  const chunks = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > maxBytes) {
      await reader.cancel();
      throw new RequestBodyError('too_large');
    }
    chunks.push(value);
  }

  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }

  let parsed;
  try {
    parsed = JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    throw new RequestBodyError('invalid');
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new RequestBodyError('invalid');
  }
  return parsed;
}

/** Returned by clientAddress when no trusted header identifies the visitor. */
export const UNKNOWN_CLIENT_ADDRESS = 'unknown';

/**
 * The address used for rate limiting.
 *
 * Only the header named by the deployment (WEBSITE_CLIENT_IP_HEADER) is read.
 * The proxy in front of this server must write that header from the connection
 * it received:
 *   x-real-ip        the proxy overwrites it with the visitor's address
 *   x-forwarded-for  the proxy appends the visitor's address; the last entry is read
 * Any other header is ignored. A client that reaches the server directly can
 * send any value in these headers, so they are not trusted unless configured.
 */
export function clientAddress(request, configuredHeader) {
  const header = typeof configuredHeader === 'string' ? configuredHeader.trim().toLowerCase() : '';

  if (header === 'x-real-ip') {
    return request.headers.get('x-real-ip')?.trim() || UNKNOWN_CLIENT_ADDRESS;
  }
  if (header === 'x-forwarded-for') {
    const last = (request.headers.get('x-forwarded-for') ?? '')
      .split(',')
      .map((part) => part.trim())
      .filter(Boolean)
      .pop();
    return last || UNKNOWN_CLIENT_ADDRESS;
  }
  return UNKNOWN_CLIENT_ADDRESS;
}
