import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('../hivemind-rs/crates/hivemind-bin/src/local_ui_bridge.js', import.meta.url), 'utf8');
const origin = 'http://127.0.0.1:8082';
const nonce = 'fixture-private-nonce';
const flush = async () => { for (let index = 0; index < 20; index++) await Promise.resolve(); };

function setup({ actualOrigin = origin, response = true, autoLocale = true, autoWindow = true, throwPost = false } = {}) {
  const listeners = new Map();
  const messages = [];
  const timers = new Map();
  let now = 0;
  let sequence = 0;
  let stored = null;
  const clock = {
    setTimeout(callback, delay) { const id = ++sequence; timers.set(id, { callback, due: now + delay }); return id; },
    clearTimeout(id) { timers.delete(id); },
    async tick(ms) {
      const target = now + ms;
      for (;;) {
        const next = [...timers].filter(([, timer]) => timer.due <= target).sort((a, b) => a[1].due - b[1].due)[0];
        if (!next) break;
        now = next[1].due; timers.delete(next[0]); next[1].callback(); await flush();
      }
      now = target; await flush();
    },
  };
  function reply(payload, detail = {}) {
    const locale = payload.channel === 'hivemind-locale-v1';
    const value = locale
      ? { page: payload.page, id: payload.id, ok: true, locale: payload.action === 'read' ? stored : payload.locale }
      : { id: payload.id, ok: response };
    listeners.get(locale ? 'hivemind:locale-response' : 'hivemind:host-response')({ detail: { ...value, ...detail } });
  }
  const window = {
    addEventListener(name, callback) { listeners.set(name, callback); },
    chrome: { webview: { postMessage(message) {
      if (throwPost) throw new Error('transport unavailable');
      assert.equal(typeof message, 'string', 'Wry requires string-valued native messages');
      const payload = JSON.parse(message);
      messages.push(payload);
      if (payload.channel === 'hivemind-window-v1' ? autoWindow : autoLocale) {
        queueMicrotask(() => {
          if (payload.action === 'write') stored = payload.locale;
          reply(payload);
        });
      }
    } } },
  };
  vm.runInNewContext(source.replace('__HIVEMIND_ORIGIN__', JSON.stringify(origin)).replace('__HIVEMIND_NONCE__', JSON.stringify(nonce)), {
    window, location: { origin: actualOrigin }, crypto: { randomUUID },
    document: { getElementById() { return null; } },
    setTimeout: clock.setTimeout, clearTimeout: clock.clearTimeout,
  });
  return { bridge: window.__HIVEMIND_DESKTOP__, window, messages, reply, clock, timers };
}

const quitMessages = (fixture) => fixture.messages.filter((message) => message.action === 'quit');

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

test('unknown actions and locale values never reach the native stream', async () => {
  const { bridge, messages } = setup();
  await assert.rejects(bridge.resolveClose('shell'), /Invalid close action/);
  for (const value of ['zh-TW', 'EN', '', null, {}, 1]) await assert.rejects(bridge.writeLocale(value), /Invalid language/);
  assert.deepEqual(messages, []);
});

test('native window rejections propagate without blocking subsequent requests', async () => {
  const { bridge, messages } = setup({ response: false });
  await assert.rejects(bridge.resolveClose('background'), /Window action failed/);
  await assert.rejects(bridge.resolveClose('cancel'), /Window action failed/);
  assert.equal(messages.length, 2);
});

test('locale reads and writes use their own strict channel and current native value', async () => {
  const { bridge, messages } = setup();
  assert.equal(await bridge.readLocale(), null);
  assert.equal(await bridge.writeLocale('zh'), undefined);
  assert.equal(await bridge.readLocale(), 'zh');
  await bridge.writeLocale('en');
  assert.equal(await bridge.readLocale(), 'en');
  for (const [index, message] of messages.entries()) {
    assert.equal(message.channel, 'hivemind-locale-v1');
    assert.equal(message.nonce, nonce);
    assert.equal(message.id, index + 1);
    assert.match(message.page, /^[0-9a-f]{32}$/);
    assert.ok(Buffer.byteLength(JSON.stringify(message)) <= 512);
    assert.deepEqual(Object.keys(message).sort(), (message.action === 'write'
      ? ['channel', 'nonce', 'page', 'action', 'id', 'locale'] : ['channel', 'nonce', 'page', 'action', 'id']).sort());
  }
});

