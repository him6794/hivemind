// Keep this aligned with jsonwebtoken's default 60-second expiration leeway.
export const JWT_CLOCK_SKEW_SECONDS = 60;

export function readStoredSession(storage, key) {
  if (!storage) return { token: '', username: '' };

  try {
    const raw = storage.getItem(key);
    if (!raw) return { token: '', username: '' };

    const parsed = JSON.parse(raw);
    const token = String(parsed?.token || '').trim();
    const username = String(parsed?.username || '').trim();

    if (!token || !username) return { token: '', username: '' };
    // The bearer JWT is self-describing: an already-expired token can never
    // recover, so drop it before the UI pretends the session is restored.
    // Signature validity stays a Nodepool concern.
    if (isExpiredJwt(token)) {
      storage.removeItem(key);
      return { token: '', username: '' };
    }
    return { token, username };
  } catch {
    return { token: '', username: '' };
  }
}

export function isExpiredJwt(token, nowMs = Date.now()) {
  const parts = String(token || '').split('.');
  if (parts.length !== 3) return false;
  try {
    const payload = JSON.parse(atob(parts[1].replace(/-/g, '+').replace(/_/g, '/')));
    const expiresAtSeconds = Number(payload?.exp);
    if (!Number.isFinite(expiresAtSeconds)) return false;
    const nowSeconds = Math.floor(nowMs / 1000);
    return expiresAtSeconds < nowSeconds - JWT_CLOCK_SKEW_SECONDS;
  } catch {
    return false;
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
