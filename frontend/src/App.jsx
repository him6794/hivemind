import React, { useCallback, useEffect, useMemo, useState } from 'react';
import Layout from './components/Layout';
import HomePage from './pages/HomePage';
import FeaturesPage from './pages/FeaturesPage';
import AccountPage from './pages/AccountPage';
import DownloadPage from './pages/DownloadPage';
import FaqPage from './pages/FaqPage';
import { createT, supportedLangs } from './i18n';

const TOKEN_KEY = 'hivemind.website.token';
const USER_KEY = 'hivemind.website.username';
const LANG_KEY = 'hivemind.website.lang';
const PAGE_IDS = new Set(['features', 'account', 'download', 'faq']);

function stripTrailingSlash(pathname) {
  if (!pathname || pathname === '/') return '/';
  return pathname.replace(/\/+$/, '') || '/';
}

function parseLocation(pathname) {
  const clean = stripTrailingSlash(pathname);
  const parts = clean.split('/').filter(Boolean);

  let lang = null;
  let index = 0;
  if (parts[0] === 'zh' || parts[0] === 'en') {
    lang = parts[0];
    index = 1;
  }

  const page = parts[index] || '';
  // Legacy aliases that should land on a real product page.
  const legacyAliases = {
    vpn: 'download',
  };
  const resolvedPage = legacyAliases[page] || page;
  const isLegacyAlias = Boolean(legacyAliases[page]);
  const known = !resolvedPage || PAGE_IDS.has(resolvedPage);
  const path = resolvedPage && PAGE_IDS.has(resolvedPage) ? `/${resolvedPage}` : '/';
  const fullPath = `/${lang || 'zh'}${path === '/' ? '' : path}`;

  return {
    lang,
    path: known ? path : '/',
    fullPath,
    known: Boolean(lang) && known && !isLegacyAlias,
    // Force redirect for missing locale, unknown pages, or legacy aliases.
    needsRedirect: !lang || (!known && Boolean(page)) || isLegacyAlias,
  };
}

function buildPath(lang, path = '/') {
  const normalized = path === '/' ? '' : stripTrailingSlash(path);
  return `/${lang}${normalized}`;
}

function readPreferredLang() {
  const stored = localStorage.getItem(LANG_KEY);
  if (supportedLangs.includes(stored)) return stored;
  return 'zh';
}