test('writes remain unconfirmed until the matching successful ACK', async () => {
  const fixture = setup({ autoLocale: false });
  let saved = false;
  const operation = fixture.bridge.writeLocale('zh').then(() => { saved = true; });
  await flush();
  assert.equal(saved, false);
  fixture.reply(fixture.messages[0], { page: 'a'.repeat(32) });
  await flush();
  assert.equal(saved, false);
  fixture.reply(fixture.messages[0]);
  await operation;
  assert.equal(saved, true);
  assert.equal(fixture.timers.size, 0);
});

test('failed and malformed locale ACKs reject instead of pretending to save', async () => {
  for (const detail of [{ ok: false }, { ok: 'true' }, { locale: 'en' }, { locale: 'zh-TW' }, { extra: true }]) {
    const fixture = setup({ autoLocale: false });
    const operation = fixture.bridge.writeLocale('zh');
    const rejection = assert.rejects(operation, /preference/);
    fixture.reply(fixture.messages[0], detail);
    await rejection;
  }
  const fixture = setup({ autoLocale: false });
  const operation = fixture.bridge.readLocale();
  const rejection = assert.rejects(operation, /Invalid language/);
  fixture.reply(fixture.messages[0], { locale: 'fr' });
  await rejection;
});

test('locale timeout rejects and a late ACK cannot settle the next request', async () => {
  const fixture = setup({ autoLocale: false });
  const first = fixture.bridge.readLocale();
  const rejection = assert.rejects(first, /timed out/);
  await fixture.clock.tick(4000);
  await rejection;
  let settled = false;
  const second = fixture.bridge.readLocale().then((value) => { settled = true; return value; });
  fixture.reply(fixture.messages[0], { locale: 'zh' });
  await flush();
  assert.equal(settled, false);
  fixture.reply(fixture.messages[1], { locale: 'en' });
  assert.equal(await second, 'en');
});

test('reload changes the private document token even when request IDs restart', async () => {
  const before = setup({ autoLocale: false });
  const first = before.bridge.readLocale();
  const after = setup({ autoLocale: false });
  let settled = false;
  const second = after.bridge.readLocale().then((value) => { settled = true; return value; });
  assert.equal(before.messages[0].id, after.messages[0].id);
  assert.notEqual(before.messages[0].page, after.messages[0].page);
  after.reply(before.messages[0], { locale: 'zh' });
  await flush();
  assert.equal(settled, false);
  after.reply(after.messages[0], { locale: 'en' });
  before.reply(before.messages[0], { locale: 'zh' });
  assert.equal(await second, 'en');
  assert.equal(await first, 'zh');
});

test('Quit waits for rapid selections, including a write started during its wait', async () => {
  const fixture = setup({ autoLocale: false });
  const first = fixture.bridge.writeLocale('zh');
  const quit = fixture.bridge.resolveClose('quit');
  const second = fixture.bridge.writeLocale('en');
  await flush();
  assert.equal(quitMessages(fixture).length, 0);
  fixture.reply(fixture.messages[0]);
  await first; await flush();
  assert.equal(quitMessages(fixture).length, 0);
  fixture.reply(fixture.messages[1]);
  await second; await quit;
  assert.deepEqual(fixture.messages.map(({ action }) => action), ['write', 'write', 'quit']);
  await assert.rejects(fixture.bridge.writeLocale('zh'), /quitting/);
});

test('the latest confirmed selection supersedes an older failed write', async () => {
  const fixture = setup({ autoLocale: false });
  const old = fixture.bridge.writeLocale('zh');
  const rejected = assert.rejects(old);
  const latest = fixture.bridge.writeLocale('en');
  const quit = fixture.bridge.resolveClose('quit');
  fixture.reply(fixture.messages[1]);
  await latest;
  fixture.reply(fixture.messages[0], { ok: false });
  await rejected; await quit;
  assert.equal(quitMessages(fixture).length, 1);
});

