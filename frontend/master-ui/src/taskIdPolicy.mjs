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

export function createTaskId(randomUuid = () => globalThis.crypto?.randomUUID?.()) {
  const taskId = randomUuid();
  const validated = validateTaskId(taskId);
  if (!validated.ok) {
    throw new Error('Unable to generate task_id');
  }
  return validated.taskId;
}

export function taskIdFromFileName(fileName) {
  const candidate = String(fileName ?? '')
    .replace(/\.[^.]+$/, '')
    .replace(/[^a-zA-Z0-9._-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 80);

  return isSafeTaskId(candidate) ? candidate : '';
}
