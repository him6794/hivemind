const TASK_ID_ERROR = "task_id must be non-empty ASCII alphanumeric, '.', '-', or '_' and cannot contain '..'";

function normalizedTaskId(taskId) {
  return String(taskId ?? '').trim();
}

export function isSafeTaskId(taskId) {
  const value = normalizedTaskId(taskId);
  if (!value || value === '.' || value.includes('..')) {
    return false;
  }
  return /^[A-Za-z0-9._-]+$/.test(value);
}

export function validateTaskId(taskId) {
  const value = normalizedTaskId(taskId);
  if (!isSafeTaskId(value)) {
    return { ok: false, taskId: '', message: TASK_ID_ERROR };
  }
  return { ok: true, taskId: value, message: '' };
}

function createHttpSafeFallbackTaskId() {
  const timestamp = Date.now().toString(36);
  const entropy = `${Math.random().toString(36).slice(2, 12)}${Math.random().toString(36).slice(2, 12)}`;
  return `task-${timestamp}-${entropy}`;
}

export function createTaskId(
  randomUuid = () => globalThis.crypto?.randomUUID?.(),
  fallbackId = createHttpSafeFallbackTaskId,
) {
  try {
    const generated = validateTaskId(randomUuid());
    if (generated.ok) {
      return generated.taskId;
    }
  } catch {
    // randomUUID is restricted to secure contexts in some browsers.
  }

  const fallback = validateTaskId(fallbackId());
  if (fallback.ok) {
    return fallback.taskId;
  }
  throw new Error('Unable to generate task_id');
}

export function taskIdFromFileName(fileName) {
  const candidate = String(fileName ?? '')
    .replace(/\.[^.]+$/, '')
    .replace(/[^a-zA-Z0-9._-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 80);

  return isSafeTaskId(candidate) ? candidate : '';
}