test('a failed save blocks the first Quit but a second explicit Quit can leave', async () => {
  const fixture = setup({ autoLocale: false });
  const operation = fixture.bridge.writeLocale('zh');
  const rejected = assert.rejects(operation);
  fixture.reply(fixture.messages[0], { ok: false });
  await rejected;
  await assert.rejects(fixture.bridge.resolveClose('quit'), (error) => error.code === 'locale-not-saved');
  assert.equal(quitMessages(fixture).length, 0);
  await fixture.bridge.resolveClose('quit');
  assert.equal(quitMessages(fixture).length, 1);
});

test('a new selection rearms the save warning after an earlier failed Quit', async () => {
  const fixture = setup({ autoLocale: false });
  for (const locale of ['zh', 'en']) {
    const operation = fixture.bridge.writeLocale(locale);
    const rejected = assert.rejects(operation);
    fixture.reply(fixture.messages.at(-1), { ok: false });
    await rejected;
    await assert.rejects(fixture.bridge.resolveClose('quit'), (error) => error.code === 'locale-not-saved');
    assert.equal(quitMessages(fixture).length, 0);
  }
});

test('a bounded Quit wait never posts a latent Quit after a late save ACK', async () => {
  const fixture = setup({ autoLocale: false });
  const first = fixture.bridge.writeLocale('zh');
  const quit = fixture.bridge.resolveClose('quit');
  const rejectedQuit = assert.rejects(quit, (error) => error.code === 'locale-not-saved');
  await fixture.clock.tick(3000);
  const second = fixture.bridge.writeLocale('en');
  fixture.reply(fixture.messages[0]);
  await first;
  await fixture.clock.tick(1000);
  await rejectedQuit;
  assert.equal(quitMessages(fixture).length, 0);
  fixture.reply(fixture.messages[1]);
  await second; await flush();
  assert.equal(quitMessages(fixture).length, 0);
  await fixture.clock.tick(10000);
  assert.equal(quitMessages(fixture).length, 0);
  const third = fixture.bridge.writeLocale('zh');
  fixture.reply(fixture.messages[2]);
  await third;
});

test('the outer close deadline covers both the locale wait and window ACK deadline', async () => {
  const fixture = setup({ autoLocale: false, autoWindow: false });
  const lifecycle = await readFile(new URL('../frontend/desktop/desktopLifecycle.mjs', import.meta.url), 'utf8');
  const context = { setTimeout: fixture.clock.setTimeout, clearTimeout: fixture.clock.clearTimeout };
  vm.runInNewContext(lifecycle.replace(/export function/g, 'function') + '\nthis.createController = createCloseController;', context);
  const controller = context.createController({ bridge: () => fixture.bridge, setVisibility() {}, wait: async () => {} });
  const saving = fixture.bridge.writeLocale('zh');
  let failure;
  const closing = controller.resolve('quit').catch((error) => { failure = error; });
  await flush();
  await fixture.clock.tick(3900);
  fixture.reply(fixture.messages[0]);
  await saving; await flush();
  assert.equal(quitMessages(fixture).length, 1);
  await fixture.clock.tick(3900);
  assert.equal(failure, undefined, 'outer timeout must not fire while native Quit is still in bounds');
  assert.equal(controller.busy, true);
  fixture.reply(fixture.messages[1]);
  await closing;
  assert.equal(failure, undefined);
  assert.equal(controller.busy, false);
  assert.equal(fixture.timers.size, 0);
});

test('transport exceptions reject and clear pending request timers', async () => {
  const fixture = setup({ throwPost: true });
  await assert.rejects(fixture.bridge.readLocale(), /transport unavailable/);
  await assert.rejects(fixture.bridge.writeLocale('zh'), /transport unavailable/);
  await assert.rejects(fixture.bridge.resolveClose('cancel'), /transport unavailable/);
  assert.equal(fixture.timers.size, 0);
});
