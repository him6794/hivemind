import React, { useCallback, useEffect, useMemo, useState } from 'react';
import Layout from './components/Layout';
import HomePage from './pages/HomePage';
import FeaturesPage from './pages/FeaturesPage';
import AccountPage from './pages/AccountPage';
import VpnPage from './pages/VpnPage';
import FaqPage from './pages/FaqPage';
import { createT, supportedLangs } from './i18n';

const TOKEN_KEY = 'hivemind.website.token';
const USER_KEY = 'hivemind.website.username';
const LANG_KEY = 'hivemind.website.lang';

function normalizePath(pathname) {
  if (!pathname || pathname === '/') return '/';
  const clean = pathname.replace(/\/+$/, '');
  return clean || '/';
}

function readInitialLang() {
  const stored = localStorage.getItem(LANG_KEY);
  if (supportedLangs.includes(stored)) return stored;
  return 'zh';
}

export default function App() {
  const apiBase = String(import.meta.env.VITE_API_BASE || 'http://localhost:8082').trim().replace(/\/$/, '');
  const masterUi = String(import.meta.env.VITE_MASTER_UI || 'http://100.124.230.74:3000').trim().replace(/\/$/, '');
  const workerUi = String(import.meta.env.VITE_WORKER_UI || 'http://100.124.230.74:3001').trim().replace(/\/$/, '');

  const [lang, setLang] = useState(readInitialLang);
  const [path, setPath] = useState(() => normalizePath(window.location.pathname));

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
  const [vpnClientName, setVpnClientName] = useState('laptop');
  const [vpnConfig, setVpnConfig] = useState(null);
  const [bootstrapping, setBootstrapping] = useState(Boolean(localStorage.getItem(TOKEN_KEY)));

  const t = useMemo(() => createT(lang), [lang]);

  const setFlash = useCallback((message, tone = 'neutral') => {
    setStatus(message);
    setStatusTone(tone);
  }, []);

  const navigate = useCallback((nextPath) => {
    const target = normalizePath(nextPath);
    if (window.location.pathname !== target) {
      window.history.pushState({}, '', target);
    }
    setPath(target);
    window.scrollTo({ top: 0, behavior: 'smooth' });
  }, []);

  useEffect(() => {
    const onPop = () => setPath(normalizePath(window.location.pathname));
    window.addEventListener('popstate', onPop);
    return () => window.removeEventListener('popstate', onPop);
  }, []);

  useEffect(() => {
    document.title = lang === 'zh' ? 'Hivemind | 官方入口' : 'Hivemind | Official Gateway';
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
    setVpnConfig(null);
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
      setVpnConfig(null);
      await refreshBalance(nextToken);
      setFlash(authMode === 'register' ? t('account.sessionRestored') : t('account.sessionRestored'), 'ok');
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

  async function handleIssueVpnConfig(e) {
    e.preventDefault();
    if (!token) {
      setFlash(t('vpn.needAuth'), 'err');
      navigate('/account');
      return;
    }
    setLoading(true);
    setBusyAction('vpn');
    setFlash(t('vpn.issuing'));
    try {
      const clientName = vpnClientName.trim() || 'default';
      const res = await api('POST', '/api/vpn/config', { client_name: clientName }, token);
      if (!res.data.success) throw new Error(res.data.message || 'VPN issue failed');
      setVpnConfig(res.data);
      setFlash(t('vpn.ready'), 'ok');
    } catch (err) {
      if (err.code === 401) logout(err.message);
      else setFlash(err.message || 'VPN issue failed', 'err');
    } finally {
      setLoading(false);
      setBusyAction('');
    }
  }

  function downloadVpnConfig() {
    if (!vpnConfig?.config_text) return;
    const blob = new Blob([vpnConfig.config_text], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${(vpnConfig.client_id || 'hivemind-vpn').replace(/[^a-zA-Z0-9:_-]/g, '_')}.txt`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
  }

  const joinCommand = vpnConfig
    ? `tailscale up --login-server=${vpnConfig.login_server} --authkey=${vpnConfig.auth_key} --hostname=${vpnConfig.client_id}`
    : '';

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
    vpnClientName,
    setVpnClientName,
    vpnConfig,
    handleIssueVpnConfig,
    downloadVpnConfig,
    joinCommand,
  };

  let page = null;
  if (path === '/features') page = <FeaturesPage lang={lang} />;
  else if (path === '/account') {
    page = (
      <AccountPage
        lang={lang}
        t={t}
        navigate={navigate}
        apiBase={apiBase}
        masterUi={masterUi}
        workerUi={workerUi}
        session={session}
      />
    );
  } else if (path === '/vpn') page = <VpnPage lang={lang} t={t} navigate={navigate} session={session} />;
  else if (path === '/faq') page = <FaqPage lang={lang} />;
  else {
    page = (
      <HomePage
        lang={lang}
        t={t}
        navigate={navigate}
        masterUi={masterUi}
        workerUi={workerUi}
      />
    );
  }

  return (
    <Layout
      t={t}
      lang={lang}
      path={path === '/features' || path === '/account' || path === '/vpn' || path === '/faq' ? path : '/'}
      navigate={navigate}
      masterUi={masterUi}
      workerUi={workerUi}
      onToggleLang={() => setLang((prev) => (prev === 'zh' ? 'en' : 'zh'))}
      sessionUser={sessionUser}
    >
      {page}
    </Layout>
  );
}
