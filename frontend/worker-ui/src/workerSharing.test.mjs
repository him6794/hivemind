import assert from 'node:assert/strict';
import { test } from 'node:test';
import { buildSharingRequest, sharingView } from './workerSharing.mjs';
import { build } from 'esbuild';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { t } from './locale.mjs';

test('sharing settings use the authenticated local control endpoint', () => {
  const request = buildSharingRequest('http://localhost:18080/', 'token');
  assert.equal(request?.url, 'http://localhost:18080/api/sharing');
  assert.equal(request.options.method, 'GET');
  assert.equal(request.options.headers.Authorization, 'Bearer token');
  const settings = { mode: 'idle', idle_seconds: 60, cpu_percent_threshold: 15 };
  const save = buildSharingRequest('', 'token', settings);
  assert.equal(save.options.method, 'POST');
  assert.deepEqual(JSON.parse(save.options.body), settings);
  assert.throws(() => buildSharingRequest('', ''), /signed-in/);
  for (const invalid of [{ ...settings, mode: 'never' }, { ...settings, idle_seconds: 0 }, { ...settings, idle_seconds: 'invalid' }, { ...settings, cpu_percent_threshold: 101 }]) {
    assert.throws(() => buildSharingRequest('', 'token', invalid), /sharing settings/);
  }
});

test('sharing UI distinguishes unavailable, busy, and accepting with the OS-specific explanation', () => {
  assert.equal(sharingView(null)?.status, 'Sharing status is unavailable.');
  assert.equal(sharingView({ accepts_tasks: true, source: 'last_input' }).status, 'Accepting new tasks');
  assert.equal(sharingView({ accepts_tasks: false, source: 'last_input' }).status, 'Waiting for this computer to become idle');
  assert.equal(sharingView({ source: 'cpu_usage' }).explanation, 'On Linux, idle means CPU usage stays below the threshold for the idle period.');
  assert.equal(sharingView({ source: 'last_input' }).explanation, 'On Windows, idle means no keyboard or mouse input in this session.');
});

test('sharing form renders selected mode, accessible controls, and both locales', async () => {
  const src = fileURLToPath(new URL('.', import.meta.url));
  const result = await build({ stdin: { contents: "export { SharingPanel } from './components/worker-sharing.jsx'; export { LocaleContext } from './LocaleProvider.jsx';", resolveDir: src }, bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external', jsx: 'automatic', alias: { '@': src } });
  const compiled = { exports: {} };
  new Function('require', 'module', 'exports', result.outputFiles[0].text)(createRequire(import.meta.url), compiled, compiled.exports);
  const { SharingPanel, LocaleContext } = compiled.exports;
  assert.equal(typeof SharingPanel, 'function');
  const settings = { mode: 'idle', idle_seconds: 60, cpu_percent_threshold: 20 };
  const render = (locale, source, saving = false) => renderToStaticMarkup(createElement(LocaleContext.Provider, { value: { t: (key) => t(locale, key) } }, createElement(SharingPanel, { sharing: { accepts_tasks: false, source }, settings, saving, error: '', onSave() {}, onChange() {} })));
  for (const locale of ['en', 'zh']) {
    const html = render(locale, 'cpu_usage');
    assert.ok(html.includes(t(locale, 'Sharing availability')));
    assert.ok(html.includes(t(locale, 'Share only while idle')));
    assert.match(html, /value="idle" selected=""/);
    assert.match(html, /for="sharing-mode"/);
    assert.match(html, /id="sharing-seconds"/);
    assert.match(html, /id="sharing-cpu"/);
    assert.ok(render(locale, 'last_input').includes(t(locale, 'On Windows, idle means no keyboard or mouse input in this session.')));
    assert.ok(!render(locale, 'last_input').includes('id="sharing-cpu"'));
    assert.match(render(locale, 'cpu_usage', true), /disabled=""/);
  }
});
