import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  createStartupStatusController,
  createTokenSetupController,
  getTaskListView,
} from './startupOrchestrator.mjs';

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe('startup status controller', () => {
  it('reports backend phases and treats legacy 404 as ready', async () => {
    const states = [];
    let checks = 0;
    const controller = createStartupStatusController({
      checkStatus: async () => {
        checks += 1;
        return checks === 1
          ? { data: { state: 'initializing', phase: 'resources' } }
          : { data: { state: 'ready', phase: 'ready' } };
      },
      onState: (state) => states.push(state),
      timeoutMs: 100,
      pollIntervalMs: 1,
    });

    assert.deepEqual(await controller.start(), { state: 'ready' });
    assert.ok(states.some((state) => state.phase === 'resources'));
    assert.equal(states.at(-1).phase, 'ready');

    const legacyController = createStartupStatusController({
      checkStatus: async () => ({ status: 404, ok: false }),
      timeoutMs: 100,
    });
    assert.deepEqual(await legacyController.start(), { state: 'ready' });
  });

  it('bounds the status poll and allows an explicit later check', async () => {
    const states = [];
    let checks = 0;
    const controller = createStartupStatusController({
      checkStatus: async (signal) => {
        checks += 1;
        if (checks > 1) return { state: 'ready' };
        return new Promise((resolve, reject) => {
          signal.addEventListener('abort', () => reject(new Error('aborted')), { once: true });
        });
      },
      onState: (state) => states.push(state),
      timeoutMs: 10,
      pollIntervalMs: 1,
    });

    assert.equal((await controller.start()).state, 'error');
    assert.equal(states.at(-1).state, 'error');
    assert.deepEqual(await controller.start(), { state: 'ready' });
    assert.equal(checks, 2);
  });

  it('stops polling on a terminal backend failure and exposes only a safe code', async () => {
    const states = [];
    let checks = 0;
    const controller = createStartupStatusController({
      checkStatus: async () => {
        checks += 1;
        return { data: { state: 'failed', phase: 'services', code: 'DB_NOT_READY' } };
      },
      onState: (state) => states.push(state),
      timeoutMs: 100,
      pollIntervalMs: 1,
    });

    assert.deepEqual(await controller.start(), { state: 'failed', code: 'DB_NOT_READY' });
    assert.equal(checks, 1);
    assert.deepEqual(states.at(-1), { state: 'failed', phase: 'services', code: 'DB_NOT_READY' });
  });

  it('does not expose an unsafe backend failure code', async () => {
    const states = [];
    const controller = createStartupStatusController({
      checkStatus: async () => ({ state: 'failed', code: 'postgres://private' }),
      onState: (state) => states.push(state),
    });

    assert.deepEqual(await controller.start(), { state: 'failed', code: '' });
    assert.equal(states.at(-1).code, '');
  });
});

describe('token setup controller', () => {
  it('keeps phase order and shares one in-flight setup across StrictMode re-entry', async () => {
    let currentToken = 'token-a';
    const bootstrap = deferred();
    const tasks = deferred();
    const phases = [];
    let bootstrapCalls = 0;
    let taskCalls = 0;
    const controller = createTokenSetupController({
      bootstrapVpn: async () => {
        bootstrapCalls += 1;
        await bootstrap.promise;
      },
      loadTasks: async () => {
        taskCalls += 1;
        await tasks.promise;
      },
      getCurrentToken: () => currentToken,
      onPhase: ({ phase }) => phases.push(phase),
    });

    const first = controller.acquire(currentToken);
    first.release();
    const strictModeReplay = controller.acquire(currentToken);
    assert.equal(bootstrapCalls, 1);
    assert.deepEqual(phases, ['connecting']);

    bootstrap.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(phases, ['connecting', 'tasks']);
    assert.equal(taskCalls, 1);
    tasks.resolve();

    assert.deepEqual(await strictModeReplay.promise, { state: 'ready' });
    assert.deepEqual(await first.promise, { state: 'ready' });
    strictModeReplay.release();
  });

  it('ignores a pending setup after logout changes the active token', async () => {
    let currentToken = 'token-a';
    const bootstrap = deferred();
    let taskCalls = 0;
    let successCalls = 0;
    const controller = createTokenSetupController({
      bootstrapVpn: async () => bootstrap.promise,
      loadTasks: async () => { taskCalls += 1; },
      getCurrentToken: () => currentToken,
      onSuccess: () => { successCalls += 1; },
    });

    const setup = controller.acquire(currentToken);
    currentToken = '';
    bootstrap.resolve();

    assert.deepEqual(await setup.promise, { state: 'cancelled' });
    assert.equal(taskCalls, 0);
    assert.equal(successCalls, 0);
    setup.release();
  });
});

describe('task list first-load state', () => {
  it('distinguishes loading, an initial error, and a genuine empty result', () => {
    assert.equal(getTaskListView({ tasks: [], initialLoading: true }), 'loading');
    assert.equal(getTaskListView({ tasks: [], error: 'Unavailable' }), 'error');
    assert.equal(getTaskListView({ tasks: [] }), 'empty');
  });

  it('keeps existing task rows visible during refresh or error', () => {
    const tasks = [{ task_id: 'task-1' }];
    assert.equal(getTaskListView({ tasks, initialLoading: true }), 'rows');
    assert.equal(getTaskListView({ tasks, error: 'Refresh failed' }), 'rows');
  });
});
