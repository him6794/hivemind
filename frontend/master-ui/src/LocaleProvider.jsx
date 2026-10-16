import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import { LOCALES, localeTag, normalizeLocale, readLocale, saveLocale, t as resolveMessage } from './locale.mjs';

export const LocaleContext = createContext({ locale: 'en', setLocale: () => {}, t: (entry, params) => resolveMessage('en', entry, params) });

export function LocaleProvider({ children }) {
  const [locale, setLocaleState] = useState(readLocale);
  const setLocale = useCallback((next) => {
    const value = normalizeLocale(next);
    saveLocale(value);
    setLocaleState(value);
  }, []);
  useEffect(() => {
    document.documentElement.lang = localeTag(locale);
  }, [locale]);
  const t = useCallback((entry, params) => resolveMessage(locale, entry, params), [locale]);
  const value = useMemo(() => ({ locale, setLocale, t }), [locale, setLocale, t]);
  return <LocaleContext.Provider value={value}>{children}</LocaleContext.Provider>;
}

export function useLocale() {
  return useContext(LocaleContext);
}

export function LocaleSelect() {
  const { locale, setLocale, t } = useLocale();
  return (
    <select
      aria-label={t('Language')}
      value={locale}
      onChange={(event) => setLocale(event.target.value)}
      className="h-9 max-w-full rounded-md border border-input bg-background px-2 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      {LOCALES.map(({ code, label, tag }) => <option key={code} value={code} lang={tag}>{label}</option>)}
    </select>
  );
}
