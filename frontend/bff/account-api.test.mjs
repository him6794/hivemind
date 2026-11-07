import assert from 'node:assert/strict';
import test from 'node:test';
import * as grpc from '@grpc/grpc-js';
import { ACCOUNT_API_PATHS, createAccountApi } from './account-api.mjs';

// Cloudflare's documented dummy secret keys. They are used in tests only.
const ALWAYS_PASS_SECRET = '1x0000000000000000000000000000000AA';
const ALWAYS_FAIL_SECRET = '2x0000000000000000000000000000000AA';
const ORIGIN = 'http://127.0.0.1:3100';
const CAPTCHA = 'widget-token-from-the-browser';

const VALID_REGISTRATION = {
  username: 'alice',
  password: 'correct horse',
  email: 'alice@example.com',
  captchaToken: CAPTCHA,
};

// Stands in for siteverify. The dummy secret decides the outcome, as Cloudflare does.
function fakeSiteverify(calls) {
  return async (url, init) => {
    calls.push({ url, form: init.body });
    return Response.json({ success: init.body.get('secret') === ALWAYS_PASS_SECRET });
  };
}

// A user service that records every call. Replies use the snake_case field names that
// keepCase produces. An Error value is thrown instead of returned.
function fakeUserService(replies) {
  const calls = [];
  const callUserService = async (method, request) => {
    calls.push({ method, request });
    const reply = replies[method] ?? { success: true };
    if (reply instanceof Error) throw reply;
    return reply;
  };
  return { calls, callUserService };
}

function makeApi({ env = { TURNSTILE_SECRET_KEY: ALWAYS_PASS_SECRET }, replies = {}, siteverifyCalls = [] } = {}) {
  const service = fakeUserService(replies);
  const api = createAccountApi({
    env,
    callUserService: service.callUserService,
    fetchImpl: fakeSiteverify(siteverifyCalls),
  });
  return { api, calls: service.calls, siteverifyCalls };
}

