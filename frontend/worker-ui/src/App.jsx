import { useEffect, useRef, useState } from 'react';
import { LogOut, Moon, Sun, UserRound } from 'lucide-react';
import './console.css';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { WorkerComputer } from './components/worker-computer.jsx';
import { WorkerDashboard } from './components/worker-dashboard.jsx';
import { LoadingPanel } from './components/worker-setup-panels.jsx';
import { clearStoredSession, isExpiredJwt, readStoredSession, saveStoredSession } from './authSession.mjs';
import { readThemePreference, writeThemePreference } from './themePreference.mjs';
import {
  buildRegisterWorkerBody,
  buildRegisterWorkerRequest,
  emptyProfile,
  normalizeWorkerProfile,
  registrationOwnerUsername,
} from './workerProfile.mjs';
import {
  buildWorkerDashboardRequest,
  createNonOverlappingPoll,
  normalizeWorkerDashboard,
} from './workerDashboard.mjs';
import {
  createSetupAttemptController,
  createWorkerStartupStatusReader,
  fetchWithDeadline,
  getWorkerLoginError,
  getWorkerSetupErrorMessage,
  WorkerFetchTimeoutError,
  WorkerLoginError,
  isCurrentSessionAttempt,
  waitForWorkerStartup,
} from './workerStartup.mjs';

const IP_PATTERN = /^[\w.-]+:\d{1,5}$/;
const SESSION_KEY = 'hivemind.worker.session.v1';

function validateWorkerEndpoint(value) {
  const endpoint = String(value || '').trim();
  if (!endpoint) return null; // blank = session-only registration
  if (!IP_PATTERN.test(endpoint)) return 'Invalid format. Expected host:port (e.g. 127.0.0.1:50053)';
  return null;
}

