import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  WorkerFetchTimeoutError,
  WorkerLoginError,
  WorkerStartupError,
  createSetupAttemptController,
  createWorkerStartupStatusReader,
  fetchWithDeadline,
  getWorkerLoginError,
  getWorkerSetupErrorMessage,
  isCurrentSessionAttempt,
  normalizeWorkerStartupStatus,
  waitForWorkerStartup,
} from './workerStartup.mjs';

describe('Worker sign-in failures', () => {
  const signInMessage = (error, options = {}) => getWorkerSetupErrorMessage(error, { kind: 'login', ...options });

  it('uses service guidance for gateway failures without forwarding upstream details', () => {
    const error = getWorkerLoginError({
      ok: false,
      status: 502,
      data: { success: false, message: 'VPN/nodepool bootstrap failed: website-api login returned HTTP 502 Bad Gateway with invalid JSON: error code: 502 password=fixture-secret' },
    });
    assert.ok(error instanceof WorkerLoginError);
    assert.equal(error.status, 502);
    assert.equal(signInMessage(error), 'The sign-in service could not complete this request. Please try again later.');
    assert.doesNotMatch(JSON.stringify(error), /website-api|fixture-secret|password=/);
    assert.doesNotMatch(signInMessage(error), /username|password|VPN\/nodepool/);
    for (const status of [500, 503, 504]) {
      assert.equal(signInMessage(getWorkerLoginError({ ok: false, status, data: {} })), signInMessage(error));
    }
  });

  it('does not assert that 401 or 403 proves the password was wrong', () => {
    for (const status of [401, 403]) {
      const error = getWorkerLoginError({ ok: false, status, data: { message: 'Internal auth failure' } });
      assert.equal(signInMessage(error), 'The sign-in request was not accepted. Check your sign-in details or try again later.');
      assert.doesNotMatch(signInMessage(error), /incorrect|wrong|Internal auth failure/);
    }
  });

  it('separates rate limits from credential and gateway failures', () => {
    assert.equal(
      signInMessage(getWorkerLoginError({ ok: false, status: 429, data: {} })),
      'Too many sign-in attempts. Please wait before trying again.',
    );
  });

  it('separates local connection failures from timeouts', () => {
    assert.equal(signInMessage(new WorkerLoginError('network')), 'The Worker app could not be reached. Make sure it is open, then try again.');
    assert.equal(signInMessage(new WorkerFetchTimeoutError()), 'The Worker app did not respond in time. Make sure it is open, then try again.');
  });

  it('requires a successful response and a nonempty string token', () => {
    assert.equal(getWorkerLoginError({ ok: true, status: 200, data: { success: true, token: ' fixture-session ' } }), null);
    for (const token of [undefined, null, '', '   ', false, 42, {}]) {
      const error = getWorkerLoginError({ ok: true, status: 200, data: { success: true, token } });
      assert.equal(error.kind, 'invalid-response');
      assert.equal(signInMessage(error), 'The sign-in service returned an incomplete response. Please try again later.');
    }
    for (const data of [{}, null, { success: false, token: 'fixture-session' }, { success: 'true', token: 'fixture-session' }]) {
      assert.ok(getWorkerLoginError({ ok: true, status: 200, data }) instanceof WorkerLoginError);
    }
  });

  it('keeps unexpected failures neutral instead of trusting raw error text', () => {
    const neutral = 'Sign-in could not be completed. Please try again later.';
    for (const error of [new Error('unauthorized password=fixture-secret'), getWorkerLoginError({ ok: false, status: 400, data: {} }), getWorkerLoginError(null)]) {
      assert.equal(signInMessage(error), neutral);
    }
    assert.equal(new WorkerLoginError('response', '502').status, null);
    assert.equal(new WorkerLoginError('response', 999).status, null);
  });

  it('preserves a successful sign-in when later connection or registration fails', () => {
    for (const error of [new WorkerLoginError('response', 502), new WorkerLoginError('network'), new Error('registration failed')]) {
      assert.equal(signInMessage(error, { loginEstablished: true }), 'Sign-in succeeded, but worker setup is incomplete. Retry connection when ready.');
    }
    assert.equal(signInMessage(new WorkerFetchTimeoutError(), { loginEstablished: true }), 'Sign-in succeeded, but worker setup timed out. Retry connection when ready.');
  });

  it('preserves non-login setup guidance', () => {
    assert.equal(getWorkerSetupErrorMessage(new WorkerLoginError('response', 502), { kind: 'restore' }), 'We could not prepare this computer. Make sure the Worker app is open, then try again.');
    assert.equal(getWorkerSetupErrorMessage(new WorkerFetchTimeoutError(), { kind: 'profile' }), 'The Worker app did not respond in time. Make sure it is open, then try again.');
  });
});

