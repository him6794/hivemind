export const WORKER_STARTUP_TIMEOUT_MS = 60_000;
export const WORKER_STARTUP_POLL_INTERVAL_MS = 1_000;

const STARTUP_STATES = new Set(['initializing', 'ready', 'failed']);
const STARTUP_PHASES = new Set(['starting', 'connecting', 'resources', 'services', 'ready']);
const SAFE_FAILURE_CODE = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;

export class WorkerStartupError extends Error {
  constructor(kind, code = null) {
    const messages = {
      aborted: 'Worker startup check was cancelled.',
      failed: 'The Worker service could not start.',
      'invalid-response': 'The Worker service returned an invalid startup status.',
      request: 'The Worker startup status could not be reached.',
      timeout: 'Worker startup is taking longer than expected.',
    };
    super(messages[kind] || messages.request);
    this.name = 'WorkerStartupError';
    this.kind = kind;
    this.code = typeof code === 'string' && SAFE_FAILURE_CODE.test(code) ? code : null;
  }
}

export class WorkerFetchTimeoutError extends Error {
  constructor() {
    super('The local Worker request timed out.');
    this.name = 'WorkerFetchTimeoutError';
  }
}

export class WorkerLoginError extends Error {
  constructor(kind, status = null) {
    super('The Worker sign-in request could not be completed.');
    this.name = 'WorkerLoginError';
    this.kind = kind;
    this.status = Number.isInteger(status) && status >= 100 && status <= 599 ? status : null;
  }
}

export function getWorkerLoginError(response) {
  const data = response?.data;
  if (!response?.ok || data?.success !== true) {
    return new WorkerLoginError('response', response?.status);
  }
  if (typeof data.token !== 'string' || !data.token.trim()) {
    return new WorkerLoginError('invalid-response');
  }
  return null;
}

export function getWorkerSetupErrorMessage(error, { kind, loginEstablished = false } = {}) {
  const requestTimedOut = error instanceof WorkerFetchTimeoutError;
  if (loginEstablished) {
    return requestTimedOut
      ? 'Sign-in succeeded, but worker setup timed out. Retry connection when ready.'
      : 'Sign-in succeeded, but worker setup is incomplete. Retry connection when ready.';
  }
  if (requestTimedOut) {
    return 'The Worker app did not respond in time. Make sure it is open, then try again.';
  }
  if (kind !== 'login') {
    return 'We could not prepare this computer. Make sure the Worker app is open, then try again.';
  }
  if (error instanceof WorkerLoginError) {
    if (error.kind === 'network') {
      return 'The Worker app could not be reached. Make sure it is open, then try again.';
    }
    if (error.status === 429) {
      return 'Too many sign-in attempts. Please wait before trying again.';
    }
    if (error.status >= 500) {
      return 'The sign-in service could not complete this request. Please try again later.';
    }
    // Older APIs also report internal authentication failures as 401. Do not
    // claim a rejected request proves that the username or password is wrong.
    if (error.status === 401 || error.status === 403) {
      return 'The sign-in request was not accepted. Check your sign-in details or try again later.';
    }
    if (error.kind === 'invalid-response') {
      return 'The sign-in service returned an incomplete response. Please try again later.';
    }
  }
  return 'Sign-in could not be completed. Please try again later.';
}

function createFetchAbortError() {
  const error = new Error('The local Worker request was cancelled.');
  error.name = 'AbortError';
  return error;
}

export async function fetchWithDeadline(
  fetchImpl,
  url,
  options = {},
  { timeoutMs = 90_000, consumeResponse = (response) => response } = {},
) {
  if (typeof fetchImpl !== 'function') throw new Error('A fetch implementation is required');
  const parentSignal = options?.signal;
  if (parentSignal?.aborted) throw createFetchAbortError();

  const controller = new AbortController();
  let timer;
  let rejectCancelled;
  const cancelled = new Promise((_, reject) => {
    rejectCancelled = reject;
  });
  const onParentAbort = () => {
    controller.abort();
    rejectCancelled(createFetchAbortError());
  };
  parentSignal?.addEventListener('abort', onParentAbort, { once: true });
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => {
      controller.abort();
      reject(new WorkerFetchTimeoutError());
    }, Math.max(0, timeoutMs));
  });
  const request = Promise.resolve()
    .then(() => fetchImpl(url, { ...options, signal: controller.signal }))
    .then((response) => consumeResponse(response, controller.signal));

  try {
    return await Promise.race([request, timeout, cancelled]);
  } finally {
    clearTimeout(timer);
    parentSignal?.removeEventListener('abort', onParentAbort);
  }
}

