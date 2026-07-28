export function readStoredSession(storage, key) {
  if (!storage) return { token: '', username: '' };

  try {
    const raw = storage.getItem(key);
    if (!raw) return { token: '', username: '' };

    const parsed = JSON.parse(raw);
    const token = String(parsed?.token || '').trim();
    const username = String(parsed?.username || '').trim();

    if (!token || !username) return { token: '', username: '' };
    return { token, username };
  } catch {
    return { token: '', username: '' };
  }
}

export function saveStoredSession(storage, key, session) {
  if (!storage) return;

  const token = String(session?.token || '').trim();
  const username = String(session?.username || '').trim();
  if (!token || !username) return;

  storage.setItem(key, JSON.stringify({ token, username }));
}

export function clearStoredSession(storage, key) {
  if (!storage) return;
  storage.removeItem(key);
}
