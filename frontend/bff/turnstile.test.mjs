import assert from 'node:assert/strict';
import test from 'node:test';
import { TURNSTILE_VERIFY_URL, verifyTurnstileToken } from './turnstile.mjs';

// Cloudflare's documented dummy secret keys. They are used in tests only.
const ALWAYS_PASS_SECRET = '1x0000000000000000000000000000000AA';
const ALWAYS_FAIL_SECRET = '2x0000000000000000000000000000000AA';

// Stands in for the siteverify endpoint. The dummy secret decides the outcome.
function fakeSiteverify(calls) {
  return async (url, init) => {
    calls.push({ url, method: init.method, form: init.body, signal: init.signal });
    const success = init.body.get('secret') === ALWAYS_PASS_SECRET;
    return Response.json({ success, 'error-codes': success ? [] : ['invalid-input-secret'] });
  };
}

test('the always-pass test secret accepts a token and posts secret and response to siteverify', async () => {
  const calls = [];
  const result = await verifyTurnstileToken({
    secret: ALWAYS_PASS_SECRET,
    token: 'XXXX.DUMMY.TOKEN.XXXX',
    fetchImpl: fakeSiteverify(calls),
  });

  assert.deepEqual(result, { ok: true });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, TURNSTILE_VERIFY_URL);
  assert.equal(calls[0].url, 'https://challenges.cloudflare.com/turnstile/v0/siteverify');
  assert.equal(calls[0].method, 'POST');
  assert.equal(calls[0].form.get('secret'), ALWAYS_PASS_SECRET);
  assert.equal(calls[0].form.get('response'), 'XXXX.DUMMY.TOKEN.XXXX');
  assert.equal(calls[0].form.has('remoteip'), false);
  assert.ok(calls[0].signal, 'the request must carry a timeout signal');
});

test('the always-fail test secret rejects the token', async () => {
  const result = await verifyTurnstileToken({
    secret: ALWAYS_FAIL_SECRET,
    token: 'XXXX.DUMMY.TOKEN.XXXX',
    fetchImpl: fakeSiteverify([]),
  });
  assert.deepEqual(result, { ok: false, reason: 'rejected' });
});

test('the client address is sent as remoteip only when it is known', async () => {
  const calls = [];
  await verifyTurnstileToken({
    secret: ALWAYS_PASS_SECRET,
    token: 'token',
    remoteIp: '203.0.113.7',
    fetchImpl: fakeSiteverify(calls),
  });
  assert.equal(calls[0].form.get('remoteip'), '203.0.113.7');
});

test('a missing or blank secret fails closed without calling siteverify', async () => {
  for (const secret of [undefined, null, '', '   ']) {
    const calls = [];
    const result = await verifyTurnstileToken({ secret, token: 'token', fetchImpl: fakeSiteverify(calls) });
    assert.deepEqual(result, { ok: false, reason: 'not_configured' }, String(secret));
    assert.equal(calls.length, 0);
  }
});

test('a missing, blank, or oversized token is refused without calling siteverify', async () => {
  for (const token of [undefined, null, '', '   ', 42, 'x'.repeat(2049)]) {
    const calls = [];
    const result = await verifyTurnstileToken({
      secret: ALWAYS_PASS_SECRET,
      token,
      fetchImpl: fakeSiteverify(calls),
    });
    assert.deepEqual(result, { ok: false, reason: 'missing_token' }, String(token).slice(0, 20));
    assert.equal(calls.length, 0);
  }
});

test('a verifier that cannot be reached or answers unexpectedly fails closed as unreachable', async () => {
  const secret = ALWAYS_PASS_SECRET;
  const token = 'token';

  const throwing = async () => {
    throw new TypeError('fetch failed');
  };
  assert.deepEqual(await verifyTurnstileToken({ secret, token, fetchImpl: throwing }), {
    ok: false,
    reason: 'unreachable',
  });

  const serverError = async () => new Response('busy', { status: 503 });
  assert.deepEqual(await verifyTurnstileToken({ secret, token, fetchImpl: serverError }), {
    ok: false,
    reason: 'unreachable',
  });

  const notJson = async () => new Response('<html>', { status: 200 });
  assert.deepEqual(await verifyTurnstileToken({ secret, token, fetchImpl: notJson }), {
    ok: false,
    reason: 'unreachable',
  });
});

test('only a boolean true success counts as a pass', async () => {
  const stringTrue = async () => Response.json({ success: 'true' });
  const missing = async () => Response.json({});
  for (const fetchImpl of [stringTrue, missing]) {
    const result = await verifyTurnstileToken({ secret: ALWAYS_PASS_SECRET, token: 'token', fetchImpl });
    assert.deepEqual(result, { ok: false, reason: 'rejected' });
  }
});
