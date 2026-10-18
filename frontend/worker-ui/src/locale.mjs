import { zh } from './messages.mjs';
import { getDesktopBridge } from './desktopLifecycle.mjs';

export const LOCALES = [
  { code: 'en', label: 'English', tag: 'en' },
  { code: 'zh', label: '繁體中文', tag: 'zh-TW' },
];
const STORAGE_KEY = 'hivemind.worker.locale.v1';

function localeStorage() {
  try { return globalThis.localStorage ?? null; } catch { return null; }
}

export function normalizeLocale(value) {
  return value === 'zh' ? 'zh' : 'en';
}

export function localeTag(locale) {
  return normalizeLocale(locale) === 'zh' ? 'zh-TW' : 'en';
}

export function readLocale(storage = localeStorage()) {
  try { return normalizeLocale(storage?.getItem(STORAGE_KEY)); } catch { return 'en'; }
}

export function saveLocale(locale, storage = localeStorage()) {
  try { storage?.setItem(STORAGE_KEY, normalizeLocale(locale)); } catch {
    // A blocked preference store must not block the console.
  }
}

export function createLocaleAdapter({ host = globalThis.window, storage, timeoutMs = 4500 } = {}) {
  const bridge = getDesktopBridge(host);
  function call(method, ...args) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('Language preference request timed out')), timeoutMs);
      Promise.resolve().then(() => {
        if (typeof bridge[method] !== 'function') throw new Error('Native language preferences are unavailable');
        return bridge[method](...args);
      }).then(resolve, reject).finally(() => clearTimeout(timer));
    });
  }
  return {
    native: Boolean(bridge),
    read() {
      if (!bridge) return Promise.resolve(readLocale(storage));
      return call('readLocale').then((value) => {
        if (value !== null && value !== 'en' && value !== 'zh') throw new Error('Invalid native language preference');
        return value ?? 'en';
      });
    },
    write(value) {
      if (value !== 'en' && value !== 'zh') return Promise.reject(new Error('Invalid language preference'));
      if (!bridge) { saveLocale(value, storage); return Promise.resolve(); }
      return call('writeLocale', value);
    },
  };
}

// Keep messages unresolved in state so switching languages updates pending UI.
export function message(key, params) {
  return { key, params };
}

export function localeError(key, params) {
  return Object.assign(new Error(t('en', key, params)), { uiMessage: message(key, params) });
}

export function errorMessage(error, fallback = 'Request failed') {
  return error?.uiMessage ?? (error?.message ? { raw: error.message } : message(fallback));
}

export function t(locale, entry, params) {
  if (entry && typeof entry === 'object') {
    // Server diagnostics, source, input, and task output are never translated.
    if (Object.hasOwn(entry, 'raw')) return String(entry.raw ?? '');
    if (typeof entry.key === 'string') return t(locale, entry.key, entry.params);
  }
  const code = normalizeLocale(locale);
  const english = typeof entry === 'string' ? entry : entry?.en ?? '';
  const translated = typeof entry === 'string'
    ? Object.hasOwn(zh, entry) ? zh[entry] : english
    : entry?.zh || english;
  const text = code === 'zh' ? translated : english;
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (placeholder, key) => {
    if (!Object.hasOwn(params, key)) return placeholder;
    const value = params[key];
    return value && typeof value === 'object' ? t(locale, value) : String(value);
  });
}

export function createFormatters(locale) {
  return {
    formatMetric(value, unit = '', maximumFractionDigits = 1) {
      if (value === null || value === undefined || !Number.isFinite(value)) return t(locale, 'Unknown');
      const formatted = new Intl.NumberFormat(localeTag(locale), { maximumFractionDigits }).format(value);
      return unit ? `${formatted} ${t(locale, unit)}` : formatted;
    },
    formatTimestamp(value) {
      if (!value) return t(locale, 'Unknown');
      const date = new Date(value);
      if (!Number.isFinite(date.getTime())) return value;
      return new Intl.DateTimeFormat(localeTag(locale), { dateStyle: 'medium', timeStyle: 'short' }).format(date);
    },
  };
}