function jsonRequest(path, body, { method = 'POST', headers = {} } = {}) {
  return new Request(`${ORIGIN}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...headers },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

function bearer(token) {
  return { authorization: `Bearer ${token}` };
}

async function send(api, request) {
  const response = await api.handle(request);
  return { status: response.status, body: await response.json(), headers: response.headers };
}

function grpcError(message, code) {
  return Object.assign(new Error(message), { code });
}

test('the account allowlist lists every route under /api/ and nothing else', () => {
  assert.deepEqual(
    new Set(ACCOUNT_API_PATHS),
    new Set([
      '/api/register',
      '/api/login',
      '/api/logout',
      '/api/balance',
      '/api/account',
      '/api/verify-email',
      '/api/resend-verification',
      '/api/bind-email',
      '/api/change-password',
      '/api/transfer',
      '/api/forgot-password',
      '/api/reset-password',
      '/api/forgot-username',
      '/api/check-email',
    ])
  );
  assert.equal(ACCOUNT_API_PATHS.length, 14);
  assert.ok(Object.isFrozen(ACCOUNT_API_PATHS));
});

test('email binding requires a session, same-site request, password and captcha', async () => {
  const body = { email: ' legacy@example.com ', currentPassword: ' correct horse ', captchaToken: CAPTCHA };
  const headers = { cookie: 'hivemind_session=owner-session' };
  const { api, calls } = makeApi();
  assert.equal((await send(api, jsonRequest('/api/bind-email', body))).status, 401);
  assert.equal((await send(api, jsonRequest('/api/bind-email', body, { headers: { ...headers, 'sec-fetch-site': 'cross-site' } }))).status, 403);
  assert.equal((await send(api, jsonRequest('/api/bind-email', { ...body, currentPassword: '' }, { headers }))).body.code, 'password_required');
  assert.equal((await send(api, jsonRequest('/api/bind-email', { ...body, captchaToken: '' }, { headers }))).body.code, 'captcha_failed');
  assert.equal(calls.length, 0);
  const result = await send(api, jsonRequest('/api/bind-email', body, { headers }));
  assert.equal(result.status, 200);
  assert.deepEqual(calls, [{ method: 'BindEmail', request: { token: 'owner-session', email: 'legacy@example.com', current_password: 'correct horse' } }]);
  assert.equal(result.body.token, undefined);
});

test('email binding reports duplicate addresses and limits password attempts', async () => {
  const { api, calls } = makeApi({ replies: { BindEmail: { success: false, status_message: 'This email address is already in use.' } } });
  const body = { email: 'taken@example.com', currentPassword: 'correct horse', captchaToken: CAPTCHA };
  for (let attempt = 0; attempt < 5; attempt++) {
    const result = await send(api, jsonRequest('/api/bind-email', body, { headers: bearer('session') }));
    assert.equal(result.body.code, 'email_in_use');
  }
  assert.equal((await send(api, jsonRequest('/api/bind-email', body, { headers: bearer('session') }))).status, 429);
  assert.equal(calls.length, 5);
});

test('login and balance keep their behaviour', async () => {
  const { api, calls } = makeApi({
    replies: {
      Login: { success: true, token: 'session-token' },
      GetBalance: { success: true, balance: '1250' },
    },
  });

  const login = await send(api, jsonRequest('/api/login', { username: ' alice ', password: '  correct horse  ', captchaToken: CAPTCHA }));
  assert.equal(login.status, 200);
  assert.equal(login.body.token, undefined, 'the token must not be returned to page scripts');
  assert.equal(login.body.username, 'alice');
  const cookie = login.headers.get('set-cookie');
  assert.match(cookie, /^hivemind_session=session-token;/);
  assert.match(cookie, /HttpOnly/);
  assert.match(cookie, /Secure/);
  assert.match(cookie, /SameSite=Lax/);
  assert.match(cookie, /Path=\/api/);
  assert.match(cookie, /Max-Age=86400/);
  assert.deepEqual(calls[0], { method: 'Login', request: { username: 'alice', password: 'correct horse' } });

  const balance = await send(api, new Request(`${ORIGIN}/api/balance`, { headers: { cookie: 'hivemind_session=session-token' } }));
  assert.equal(balance.status, 200);
  assert.equal(balance.body.balance, 1250);
  assert.deepEqual(calls[1], { method: 'GetBalance', request: { username: '', token: 'session-token' } });
});

test('a cookie session refuses cross-site requests, and logout clears the cookie', async () => {
  const { api, calls } = makeApi();
  const crossSite = await send(
    api,
    new Request(`${ORIGIN}/api/transfer`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', cookie: 'hivemind_session=session-token', 'sec-fetch-site': 'cross-site' },
      body: JSON.stringify({ toUsername: 'bob', amountCpt: 1 }),
    })
  );
  assert.equal(crossSite.status, 403);
  assert.equal(crossSite.body.code, 'cross_site_refused');
  assert.equal(calls.length, 0);

  const logout = await send(
    api,
    new Request(`${ORIGIN}/api/logout`, {
      method: 'POST',
      headers: { cookie: 'hivemind_session=session-token', 'sec-fetch-site': 'same-origin' },
    })
  );
  assert.equal(logout.status, 200);
  assert.match(logout.headers.get('set-cookie'), /^hivemind_session=;.*Max-Age=0/);
});

test('logout asks Nodepool to end the session, and clears the cookie even when that call fails', async () => {
  const signedOut = makeApi({ replies: { Logout: { success: true } } });
  const first = await send(
    signedOut.api,
    new Request(`${ORIGIN}/api/logout`, {
      method: 'POST',
      headers: { cookie: 'hivemind_session=session-token', 'sec-fetch-site': 'same-origin' },
    })
  );
  assert.equal(first.status, 200);
  assert.deepEqual(
    signedOut.calls.filter((call) => call.method === 'Logout'),
    [{ method: 'Logout', request: { token: 'session-token' } }]
  );

  const unreachable = makeApi({ replies: { Logout: grpcError('nodepool unavailable', grpc.status.UNAVAILABLE) } });
  const second = await send(
    unreachable.api,
    new Request(`${ORIGIN}/api/logout`, {
      method: 'POST',
      headers: { cookie: 'hivemind_session=session-token', 'sec-fetch-site': 'same-origin' },
    })
  );
  assert.equal(second.status, 200);
  assert.match(second.headers.get('set-cookie'), /^hivemind_session=;.*Max-Age=0/);
});

test('register verifies the captcha, then forwards username, password and trimmed email to RegisterUser', async () => {
  const { api, calls, siteverifyCalls } = makeApi();
  const result = await send(
    api,
    jsonRequest('/api/register', { ...VALID_REGISTRATION, email: '  Alice@Example.com ' })
  );

  assert.equal(result.status, 201);
  assert.equal(result.body.success, true);
  assert.equal(siteverifyCalls.length, 1);
  assert.equal(siteverifyCalls[0].form.get('response'), CAPTCHA);
  assert.deepEqual(calls, [
    {
      method: 'RegisterUser',
      request: { username: 'alice', password: 'correct horse', email: 'Alice@Example.com' },
    },
  ]);
});

test('a failed captcha stops registration before Nodepool is called', async () => {
  const { api, calls } = makeApi({ env: { TURNSTILE_SECRET_KEY: ALWAYS_FAIL_SECRET } });
  const result = await send(api, jsonRequest('/api/register', VALID_REGISTRATION));

  assert.equal(result.status, 400);
  assert.equal(result.body.code, 'captcha_failed');
  assert.equal(calls.length, 0);
});

test('a missing captcha token stops sign-in before Nodepool is called', async () => {
  const siteverifyCalls = [];
  const { api, calls } = makeApi({ siteverifyCalls });
  const missing = await send(api, jsonRequest('/api/login', { username: 'alice', password: 'correct horse', captchaToken: '' }));
  assert.equal(missing.status, 400);
  assert.equal(missing.body.code, 'captcha_failed');
  assert.equal(siteverifyCalls.length, 0);
  assert.equal(calls.length, 0);

  const failing = makeApi({ env: { TURNSTILE_SECRET_KEY: ALWAYS_FAIL_SECRET } });
  const refused = await send(failing.api, jsonRequest('/api/login', { username: 'alice', password: 'correct horse', captchaToken: CAPTCHA }));
  assert.equal(refused.body.code, 'captcha_failed');
  assert.equal(failing.calls.length, 0);
});

test('sign-in is limited per username, and attempts that fail the captcha do not count', async () => {
  // The secret is switched on the same API instance, so the limiter state carries over.
  const env = { TURNSTILE_SECRET_KEY: ALWAYS_PASS_SECRET };
  const { api, calls } = makeApi({ env, replies: { Login: { success: false } } });

  env.TURNSTILE_SECRET_KEY = ALWAYS_FAIL_SECRET;
  for (let index = 0; index < 12; index += 1) {
    const refused = await send(api, jsonRequest('/api/login', { username: 'victim', password: 'guess', captchaToken: CAPTCHA }));
    assert.equal(refused.body.code, 'captcha_failed');
  }
  assert.equal(calls.length, 0);

  env.TURNSTILE_SECRET_KEY = ALWAYS_PASS_SECRET;
  for (let index = 0; index < 10; index += 1) {
    const attempt = await send(api, jsonRequest('/api/login', { username: 'victim', password: 'guess', captchaToken: CAPTCHA }));
    assert.equal(attempt.status, 401);
  }
  const limited = await send(api, jsonRequest('/api/login', { username: 'victim', password: 'guess', captchaToken: CAPTCHA }));
  assert.equal(limited.status, 429);
  assert.equal(calls.length, 10);
});

test('successful sign-ins do not count toward the sign-in limit', async () => {
  const { api, calls } = makeApi({ replies: { Login: { success: true, token: 'session-token' } } });
  for (let index = 0; index < 12; index += 1) {
    const signedIn = await send(api, jsonRequest('/api/login', { username: 'alice', password: 'correct horse', captchaToken: CAPTCHA }));
    assert.equal(signedIn.status, 200);
  }
  assert.equal(calls.length, 12);
});

test('a missing captcha token stops registration before Nodepool is called', async () => {
  const siteverifyCalls = [];
  const { api, calls } = makeApi({ siteverifyCalls });
  const result = await send(api, jsonRequest('/api/register', { ...VALID_REGISTRATION, captchaToken: '' }));

  assert.equal(result.status, 400);
  assert.equal(result.body.code, 'captcha_failed');
  assert.equal(siteverifyCalls.length, 0);
  assert.equal(calls.length, 0);
});

test('a missing Turnstile secret fails closed with captcha_not_configured', async () => {
  const { api, calls, siteverifyCalls } = makeApi({ env: {} });
  const result = await send(api, jsonRequest('/api/register', VALID_REGISTRATION));

  assert.equal(result.status, 503);
  assert.equal(result.body.code, 'captcha_not_configured');
  assert.equal(siteverifyCalls.length, 0);
  assert.equal(calls.length, 0);
});

test('registration validates email and password before any captcha or Nodepool call', async () => {
  const cases = [
    [{ email: undefined }, 'invalid_email'],
    [{ email: 'not-an-email' }, 'invalid_email'],
    [{ email: 'alice@localhost' }, 'invalid_email'],
    [{ password: 'short' }, 'password_too_short'],
    [{ password: 'x'.repeat(73) }, 'password_too_long'],
    [{ password: ' long enough' }, 'password_edge_whitespace'],
    [{ password: '   ' }, 'credentials_required'],
  ];
  for (const [override, code] of cases) {
    const { api, calls, siteverifyCalls } = makeApi();
    const result = await send(api, jsonRequest('/api/register', { ...VALID_REGISTRATION, ...override }));
    assert.equal(result.status, 400, code);
    assert.equal(result.body.code, code, code);
    assert.equal(siteverifyCalls.length, 0, code);
    assert.equal(calls.length, 0, code);
  }
});

test('a refused registration does not repeat the account service text', async (t) => {
  t.mock.method(console, 'error', () => {});
  const { api } = makeApi({
    replies: { RegisterUser: { success: false, status_message: 'username alice already exists' } },
  });
  const result = await send(api, jsonRequest('/api/register', VALID_REGISTRATION));

  assert.equal(result.status, 400);
  assert.equal(result.body.code, 'registration_failed');
  assert.doesNotMatch(JSON.stringify(result.body), /already exists/);
});

test('register, verify, and resend name a duplicate username or address as already used', async () => {
  const addressTaken = makeApi({ replies: { RegisterUser: { success: false, status_message: 'This email address is already in use.' } } });
  const emailTaken = await send(addressTaken.api, jsonRequest('/api/register', { ...VALID_REGISTRATION }));
  assert.equal(emailTaken.status, 400);
  assert.equal(emailTaken.body.code, 'email_in_use');
  assert.equal(addressTaken.calls.length, 1);

  const usernameTaken = makeApi({ replies: { RegisterUser: { success: false, status_message: 'Username already exists' } } });
  const nameResult = await send(usernameTaken.api, jsonRequest('/api/register', { ...VALID_REGISTRATION }));
  assert.equal(nameResult.body.code, 'username_taken');

  const verify = makeApi({ replies: { VerifyEmail: { success: false, status_message: 'This email address is already in use.' } } });
  const verifyResult = await send(verify.api, jsonRequest('/api/verify-email', { verificationToken: 'verify-token-123' }));
  assert.equal(verifyResult.body.code, 'email_in_use');

  const resend = makeApi({ replies: { ResendVerification: { success: false, status_message: 'This email address is already in use.' } } });
  const resendResult = await send(
    resend.api,
    jsonRequest('/api/resend-verification', { captchaToken: CAPTCHA }, { headers: bearer('session-token') })
  );
  assert.equal(resendResult.body.code, 'email_in_use');
});

test('verify-email forwards the token and reports the welcome grant status', async () => {
  const { api, calls } = makeApi({
    replies: { VerifyEmail: { success: true, welcome_grant_status: 'granted' } },
  });
  const result = await send(api, jsonRequest('/api/verify-email', { verificationToken: 'verify-token-123' }));

  assert.equal(result.status, 200);
  assert.deepEqual(result.body, { success: true, message: 'Email verified.', welcomeGrantStatus: 'granted' });
  assert.deepEqual(calls, [{ method: 'VerifyEmail', request: { verification_token: 'verify-token-123' } }]);
});

test('verify-email maps unexpected grant values to unknown', async () => {
  const { api } = makeApi({
    replies: { VerifyEmail: { success: true, welcome_grant_status: 'surprise' } },
  });
  const result = await send(api, jsonRequest('/api/verify-email', { verificationToken: 'verify-token-123' }));
  assert.equal(result.body.welcomeGrantStatus, 'unknown');
});

test('verify-email refuses a missing or refused token without repeating the account service text', async (t) => {
  t.mock.method(console, 'error', () => {});
  const missing = makeApi();
  const emptyResult = await send(missing.api, jsonRequest('/api/verify-email', { verificationToken: '  ' }));
  assert.equal(emptyResult.status, 400);
  assert.equal(emptyResult.body.code, 'verification_failed');
  assert.equal(missing.calls.length, 0);

  const refused = makeApi({ replies: { VerifyEmail: { success: false, status_message: 'token expired for alice' } } });
  const result = await send(refused.api, jsonRequest('/api/verify-email', { verificationToken: 'old-token' }));
  assert.equal(result.status, 400);
  assert.equal(result.body.code, 'verification_failed');
  assert.doesNotMatch(JSON.stringify(result.body), /alice/);
});

test('GET account forwards the bearer token and returns the account summary', async () => {
  const { api, calls } = makeApi({
    replies: {
      GetAccount: {
        success: true,
        status_message: 'ok',
        username: 'alice',
        email: 'alice@example.com',
        email_verified: true,
        balance: '1000',
        welcome_grant_status: 'already_granted',
      },
    },
  });
  const result = await send(api, new Request(`${ORIGIN}/api/account`, { headers: bearer('session-token') }));

  assert.equal(result.status, 200);
  assert.deepEqual(result.body, {
    success: true,
    message: 'OK',
    username: 'alice',
    email: 'alice@example.com',
    emailVerified: true,
    balance: 1000,
    welcomeGrantStatus: 'already_granted',
  });
  assert.deepEqual(calls, [{ method: 'GetAccount', request: { token: 'session-token' } }]);
});

test('GET account needs a bearer token, and an expired session is reported as session_expired', async (t) => {
  t.mock.method(console, 'error', () => {});
  const anonymous = makeApi();
  const missing = await send(anonymous.api, new Request(`${ORIGIN}/api/account`));
  assert.equal(missing.status, 401);
  assert.equal(missing.body.code, 'authentication_required');
  assert.equal(anonymous.calls.length, 0);

  const expired = makeApi({
    replies: { GetAccount: grpcError('session unknown', grpc.status.UNAUTHENTICATED) },
  });
  const result = await send(expired.api, new Request(`${ORIGIN}/api/account`, { headers: bearer('stale') }));
  assert.equal(result.status, 401);
  assert.equal(result.body.code, 'session_expired');
});

test('resend-verification needs a session and a captcha, then forwards the session token', async () => {
  const anonymous = makeApi();
  const missing = await send(anonymous.api, jsonRequest('/api/resend-verification', { captchaToken: CAPTCHA }));
  assert.equal(missing.status, 401);
  assert.equal(missing.body.code, 'authentication_required');
  assert.equal(anonymous.calls.length, 0);

  const failing = makeApi({ env: { TURNSTILE_SECRET_KEY: ALWAYS_FAIL_SECRET } });
  const refused = await send(
    failing.api,
    jsonRequest('/api/resend-verification', { captchaToken: CAPTCHA }, { headers: bearer('session-token') })
  );
  assert.equal(refused.body.code, 'captcha_failed');
  assert.equal(failing.calls.length, 0);

  const { api, calls } = makeApi();
  const result = await send(
    api,
    jsonRequest('/api/resend-verification', { captchaToken: CAPTCHA }, { headers: bearer('session-token') })
  );
  assert.equal(result.status, 200);
  assert.equal(result.body.success, true);
  assert.deepEqual(calls, [{ method: 'ResendVerification', request: { token: 'session-token' } }]);
});

test('resend-verification reports a refusal as resend_failed', async (t) => {
  t.mock.method(console, 'error', () => {});
  const { api } = makeApi({
    replies: { ResendVerification: { success: false, status_message: 'already verified' } },
  });
  const result = await send(
    api,
    jsonRequest('/api/resend-verification', { captchaToken: CAPTCHA }, { headers: bearer('session-token') })
  );
  assert.equal(result.status, 400);
  assert.equal(result.body.code, 'resend_failed');
});

test('change-password forwards the trimmed current password and the new password', async () => {
  const { api, calls } = makeApi();
  const result = await send(
    api,
    jsonRequest(
      '/api/change-password',
      { currentPassword: '  old password  ', newPassword: 'new password 2' },
      { headers: bearer('session-token') }
    )
  );

  assert.equal(result.status, 200);
  assert.equal(result.body.success, true);
  assert.deepEqual(calls, [
    {
      method: 'ChangePassword',
      request: { token: 'session-token', current_password: 'old password', new_password: 'new password 2' },
    },
  ]);
});

test('change-password checks the new password and the current password before calling Nodepool', async () => {
  const cases = [
    [{ currentPassword: '   ', newPassword: 'new password 2' }, 'password_required'],
    [{ currentPassword: 'old', newPassword: 'short' }, 'password_too_short'],
    [{ currentPassword: 'old', newPassword: ' new password 2' }, 'password_edge_whitespace'],
  ];
  for (const [body, code] of cases) {
    const { api, calls } = makeApi();
    const result = await send(api, jsonRequest('/api/change-password', body, { headers: bearer('session-token') }));
    assert.equal(result.status, 400, code);
    assert.equal(result.body.code, code, code);
    assert.equal(calls.length, 0, code);
  }

  const anonymous = makeApi();
  const missing = await send(anonymous.api, jsonRequest('/api/change-password', { currentPassword: 'x', newPassword: 'new password 2' }));
  assert.equal(missing.body.code, 'authentication_required');
  assert.equal(anonymous.calls.length, 0);
});

test('change-password limits attempts per session and reports a refused change as change_password_failed', async (t) => {
  t.mock.method(console, 'error', () => {});
  const { api, calls } = makeApi({ replies: { ChangePassword: { success: false, status_message: 'wrong password' } } });
  const attempt = () =>
    send(
      api,
      jsonRequest(
        '/api/change-password',
        { currentPassword: 'guess', newPassword: 'new password 2' },
        { headers: bearer('session-token') }
      )
    );

  for (let index = 0; index < 5; index += 1) {
    const refused = await attempt();
    assert.equal(refused.status, 400);
    assert.equal(refused.body.code, 'change_password_failed');
  }
  const limited = await attempt();
  assert.equal(limited.status, 429);
  assert.equal(limited.body.code, 'rate_limited');
  assert.equal(calls.length, 5);
});

test('transfer forwards the recipient, amount and key, and converts balances to numbers', async () => {
  const { api, calls } = makeApi({
    replies: { TransferCpt: { success: true, from_balance: '900', to_balance: '150', transfer_id: 'tr-1' } },
  });
  const result = await send(
    api,
    jsonRequest(
      '/api/transfer',
      { toUsername: ' bob ', amountCpt: 100, idempotencyKey: 'attempt-0001' },
      { headers: bearer('session-token') }
    )
  );

  assert.equal(result.status, 200);
  assert.deepEqual(result.body, {
    success: true,
    message: 'Transfer completed.',
    fromBalance: 900,
    toBalance: 150,
    transferId: 'tr-1',
  });
  assert.deepEqual(calls, [
    {
      method: 'TransferCpt',
      request: { token: 'session-token', to_username: 'bob', amount_cpt: 100, idempotency_key: 'attempt-0001' },
    },
  ]);
});

test('transfer generates an idempotency key when the caller sends none', async () => {
  const { api, calls } = makeApi();
  await send(api, jsonRequest('/api/transfer', { toUsername: 'bob', amountCpt: 5 }, { headers: bearer('session-token') }));
  assert.match(calls[0].request.idempotency_key, /^[0-9a-f]{32}$/);
});

test('transfer rejects amounts that are not positive safe integers before calling Nodepool', async () => {
  for (const amountCpt of [0, -5, 1.5, '10', Number.MAX_SAFE_INTEGER + 1, null, undefined]) {
    const { api, calls } = makeApi();
    const result = await send(
      api,
      jsonRequest('/api/transfer', { toUsername: 'bob', amountCpt }, { headers: bearer('session-token') })
    );
    assert.equal(result.status, 400, String(amountCpt));
    assert.equal(result.body.code, 'invalid_amount', String(amountCpt));
    assert.equal(calls.length, 0, String(amountCpt));
  }
});

test('transfer rejects a missing recipient and a malformed idempotency key', async () => {
  const { api, calls } = makeApi();
  const noRecipient = await send(
    api,
    jsonRequest('/api/transfer', { toUsername: '   ', amountCpt: 5 }, { headers: bearer('session-token') })
  );
  assert.equal(noRecipient.body.code, 'invalid_recipient');

  const badKey = await send(
    api,
    jsonRequest(
      '/api/transfer',
      { toUsername: 'bob', amountCpt: 5, idempotencyKey: 'has a space' },
      { headers: bearer('session-token') }
    )
  );
  assert.equal(badKey.status, 400);
  assert.equal(badKey.body.code, 'invalid_request');
  assert.equal(calls.length, 0);
});

test('transfer needs a session, and a refused transfer does not repeat the account service text', async (t) => {
  t.mock.method(console, 'error', () => {});
  const anonymous = makeApi();
  const missing = await send(anonymous.api, jsonRequest('/api/transfer', { toUsername: 'bob', amountCpt: 5 }));
  assert.equal(missing.body.code, 'authentication_required');
  assert.equal(anonymous.calls.length, 0);

  const refused = makeApi({ replies: { TransferCpt: { success: false, status_message: 'email must be verified' } } });
  const result = await send(
    refused.api,
    jsonRequest('/api/transfer', { toUsername: 'bob', amountCpt: 5 }, { headers: bearer('session-token') })
  );
  assert.equal(result.status, 400);
  assert.equal(result.body.code, 'transfer_failed');
  assert.doesNotMatch(JSON.stringify(result.body), /email must be verified/);

  const expired = makeApi({ replies: { TransferCpt: grpcError('unknown token', grpc.status.UNAUTHENTICATED) } });
  const stale = await send(
    expired.api,
    jsonRequest('/api/transfer', { toUsername: 'bob', amountCpt: 5 }, { headers: bearer('stale') })
  );
  assert.equal(stale.status, 401);
  assert.equal(stale.body.code, 'session_expired');
});

test('forgot-password checks the captcha, then returns the same generic answer whatever Nodepool says', async () => {
  for (const [index, reply] of [{ success: true }, { success: false, status_message: 'no such account' }].entries()) {
    const email = `reset-${index}@example.com`;
    const { api, calls, siteverifyCalls } = makeApi({ replies: { RequestPasswordReset: reply } });
    const result = await send(api, jsonRequest('/api/forgot-password', { email, captchaToken: CAPTCHA }));

    assert.equal(result.status, 200);
    assert.equal(result.body.success, true);
    assert.match(result.body.message, /If an account uses that email address/);
    assert.doesNotMatch(JSON.stringify(result.body), /no such account/);
    assert.equal(siteverifyCalls.length, 1);
    assert.deepEqual(calls, [{ method: 'RequestPasswordReset', request: { email } }]);
  }
});

test('forgot-password and forgot-username validate the email and the captcha before Nodepool', async () => {
  for (const path of ['/api/forgot-password', '/api/forgot-username']) {
    const invalid = makeApi();
    const badEmail = await send(invalid.api, jsonRequest(path, { email: 'nope', captchaToken: CAPTCHA }));
    assert.equal(badEmail.status, 400);
    assert.equal(badEmail.body.code, 'invalid_email');
    assert.equal(invalid.siteverifyCalls.length, 0);
    assert.equal(invalid.calls.length, 0);

    const failing = makeApi({ env: { TURNSTILE_SECRET_KEY: ALWAYS_FAIL_SECRET } });
    const refused = await send(failing.api, jsonRequest(path, { email: 'alice@example.com', captchaToken: CAPTCHA }));
    assert.equal(refused.body.code, 'captcha_failed');
    assert.equal(failing.calls.length, 0);
  }
});

test('forgot-username asks Nodepool for the username reminder with a generic answer', async () => {
  const { api, calls } = makeApi();
  const result = await send(api, jsonRequest('/api/forgot-username', { email: 'alice@example.com', captchaToken: CAPTCHA }));

  assert.equal(result.status, 200);
  assert.match(result.body.message, /username has been sent/);
  assert.deepEqual(calls, [{ method: 'RequestUsernameReminder', request: { email: 'alice@example.com' } }]);
});

test('recovery requests are limited per email address, after the captcha passes', async () => {
  const siteverifyCalls = [];
  const { api, calls } = makeApi({ siteverifyCalls });
  const send3 = () => send(api, jsonRequest('/api/forgot-password', { email: 'Repeat@Example.com', captchaToken: CAPTCHA }));

  for (let index = 0; index < 3; index += 1) {
    assert.equal((await send3()).status, 200);
  }
  const limited = await send3();
  assert.equal(limited.status, 429);
  assert.equal(limited.body.code, 'rate_limited');
  assert.equal(calls.length, 3);

  // A request without a valid captcha does not count against the address's budget.
  const bogus = makeApi({ env: { TURNSTILE_SECRET_KEY: ALWAYS_FAIL_SECRET } });
  for (let index = 0; index < 4; index += 1) {
    await send(bogus.api, jsonRequest('/api/forgot-password', { email: 'victim@example.com', captchaToken: CAPTCHA }));
  }
  const after = await send(bogus.api, jsonRequest('/api/forgot-password', { email: 'victim@example.com', captchaToken: CAPTCHA }));
  assert.equal(after.body.code, 'captcha_failed');
  assert.equal(bogus.calls.length, 0);
});

test('address limits apply when a proxy supplies the visitor address, and the captcha gets remoteip', async () => {
  const siteverifyCalls = [];
  const { api } = makeApi({
    env: { TURNSTILE_SECRET_KEY: ALWAYS_PASS_SECRET, WEBSITE_CLIENT_IP_HEADER: 'x-real-ip' },
    siteverifyCalls,
  });
  const headers = { 'x-real-ip': '203.0.113.7' };

  for (let index = 0; index < 5; index += 1) {
    const result = await send(
      api,
      jsonRequest('/api/forgot-password', { email: `visitor${index}@example.com`, captchaToken: CAPTCHA }, { headers })
    );
    assert.equal(result.status, 200);
  }
  const limited = await send(
    api,
    jsonRequest('/api/forgot-password', { email: 'visitor9@example.com', captchaToken: CAPTCHA }, { headers })
  );
  assert.equal(limited.status, 429);
  assert.ok(Number(limited.headers.get('retry-after')) >= 1);
  assert.equal(siteverifyCalls[0].form.get('remoteip'), '203.0.113.7');
});

test('the sign-up email check says whether a verified account owns the address, and asks Nodepool only for a well-formed one', async () => {
  const { api, calls } = makeApi({ replies: { CheckEmail: { success: true, available: false } } });

  const malformed = await send(api, jsonRequest('/api/check-email', { email: 'not-an-address' }));
  assert.equal(malformed.status, 400);
  assert.equal(malformed.body.code, 'invalid_email');
  assert.equal(calls.length, 0);

  // The address is trimmed here, and Nodepool lowercases it the way sign-up does.
  const taken = await send(api, jsonRequest('/api/check-email', { email: '  Alice@Example.com ' }));
  assert.equal(taken.status, 200);
  assert.deepEqual(taken.body, { success: true, available: false, verified: false });
  assert.deepEqual(calls, [{ method: 'CheckEmail', request: { email: 'Alice@Example.com' } }]);
});

test('the email check says when the account that uses the address has verified it', async () => {
  const { api } = makeApi({ replies: { CheckEmail: { success: true, available: false, verified: true } } });
  const result = await send(api, jsonRequest('/api/check-email', { email: 'alice@example.com' }));
  assert.equal(result.status, 200);
  assert.deepEqual(result.body, { success: true, available: false, verified: true });
});

test('the email check answers available when no account uses the address', async () => {
  const { api } = makeApi({ replies: { CheckEmail: { success: true, available: true, verified: false } } });
  const result = await send(api, jsonRequest('/api/check-email', { email: 'new@example.com' }));
  assert.equal(result.status, 200);
  assert.deepEqual(result.body, { success: true, available: true, verified: false });
});

test('the email check is limited per address, and a Nodepool failure is reported as unavailable', async () => {
  const limited = makeApi({
    env: { TURNSTILE_SECRET_KEY: ALWAYS_PASS_SECRET, WEBSITE_CLIENT_IP_HEADER: 'x-real-ip' },
    replies: { CheckEmail: { success: true, available: true } },
  });
  const headers = { 'x-real-ip': '203.0.113.9' };
  for (let index = 0; index < 30; index += 1) {
    const result = await send(
      limited.api,
      jsonRequest('/api/check-email', { email: `typing${index}@example.com` }, { headers })
    );
    assert.equal(result.status, 200);
  }
  const refused = await send(
    limited.api,
    jsonRequest('/api/check-email', { email: 'typing30@example.com' }, { headers })
  );
  assert.equal(refused.status, 429);
  assert.ok(Number(refused.headers.get('retry-after')) >= 1);

  const unreachable = makeApi({ replies: { CheckEmail: { success: false } } });
  const failed = await send(unreachable.api, jsonRequest('/api/check-email', { email: 'new@example.com' }));
  assert.equal(failed.status, 503);
  assert.equal(failed.body.code, 'service_unavailable');
});

test('reset-password forwards the reset token and the new password', async () => {
  const { api, calls } = makeApi();
  const result = await send(api, jsonRequest('/api/reset-password', { resetToken: 'reset-token-1', newPassword: 'brand new password' }));

  assert.equal(result.status, 200);
  assert.equal(result.body.success, true);
  assert.deepEqual(calls, [
    { method: 'ResetPassword', request: { reset_token: 'reset-token-1', new_password: 'brand new password' } },
  ]);
});

test('reset-password refuses a missing link or a weak password before calling Nodepool', async () => {
  const noToken = makeApi();
  const missing = await send(noToken.api, jsonRequest('/api/reset-password', { resetToken: '', newPassword: 'brand new password' }));
  assert.equal(missing.body.code, 'reset_failed');
  assert.equal(noToken.calls.length, 0);

  const weak = makeApi();
  const short = await send(weak.api, jsonRequest('/api/reset-password', { resetToken: 'token', newPassword: 'short' }));
  assert.equal(short.body.code, 'password_too_short');
  assert.equal(weak.calls.length, 0);
});

test('reset-password reports a refused reset as reset_failed', async (t) => {
  t.mock.method(console, 'error', () => {});
  const { api } = makeApi({ replies: { ResetPassword: { success: false, status_message: 'token already used' } } });
  const result = await send(api, jsonRequest('/api/reset-password', { resetToken: 'used', newPassword: 'brand new password' }));
  assert.equal(result.status, 400);
  assert.equal(result.body.code, 'reset_failed');
  assert.doesNotMatch(JSON.stringify(result.body), /already used/);
});

test('an unavailable user service returns service_unavailable without the gRPC detail', async (t) => {
  t.mock.method(console, 'error', () => {});
  const { api } = makeApi({
    replies: { GetAccount: grpcError('connect ECONNREFUSED 10.0.0.5:50051', grpc.status.UNAVAILABLE) },
  });
  const result = await send(api, new Request(`${ORIGIN}/api/account`, { headers: bearer('session-token') }));

  assert.equal(result.status, 503);
  assert.equal(result.body.code, 'service_unavailable');
  assert.doesNotMatch(JSON.stringify(result.body), /ECONNREFUSED|10\.0\.0\.5/);
});

test('oversized and non-object bodies are refused before Nodepool is called', async () => {
  const { api, calls } = makeApi();
  const oversized = await send(
    api,
    jsonRequest('/api/register', { ...VALID_REGISTRATION, username: 'x'.repeat(5000) })
  );
  assert.equal(oversized.status, 413);
  assert.equal(oversized.body.code, 'request_too_large');

  const notObject = await send(
    api,
    new Request(`${ORIGIN}/api/login`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '[1,2,3]' })
  );
  assert.equal(notObject.status, 400);
  assert.equal(notObject.body.code, 'invalid_request');
  assert.equal(calls.length, 0);
});

test('unknown paths, prototype names, and wrong methods are not routed', async () => {
  const { api, calls } = makeApi();
  assert.equal((await send(api, jsonRequest('/api/tasks', {}))).status, 404);
  assert.equal((await send(api, jsonRequest('/api/constructor', {}))).status, 404);
  assert.equal((await send(api, jsonRequest('/api/toString', {}))).status, 404);
  assert.equal((await send(api, jsonRequest('/api/balance', {}))).status, 404);
  assert.equal((await send(api, jsonRequest('/api/register/extra', {}))).status, 404);
  assert.equal((await send(api, new Request(`${ORIGIN}/api/register`, { method: 'GET' }))).status, 404);

  const put = await send(api, new Request(`${ORIGIN}/api/login`, { method: 'PUT' }));
  assert.equal(put.status, 405);
  assert.equal(put.headers.get('allow'), 'GET, POST');
  assert.equal(calls.length, 0);
});
