const THEME_KEY = 'hivemind.worker.theme';

export function readThemePreference(host = globalThis.window) {
  try {
    return host?.localStorage?.getItem(THEME_KEY) === 'dark' ? 'dark' : 'light';
  } catch {
    return 'light';
  }
}

export function writeThemePreference(theme, host = globalThis.window) {
  try {
    host?.localStorage?.setItem(THEME_KEY, theme === 'dark' ? 'dark' : 'light');
  } catch {
    // A blocked local preference should not prevent the Worker UI from loading.
  }
}
