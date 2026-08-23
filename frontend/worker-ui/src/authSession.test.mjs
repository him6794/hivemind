import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { readFile } from 'node:fs/promises';

import {
  clearStoredSession,
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

    saveStoredSession(storage, 'session-key', { token: 'jwt-token', username: 'worker-owner' });

    assert.deepEqual(readStoredSession(storage, 'session-key'), {
      token: 'jwt-token',
      username: 'worker-owner',
    });
  });

  it('ignores malformed or incomplete sessions', () => {
    const storage = memoryStorage();

    storage.setItem('bad-json', '{');
    storage.setItem('missing-token', JSON.stringify({ username: 'worker-owner' }));

    assert.deepEqual(readStoredSession(storage, 'bad-json'), { token: '', username: '' });
    assert.deepEqual(readStoredSession(storage, 'missing-token'), { token: '', username: '' });
  });

  it('clears stored session explicitly', () => {
    const storage = memoryStorage();

    saveStoredSession(storage, 'session-key', { token: 'jwt-token', username: 'worker-owner' });
    clearStoredSession(storage, 'session-key');

    assert.deepEqual(readStoredSession(storage, 'session-key'), { token: '', username: '' });
  });
});

describe('worker UI durable secret policy', () => {
  it('never persists the bearer session in window.localStorage', async () => {
    const appSource = await readFile(new URL('./App.jsx', import.meta.url), 'utf8');
    assert.equal(appSource.includes('window.localStorage'), false);
    assert.ok(appSource.includes('window.sessionStorage'));
  });
});
