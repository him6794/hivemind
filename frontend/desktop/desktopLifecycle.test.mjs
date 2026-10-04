import test from 'node:test';
import assert from 'node:assert/strict';
import { createCloseController, getDesktopBridge, motionDelay } from './desktopLifecycle.mjs';

function fixture(overrides = {}) {
  const actions = [];
  const states = [];
  const controller = createCloseController({
    bridge: () => ({ resolveClose: async (action) => { actions.push(action); } }),
    setVisibility: (state) => states.push(state),
    wait: async () => {},
    ...overrides,
  });
  return { controller, actions, states };
}

test('normal browsers do not receive desktop-only controls', () => {
  assert.equal(getDesktopBridge({}), null);
  assert.equal(getDesktopBridge({ __HIVEMIND_DESKTOP__: { ready() {} } }), null);
  const bridge = { ready() {}, requestClose() {}, resolveClose() {} };
  assert.equal(getDesktopBridge({ __HIVEMIND_DESKTOP__: bridge }), bridge);
});

test('cancel never animates out, hides the window, or requests quit', async () => {
  const { controller, actions, states } = fixture();
  await controller.resolve('cancel');
  assert.deepEqual(actions, ['cancel']);
  assert.deepEqual(states, ['visible']);
});

test('background and quit animate before making the host decision', async () => {
  for (const action of ['background', 'quit']) {
    const events = [];
    const { controller } = fixture({
      setVisibility: (state) => events.push(state),
      wait: async () => { events.push('animation'); },
      bridge: () => ({ resolveClose: async (choice) => events.push(choice) }),
    });
    await controller.resolve(action);
    assert.deepEqual(events, ['closing', 'animation', action, 'visible']);
  }
});

test('repeated confirmation resolves only once while pending', async () => {
  let finish;
  const { controller, actions } = fixture({ wait: () => new Promise((resolve) => { finish = resolve; }) });
  const first = controller.resolve('background');
  assert.equal(controller.busy, true);
  assert.equal(await controller.resolve('quit'), false);
  finish();
  await first;
  assert.deepEqual(actions, ['background']);
  assert.equal(controller.busy, false);
});

test('a rejected or timed-out host action restores visible state and permits retry', async () => {
  const { controller, states } = fixture({
    timeoutMs: 10,
    bridge: () => ({ resolveClose: () => new Promise(() => {}) }),
  });
  await assert.rejects(controller.resolve('quit'), /timed out/);
  assert.deepEqual(states, ['closing', 'visible']);
  assert.equal(controller.busy, false);
});

test('invalid actions cannot reach the host', async () => {
  const { controller, actions } = fixture();
  await assert.rejects(controller.resolve('shell'), /Invalid/);
  assert.deepEqual(actions, []);
});

test('reduced motion removes the animation wait', () => {
  assert.equal(motionDelay({ matchMedia: () => ({ matches: true }) }), 0);
  assert.equal(motionDelay({ matchMedia: () => ({ matches: false }) }), 180);
});
