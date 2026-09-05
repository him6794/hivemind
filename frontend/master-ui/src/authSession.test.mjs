import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { readFile } from 'node:fs/promises';

import {
  clearStoredSession,
  isExpiredJwt,
  readStoredSession,
  saveStoredSession,
} from './authSession.mjs';

function memoryStorage() {
  const values = new Map();
  return {
    getItem: (key) => (values.has(key) ? values.get(key) : null),
    setItem: (key, value) => values.set(key, String(value)),
    removeItem: (key) => values.delete(key),
  };
}

describe('auth session storage', () => {
  it('round-trips token and username', () => {
    const storage = memoryStorage();

    saveStoredSession(storage, 'session-key', { token: 'jwt-token', username: 'alice' });

    assert.deepEqual(readStoredSession(storage, 'session-key'), {
      token: 'jwt-token',
      username: 'alice',
    });
  });

  it('ignores malformed or incomplete sessions', () => {
    const storage = memoryStorage();

    storage.setItem('bad-json', '{');
    storage.setItem('missing-token', JSON.stringify({ username: 'alice' }));

    assert.deepEqual(readStoredSession(storage, 'bad-json'), { token: '', username: '' });
    assert.deepEqual(readStoredSession(storage, 'missing-token'), { token: '', username: '' });
  });

  it('clears stored session explicitly', () => {
    const storage = memoryStorage();

    saveStoredSession(storage, 'session-key', { token: 'jwt-token', username: 'alice' });
    clearStoredSession(storage, 'session-key');

    assert.deepEqual(readStoredSession(storage, 'session-key'), { token: '', username: '' });
  });

  it('drops an expired JWT instead of restoring it', () => {
    const storage = memoryStorage();
    const expired = makeJwt({ exp: 1000 });
    const live = makeJwt({ exp: Math.floor(Date.now() / 1000) + 3600 });

    saveStoredSession(storage, 'expired-key', { token: expired, username: 'alice' });
    saveStoredSession(storage, 'live-key', { token: live, username: 'alice' });

    assert.deepEqual(readStoredSession(storage, 'expired-key'), { token: '', username: '' });
    assert.equal(storage.getItem('expired-key'), null, 'expired session is removed');
    assert.deepEqual(readStoredSession(storage, 'live-key'), { token: live, username: 'alice' });
  });
});

function base64Url(value) {
  return Buffer.from(JSON.stringify(value)).toString('base64url');
}

function makeJwt(payload) {
  return `header.${base64Url(payload)}.signature`;
}

describe('JWT expiry detection', () => {
  const future = Math.floor(Date.now() / 1000) + 3600;

  it('matches the server expiration leeway under clock skew', () => {
    const nowMs = 1_700_000_000_000;
    const nowSeconds = nowMs / 1000;
    assert.equal(isExpiredJwt(makeJwt({ exp: nowSeconds - 30 }), nowMs), false);
    assert.equal(isExpiredJwt(makeJwt({ exp: nowSeconds - 60 }), nowMs), false);
    assert.equal(isExpiredJwt(makeJwt({ exp: nowSeconds - 61 }), nowMs), true);
  });

  it('keeps live and opaque tokens', () => {
    assert.equal(isExpiredJwt(makeJwt({ exp: future })), false);
    // Not a decodable three-part JWT: leave the verdict to the server.
    assert.equal(isExpiredJwt('opaque-session-token'), false);
  });

  it('treats missing or malformed exp as not expired', () => {
    assert.equal(isExpiredJwt(makeJwt({})), false);
    assert.equal(
      isExpiredJwt(`header.${Buffer.from('not json').toString('base64url')}.sig`),
      false
    );
  });
});

describe('master UI durable secret policy', () => {
  it('never persists the bearer session in window.localStorage', async () => {
    const appSource = await readFile(new URL('./App.jsx', import.meta.url), 'utf8');
    assert.equal(appSource.includes('window.localStorage'), false);
    assert.ok(appSource.includes('window.sessionStorage'));
  });
});
