const THEME_PREFERENCE_KEY = 'hivemind.master.theme.v1';

function browserStorage() {
  try {
    return globalThis.localStorage;
  } catch {
    return null;
  }
}

export function readThemePreference(storage = browserStorage()) {
  try {
    return storage?.getItem(THEME_PREFERENCE_KEY) === 'dark' ? 'dark' : 'light';
  } catch {
    return 'light';
  }
}

export function saveThemePreference(theme, storage = browserStorage()) {
  try {
    storage?.setItem(THEME_PREFERENCE_KEY, theme === 'dark' ? 'dark' : 'light');
  } catch {
    // Theme preference is optional; it never blocks console startup.
  }
}
