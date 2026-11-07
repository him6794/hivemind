// Cloudflare Turnstile check for the website backend. The browser obtains a token from
// the widget, and this module asks Cloudflare whether the token is valid. The check
// fails closed: a missing secret, a missing token, a rejected token, or an unreachable
// verifier all refuse the request before Nodepool is called.
export const TURNSTILE_VERIFY_URL = 'https://challenges.cloudflare.com/turnstile/v0/siteverify';

const MAX_TOKEN_LENGTH = 2048;
const VERIFY_TIMEOUT_MS = 5000;

/**
 * @param {{
 *   secret: string | undefined,
 *   token: unknown,
 *   remoteIp?: string,
 *   fetchImpl?: typeof fetch,
 * }} input
 * @returns {Promise<
 *   { ok: true } | { ok: false, reason: 'not_configured' | 'missing_token' | 'rejected' | 'unreachable' }
 * >}
 */
export async function verifyTurnstileToken({ secret, token, remoteIp, fetchImpl = globalThis.fetch }) {
  const secretValue = typeof secret === 'string' ? secret.trim() : '';
  if (!secretValue) return { ok: false, reason: 'not_configured' };

  const response = typeof token === 'string' ? token.trim() : '';
  if (!response || response.length > MAX_TOKEN_LENGTH) return { ok: false, reason: 'missing_token' };

  const form = new URLSearchParams({ secret: secretValue, response });
  if (remoteIp) form.set('remoteip', remoteIp);

  let reply;
  try {
    reply = await fetchImpl(TURNSTILE_VERIFY_URL, {
      method: 'POST',
      body: form,
      signal: AbortSignal.timeout(VERIFY_TIMEOUT_MS),
    });
  } catch {
    return { ok: false, reason: 'unreachable' };
  }
  if (!reply.ok) return { ok: false, reason: 'unreachable' };

  let outcome;
  try {
    outcome = await reply.json();
  } catch {
    return { ok: false, reason: 'unreachable' };
  }
  return outcome?.success === true ? { ok: true } : { ok: false, reason: 'rejected' };
}
