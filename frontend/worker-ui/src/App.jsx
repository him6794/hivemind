import React, { useEffect, useMemo, useState } from 'react';

const emptySettings = {
  enabled: true,
  cpu_cores_limit: 0,
  memory_gb_limit: 0,
  gpu_memory_gb_limit: 0,
  storage_gb_limit: 0,
  min_cpt_per_hour: 0,
};

const fieldStyle = {
  width: '100%',
  padding: '9px 10px',
  marginTop: 5,
  boxSizing: 'border-box',
  border: '1px solid #cfd6df',
  borderRadius: 6,
  background: '#fff',
};

const buttonStyle = {
  padding: '9px 12px',
  border: 'none',
  borderRadius: 6,
  cursor: 'pointer',
  fontWeight: 600,
};

const panelStyle = {
  padding: 16,
  background: '#f7f9fb',
  border: '1px solid #dfe5ec',
  borderRadius: 8,
};

function numberValue(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function normalizeWorkerId(worker) {
  return worker?.worker_id || worker?.id || '';
}

export default function WorkerApp() {
  const apiBase = String(import.meta.env.VITE_API_BASE || 'http://localhost:8082')
    .trim()
    .replace(/\/$/, '');
  const workerControlBase = String(
    import.meta.env.VITE_WORKER_CONTROL_BASE || 'http://localhost:18080',
  )
    .trim()
    .replace(/\/$/, '');

  const [username, setUsername] = useState('testuser');
  const [password, setPassword] = useState('testpass123');
  const [status, setStatus] = useState('Not connected');
  const [token, setToken] = useState('');
  const [loading, setLoading] = useState(false);
  const [workerIp, setWorkerIp] = useState(`${window.location.hostname || '127.0.0.1'}:50053`);
  const [workerInfo, setWorkerInfo] = useState({
    cpuCores: 8,
    memoryGb: 16,
    cpuScore: 100,
    gpuScore: 0,
    gpuMemoryGb: 0,
    storageGb: 0,
    location: 'local',
  });
  const [settings, setSettings] = useState(emptySettings);
  const [clusterWorkers, setClusterWorkers] = useState([]);
  const [earnings, setEarnings] = useState({
    total_earned_cpt: 0,
    currency: 'CPT',
    entries: [],
  });

  const ownedWorker = useMemo(
    () => clusterWorkers.find((worker) => normalizeWorkerId(worker) === username),
    [clusterWorkers, username],
  );

  async function readJson(res) {
    const text = await res.text();
    if (!text) return {};
    return JSON.parse(text);
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

  async function refreshClusterWorkers(authToken = token) {
    if (!authToken) return [];
    const data = await authedFetch('/api/workers?include_offline=1', {}, authToken);
    const workers = data.success ? data.workers || [] : [];
    setClusterWorkers(workers);
    return workers;
  }

  async function refreshEarnings(authToken = token) {
    if (!authToken) return;
    const data = await authedFetch('/api/provider/earnings?limit=5', {}, authToken);
    if (data.success) {
      setEarnings({
        total_earned_cpt: data.total_earned_cpt || 0,
        currency: data.currency || 'CPT',
        entries: data.entries || [],
      });
    }
  }

  async function refreshSettings(workerId = username, authToken = token) {
    if (!authToken || !workerId) return;
    const data = await authedFetch(`/api/provider/workers/${workerId}/settings`, {}, authToken);
    if (data.success && data.settings) {
      setSettings(data.settings);
    }
  }

  async function refreshProviderData(authToken = token) {
    const workers = await refreshClusterWorkers(authToken);
    const worker = workers.find((item) => normalizeWorkerId(item) === username);
    if (worker) {
      await refreshSettings(normalizeWorkerId(worker), authToken);
    }
    await refreshEarnings(authToken);
  }

  async function onSubmit(e) {
    e.preventDefault();
    setLoading(true);
    setStatus('Signing in...');
    setToken('');

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
      setToken(data.token || '');
      setStatus('Signed in');
      await refreshProviderData(data.token || '');
    } catch (err) {
      setStatus(`Login failed: ${err.message}`);
    } finally {
      setLoading(false);
    }
  }

  async function collectWorkerInfo() {
    const res = await fetch(`${workerControlBase}/api/worker-info`);
    const data = await readJson(res);
    if (!res.ok || !data.success || !data.profile) {
      throw new Error(data.status_message || data.message || 'Local worker agent unavailable');
    }

    const p = data.profile;
    const nextInfo = {
      cpuCores: Number(p.cpu_cores ?? p.CpuCores ?? 8),
      memoryGb: Number(p.memory_gb ?? p.MemoryGb ?? 16),
      cpuScore: Number(p.cpu_score ?? p.CpuScore ?? 100),
      gpuScore: Number(p.gpu_score ?? p.GpuScore ?? 0),
      gpuMemoryGb: Number(p.gpu_memory_gb ?? p.GpuMemoryGb ?? 0),
      storageGb: Number(p.storage_available_gb ?? p.StorageAvailableGb ?? 0),
      location: String(p.location ?? p.Location ?? 'local'),
    };
    setWorkerInfo(nextInfo);
    setWorkerIp(String(p.ip ?? p.IP ?? workerIp));
    return nextInfo;
  }

  async function registerWorker() {
    if (!token) return;
    setLoading(true);
    setStatus('Reading local worker profile...');

    try {
      const profile = await collectWorkerInfo();
      setStatus('Registering worker...');
      const data = await authedFetch('/api/register-worker', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          username,
          ip: workerIp,
          cpu_cores: numberValue(profile.cpuCores),
          memory_gb: numberValue(profile.memoryGb),
          cpu_score: numberValue(profile.cpuScore),
          gpu_score: numberValue(profile.gpuScore),
          gpu_memory_gb: numberValue(profile.gpuMemoryGb),
          location: profile.location,
        }),
      });
      if (!data.success) {
        throw new Error(data.status_message || 'Worker registration failed');
      }
      setStatus(`Worker registered as ${username}`);
      await refreshProviderData();
    } catch (err) {
      setStatus(`Worker registration failed: ${err.message}`);
    } finally {
      setLoading(false);
    }
  }

  async function saveSettings() {
    if (!token) return;
    const workerId = normalizeWorkerId(ownedWorker) || username;
    setLoading(true);
    setStatus('Saving provider settings...');

    try {
      const data = await authedFetch(`/api/provider/workers/${workerId}/settings`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          enabled: Boolean(settings.enabled),
          cpu_cores_limit: numberValue(settings.cpu_cores_limit),
          memory_gb_limit: numberValue(settings.memory_gb_limit),
          gpu_memory_gb_limit: numberValue(settings.gpu_memory_gb_limit),
          storage_gb_limit: numberValue(settings.storage_gb_limit),
          min_cpt_per_hour: numberValue(settings.min_cpt_per_hour),
        }),
      });
      if (data.success && data.settings) {
        setSettings(data.settings);
      }
      setStatus('Provider settings saved');
      await refreshClusterWorkers();
    } catch (err) {
      setStatus(`Settings save failed: ${err.message}`);
    } finally {
      setLoading(false);
    }
  }

  async function removeWorker(workerId) {
    if (!token || !workerId) return;
    setLoading(true);
    try {
      const data = await authedFetch('/api/remove-worker', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ worker_id: workerId }),
      });
      if (!data.success) throw new Error(data.status_message || 'Remove failed');
      setStatus(`Removed worker ${workerId}`);
      await refreshProviderData();
    } catch (err) {
      setStatus(`Remove failed: ${err.message}`);
    } finally {
      setLoading(false);
    }
  }

  function updateSetting(name, value) {
    setSettings((current) => ({
      ...current,
      [name]: name === 'enabled' ? value : numberValue(value),
    }));
  }

  function logout() {
    setToken('');
    setStatus('Signed out');
    setClusterWorkers([]);
    setEarnings({ total_earned_cpt: 0, currency: 'CPT', entries: [] });
    setSettings(emptySettings);
  }

  useEffect(() => {
    collectWorkerInfo().catch(() => {});
  }, []);

  return (
    <main
      style={{
        maxWidth: 980,
        margin: '28px auto',
        fontFamily: 'Inter, Arial, sans-serif',
        padding: '0 20px 36px',
        color: '#16202a',
      }}
    >
      <header
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          gap: 16,
          alignItems: 'center',
          flexWrap: 'wrap',
        }}
      >
        <div>
          <h1 style={{ margin: 0, fontSize: 28 }}>Hivemind Worker</h1>
          <p style={{ margin: '6px 0 0', color: '#5f6b78' }}>Provider control panel</p>
        </div>
        {token ? (
          <button
            type="button"
            onClick={logout}
            style={{ ...buttonStyle, background: '#eef2f6', color: '#24313f' }}
          >
            Sign out
          </button>
        ) : null}
      </header>

      <section style={{ ...panelStyle, marginTop: 18, display: 'grid', gap: 12 }}>
        <form
          onSubmit={onSubmit}
          style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 12 }}
        >
          <label>
            Username
            <input value={username} onChange={(e) => setUsername(e.target.value)} style={fieldStyle} />
          </label>
          <label>
            Password
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              style={fieldStyle}
            />
          </label>
          <button
            type="submit"
            disabled={loading}
            style={{ ...buttonStyle, alignSelf: 'end', background: '#1769aa', color: '#fff' }}
          >
            {loading ? 'Working...' : 'Sign in'}
          </button>
        </form>
        <div style={{ padding: 10, background: '#eaf2f8', borderRadius: 6, color: '#27465d' }}>{status}</div>
      </section>

      {token ? (
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))', gap: 18, marginTop: 18 }}>
          <section style={panelStyle}>
            <h2 style={{ margin: '0 0 12px', fontSize: 19 }}>Local Worker</h2>
            <label>
              Worker IP:Port
              <input value={workerIp} onChange={(e) => setWorkerIp(e.target.value)} style={fieldStyle} />
            </label>
            <dl style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, margin: '14px 0' }}>
              <dt>CPU cores</dt>
              <dd>{workerInfo.cpuCores}</dd>
              <dt>Memory</dt>
              <dd>{workerInfo.memoryGb} GB</dd>
              <dt>CPU score</dt>
              <dd>{workerInfo.cpuScore}</dd>
              <dt>GPU score</dt>
              <dd>{workerInfo.gpuScore}</dd>
              <dt>GPU memory</dt>
              <dd>{workerInfo.gpuMemoryGb} GB</dd>
              <dt>Location</dt>
              <dd>{workerInfo.location || 'local'}</dd>
            </dl>
            <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
              <button
                type="button"
                onClick={() => collectWorkerInfo().then(() => setStatus('Local profile refreshed'))}
                style={{ ...buttonStyle, background: '#e7edf3', color: '#22313f' }}
              >
                Refresh Profile
              </button>
              <button
                type="button"
                onClick={registerWorker}
                disabled={loading}
                style={{ ...buttonStyle, background: '#1f7a4d', color: '#fff' }}
              >
                Register Worker
              </button>
            </div>
          </section>

          <section style={panelStyle}>
            <h2 style={{ margin: '0 0 12px', fontSize: 19 }}>Earnings</h2>
            <div style={{ fontSize: 32, fontWeight: 700 }}>
              {earnings.total_earned_cpt} {earnings.currency}
            </div>
            <ul style={{ padding: 0, listStyle: 'none', margin: '14px 0 0', display: 'grid', gap: 8 }}>
              {earnings.entries.length === 0 ? (
                <li style={{ color: '#6f7b87' }}>No settled provider credits yet.</li>
              ) : (
                earnings.entries.map((entry) => (
                  <li key={`${entry.task_id}-${entry.created_at}`} style={{ borderTop: '1px solid #dde4eb', paddingTop: 8 }}>
                    <strong>{entry.amount_cpt} CPT</strong> from {entry.task_id}
                    <div style={{ color: '#6f7b87', fontSize: 12 }}>{entry.status}</div>
                  </li>
                ))
              )}
            </ul>
          </section>

          <section style={panelStyle}>
            <h2 style={{ margin: '0 0 12px', fontSize: 19 }}>Provider Settings</h2>
            <label style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12 }}>
              <input
                type="checkbox"
                checked={Boolean(settings.enabled)}
                onChange={(e) => updateSetting('enabled', e.target.checked)}
              />
              Accept tasks
            </label>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
              <label>
                CPU cores limit
                <input
                  type="number"
                  min="0"
                  value={settings.cpu_cores_limit}
                  onChange={(e) => updateSetting('cpu_cores_limit', e.target.value)}
                  style={fieldStyle}
                />
              </label>
              <label>
                Memory limit GB
                <input
                  type="number"
                  min="0"
                  value={settings.memory_gb_limit}
                  onChange={(e) => updateSetting('memory_gb_limit', e.target.value)}
                  style={fieldStyle}
                />
              </label>
              <label>
                GPU memory limit GB
                <input
                  type="number"
                  min="0"
                  value={settings.gpu_memory_gb_limit}
                  onChange={(e) => updateSetting('gpu_memory_gb_limit', e.target.value)}
                  style={fieldStyle}
                />
              </label>
              <label>
                Storage limit GB
                <input
                  type="number"
                  min="0"
                  value={settings.storage_gb_limit}
                  onChange={(e) => updateSetting('storage_gb_limit', e.target.value)}
                  style={fieldStyle}
                />
              </label>
              <label style={{ gridColumn: '1 / -1' }}>
                Minimum CPT/hour
                <input
                  type="number"
                  min="0"
                  value={settings.min_cpt_per_hour}
                  onChange={(e) => updateSetting('min_cpt_per_hour', e.target.value)}
                  style={fieldStyle}
                />
              </label>
            </div>
            <button
              type="button"
              onClick={saveSettings}
              disabled={loading || !ownedWorker}
              style={{
                ...buttonStyle,
                marginTop: 14,
                background: ownedWorker ? '#1769aa' : '#aab4bf',
                color: '#fff',
              }}
            >
              Save Settings
            </button>
          </section>

          <section style={panelStyle}>
            <h2 style={{ margin: '0 0 12px', fontSize: 19 }}>Registered Workers</h2>
            <button
              type="button"
              onClick={() => refreshProviderData().then(() => setStatus('Provider data refreshed'))}
              style={{ ...buttonStyle, background: '#e7edf3', color: '#22313f', marginBottom: 12 }}
            >
              Refresh
            </button>
            <ul style={{ listStyle: 'none', margin: 0, padding: 0, display: 'grid', gap: 10 }}>
              {clusterWorkers.length === 0 ? (
                <li style={{ color: '#6f7b87' }}>No workers registered.</li>
              ) : (
                clusterWorkers.map((worker) => {
                  const workerId = normalizeWorkerId(worker);
                  return (
                    <li key={`${workerId}-${worker.addr}`} style={{ borderTop: '1px solid #dde4eb', paddingTop: 10 }}>
                      <div style={{ display: 'flex', justifyContent: 'space-between', gap: 10 }}>
                        <div>
                          <strong>{workerId}</strong>
                          <div style={{ color: '#6f7b87', fontSize: 12 }}>
                            {worker.addr || worker.ip} · {worker.status || 'UNKNOWN'} · min {worker.min_cpt_per_hour || 0} CPT/h
                          </div>
                        </div>
                        <button
                          type="button"
                          onClick={() => removeWorker(workerId)}
                          style={{ ...buttonStyle, background: '#f4d7d7', color: '#8d1d1d', padding: '5px 9px' }}
                        >
                          Remove
                        </button>
                      </div>
                    </li>
                  );
                })
              )}
            </ul>
          </section>
        </div>
      ) : null}
    </main>
  );
}
