import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('../hivemind-rs/crates/hivemind-bin/src/local_ui_bridge.js', import.meta.url), 'utf8');
const origin = 'http://127.0.0.1:8082';
const nonce = 'fixture-private-nonce';

function setup({ actualOrigin = origin, response = true } = {}) {
  const listeners = new Map();
  const messages = [];
  const window = {
    addEventListener(name, callback) { listeners.set(name, callback); },
    chrome: { webview: { postMessage(message) {
      assert.equal(typeof message, 'string', 'Wry requires string-valued native messages');
      const payload = JSON.parse(message);
      messages.push(payload);
      queueMicrotask(() => listeners.get('hivemind:host-response')({ detail: { id: payload.id, ok: response } }));
    } } },
  };
  vm.runInNewContext(source.replace('__HIVEMIND_ORIGIN__', JSON.stringify(origin)).replace('__HIVEMIND_NONCE__', JSON.stringify(nonce)), {
    window, location: { origin: actualOrigin },
    document: { getElementById() { return null; } },
    setTimeout, clearTimeout,
  });
  return { bridge: window.__HIVEMIND_DESKTOP__, window, messages };
}

test('native lifecycle uses JSON text compatible with the existing Wry receiver', async () => {
  const { bridge, window, messages } = setup();
  await bridge.ready();
  await bridge.requestClose();
  for (const action of ['cancel', 'background', 'quit']) await bridge.resolveClose(action);
  assert.deepEqual(messages, ['ready', 'request', 'cancel', 'background', 'quit'].map((action, index) => ({
    channel: 'hivemind-window-v1', nonce, action, id: index + 1,
  })));
  assert.equal(Object.isFrozen(bridge), true);
  const descriptor = Object.getOwnPropertyDescriptor(window, '__HIVEMIND_DESKTOP__');
  assert.equal(descriptor.writable, false);
  assert.equal(descriptor.configurable, false);
});

test('foreign origins receive no local window bridge', () => {
  for (const actualOrigin of ['http://127.0.0.1:8083', 'https://127.0.0.1:8082', 'http://localhost:8082', 'https://example.com']) {
    const { bridge, messages } = setup({ actualOrigin });
    assert.equal(bridge, undefined);
    assert.deepEqual(messages, []);
  }
});

test('unknown actions never reach the native stream', async () => {
  const { bridge, messages } = setup();
  await assert.rejects(bridge.resolveClose('shell'), /Invalid close action/);
  assert.deepEqual(messages, []);
});

test('native rejections propagate without blocking subsequent requests', async () => {
  const { bridge, messages } = setup({ response: false });
  await assert.rejects(bridge.resolveClose('background'), /Window action failed/);
  await assert.rejects(bridge.resolveClose('cancel'), /Window action failed/);
  assert.equal(messages.length, 2);
});
