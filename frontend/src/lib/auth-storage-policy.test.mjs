import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  LEGACY_AUTH_STORAGE_KEY,
  clearLegacyAuthStorage,
} from './auth-storage-policy.mjs';

describe('legacy official-site auth storage cleanup', () => {
  it('removes the previously persisted bearer-token entry', () => {
    const removed = [];
    const storage = {
      removeItem(key) {
        removed.push(key);
      },
    };

    assert.equal(clearLegacyAuthStorage(storage), true);
    assert.deepEqual(removed, [LEGACY_AUTH_STORAGE_KEY]);
    assert.equal(LEGACY_AUTH_STORAGE_KEY, 'hivemind-site-auth');
  });

  it('fails closed without breaking app startup when storage is unavailable', () => {
    const storage = {
      removeItem() {
        throw new Error('storage denied');
      },
    };

    assert.equal(clearLegacyAuthStorage(storage), false);
    assert.equal(clearLegacyAuthStorage(null), false);
  });
});
