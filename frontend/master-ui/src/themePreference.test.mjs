import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { readThemePreference, saveThemePreference } from './themePreference.mjs';

function memoryStorage(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, String(value)),
  };
}

describe('theme preference', () => {
  it('defaults to light and restores the saved theme', () => {
    const storage = memoryStorage();
    assert.equal(readThemePreference(storage), 'light');
    saveThemePreference('dark', storage);
    assert.equal(readThemePreference(storage), 'dark');
    saveThemePreference('light', storage);
    assert.equal(readThemePreference(storage), 'light');
  });

  it('falls back to light when preference storage is unavailable', () => {
    const unavailable = {
      getItem() { throw new Error('blocked'); },
      setItem() { throw new Error('blocked'); },
    };
    assert.equal(readThemePreference(unavailable), 'light');
    assert.doesNotThrow(() => saveThemePreference('dark', unavailable));
  });
});