export default function App() {
  const apiBase = String(import.meta.env.VITE_API_BASE || 'http://localhost:8090').trim().replace(/\/$/, '');

  const initial = parseLocation(window.location.pathname);
  const [lang, setLang] = useState(() => initial.lang || readPreferredLang());
  const [path, setPath] = useState(() => initial.path);

  const [authMode, setAuthMode] = useState('login');
  const [username, setUsername] = useState(() => localStorage.getItem(USER_KEY) || '');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [status, setStatus] = useState('');
  const [statusTone, setStatusTone] = useState('neutral');
  const [loading, setLoading] = useState(false);
  const [busyAction, setBusyAction] = useState('');
  const [token, setToken] = useState(() => localStorage.getItem(TOKEN_KEY) || '');
  const [sessionUser, setSessionUser] = useState(() => localStorage.getItem(USER_KEY) || '');
  const [balance, setBalance] = useState(null);
  const [transferTo, setTransferTo] = useState('');
  const [transferAmount, setTransferAmount] = useState('');
  const [transferNote, setTransferNote] = useState('');
  const [bootstrapping, setBootstrapping] = useState(Boolean(localStorage.getItem(TOKEN_KEY)));

  const t = useMemo(() => createT(lang), [lang]);

  const setFlash = useCallback((message, tone = 'neutral') => {
    setStatus(message);
    setStatusTone(tone);
  }, []);

  const navigate = useCallback((nextPath, nextLang = lang) => {
    const targetLang = supportedLangs.includes(nextLang) ? nextLang : 'zh';
    let targetPath = '/';

    if (typeof nextPath === 'string') {
      const parsed = parseLocation(nextPath);
      if (nextPath === '/' || nextPath === '') {
        targetPath = '/';
      } else if (parsed.lang && (nextPath === `/${parsed.lang}` || nextPath.startsWith(`/${parsed.lang}/`))) {
        targetPath = parsed.path;
      } else {
        targetPath = stripTrailingSlash(nextPath.startsWith('/') ? nextPath : `/${nextPath}`);
        if (!PAGE_IDS.has(targetPath.slice(1)) && targetPath !== '/') {
          targetPath = '/';
        }
      }
    }

    const target = buildPath(targetLang, targetPath);
    if (window.location.pathname !== target) {
      window.history.pushState({}, '', target);
    }
    setLang(targetLang);
    setPath(targetPath);
    window.scrollTo({ top: 0, behavior: 'smooth' });
  }, [lang]);

  const syncFromLocation = useCallback(() => {
    const parsed = parseLocation(window.location.pathname);
    const preferred = parsed.lang || readPreferredLang();
    if (!parsed.lang || parsed.needsRedirect) {
      const target = buildPath(preferred, parsed.path);
      window.history.replaceState({}, '', target);
      setLang(preferred);
      setPath(parsed.path);
      return;
    }
    setLang(parsed.lang);
    setPath(parsed.path);
  }, []);

  useEffect(() => {
    syncFromLocation();
    const onPop = () => syncFromLocation();
    window.addEventListener('popstate', onPop);
    return () => window.removeEventListener('popstate', onPop);
  }, [syncFromLocation]);

  useEffect(() => {
    document.title = lang === 'zh' ? 'Hivemind | 開放算力網路' : 'Hivemind | Open Compute Network';
    document.documentElement.lang = lang === 'zh' ? 'zh-Hant' : 'en';
  }, [lang]);

  useEffect(() => {
    localStorage.setItem(LANG_KEY, lang);
  }, [lang]);

  async function readJson(res) {
    const text = await res.text();
    if (!text) return {};
    try {
      return JSON.parse(text);
    } catch {
      return { message: text };
    }
  }

  async function api(method, pathName, body, authToken = token) {
    const headers = {};
    if (authToken) headers.Authorization = `Bearer ${authToken}`;
    if (body !== undefined) headers['Content-Type'] = 'application/json';

    let res;
    try {
      res = await fetch(`${apiBase}${pathName}`, {
        method,
        headers,
        body: body !== undefined ? JSON.stringify(body) : undefined,
      });
    } catch (err) {
      throw new Error(`API unreachable (${apiBase}): ${err.message || err}`);
    }

    const data = await readJson(res);
    if (res.status === 401) {
      const error = new Error(data.message || t('account.sessionExpired'));
      error.code = 401;
      throw error;
    }
    return { ok: res.ok, status: res.status, data };
  }

  const persistSession = useCallback((nextToken, nextUser) => {
    setToken(nextToken || '');
    setSessionUser(nextUser || '');
    if (nextToken) localStorage.setItem(TOKEN_KEY, nextToken);
    else localStorage.removeItem(TOKEN_KEY);
    if (nextUser) localStorage.setItem(USER_KEY, nextUser);
    else localStorage.removeItem(USER_KEY);
  }, []);

  const logout = useCallback((message) => {
    persistSession('', sessionUser);
    setBalance(null);
    setTransferTo('');
    setTransferAmount('');
    setTransferNote('');
    setFlash(message || t('account.signedOut'), 'neutral');
  }, [persistSession, sessionUser, setFlash, t]);

  const refreshBalance = useCallback(async (authToken = token) => {
    if (!authToken) return null;
    const { data } = await api('GET', '/api/balance', undefined, authToken);
    if (!data.success) throw new Error(data.message || 'Failed to load balance');
    const next = Number(data.balance || 0);
    setBalance(next);
    return next;
  }, [token, apiBase, t]);

  useEffect(() => {
    let cancelled = false;
    async function bootstrap() {
      if (!token) {
        setBootstrapping(false);
        return;
      }
      try {
        await refreshBalance(token);
        if (!cancelled) setFlash(t('account.sessionRestored'), 'ok');
      } catch (err) {
        if (!cancelled) {
          persistSession('', sessionUser);
          setBalance(null);
          setFlash(err.message || t('account.sessionExpired'), 'err');
        }
      } finally {
        if (!cancelled) setBootstrapping(false);
      }
    }
    bootstrap();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!token) return undefined;
    const id = setInterval(() => {
      refreshBalance(token).catch(() => {});
    }, 15000);
    return () => clearInterval(id);
  }, [token, refreshBalance]);

  async function handleAuthSubmit(e) {
    e.preventDefault();
    setLoading(true);
    setBusyAction('auth');
    setFlash(authMode === 'register' ? t('account.creating') : t('account.signingIn'));

    try {
      const name = username.trim();
      if (name.length < 3) throw new Error(t('account.usernameShort'));
      if (password.length < 8) throw new Error(t('account.passwordShort'));

      if (authMode === 'register') {
        if (password !== confirmPassword) throw new Error(t('account.passwordMismatch'));
        const register = await api('POST', '/api/register', { username: name, password }, '');
        if (!register.data.success) throw new Error(register.data.message || 'Registration failed');
      }

      const login = await api('POST', '/api/login', { username: name, password }, '');
      if (!login.data.success || !login.data.token) {
        throw new Error(login.data.message || login.data.status_message || 'Sign-in failed');
      }

      const nextToken = login.data.token;
      persistSession(nextToken, name);
      setUsername(name);
      setPassword('');
      setConfirmPassword('');
      await refreshBalance(nextToken);
      setFlash(t('account.sessionRestored'), 'ok');
    } catch (err) {
      if (err.code === 401) logout(err.message);
      else setFlash(err.message || 'Request failed', 'err');
    } finally {
      setLoading(false);
      setBusyAction('');
    }
  }

  async function handleTransfer(e) {
    e.preventDefault();
    if (!token) return;
    setLoading(true);
    setBusyAction('transfer');
    setFlash(t('account.sending'));
    try {
      const to = transferTo.trim();
      const amount = Number(transferAmount);
      if (!to) throw new Error(t('account.needRecipient'));
      if (sessionUser && to === sessionUser) throw new Error(t('account.cannotSelf'));
      if (!Number.isFinite(amount) || amount <= 0 || !Number.isInteger(amount)) {
        throw new Error(t('account.needAmount'));
      }
      const res = await api(
        'POST',
        '/api/transfer',
        {
          to_username: to,
          amount_cpt: amount,
          idempotency_key: `web-${Date.now()}-${Math.random().toString(16).slice(2, 10)}`,
        },
        token,
      );
      if (!res.data.success) throw new Error(res.data.message || 'Transfer failed');
      setBalance(Number(res.data.from_balance ?? balance ?? 0));
      setTransferAmount('');
      setTransferNote(t('account.transferred', { amount, to }));
      setFlash(t('account.transferred', { amount, to }), 'ok');
      await refreshBalance(token);
    } catch (err) {
      if (err.code === 401) logout(err.message);
      else setFlash(err.message || 'Transfer failed', 'err');
    } finally {
      setLoading(false);
      setBusyAction('');
    }
  }

  const session = {
    token,
    sessionUser,
    username,
    setUsername,
    password,
    setPassword,
    confirmPassword,
    setConfirmPassword,
    balance,
    bootstrapping,
    loading,
    busyAction,
    status,
    statusTone,
    transferTo,
    setTransferTo,
    transferAmount,
    setTransferAmount,
    transferNote,
    handleAuthSubmit,
    handleTransfer,
    logout,
    authMode,
    setAuthMode,
  };

  let page = null;
  if (path === '/features') page = <FeaturesPage lang={lang} />;
  else if (path === '/account') page = <AccountPage lang={lang} t={t} navigate={navigate} session={session} />;
  else if (path === '/download') page = <DownloadPage lang={lang} t={t} navigate={navigate} />;
  else if (path === '/faq') page = <FaqPage lang={lang} />;
  else page = <HomePage lang={lang} t={t} navigate={navigate} />;

  return (
    <Layout
      t={t}
      lang={lang}
      path={path}
      navigate={navigate}
      onToggleLang={() => navigate(path, lang === 'zh' ? 'en' : 'zh')}
      sessionUser={sessionUser}
    >
      {page}
    </Layout>
  );
}
