import { useEffect, useState } from 'react';
import './console.css';
import { clearStoredSession, isExpiredJwt, readStoredSession, saveStoredSession } from './authSession.mjs';
import {
  buildRegisterWorkerBody,
  buildRegisterWorkerRequest,
  emptyProfile,
  normalizeWorkerProfile,
  registrationOwnerUsername,
} from './workerProfile.mjs';

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
    .replace(/\/$/, '');
  const initialSession = readStoredSession(window.sessionStorage, SESSION_KEY);

  const [username, setUsername] = useState(initialSession.username);
  const [password, setPassword] = useState('');
  const [token, setToken] = useState(initialSession.token);
  const [authenticatedUsername, setAuthenticatedUsername] = useState(initialSession.username);
  const [status, setStatus] = useState(initialSession.token ? 'Session restored' : '');
  const [loginLoading, setLoginLoading] = useState(false);
  const [profileLoading, setProfileLoading] = useState(false);
  const [profileError, setProfileError] = useState(null);
  const [registerLoading, setRegisterLoading] = useState(false);
  const [refreshLoading, setRefreshLoading] = useState(false);
  const [workerIp, setWorkerIp] = useState('');
  const [profile, setProfile] = useState(emptyProfile);
  const [registration, setRegistration] = useState(null);

  async function readJson(res) {
    const text = await res.text();
    if (!text) return {};
    try {
      return JSON.parse(text);
    } catch {
      return {};
    }
  }

  async function refreshLocalProfile() {
    const ipError = validateWorkerEndpoint(workerIp);
    if (ipError) {
      throw new Error(ipError);
    }

    let res;
    try {
      res = await fetch(`${workerControlBase}/api/worker-info`);
    } catch {
      throw new Error('This computer app could not be reached. Make sure it is open and try again.');
    }
    const data = await readJson(res);
    if (!res.ok || !data.success || !data.profile) {
      throw new Error('This computer app could not be reached. Make sure it is open and try again.');
    }

    const normalized = normalizeWorkerProfile(data.profile, workerIp);
    setProfile(normalized);
    // Keep a detected callback address editable but never mandatory; a blank
    // field registers this worker as session-only.
    if (!workerIp.trim() && normalized.ip) {
      setWorkerIp(normalized.ip);
    }
    return normalized;
  }

  async function bootstrapVpn(authToken = token) {
    if (!authToken) throw new Error('Login is required before VPN bootstrap');
    let res;
    try {
      res = await fetch(`${workerControlBase}/api/vpn/bootstrap`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${authToken}` },
      });
    } catch {
      throw new Error('The network connection is not ready. Please try again.');
    }
    const data = await readJson(res);
    if (res.status === 401) {
      logout();
      throw new Error('Session expired. Please log in again.');
    }
    if (!res.ok || !data.success || !['ready', 'disabled'].includes(String(data.state || ''))) {
      throw new Error('The network connection is not ready. Please try again.');
    }
    setStatus(data.state === 'disabled' ? 'Connected in local mode' : 'Connected to Hivemind network');
    return data;
  }

  async function registerWorker(authToken = token, workerProfile = profile, endpoint = workerIp) {
    const ownerUsername = registrationOwnerUsername(authenticatedUsername, username);
    if (!authToken || !ownerUsername) return;
    // The token may expire while this tab sits open; fail toward login
    // instead of sending a request Nodepool must reject.
    if (isExpiredJwt(authToken)) {
      logout();
      setStatus('Session expired. Please log in again.');
      setRegistration({ success: false, message: 'Session expired. Please log in again.' });
      return;
    }
    setRegisterLoading(true);
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
        res = await fetch(request.url, request.options);
      } catch {
        throw new Error('The network connection is not ready. Please try again.');
      }
      const data = await readJson(res);
      if (!res.ok) {
        if (res.status === 401) {
          logout();
          throw new Error('Session expired. Please log in again.');
        }
        throw new Error(data.message || data.status_message || `HTTP ${res.status}`);
      }

      if (!data.success) {
        throw new Error('This computer could not be connected. Please try again.');
      }

      setRegistration({
        success: true,
        message: 'This computer is connected and ready to receive tasks.',
        workerId,
      });
      setStatus('This computer is ready to receive tasks.');
    } catch (err) {
      console.error('Computer connection failed:', err);
      setRegistration({ success: false, message: 'This computer could not be connected. Please try again.' });
      setStatus('This computer could not be connected. Please try again.');
    } finally {
      setRegisterLoading(false);
    }
  }
  async function handleLogin(e) {
    e.preventDefault();
    setLoginLoading(true);
    setStatus('Logging in...');
    setToken('');
    setAuthenticatedUsername('');
    setRegistration(null);

    try {
      let res;
      try {
        res = await fetch(`${workerControlBase}/api/login`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ username, password }),
        });
      } catch {
        throw new Error('This computer app could not be reached. Make sure it is open and try again.');
      }
      const data = await readJson(res);
      if (!res.ok || !data.success) {
        throw new Error(data.message || data.status_message || 'Login failed');
      }

      const authToken = data.token || '';
      const ownerUsername = username.trim();
      // The bearer JWT lives only in tab session storage: closing the Worker
      // console discards it, so no reusable credential persists in the
      // browser profile.
      saveStoredSession(window.sessionStorage, SESSION_KEY, {
        token: authToken,
        username: ownerUsername,
      });
      setToken(authToken);
      setUsername(ownerUsername);
      setAuthenticatedUsername(ownerUsername);
      await bootstrapVpn(authToken);
      setStatus('Connected. Checking this computer...');
      const localProfile = await refreshLocalProfile();
      await registerWorker(authToken, localProfile, localProfile.ip);
    } catch (err) {
      setStatus(`Login failed: ${err.message}`);
    } finally {
      setLoginLoading(false);
    }
  }

  function logout() {
    clearStoredSession(window.sessionStorage, SESSION_KEY);
    setToken('');
    setAuthenticatedUsername('');
    setRegistration(null);
    setStatus('Signed out');
  }

  async function handleRefresh() {
    setRefreshLoading(true);
    setProfileError(null);
    try {
      await refreshLocalProfile();
      setStatus('Profile refreshed');
    } catch (err) {
      setProfileError(err.message);
      setStatus('Refresh failed. Please try again.');
    } finally {
      setRefreshLoading(false);
    }
  }

  useEffect(() => {
    setProfileLoading(true);
    setProfileError(null);
    (async () => {
      try {
        if (initialSession.token && initialSession.username) {
          setStatus('Session restored. Connecting to the network...');
          await bootstrapVpn(initialSession.token);
          const localProfile = await refreshLocalProfile();
          setStatus('Connected. Preparing this computer...');
          await registerWorker(initialSession.token, localProfile, localProfile.ip);
        } else {
          await refreshLocalProfile();
          setStatus('This computer is ready to connect.');
        }
      } catch (err) {
        setProfileError(err.message);
        setStatus('We could not prepare this computer. Please try again.');
      } finally {
        setProfileLoading(false);
      }
    })();
  }, []);

  return (
    <main className="app-shell">
      <div className="app-container">
        <header className="app-header">
          <div className="brand-lockup">
            <div className="brand-mark" aria-hidden="true" />
            <div>
              <p className="eyebrow">Hivemind</p>
              <h1>Share this computer</h1>
              <p className="lead">
                Sign in to put this machine to work. It registers with the network, shares what it can do, and is ready to accept jobs.
              </p>
            </div>
          </div>
          {token ? (
            <button type="button" onClick={logout} className="button ghost">
              Sign out
            </button>
          ) : null}
        </header>

        <section className="surface">
          {token ? (
            <div className="toolbar">
              <div>
                <p className="eyebrow">Signed in</p>
                <strong>{authenticatedUsername || username}</strong>
              </div>
              <span className="subtle">This computer connects automatically after sign-in.</span>
            </div>
          ) : (
            <form onSubmit={handleLogin} className="form-grid">
              <label>
                Username
                <input value={username} onChange={(e) => setUsername(e.target.value)} className="field" />
              </label>
              <label>
                Password
                <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} className="field" />
              </label>
              <button type="submit" disabled={loginLoading} className="button primary">
                {loginLoading ? 'Connecting...' : 'Sign in and connect'}
              </button>
            </form>
          )}
          {status ? (
            <div className={`status ${status.toLowerCase().includes('failed') || status.toLowerCase().includes('cannot') ? 'error' : ''}`}>
              {status}
            </div>
          ) : null}
        </section>

        <div className="grid two" style={{ marginTop: 18 }}>
          <section className="surface">
            <h2>This computer</h2>
            <p className="subtle">Hivemind detects the computer settings it needs. There is nothing else to configure.</p>

            {profileLoading ? (
              <div className="status">Checking this computer...</div>
            ) : profileError ? (
              <div className="status error">
                <strong>We could not check this computer</strong>
                <p className="subtle" style={{ marginTop: 6 }}>Make sure the app is open, then try again.</p>
                <button type="button" onClick={handleRefresh} disabled={refreshLoading} className="button">
                  {refreshLoading ? 'Trying again...' : 'Try again'}
                </button>
              </div>
            ) : (
              <>
                <dl>
                  <dt>Computer ID</dt>
                  <dd>{profile.worker_id || '(not connected yet)'}</dd>
                  <dt>CPU cores</dt>
                  <dd>{profile.cpu_cores}</dd>
                  <dt>Memory</dt>
                  <dd>{profile.memory_gb} GB</dd>
                  <dt>CPU rating</dt>
                  <dd>{profile.cpu_score}</dd>
                  <dt>Graphics rating</dt>
                  <dd>{profile.gpu_score}</dd>
                  <dt>Graphics memory</dt>
                  <dd>{profile.gpu_memory_gb} GB</dd>
                  <dt>Graphics card</dt>
                  <dd>{profile.gpu_name || '-'}</dd>
                  <dt>Storage</dt>
                  <dd>{profile.storage_available_gb} / {profile.storage_total_gb} GB</dd>
                  <dt>Location</dt>
                  <dd>{profile.location || 'local'}</dd>
                </dl>
                <div className="actions">
                  <button type="button" onClick={handleRefresh} disabled={refreshLoading} className="button">
                    {refreshLoading ? 'Checking...' : 'Check again'}
                  </button>
                </div>
              </>
            )}
          </section>

          <section className="surface">
            <h2>Connection</h2>
            {registration ? (
              <div className={`status ${registration.success ? 'success' : 'error'}`}>
                <strong>{registration.success ? 'Ready' : 'Not connected'}</strong>
                <div style={{ marginTop: 6 }}>{registration.message}</div>
                {registration.workerId ? (
                  <div className="subtle" style={{ marginTop: 6 }}>Computer ID: {registration.workerId}</div>
                ) : null}
              </div>
            ) : (
              <p className="subtle">
                Sign in to connect this computer. Once it is ready, the network can send it tasks and check the work before charging.
              </p>
            )}
          </section>
        </div>
      </div>
    </main>
  );
}