describe('local Worker fetch deadlines', () => {
  it('times out a hanging fetch even when it ignores child abort', async () => {
    let childSignal;
    await assert.rejects(
      fetchWithDeadline(async (_url, options) => {
        childSignal = options.signal;
        return new Promise(() => {});
      }, '/api/login', { method: 'POST' }, { timeoutMs: 5 }),
      (error) => error instanceof WorkerFetchTimeoutError,
    );
    assert.equal(childSignal.aborted, true);
  });

  it('keeps the deadline active after headers while the response body stalls', async () => {
    let bodySignal;
    await assert.rejects(
      fetchWithDeadline(
        async () => ({ text: () => new Promise(() => {}) }),
        '/api/worker-info',
        {},
        {
          timeoutMs: 5,
          consumeResponse: async (response, signal) => {
            bodySignal = signal;
            return response.text();
          },
        },
      ),
      (error) => error instanceof WorkerFetchTimeoutError,
    );
    assert.equal(bodySignal.aborted, true);
  });

  it('cancels a stalled body when the owning attempt is cancelled', async () => {
    const parent = new AbortController();
    let bodySignal;
    let bodyStarted;
    const enteredBody = new Promise((resolve) => { bodyStarted = resolve; });
    const pending = fetchWithDeadline(
      async () => ({ status: 200 }),
      '/api/register-worker',
      { signal: parent.signal },
      {
        timeoutMs: 1_000,
        consumeResponse: (_response, signal) => {
          bodySignal = signal;
          bodyStarted();
          return new Promise(() => {});
        },
      },
    );
    await enteredBody;
    parent.abort();

    await assert.rejects(pending, (error) => error.name === 'AbortError');
    assert.equal(bodySignal.aborted, true);
  });

  it('distinguishes an owning attempt cancellation from its deadline', async () => {
    const parent = new AbortController();
    let childSignal;
    const pending = fetchWithDeadline(async (_url, options) => {
      childSignal = options.signal;
      return new Promise(() => {});
    }, '/api/register-worker', { signal: parent.signal }, { timeoutMs: 1_000 });
    await Promise.resolve();
    parent.abort();

    await assert.rejects(pending, (error) => error.name === 'AbortError');
    assert.equal(childSignal.aborted, true);
  });
});