export function normalizeWorkerStartupStatus(payload) {
  const source = payload && typeof payload === 'object' && !Array.isArray(payload) ? payload : {};
  const state = STARTUP_STATES.has(source.state) ? source.state : 'failed';
  const phase = STARTUP_PHASES.has(source.phase) ? source.phase : 'starting';
  const code = typeof source.code === 'string' && SAFE_FAILURE_CODE.test(source.code) ? source.code : null;

  if (state === 'failed' && !STARTUP_STATES.has(source.state)) {
    return { state, phase: 'starting', code: 'invalid_status' };
  }

  return {
    state,
    phase: state === 'ready' ? 'ready' : phase,
    code: state === 'failed' ? code : null,
  };
}

export function createWorkerStartupStatusReader({ baseUrl, fetchImpl = globalThis.fetch } = {}) {
  const base = String(baseUrl || '').trim().replace(/\/+$/, '');
  if (typeof fetchImpl !== 'function') throw new Error('A fetch implementation is required');
  const statusUrl = `${base}/api/startup-status`;

  return async ({ signal } = {}) => {
    let response;
    try {
      response = await fetchImpl(statusUrl, {
        method: 'GET',
        cache: 'no-store',
        signal,
      });
    } catch (error) {
      if (signal?.aborted) throw error;
      throw new WorkerStartupError('request');
    }

    // Older local Worker APIs do not expose the startup route. Keep their
    // read-only onboarding path usable instead of treating the 404 as failure.
    if (response.status === 404) {
      return { state: 'ready', phase: 'ready', code: null, legacy: true };
    }
    if (!response.ok) throw new WorkerStartupError('request');

    let payload;
    try {
      payload = await response.json();
    } catch {
      throw new WorkerStartupError('invalid-response');
    }
    return normalizeWorkerStartupStatus(payload);
  };
}

function waitForInterval(milliseconds, signal) {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new WorkerStartupError('aborted'));
      return;
    }

    const timer = setTimeout(() => {
      signal.removeEventListener('abort', abort);
      resolve();
    }, milliseconds);
    const abort = () => {
      clearTimeout(timer);
      signal.removeEventListener('abort', abort);
      reject(new WorkerStartupError('aborted'));
    };
    signal.addEventListener('abort', abort, { once: true });
  });
}

export async function waitForWorkerStartup({
  readStatus,
  onStatus = () => {},
  signal,
  timeoutMs = WORKER_STARTUP_TIMEOUT_MS,
  intervalMs = WORKER_STARTUP_POLL_INTERVAL_MS,
} = {}) {
  if (typeof readStatus !== 'function') throw new Error('A startup status reader is required');

  const controller = new AbortController();
  let rejectTermination;
  let finished = false;
  const termination = new Promise((_, reject) => {
    rejectTermination = reject;
  });
  const terminate = (error) => {
    if (finished) return;
    finished = true;
    controller.abort();
    rejectTermination(error);
  };
  const onAbort = () => terminate(new WorkerStartupError('aborted'));
  if (signal?.aborted) onAbort();
  else signal?.addEventListener('abort', onAbort, { once: true });

  const timer = setTimeout(() => terminate(new WorkerStartupError('timeout')), Math.max(0, timeoutMs));
  const poll = async () => {
    while (!controller.signal.aborted) {
      let rawStatus;
      try {
        rawStatus = await readStatus({ signal: controller.signal });
      } catch (error) {
        if (controller.signal.aborted) throw new WorkerStartupError('aborted');
        // A temporarily unavailable local endpoint is safe to poll again. The
        // overall timeout bounds this read-only retry loop.
        await waitForInterval(Math.max(0, intervalMs), controller.signal);
        continue;
      }
      if (controller.signal.aborted) throw new WorkerStartupError('aborted');

      const status = normalizeWorkerStartupStatus(rawStatus);
      onStatus(status);
      if (status.state === 'ready') return status;
      if (status.state === 'failed') throw new WorkerStartupError('failed', status.code);
      await waitForInterval(Math.max(0, intervalMs), controller.signal);
    }
    throw new WorkerStartupError('aborted');
  };

  try {
    return await Promise.race([poll(), termination]);
  } finally {
    finished = true;
    clearTimeout(timer);
    signal?.removeEventListener('abort', onAbort);
    controller.abort();
  }
}

export function isCurrentSessionAttempt(isAttemptCurrent, getSessionToken, expectedToken) {
  return Boolean(
    typeof isAttemptCurrent === 'function'
      && isAttemptCurrent()
      && typeof getSessionToken === 'function'
      && getSessionToken() === expectedToken,
  );
}

export function createSetupAttemptController() {
  let generation = 0;
  let active = null;

  return {
    run(task) {
      if (active) return active.promise;
      const attemptId = ++generation;
      const controller = new AbortController();
      const isCurrent = () => attemptId === generation && !controller.signal.aborted;
      const attempt = { attemptId, controller, promise: null };
      attempt.promise = Promise.resolve()
        .then(() => task({ attemptId, signal: controller.signal, isCurrent }))
        .finally(() => {
          if (active?.attemptId === attemptId) active = null;
        });
      active = attempt;
      return attempt.promise;
    },
    invalidate() {
      generation += 1;
      const previous = active;
      active = null;
      previous?.controller.abort();
    },
  };
}
