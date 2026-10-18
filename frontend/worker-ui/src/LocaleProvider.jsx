import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { LOCALES, createFormatters, createLocaleAdapter, localeTag, normalizeLocale, readLocale, saveLocale, t as resolveMessage } from './locale.mjs';

export const LocaleContext = createContext({
  locale: 'en', setLocale: () => {}, t: (entry, params) => resolveMessage('en', entry, params),
  ...createFormatters('en'),
});

export function LocaleProvider({ children }) {
  const [adapter] = useState(createLocaleAdapter);
  const [locale, setLocaleState] = useState(() => adapter.native ? 'en' : readLocale());
  const [initialized, setInitialized] = useState(!adapter.native);
  const [preferenceError, setPreferenceError] = useState('');
  const [saveState, setSaveState] = useState('');
  const alive = useRef(false);
  const revision = useRef(0);
  useEffect(() => {
    alive.current = true;
    let active = true;
    const version = revision.current;
    if (adapter.native) {
      adapter.read().then((value) => {
        if (active && version === revision.current) setLocaleState(value);
      }).catch(() => {
        if (active && version === revision.current) setPreferenceError('Could not read the language preference. Using English.');
      }).finally(() => { if (active) setInitialized(true); });
    }
    return () => { active = false; alive.current = false; revision.current += 1; };
  }, [adapter]);
  const setLocale = useCallback((next) => {
    const value = normalizeLocale(next);
    const version = ++revision.current;
    setLocaleState(value);
    setPreferenceError('');
    if (!adapter.native) { saveLocale(value); return; }
    setSaveState('saving');
    adapter.write(value).then(() => {
      if (alive.current && version === revision.current) setSaveState('saved');
    }).catch(() => {
      if (alive.current && version === revision.current) {
        setSaveState('error');
        setPreferenceError('Language changed, but saving could not be confirmed.');
      }
    });
  }, [adapter]);
  useEffect(() => {
    document.documentElement.lang = localeTag(locale);
  }, [locale]);
  const t = useCallback((entry, params) => resolveMessage(locale, entry, params), [locale]);
  const value = useMemo(() => ({ locale, setLocale, t, preferenceError, saveState, ...createFormatters(locale) }), [locale, setLocale, t, preferenceError, saveState]);
  return <LocaleContext.Provider value={value}>{initialized ? children : null}</LocaleContext.Provider>;
}

export function useLocale() {
  return useContext(LocaleContext);
}

export function LocaleSelect() {
  const { locale, setLocale, t, preferenceError, saveState } = useLocale();
  return (
    <div className="flex max-w-xs flex-col gap-1">
      <select
        aria-label={t('Language')}
        aria-busy={saveState === 'saving'}
        value={locale}
        onChange={(event) => setLocale(event.target.value)}
        className="h-9 max-w-full rounded-md border border-input bg-background px-2 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        {LOCALES.map(({ code, label, tag }) => <option key={code} value={code} lang={tag}>{label}</option>)}
      </select>
      {preferenceError && <p role="alert" className="text-xs text-destructive">{t(preferenceError)}</p>}
    </div>
  );
}