export default function WorkerApp() {
  const workerControlBase = String(import.meta.env.VITE_WORKER_CONTROL_BASE || 'http://127.0.0.1:18080')
    .trim()
    .replace(/\/+$/, '');
  const [initialSession] = useState(() => readStoredSession(window.sessionStorage, SESSION_KEY));
  const [theme, setTheme] = useState(() => readThemePreference());

  useEffect(() => {
    document.documentElement.classList.toggle('dark', theme === 'dark');
    writeThemePreference(theme);
  }, [theme]);

  const [username, setUsername] = useState(initialSession.username);
  const [password, setPassword] = useState('');
  const [token, setToken] = useState(initialSession.token);
  const [authenticatedUsername, setAuthenticatedUsername] = useState(initialSession.username);
  const [status, setStatus] = useState(initialSession.token ? 'Session restored' : '');
  const [loginLoading, setLoginLoading] = useState(false);
  const [profileLoading, setProfileLoading] = useState(false);
  const [profileError, setProfileError] = useState(null);
  const [refreshLoading, setRefreshLoading] = useState(false);
  const [workerIp, setWorkerIp] = useState('');
  const [profile, setProfile] = useState(emptyProfile);
  const [profileLoaded, setProfileLoaded] = useState(false);
  const [registration, setRegistration] = useState(null);
  const [dashboard, setDashboard] = useState(null);
  const [dashboardLoading, setDashboardLoading] = useState(false);
  const [dashboardError, setDashboardError] = useState('');
  const [backendReady, setBackendReady] = useState(false);
  const [startupStatus, setStartupStatus] = useState({ state: 'initializing', phase: 'starting', code: null });
  const [startupError, setStartupError] = useState(null);
  const [startupRetryCount, setStartupRetryCount] = useState(0);
  const [setupActive, setSetupActive] = useState(false);
  const [setupPhase, setSetupPhase] = useState('starting');
  const [setupError, setSetupError] = useState('');
  const [setupErrorKind, setSetupErrorKind] = useState('');
  const sessionTokenRef = useRef(initialSession.token);
  const dashboardRefreshRef = useRef(null);
  const setupControllerRef = useRef(null);
  if (!setupControllerRef.current) setupControllerRef.current = createSetupAttemptController();

  async function readJson(res) {
    const text = await res.text();
    if (!text) return {};
    try {
      return JSON.parse(text);
    } catch {
      return {};
    }
  }

  async function fetchJsonWithDeadline(url, options) {
    return fetchWithDeadline(fetch, url, options, {
      consumeResponse: async (response) => ({
        ok: response.ok,
        status: response.status,
        data: await readJson(response),
      }),
    });
  }

  async function refreshLocalProfile({ signal, isCurrent = () => true } = {}) {
    if (!isCurrent()) return null;
    const ipError = validateWorkerEndpoint(workerIp);
    if (ipError) throw new Error(ipError);

    let res;
    try {
      res = await fetchJsonWithDeadline(`${workerControlBase}/api/worker-info`, { signal });
    } catch (error) {
      if (!isCurrent()) return null;
      if (error instanceof WorkerFetchTimeoutError) throw error;
      throw new Error('This computer app could not be reached. Make sure it is open and try again.');
    }
    if (!isCurrent()) return null;
    const data = res.data;
    if (!res.ok || !data.success || !data.profile) {
      throw new Error('This computer app could not be reached. Make sure it is open and try again.');
    }

    const normalized = normalizeWorkerProfile(data.profile, workerIp);
    if (!isCurrent()) return null;
    setProfile(normalized);
    setProfileLoaded(true);
    // Keep a detected callback address editable but never mandatory; a blank
    // field registers this worker as session-only.
    if (!workerIp.trim() && normalized.ip) setWorkerIp(normalized.ip);
    return normalized;
  }

  async function bootstrapVpn(authToken, { signal, isCurrent = () => true } = {}) {
    if (!authToken) throw new Error('Login is required before VPN bootstrap');
    if (!isCurrent() || sessionTokenRef.current !== authToken) return null;

    let res;
    try {
      res = await fetchJsonWithDeadline(`${workerControlBase}/api/vpn/bootstrap`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${authToken}` },
        signal,
      });
    } catch (error) {
      if (!isCurrent() || sessionTokenRef.current !== authToken) return null;
      if (error instanceof WorkerFetchTimeoutError) throw error;
      throw new Error('The network connection is not ready. Please try again.');
    }
    if (!isCurrent() || sessionTokenRef.current !== authToken) return null;
    const data = res.data;
    if (res.status === 401) {
      logout('Session expired. Please log in again.');
      throw new Error('Session expired. Please log in again.');
    }
    if (!res.ok || !data.success || !['ready', 'disabled'].includes(String(data.state || ''))) {
      throw new Error('The network connection is not ready. Please try again.');
    }
    setStatus(data.state === 'disabled' ? 'Connected in local mode' : 'Connected to Hivemind network');
    return data;
  }

  async function registerWorker(authToken, workerProfile, endpoint, { signal, isCurrent = () => true } = {}) {
    const ownerUsername = registrationOwnerUsername(authenticatedUsername, username);
    if (!authToken || !ownerUsername) throw new Error('Sign in before connecting this computer.');
    if (!isCurrent() || sessionTokenRef.current !== authToken) return null;
    // Avoid a request Nodepool must reject after this tab's JWT has expired.
    if (isExpiredJwt(authToken)) {
      logout('Session expired. Please log in again.');
      setRegistration({ success: false, message: 'Session expired. Please log in again.' });
      throw new Error('Session expired. Please log in again.');
    }
    setStatus('Preparing this computer...');

    try {
      const workerId = String(workerProfile.worker_id || '').trim() || ownerUsername;
      const request = buildRegisterWorkerRequest(
        workerControlBase,
        authToken,
        buildRegisterWorkerBody(ownerUsername, workerProfile, endpoint),
      );
      let res;
      try {
        res = await fetchJsonWithDeadline(request.url, { ...request.options, signal });
      } catch (error) {
        if (!isCurrent() || sessionTokenRef.current !== authToken) return null;
        if (error instanceof WorkerFetchTimeoutError) throw error;
        throw new Error('The network connection is not ready. Please try again.');
      }
      if (!isCurrent() || sessionTokenRef.current !== authToken) return null;
      const data = res.data;
      if (!res.ok) {
        if (res.status === 401) {
          logout('Session expired. Please log in again.');
          throw new Error('Session expired. Please log in again.');
        }
        throw new Error(data.message || data.status_message || `HTTP ${res.status}`);
      }
      if (!data.success) throw new Error('This computer could not be connected. Please try again.');

      setRegistration({
        success: true,
        message: 'This computer is connected and ready to receive tasks.',
        workerId,
      });
      setStatus('This computer is ready to receive tasks.');
      return data;
    } catch (err) {
      if (isCurrent() && sessionTokenRef.current === authToken) {
        setRegistration({ success: false, message: 'This computer could not be connected. Please try again.' });
        setStatus('This computer could not be connected. Please try again.');
      }
      throw err;
    }
  }
  useEffect(() => {
    if (!token || !backendReady) {
      if (!token) setDashboard(null);
      setDashboardError('');
      setDashboardLoading(false);
      dashboardRefreshRef.current = null;
      return undefined;
    }

    let cancelled = false;
    const refreshDashboard = createNonOverlappingPoll(async () => {
      if (cancelled || sessionTokenRef.current !== token) return;
      setDashboardLoading(true);
      try {
        const request = buildWorkerDashboardRequest(workerControlBase, token);
        let res;
        try {
          res = await fetchJsonWithDeadline(request.url, request.options);
        } catch {
          throw new Error('The worker dashboard could not be reached.');
        }
        if (cancelled || sessionTokenRef.current !== token) return;
        const data = res.data;
        if (cancelled || sessionTokenRef.current !== token) return;
        if (res.status === 401) {
          if (!cancelled && sessionTokenRef.current === token) {
            logout();
            setStatus('Session expired. Please log in again.');
          }
          return;
        }
        if (!res.ok || !data.success) {
          throw new Error(data.message || data.status_message || `Dashboard request failed (${res.status})`);
        }
        const nextDashboard = normalizeWorkerDashboard(data);
        if (!cancelled && sessionTokenRef.current === token) {
          setDashboard(nextDashboard);
          setDashboardError('');
        }
      } catch (err) {
        if (!cancelled && sessionTokenRef.current === token) {
          setDashboardError(err.message || 'The worker dashboard could not be refreshed.');
        }
      } finally {
        if (!cancelled && sessionTokenRef.current === token) setDashboardLoading(false);
      }
    });
    dashboardRefreshRef.current = refreshDashboard;
    void refreshDashboard();
    const id = setInterval(() => { void refreshDashboard(); }, 15000);
    return () => {
      cancelled = true;
      clearInterval(id);
      if (dashboardRefreshRef.current === refreshDashboard) dashboardRefreshRef.current = null;
    };
  }, [token, workerControlBase, backendReady]);

  async function runSetup(options = {}) {
    const {
      kind = 'profile',
      authToken: suppliedToken = '',
      ownerUsername = '',
      credentials = null,
    } = options;
    const startingToken = suppliedToken || (kind === 'profile' || kind === 'login' ? '' : sessionTokenRef.current);
    let attemptToken = startingToken;

    setSetupActive(true);
    setSetupError('');
    setSetupErrorKind('');
    setProfileError(null);
    setProfileLoading(kind !== 'login');
    setLoginLoading(kind === 'login');
    setSetupPhase(kind === 'login' || (kind === 'restore' && attemptToken) ? 'connecting' : 'resources');
    if (kind === 'login') setStatus('Signing in and connecting...');
    else if (kind === 'restore' && attemptToken) setStatus('Signing in and connecting...');
    else setStatus('Checking this computer...');

    return setupControllerRef.current.run(async ({ signal, isCurrent: isAttemptCurrent }) => {
      const isCurrent = () => isCurrentSessionAttempt(
        isAttemptCurrent,
        () => sessionTokenRef.current,
        attemptToken,
      );
      const requireCurrent = () => {
        if (!isCurrent()) {
          const error = new Error('This setup attempt is no longer active.');
          error.name = 'AbortError';
          throw error;
        }
      };

      try {
        let authToken = attemptToken;
        let accountName = ownerUsername || authenticatedUsername || username;

        if (kind === 'login') {
          requireCurrent();
          let res;
          try {
            res = await fetchJsonWithDeadline(`${workerControlBase}/api/login`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ username: credentials?.username || '', password: credentials?.password || '' }),
              signal,
            });
          } catch (error) {
            requireCurrent();
            if (error instanceof WorkerFetchTimeoutError || error?.name === 'AbortError') throw error;
            throw new WorkerLoginError('network');
          }
          requireCurrent();
          const loginError = getWorkerLoginError(res);
          if (loginError) throw loginError;
          const data = res.data;

          authToken = data.token.trim();
          accountName = String(credentials?.username || '').trim();
          if (!accountName) throw new WorkerLoginError('invalid-response');
          attemptToken = authToken;
          sessionTokenRef.current = authToken;
          saveStoredSession(window.sessionStorage, SESSION_KEY, { token: authToken, username: accountName });
          setToken(authToken);
          setUsername(accountName);
          setPassword('');
          setAuthenticatedUsername(accountName);
          setRegistration(null);
          requireCurrent();
        }

        if (kind === 'restore' || kind === 'login') {
          if (!authToken) throw new Error('Sign in before connecting this computer.');
          setSetupPhase('connecting');
          setStatus('Signing in and connecting...');
          await bootstrapVpn(authToken, { signal, isCurrent });
          requireCurrent();
        }

        setSetupPhase('resources');
        setProfileLoading(true);
        setStatus('Checking this computer...');
        const localProfile = await refreshLocalProfile({ signal, isCurrent });
        requireCurrent();
        if (!localProfile) throw new Error('This computer profile could not be loaded. Please try again.');

        if (authToken && kind !== 'refresh-profile') {
          setSetupPhase('services');
          setStatus('Preparing this computer...');
          await registerWorker(authToken, localProfile, localProfile.ip, { signal, isCurrent });
          requireCurrent();
        }

        setSetupError('');
        setSetupErrorKind('');
        setStatus(authToken ? 'This computer is ready to receive tasks.' : 'This computer is ready to connect.');
        return true;
      } catch (err) {
        if (!isCurrent()) return false;
        const loginEstablished = kind === 'login'
          && Boolean(attemptToken)
          && sessionTokenRef.current === attemptToken;
        const userMessage = getWorkerSetupErrorMessage(err, { kind, loginEstablished });
        setSetupError(userMessage);
        setSetupErrorKind(kind);
        if (kind === 'profile' || kind === 'refresh-profile' || kind === 'restore') {
          setProfileError(userMessage);
        }
        setStatus(userMessage);
        return false;
      } finally {
        if (isCurrent()) {
          setSetupActive(false);
          setProfileLoading(false);
          setLoginLoading(false);
        }
      }
    });
  }

  function retryCurrentSetup() {
    if (token) {
      return runSetup({ kind: 'restore', authToken: token, ownerUsername: authenticatedUsername || username });
    }
    return runSetup({ kind: 'profile' });
  }

  async function handleLogin(event) {
    event.preventDefault();
    if (!backendReady || setupActive) return;
    setDashboard(null);
    setDashboardError('');
    setRegistration(null);
    void runSetup({ kind: 'login', credentials: { username, password } });
  }

  function logout(message = 'Signed out') {
    setupControllerRef.current.invalidate();
    clearStoredSession(window.sessionStorage, SESSION_KEY);
    sessionTokenRef.current = '';
    setToken('');
    setAuthenticatedUsername('');
    setDashboard(null);
    setDashboardError('');
    setDashboardLoading(false);
    setRegistration(null);
    setSetupActive(false);
    setSetupError('');
    setSetupErrorKind('');
    setProfileLoading(false);
    setLoginLoading(false);
    setStatus(message);
  }

  async function handleRefresh() {
    if (setupActive || refreshLoading) return;
    setRefreshLoading(true);
    try {
      await runSetup({ kind: 'refresh-profile' });
    } finally {
      setRefreshLoading(false);
    }
  }

  useEffect(() => {
    const controller = new AbortController();
    let active = true;
    setStartupStatus({ state: 'initializing', phase: 'starting', code: null });
    setStartupError(null);
    setBackendReady(false);

    const readStatus = createWorkerStartupStatusReader({
      baseUrl: workerControlBase,
      fetchImpl: fetch,
    });
    void waitForWorkerStartup({
      readStatus,
      signal: controller.signal,
      onStatus: (nextStatus) => {
        if (active) setStartupStatus(nextStatus);
      },
    }).then((readyStatus) => {
      if (!active) return;
      setStartupStatus(readyStatus);
      setBackendReady(true);
      void runSetup(
        initialSession.token && initialSession.username
          ? { kind: 'restore', authToken: initialSession.token, ownerUsername: initialSession.username }
          : { kind: 'profile' },
      );
    }).catch((error) => {
      if (!active || error.kind === 'aborted') return;
      setBackendReady(false);
      setStartupError(error);
    });

    return () => {
      active = false;
      controller.abort();
      setupControllerRef.current.invalidate();
    };
  }, [workerControlBase, startupRetryCount]);

  function retryStartupCheck() {
    setStartupError(null);
    setStartupStatus({ state: 'initializing', phase: 'starting', code: null });
    setBackendReady(false);
    setStartupRetryCount((count) => count + 1);
  }

  const themeToggle = (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      className="theme-toggle"
      onClick={() => setTheme((current) => current === 'dark' ? 'light' : 'dark')}
      aria-label={theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'}
      title={theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'}
    >
      {theme === 'dark' ? <Sun aria-hidden="true" /> : <Moon aria-hidden="true" />}
    </Button>
  );
  const statusIsError = Boolean(setupError && setupErrorKind === 'login')
    || ['failed', 'cannot', 'could not', 'expired', 'unavailable', 'error'].some((part) => status.toLowerCase().includes(part));

  if (!backendReady) {
    const startupFailureMessage = startupError?.kind === 'failed'
      ? `Worker startup failed${startupError.code ? ` (${startupError.code})` : ''}. Restart the Worker app, then check again.`
      : startupError?.kind === 'timeout'
        ? 'Worker startup is taking longer than expected. Make sure the Worker app is running, then retry.'
        : startupError?.message || '';
    return (
      <main className="app-shell">
        <div className="app-container">
          <header className="app-header">
            <div className="brand-lockup">
              <div className="brand-mark" aria-hidden="true"><span /></div>
              <div className="brand-copy">
                <p className="eyebrow">Hivemind · Worker</p>
                <h1>Worker console</h1>
              </div>
            </div>
            <div className="header-actions">{themeToggle}</div>
          </header>
          <LoadingPanel
            phase={startupStatus.phase}
            error={startupFailureMessage}
            onRetry={startupError ? retryStartupCheck : undefined}
            retryLabel={startupError?.kind === 'failed' ? 'Check again' : 'Retry startup'}
            showSkeleton={!startupError}
          />
        </div>
      </main>
    );
  }

  return (
    <main className="app-shell">
      <div className="app-container">
        <header className="app-header">
          <div className="brand-lockup">
            <div className="brand-mark" aria-hidden="true"><span /></div>
            <div className="brand-copy">
              <p className="eyebrow">Hivemind · Worker</p>
              <h1>Worker console</h1>
            </div>
          </div>
          <div className="header-actions">
            {token ? (
              <Button type="button" variant="outline" onClick={() => logout()}>
                <LogOut aria-hidden="true" />
                Sign out
              </Button>
            ) : null}
            {themeToggle}
          </div>
        </header>

        {setupActive ? <LoadingPanel phase={setupPhase} compact /> : null}
        {setupError && token && (setupErrorKind === 'restore' || setupErrorKind === 'login') ? (
          <div className="setup-alert" role="alert">
            <div>
              <strong>Session needs attention</strong>
              <p>{setupError}</p>
            </div>
            <Button type="button" variant="outline" onClick={retryCurrentSetup} disabled={setupActive}>
              {setupActive ? 'Connecting…' : 'Retry connection'}
            </Button>
          </div>
        ) : null}

        <Card className="account-card">
          <CardContent className="account-card-content">
            {token ? (
              <div className="signed-in-account">
                <div className="account-avatar" aria-hidden="true"><UserRound /></div>
                <div className="signed-in-copy">
                  <div className="signed-in-heading">
                    <Badge variant="secondary">Signed in</Badge>
                    <strong>{authenticatedUsername || username}</strong>
                  </div>
                  <p>This computer reconnects automatically after sign-in.</p>
                </div>
              </div>
            ) : (
              <form onSubmit={handleLogin} className="login-form">
                <div className="login-heading">
                  <p className="section-kicker">Account</p>
                  <h2>Sign in to connect</h2>
                </div>
                <div className="login-fields">
                  <div className="login-field">
                    <Label htmlFor="worker-username">Username</Label>
                    <Input
                      id="worker-username"
                      name="username"
                      autoComplete="username"
                      value={username}
                      onChange={(event) => setUsername(event.target.value)}
                      disabled={loginLoading || setupActive}
                    />
                  </div>
                  <div className="login-field">
                    <Label htmlFor="worker-password">Password</Label>
                    <Input
                      id="worker-password"
                      name="password"
                      type="password"
                      autoComplete="current-password"
                      value={password}
                      onChange={(event) => setPassword(event.target.value)}
                      disabled={loginLoading || setupActive}
                    />
                  </div>
                  <Button type="submit" className="login-submit" disabled={loginLoading || setupActive}>
                    {loginLoading || setupActive ? 'Connecting…' : 'Sign in and connect'}
                  </Button>
                </div>
              </form>
            )}
            {status ? (
              <div className={`account-status${statusIsError ? ' is-error' : ''}`} role={statusIsError ? 'alert' : 'status'}>
                {status}
              </div>
            ) : null}
          </CardContent>
        </Card>

        {token ? (
          <Tabs defaultValue="overview" className="worker-tabs">
            <TabsList className="worker-tabs-list" aria-label="Worker console sections">
              <TabsTrigger value="overview">Overview</TabsTrigger>
              <TabsTrigger value="computer">Computer</TabsTrigger>
            </TabsList>
            <TabsContent value="overview" className="worker-tab-content">
              <WorkerDashboard
                dashboard={dashboard}
                dashboardError={dashboardError}
                dashboardLoading={dashboardLoading}
                onRefresh={() => { void dashboardRefreshRef.current?.(); }}
              />
            </TabsContent>
            <TabsContent value="computer" className="worker-tab-content">
              <WorkerComputer
                profile={profile}
                profileLoaded={profileLoaded}
                profileLoading={profileLoading}
                profileError={profileError}
                registration={registration}
                refreshLoading={refreshLoading}
                setupActive={setupActive}
                onRefresh={handleRefresh}
              />
            </TabsContent>
          </Tabs>
        ) : null}
      </div>
    </main>
  );
}
