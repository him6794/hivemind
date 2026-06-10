import React, { useEffect, useState } from 'react';
import { buildRegisterWorkerBody, emptyProfile, normalizeWorkerProfile, registrationOwnerUsername } from './workerProfile.mjs';

const panelStyle = {
  border: '1px solid #d8e0e8',
  borderRadius: 14,
  background: '#fff',
  padding: 18,
  boxShadow: '0 12px 32px rgba(15, 23, 42, 0.06)',
};

const fieldStyle = {
  width: '100%',
  boxSizing: 'border-box',
  padding: '10px 12px',
  marginTop: 6,
  border: '1px solid #cad5df',
  borderRadius: 10,
  background: '#fff',
};

const buttonStyle = {
  padding: '10px 14px',
  border: 'none',
  borderRadius: 10,
  cursor: 'pointer',
  fontWeight: 700,
};

export default function WorkerApp() {
  const apiBase = String(import.meta.env.VITE_API_BASE || 'http://localhost:8082').trim().replace(/\/$/, '');
  const workerControlBase = String(import.meta.env.VITE_WORKER_CONTROL_BASE || 'http://localhost:18080')
    .trim()
    .replace(/\/$/, '');

  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [token, setToken] = useState('');
  const [authenticatedUsername, setAuthenticatedUsername] = useState('');
  const [status, setStatus] = useState('請先登入後再註冊本機節點');
  const [loading, setLoading] = useState(false);
  const [workerIp, setWorkerIp] = useState(`${window.location.hostname || '127.0.0.1'}:50053`);
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

  async function authedFetch(path, options = {}, authToken = token) {
    const res = await fetch(`${apiBase}${path}`, {
      ...options,
      headers: {
        ...(options.headers || {}),
        Authorization: `Bearer ${authToken}`,
      },
    });
    const data = await readJson(res);
    if (!res.ok) {
      throw new Error(data.message || data.status_message || `HTTP ${res.status}`);
    }
    return data;
  }

  async function refreshLocalProfile() {
    const res = await fetch(`${workerControlBase}/api/worker-info`);
    const data = await readJson(res);
    if (!res.ok || !data.success || !data.profile) {
      throw new Error(data.status_message || data.message || 'Local worker agent unavailable');
    }

    const normalized = normalizeWorkerProfile(data.profile, workerIp);
    setProfile(normalized);
    setWorkerIp(normalized.ip);
    return normalized;
  }

  async function registerWorker(authToken = token, workerProfile = profile, endpoint = workerIp) {
    const ownerUsername = registrationOwnerUsername(authenticatedUsername, username);
    if (!authToken || !ownerUsername) return;
    setLoading(true);
    setStatus('正在註冊本機節點到 nodepool...');

    try {
      const workerId = String(workerProfile.worker_id || '').trim() || ownerUsername;
      const data = await authedFetch('/api/register-worker', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(buildRegisterWorkerBody(ownerUsername, workerProfile, endpoint)),
      }, authToken);

      if (!data.success) {
        throw new Error(data.status_message || 'Worker registration failed');
      }

      setRegistration({
        success: true,
        message: data.status_message || 'Registered',
        workerId,
      });
      setStatus(`節點已註冊: ${workerId}`);
    } catch (err) {
      setRegistration({ success: false, message: err.message });
      setStatus(`註冊失敗: ${err.message}`);
    } finally {
      setLoading(false);
    }
  }

  async function handleLogin(e) {
    e.preventDefault();
    setLoading(true);
    setStatus('登入中...');
    setToken('');
    setAuthenticatedUsername('');
    setRegistration(null);

    try {
      const res = await fetch(`${apiBase}/api/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, password }),
      });
      const data = await readJson(res);
      if (!res.ok || !data.success) {
        throw new Error(data.message || data.status_message || 'Login failed');
      }

      const authToken = data.token || '';
      const ownerUsername = username.trim();
      setToken(authToken);
      setAuthenticatedUsername(ownerUsername);
      setStatus('登入成功，準備註冊節點...');
      const localProfile = await refreshLocalProfile();
      await registerWorker(authToken, localProfile, localProfile.ip);
    } catch (err) {
      setStatus(`登入失敗: ${err.message}`);
    } finally {
      setLoading(false);
    }
  }

  function logout() {
    setToken('');
    setAuthenticatedUsername('');
    setRegistration(null);
    setStatus('已登出');
  }

  useEffect(() => {
    refreshLocalProfile().catch((err) => {
      setStatus(`讀取本機資源失敗: ${err.message}`);
    });
  }, []);

  return (
    <main
      style={{
        minHeight: '100vh',
        padding: '32px 20px 40px',
        fontFamily: 'Inter, system-ui, sans-serif',
        color: '#16202a',
        background: 'linear-gradient(180deg, #eff4f8 0%, #f8fbfd 100%)',
      }}
    >
      <div style={{ maxWidth: 1040, margin: '0 auto' }}>
        <header style={{ display: 'flex', justifyContent: 'space-between', gap: 16, alignItems: 'center', flexWrap: 'wrap' }}>
          <div>
            <h1 style={{ margin: 0, fontSize: 30 }}>Hivemind Worker</h1>
            <p style={{ margin: '6px 0 0', color: '#5e6c7a' }}>Local node registration console</p>
          </div>
          {token ? (
            <button type="button" onClick={logout} style={{ ...buttonStyle, background: '#eef2f6', color: '#24313f' }}>
              Sign out
            </button>
          ) : null}
        </header>

        <section style={{ ...panelStyle, marginTop: 20 }}>
          <form onSubmit={handleLogin} style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 12 }}>
            <label>
              Username
              <input value={username} onChange={(e) => setUsername(e.target.value)} style={fieldStyle} />
            </label>
            <label>
              Password
              <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} style={fieldStyle} />
            </label>
            <button
              type="submit"
              disabled={loading}
              style={{ ...buttonStyle, alignSelf: 'end', background: '#1f7a4d', color: '#fff' }}
            >
              {loading ? 'Working...' : 'Login and register'}
            </button>
          </form>
          <div style={{ marginTop: 12, padding: '10px 12px', borderRadius: 10, background: '#edf4fa', color: '#27465d' }}>
            {status}
          </div>
        </section>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))', gap: 18, marginTop: 18 }}>
          <section style={panelStyle}>
            <h2 style={{ margin: '0 0 12px', fontSize: 20 }}>Local Capacity</h2>
            <label>
              Worker endpoint
              <input value={workerIp} onChange={(e) => setWorkerIp(e.target.value)} style={fieldStyle} />
            </label>
            <dl style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, margin: '16px 0 0' }}>
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
            <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 16 }}>
              <button
                type="button"
                onClick={() => refreshLocalProfile().then(() => setStatus('本機資源已更新')).catch((err) => setStatus(`更新失敗: ${err.message}`))}
                style={{ ...buttonStyle, background: '#e7edf3', color: '#22313f' }}
              >
                Refresh profile
              </button>
              {token ? (
                <button
                  type="button"
                  onClick={() => registerWorker().catch(() => {})}
                  disabled={loading}
                  style={{ ...buttonStyle, background: '#1769aa', color: '#fff' }}
                >
                  Register again
                </button>
              ) : null}
            </div>
          </section>

          <section style={panelStyle}>
            <h2 style={{ margin: '0 0 12px', fontSize: 20 }}>Registration Status</h2>
            {registration ? (
              <div style={{ padding: 12, borderRadius: 10, background: registration.success ? '#eef7ef' : '#fff2f2' }}>
                <div style={{ fontWeight: 700 }}>
                  {registration.success ? 'Registered' : 'Not registered'}
                </div>
                <div style={{ marginTop: 6 }}>{registration.message}</div>
                {registration.workerId ? (
                  <div style={{ marginTop: 6, color: '#5e6c7a' }}>worker_id: {registration.workerId}</div>
                ) : null}
              </div>
            ) : (
              <p style={{ color: '#5e6c7a' }}>登入後會把本機節點資訊送到 nodepool，UI 不提供其他 worker 管理功能。</p>
            )}
          </section>
        </div>
      </div>
    </main>
  );
}
