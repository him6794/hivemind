const STARTUP_PHASES = new Set(['starting', 'connecting', 'resources', 'services', 'ready']);

export function createRequestTimeout(parentSignal, timeoutMs) {
  const controller = new AbortController();
  let timedOut = false;
  const onParentAbort = () => controller.abort(parentSignal?.reason);

  if (parentSignal?.aborted) {
    onParentAbort();
  } else {
    parentSignal?.addEventListener('abort', onParentAbort, { once: true });
  }

  const timeoutId = setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, timeoutMs);

  return {
    signal: controller.signal,
    didTimeout: () => timedOut,
    dispose: () => {
      clearTimeout(timeoutId);
      parentSignal?.removeEventListener('abort', onParentAbort);
    },
  };
}

function waitForDelay(milliseconds, signal) {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new Error('Startup check cancelled'));
      return;
    }

    const onAbort = () => {
      clearTimeout(timeoutId);
      reject(new Error('Startup check cancelled'));
    };
    const timeoutId = setTimeout(() => {
      signal.removeEventListener('abort', onAbort);
      resolve();
    }, milliseconds);
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

function safeFailureCode(value) {
  const code = String(value ?? '').trim();
  return /^[a-zA-Z0-9_-]{1,64}$/.test(code) ? code : '';
}

export function createStartupStatusController({
  checkStatus,
  onState = () => {},
  timeoutMs = 60_000,
  pollIntervalMs = 1_000,
}) {
  let activeAttempt = null;

  function cancel() {
    const attempt = activeAttempt;
    if (!attempt) return;
    activeAttempt = null;
    clearTimeout(attempt.timeoutId);
    attempt.controller.abort();
  }

  function start() {
    if (activeAttempt) return activeAttempt.promise;

    const controller = new AbortController();
    const attempt = {
      controller,
      promise: null,
      timeoutId: null,
    };
    activeAttempt = attempt;
    const deadline = Date.now() + timeoutMs;
    let timedOut = false;

    const publish = (value, allowAborted = false) => {
      if (activeAttempt === attempt && (!controller.signal.aborted || allowAborted)) onState(value);
    };

    publish({ state: 'checking', phase: 'starting' });
    attempt.timeoutId = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, timeoutMs);

    attempt.promise = (async () => {
      try {
        while (!controller.signal.aborted) {
          try {
            const response = await checkStatus(controller.signal);
            if (activeAttempt !== attempt || controller.signal.aborted) break;

            if (response?.legacy === true || response?.status === 404) {
              publish({ state: 'ready', phase: 'ready' });
              return { state: 'ready' };
            }

            const payload = response?.data ?? response;
            if (payload?.state === 'ready') {
              publish({ state: 'ready', phase: 'ready' });
              return { state: 'ready' };
            }
            if (payload?.state === 'failed') {
              const code = safeFailureCode(payload.code);
              const phase = STARTUP_PHASES.has(payload.phase) ? payload.phase : 'services';
              publish({ state: 'failed', phase, code });
              return { state: 'failed', code };
            }
            if (payload?.state !== 'initializing') {
              throw new Error('Invalid startup status response');
            }

            const phase = STARTUP_PHASES.has(payload.phase) && payload.phase !== 'ready'
              ? payload.phase
              : 'starting';
            publish({ state: 'checking', phase });
          } catch (error) {
            if (controller.signal.aborted || activeAttempt !== attempt) break;
          }

          const remainingMs = deadline - Date.now();
          if (remainingMs <= 0) {
            timedOut = true;
            break;
          }
          try {
            await waitForDelay(Math.min(pollIntervalMs, remainingMs), controller.signal);
          } catch {
            break;
          }
        }

        if (timedOut || Date.now() >= deadline) {
          const message = 'Hivemind did not become ready within 60 seconds. Check the connection and retry.';
          publish({ state: 'error', phase: 'starting', message }, true);
          return { state: 'error', message };
        }
        return { state: 'cancelled' };
      } finally {
        clearTimeout(attempt.timeoutId);
        if (activeAttempt === attempt) activeAttempt = null;
      }
    })();

    return attempt.promise;
  }

  return { start, cancel };
}

export function createTokenSetupController({
  bootstrapVpn,
  loadTasks,
  getCurrentToken,
  onPhase = () => {},
  onSuccess = () => {},
  onError = () => {},
}) {
  let currentAttempt = null;

  function cancelAttempt(attempt) {
    if (!attempt || attempt.controller.signal.aborted) return;
    attempt.controller.abort();
    if (currentAttempt === attempt) currentAttempt = null;
  }

  function makeAttempt(token) {
    const attempt = {
      token,
      controller: new AbortController(),
      subscribers: 0,
      settled: false,
      promise: null,
    };
    currentAttempt = attempt;

    const isCurrent = () =>
      currentAttempt === attempt &&
      !attempt.controller.signal.aborted &&
      getCurrentToken() === token;

    attempt.promise = (async () => {
      let phase = 'connecting';
      try {
        if (!isCurrent()) return { state: 'cancelled' };
        onPhase({ token, phase });
        await bootstrapVpn(token, attempt.controller.signal);
        if (!isCurrent()) return { state: 'cancelled' };

        phase = 'tasks';
        onPhase({ token, phase });
        await loadTasks(token, attempt.controller.signal);
        if (!isCurrent()) return { state: 'cancelled' };

        onSuccess({ token });
        return { state: 'ready' };
      } catch (error) {
        if (!isCurrent()) return { state: 'cancelled' };
        onError({ token, phase, error });
        return { state: 'error', phase, error };
      } finally {
        attempt.settled = true;
      }
    })();

    return attempt;
  }

  function acquire(token) {
    if (!token) {
      return { promise: Promise.resolve({ state: 'cancelled' }), release() {} };
    }

    if (currentAttempt && currentAttempt.token !== token) {
      cancelAttempt(currentAttempt);
    }

    let attempt = currentAttempt;
    if (!attempt || attempt.settled || attempt.controller.signal.aborted) {
      attempt = makeAttempt(token);
    }
    attempt.subscribers += 1;

    let released = false;
    return {
      promise: attempt.promise,
      release() {
        if (released) return;
        released = true;
        attempt.subscribers = Math.max(0, attempt.subscribers - 1);
        queueMicrotask(() => {
          if (attempt.subscribers === 0 && currentAttempt === attempt) {
            cancelAttempt(attempt);
          }
        });
      },
    };
  }

  function cancel() {
    cancelAttempt(currentAttempt);
  }

  return { acquire, cancel };
}

export function getTaskListView({ tasks = [], initialLoading = false, error = '' }) {
  if (tasks.length > 0) return 'rows';
  if (initialLoading) return 'loading';
  if (error) return 'error';
  return 'empty';
}
