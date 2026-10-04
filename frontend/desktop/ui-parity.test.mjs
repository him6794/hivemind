import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const frontend = new URL('../', import.meta.url);

test('all three surfaces preserve the supplied light and dark theme without token drift', async () => {
  const canonical = await readFile(new URL('shadcn-theme.css', frontend), 'utf8');
  assert.match(canonical, /:root\s*\{/);
  assert.doesNotMatch(canonical, /::root/);
  assert.match(canonical, /--background: oklch\(1 0 0\)/);
  assert.match(canonical, /--background: oklch\(0\.145 0 0\)/);
  assert.match(canonical, /--radius: 0\.625rem/);
  assert.match(canonical, /--sidebar-primary: oklch\(0\.488 0\.243 264\.376\)/);
  for (const file of ['src/theme.css', 'master-ui/src/theme.css', 'worker-ui/src/theme.css']) {
    assert.equal(await readFile(new URL(file, frontend), 'utf8'), canonical, `${file} differs from supplied tokens`);
  }
});

test('packaged clients contain matching self-contained lifecycle code', async () => {
  for (const filename of ['DesktopLifecycle.jsx', 'desktopLifecycle.mjs', 'desktop-lifecycle.css']) {
    const canonical = await readFile(new URL(`desktop/${filename}`, frontend), 'utf8');
    for (const role of ['master', 'worker']) {
      assert.equal(await readFile(new URL(`${role}-ui/src/${filename}`, frontend), 'utf8'), canonical, `${role} lifecycle has drifted: ${filename}`);
    }
  }
});