describe('worker startup status', () => {
  it('publishes actual phases while polling and resolves as soon as ready', async () => {
    const statuses = [
      { state: 'initializing', phase: 'connecting', code: null },
      { state: 'initializing', phase: 'services', code: null },
      { state: 'ready', phase: 'ready', code: null },
    ];
    const updates = [];
    const result = await waitForWorkerStartup({
      readStatus: async () => statuses.shift(),
      onStatus: (status) => updates.push(status.phase),
      intervalMs: 0,
      timeoutMs: 100,
    });

    assert.deepEqual(updates, ['connecting', 'services', 'ready']);
    assert.deepEqual(result, { state: 'ready', phase: 'ready', code: null });
  });

  it('stops on terminal failure and only preserves a stable safe code', async () => {
    let calls = 0;
    await assert.rejects(
      waitForWorkerStartup({
        readStatus: async () => {
          calls += 1;
          return { state: 'failed', phase: 'services', code: 'worker_start_failed' };
        },
        intervalMs: 0,
        timeoutMs: 100,
      }),
      (error) => error instanceof WorkerStartupError
        && error.kind === 'failed'
        && error.code === 'worker_start_failed',
    );
    assert.equal(calls, 1);
    assert.deepEqual(
      normalizeWorkerStartupStatus({ state: 'failed', phase: 'services', code: 'password=secret' }),
      { state: 'failed', phase: 'services', code: null },
    );
  });

  it('treats a missing startup endpoint as an older ready worker', async () => {
    const readStatus = createWorkerStartupStatusReader({
      baseUrl: 'http://127.0.0.1:18080/',
      fetchImpl: async (url, options) => {
        assert.equal(url, 'http://127.0.0.1:18080/api/startup-status');
        assert.equal(options.method, 'GET');
        assert.equal(options.cache, 'no-store');
        return { status: 404, ok: false };
      },
    });

    assert.deepEqual(await readStatus(), {
      state: 'ready', phase: 'ready', code: null, legacy: true,
    });
  });

  it('uses a same-origin startup route when the configured base is root', async () => {
    const readStatus = createWorkerStartupStatusReader({
      baseUrl: '/',
      fetchImpl: async (url) => {
        assert.equal(url, '/api/startup-status');
        return { status: 200, ok: true, json: async () => ({ state: 'ready', phase: 'ready', code: null }) };
      },
    });

    assert.deepEqual(await readStatus(), { state: 'ready', phase: 'ready', code: null });
  });

  it('times out a delayed status read and aborts it', async () => {
    let receivedSignal;
    await assert.rejects(
      waitForWorkerStartup({
        readStatus: ({ signal }) => {
          receivedSignal = signal;
          return new Promise(() => {});
        },
        timeoutMs: 5,
        intervalMs: 0,
      }),
      (error) => error instanceof WorkerStartupError && error.kind === 'timeout',
    );
    assert.equal(receivedSignal.aborted, true);
  });

  it('cancels polling when its owning view is abandoned', async () => {
    const abortController = new AbortController();
    const result = waitForWorkerStartup({
      readStatus: async () => ({ state: 'initializing', phase: 'resources' }),
      intervalMs: 100,
      timeoutMs: 1_000,
      signal: abortController.signal,
    });
    abortController.abort();

    await assert.rejects(result, (error) => error.kind === 'aborted');
  });
});

describe('setup attempt ownership', () => {
  it('shares a single in-flight setup and invalidates stale session continuations', async () => {
    const controller = createSetupAttemptController();
    let resolveSetup;
    let calls = 0;
    let sessionToken = 'session-a';
    const isCurrentAttempt = () => true;
    const setup = () => controller.run(({ isCurrent }) => {
      calls += 1;
      return new Promise((resolve) => {
        resolveSetup = () => resolve(isCurrentSessionAttempt(isCurrent, () => sessionToken, 'session-a'));
      });
    });

    const first = setup();
    const second = setup();
    assert.equal(first, second);
    await Promise.resolve();
    assert.equal(calls, 1);

    assert.equal(isCurrentSessionAttempt(isCurrentAttempt, () => sessionToken, 'session-a'), true);
    sessionToken = 'session-b';
    assert.equal(isCurrentSessionAttempt(isCurrentAttempt, () => sessionToken, 'session-a'), false);
    resolveSetup();
    assert.equal(await first, false);
  });

  it('does not let an invalidated attempt continue', async () => {
    const controller = createSetupAttemptController();
    let continueAttempt;
    let didContinue = false;
    const pending = controller.run(({ isCurrent }) => new Promise((resolve) => {
      continueAttempt = () => {
        if (isCurrent()) didContinue = true;
        resolve();
      };
    }));

    await Promise.resolve();
    controller.invalidate();
    continueAttempt();
    await pending;
    assert.equal(didContinue, false);
  });
});
