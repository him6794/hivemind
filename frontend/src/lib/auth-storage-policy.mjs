export const LEGACY_AUTH_STORAGE_KEY = 'hivemind-site-auth';

export function clearLegacyAuthStorage(storage) {
  if (!storage || typeof storage.removeItem !== 'function') {
    return false;
  }

  try {
    storage.removeItem(LEGACY_AUTH_STORAGE_KEY);
    return true;
  } catch {
    return false;
  }
}
