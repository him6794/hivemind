import assert from 'node:assert/strict';
import test from 'node:test';
import path from 'node:path';
import { readdir, rm } from 'node:fs/promises';
import { nativeTestEnvironment } from './native-ui-smoke.mjs';

test('every native smoke environment gets a fresh absolute preference root without case-duplicate operator paths', async () => {
  const source = { LOCALAPPDATA: 'operator-root', LocalAppData: 'other-operator-root', PATH: 'unchanged' };
  const first = await nativeTestEnvironment(source);
  const second = await nativeTestEnvironment(source);
  try {
    for (const env of [first, second]) {
      assert.deepEqual(Object.keys(env).filter((key) => key.toLowerCase() === 'localappdata'), ['LOCALAPPDATA']);
      assert.ok(path.isAbsolute(env.LOCALAPPDATA));
      assert.deepEqual(await readdir(env.LOCALAPPDATA), []);
      assert.equal(env.PATH, source.PATH);
    }
    assert.notEqual(first.LOCALAPPDATA, second.LOCALAPPDATA);
    assert.equal(source.LOCALAPPDATA, 'operator-root');
    assert.equal(source.LocalAppData, 'other-operator-root');
  } finally {
    await rm(first.LOCALAPPDATA, { recursive: true });
    await rm(second.LOCALAPPDATA, { recursive: true });
  }
});
