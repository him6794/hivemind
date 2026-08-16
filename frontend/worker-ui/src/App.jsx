import { useEffect, useState } from 'react';
import './console.css';
import { clearStoredSession, readStoredSession, saveStoredSession } from './authSession.mjs';
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
  if (!value || !value.trim()) return 'Worker endpoint is required';
  if (!IP_PATTERN.test(value.trim())) return 'Invalid format. Expected host:port (e.g. 127.0.0.1:50053)';
  return null;
}

export default function WorkerApp() {
  const apiBase = String(import.meta.env.VITE_API_BASE || '').trim().replace(/\/$/, '');
  const workerControlBase = String(import.meta.env.VITE_WORKER_CONTROL_BASE || '')
    .trim()
    .replace(/\/$/, '');
  const initialSession = readStoredSession(window.localStorage, SESSION_KEY);

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
  const [workerIp, setWorkerIp] = useState(`${window.location.hostname || '127.0.0.1'}:50053`);
  const [workerIpError, setWorkerIpError] = useState(null);
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
      setWorkerIpError(ipError);
      throw new Error(ipError);
    }
    setWorkerIpError(null);

    let res;
    try {
      res = await fetch(`${workerControlBase}/api/worker-info`);
    } catch {
      throw new Error(
        `Worker agent not responding at ${workerControlBase}. Verify the worker is running and VITE_WORKER_CONTROL_BASE is correct.`
      );
    }
    const data = await readJson(res);
    if (!res.ok || !data.success || !data.profile) {
      throw new Error(
        `Worker agent not responding at ${workerControlBase}. Verify the worker is running and VITE_WORKER_CONTROL_BASE is correct.`
      );
    }

    const normalized = normalizeWorkerProfile(data.profile, workerIp);
    setProfile(normalized);
    setWorkerIp(normalized.ip);
    return normalized;
  }

  async function registerWorker(authToken = token, workerProfile = profile, endpoint = workerIp) {
    const ownerUsername = registrationOwnerUsername(authenticatedUsername, username);
    if (!authToken || !ownerUsername) return;
    setRegisterLoading(true);
    setStatus('Registering worker with nodepool...');

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
        throw new Error(`Cannot reach Worker Control at ${workerControlBase}.`);
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
        throw new Error(data.status_message || 'Worker registration failed');
      }

      setRegistration({
        success: true,
        message: data.status_message || 'Registered',
        workerId,
      });
      setStatus(`Worker registered: ${workerId}`);
    } catch (err) {
      setRegistration({ success: false, message: err.message });
      setStatus(`Registration failed: ${err.message}`);
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
        res = await fetch(`${apiBase}/api/login`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ username, password }),
        });
      } catch {
        throw new Error(`Cannot reach Hivemind API at ${apiBase}. Check VITE_API_BASE.`);
      }
      const data = await readJson(res);
      if (!res.ok || !data.success) {
        throw new Error(data.message || data.status_message || 'Login failed');
      }

      const authToken = data.token || '';
      const ownerUsername = username.trim();
      saveStoredSession(window.localStorage, SESSION_KEY, {
        token: authToken,
        username: ownerUsername,
      });
      setToken(authToken);
      setUsername(ownerUsername);
      setAuthenticatedUsername(ownerUsername);
      setStatus('Logged in. Fetching local worker info...');
      const localProfile = await refreshLocalProfile();
      await registerWorker(authToken, localProfile, localProfile.ip);
    } catch (err) {
      setStatus(`Login failed: ${err.message}`);
    } finally {
      setLoginLoading(false);
    }
  }

  function logout() {
    clearStoredSession(window.localStorage, SESSION_KEY);
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
      setStatus(`Refresh failed: ${err.message}`);
    } finally {
      setRefreshLoading(false);
    }
  }

  async function handleRegisterAgain() {
    const ipError = validateWorkerEndpoint(workerIp);
    if (ipError) {
      setWorkerIpError(ipError);
      setStatus(`Validation error: ${ipError}`);
      return;
    }
    setWorkerIpError(null);
    await registerWorker();
  }

  function handleWorkerIpChange(value) {
    setWorkerIp(value);
    const error = validateWorkerEndpoint(value);
    setWorkerIpError(error);
  }

  useEffect(() => {
    setProfileLoading(true);
    setProfileError(null);
    refreshLocalProfile()
      .then((localProfile) => {
        if (initialSession.token && initialSession.username) {
          setStatus('Session restored. Registering local worker...');
          return registerWorker(initialSession.token, localProfile, localProfile.ip);
        }
        setStatus('Local worker profile loaded');
        return undefined;
      })
      .catch((err) => {
        setProfileError(err.message);
        setStatus(`Cannot reach local worker agent: ${err.message}`);
      })
      .finally(() => setProfileLoading(false));
  }, []);

  return (
    <main className="app-shell">
      <div className="app-container">
        <header className="app-header">
          <div className="brand-lockup">
            <div className="brand-mark" aria-hidden="true" />
            <div>
              <p className="eyebrow">Hivemind Console</p>
              <h1>Worker UI</h1>
              <p className="lead">
                Register this local worker with the nodepool, publish hardware capacity, and keep the provider session active across page reloads.
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
                <p className="eyebrow">Authenticated</p>
                <strong>{authenticatedUsername || username}</strong>
              </div>
              <button
                type="button"
                onClick={handleRegisterAgain}
                disabled={registerLoading}
                className="button primary"
              >
                {registerLoading ? 'Registering...' : 'Register worker'}
              </button>
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
                {loginLoading ? 'Working...' : 'Login and register'}
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
            <h2>Local Capacity</h2>
            <label>
              Worker endpoint
              <input
                value={workerIp}
                onChange={(e) => handleWorkerIpChange(e.target.value)}
                className={`field ${workerIpError ? 'error' : ''}`}
              />
            </label>
            {workerIpError ? <div className="status error">{workerIpError}</div> : null}

            {profileLoading ? (
              <div className="status">Fetching local worker info...</div>
            ) : profileError ? (
              <div className="status error">
                <strong>Cannot reach local worker agent</strong>
                <p className="subtle" style={{ marginTop: 6 }}>{profileError}</p>
                <button type="button" onClick={handleRefresh} disabled={refreshLoading} className="button">
                  {refreshLoading ? 'Retrying...' : 'Retry'}
                </button>
              </div>
            ) : (
              <>
                <dl>
                  <dt>Worker ID</dt>
                  <dd>{profile.worker_id || '(unregistered)'}</dd>
                  <dt>CPU cores</dt>
                  <dd>{profile.cpu_cores}</dd>
                  <dt>Memory</dt>
                  <dd>{profile.memory_gb} GB</dd>
                  <dt>CPU score</dt>
                  <dd>{profile.cpu_score}</dd>
                  <dt>GPU score</dt>
                  <dd>{profile.gpu_score}</dd>
                  <dt>GPU memory</dt>
                  <dd>{profile.gpu_memory_gb} GB</dd>
                  <dt>GPU name</dt>
                  <dd>{profile.gpu_name || '-'}</dd>
                  <dt>Storage</dt>
                  <dd>{profile.storage_available_gb} / {profile.storage_total_gb} GB</dd>
                  <dt>Location</dt>
                  <dd>{profile.location || 'local'}</dd>
                </dl>
                <div className="actions">
                  <button type="button" onClick={handleRefresh} disabled={refreshLoading} className="button">
                    {refreshLoading ? 'Refreshing...' : 'Refresh profile'}
                  </button>
                  {token ? (
                    <button type="button" onClick={handleRegisterAgain} disabled={registerLoading} className="button primary">
                      {registerLoading ? 'Registering...' : 'Register again'}
                    </button>
                  ) : null}
                </div>
              </>
            )}
          </section>

          <section className="surface">
            <h2>Registration Status</h2>
            {registration ? (
              <div className={`status ${registration.success ? 'success' : 'error'}`}>
                <strong>{registration.success ? 'Registered' : 'Not registered'}</strong>
                <div style={{ marginTop: 6 }}>{registration.message}</div>
                {registration.workerId ? (
                  <div className="subtle" style={{ marginTop: 6 }}>worker_id: {registration.workerId}</div>
                ) : null}
              </div>
            ) : (
              <p className="subtle">
                Log in to register your worker node with the master nodepool. The nodepool can then assign tasks to this machine.
              </p>
            )}
          </section>
        </div>
      </div>
    </main>
  );
}
