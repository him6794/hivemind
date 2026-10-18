import assert from 'node:assert/strict';
import test from 'node:test';

function host(methods = {}) {
  return { __HIVEMIND_DESKTOP__: { ready() {}, requestClose() {}, resolveClose() {}, ...methods } };
}

export function testNativeLocaleAdapter(createLocaleAdapter) {
  test('native locale reads never consult browser storage or automatically write a default', async () => {
    const writes = [];
    let value = 'zh';
    const adapter = createLocaleAdapter({
      host: host({ readLocale: async () => value, writeLocale: async (locale) => writes.push(locale) }),
      storage: { getItem() { throw new Error('native read touched browser storage'); }, setItem() { throw new Error('native write touched browser storage'); } },
    });
    assert.equal(adapter.native, true);
    assert.equal(await adapter.read(), 'zh');
    value = null;
    assert.equal(await adapter.read(), 'en');
    assert.deepEqual(writes, []);
    await adapter.write('en');
    assert.deepEqual(writes, ['en']);
  });

  test('native read errors and corrupt values reject without saving English', async () => {
    const writes = [];
    for (const readLocale of [async () => { throw new Error('disk failed'); }, async () => 'fr', async () => undefined, async () => false]) {
      const adapter = createLocaleAdapter({ host: host({ readLocale, writeLocale: async (locale) => writes.push(locale) }) });
      await assert.rejects(adapter.read());
    }
    assert.deepEqual(writes, []);
  });

  test('an older desktop bridge fails explicitly instead of falling back to localStorage', async () => {
    const adapter = createLocaleAdapter({ host: host() });
    assert.equal(adapter.native, true);
    await assert.rejects(adapter.read(), /unavailable/);
    await assert.rejects(adapter.write('zh'), /unavailable/);
  });

  test('native writes are pending until ACK and propagate failures', async () => {
    let acknowledge;
    const adapter = createLocaleAdapter({ host: host({ writeLocale: () => new Promise((resolve) => { acknowledge = resolve; }) }) });
    let saved = false;
    const saving = adapter.write('zh').then(() => { saved = true; });
    await Promise.resolve();
    assert.equal(saved, false);
    acknowledge();
    await saving;
    assert.equal(saved, true);
    const failed = createLocaleAdapter({ host: host({ writeLocale: async () => { throw new Error('disk failed'); } }) });
    await assert.rejects(failed.write('en'), /disk failed/);
    for (const invalid of ['zh-TW', null, 'EN', '']) await assert.rejects(adapter.write(invalid), /Invalid/);
  });

  test('native reads and writes have bounded failure, including missing ACKs', async () => {
    let late;
    const adapter = createLocaleAdapter({
      host: host({ readLocale: () => new Promise((resolve) => { late = resolve; }), writeLocale: () => new Promise(() => {}) }),
      timeoutMs: 5,
    });
    await assert.rejects(adapter.read(), /timed out/);
    late('zh');
    await assert.rejects(adapter.write('en'), /timed out/);
  });

  test('browser adapters retain the existing localStorage preference contract', async () => {
    const values = new Map();
    const storage = { getItem: (key) => values.get(key), setItem: (key, value) => values.set(key, value) };
    const adapter = createLocaleAdapter({ host: {}, storage });
    assert.equal(adapter.native, false);
    assert.equal(await adapter.read(), 'en');
    await adapter.write('zh');
    assert.equal(await adapter.read(), 'zh');
    assert.equal(values.size, 1);
  });
}
