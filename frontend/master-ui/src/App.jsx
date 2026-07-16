import React, { useEffect, useState } from 'react';
import { artifactFilenameFromContentDisposition } from './artifactDownloadPolicy.mjs';

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

function toNumber(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export default function MasterApp() {
  const apiBase = String(import.meta.env.VITE_API_BASE || '').trim().replace(/\/$/, '');

  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [token, setToken] = useState('');
  const [status, setStatus] = useState('請先登入');
  const [loading, setLoading] = useState(false);

  const [zipFile, setZipFile] = useState(null);
  const [cpuScore, setCpuScore] = useState(0);
  const [gpuScore, setGpuScore] = useState(0);
  const [memoryGb, setMemoryGb] = useState(0);
  const [gpuMemoryGb, setGpuMemoryGb] = useState(0);
  const [storageGb, setStorageGb] = useState(0);
  const [hostCount, setHostCount] = useState(1);
  const [maxCpt, setMaxCpt] = useState(0);

  const [tasks, setTasks] = useState([]);
  const [selectedTask, setSelectedTask] = useState('');
  const [taskLog, setTaskLog] = useState('');
  const [taskResult, setTaskResult] = useState('');

  async function readJson(res) {
    const text = await res.text();
    if (!text) return {};
    try {
      return JSON.parse(text);
    } catch {
      return {};
    }
  }

  async function api(method, path, body, authToken = token) {
    const headers = {};
    if (authToken) headers.Authorization = `Bearer ${authToken}`;
    if (body !== undefined && !(body instanceof FormData)) {
      headers['Content-Type'] = 'application/json';
    }

    const res = await fetch(`${apiBase}${path}`, {
      method,
      headers,
      body: body instanceof FormData ? body : body !== undefined ? JSON.stringify(body) : undefined,
    });

    const data = await readJson(res);
    return { ok: res.ok, data };
  }

  async function refreshTasks(authToken = token) {
    if (!authToken) return;
    const { data } = await api('GET', '/api/tasks', undefined, authToken);
    if (data.success) {
      setTasks(data.tasks || []);
    } else {
      throw new Error(data.message || data.status_message || 'Failed to load tasks');
    }
  }

  useEffect(() => {
    if (!token) return undefined;
    refreshTasks().catch((err) => setStatus(`任務列表更新失敗: ${err.message}`));
    const id = setInterval(() => {
      refreshTasks().catch(() => {});
    }, 5000);
    return () => clearInterval(id);
  }, [token]);

  async function handleLogin(e) {
    e.preventDefault();
    setLoading(true);
    setStatus('登入中...');
    setToken('');

    try {
      const { data } = await api('POST', '/api/login', { username, password }, '');
      if (!data.success || !data.token) {
        throw new Error(data.message || data.status_message || 'Login failed');
      }

      setToken(data.token);
      setStatus('登入成功');
      await refreshTasks(data.token);
    } catch (err) {
      setStatus(`登入失敗: ${err.message}`);
    } finally {
      setLoading(false);
    }
  }

  async function submitTask() {
    if (!token || !zipFile) return;
    setLoading(true);
    setStatus('上傳 ZIP 任務中...');

    try {
      const form = new FormData();
      form.append('file', zipFile);

      if (cpuScore > 0) form.append('cpu_score', String(toNumber(cpuScore)));
      if (gpuScore > 0) form.append('gpu_score', String(toNumber(gpuScore)));
      if (memoryGb > 0) form.append('memory_gb', String(toNumber(memoryGb)));
      if (gpuMemoryGb > 0) form.append('gpu_memory_gb', String(toNumber(gpuMemoryGb)));
      if (storageGb > 0) form.append('storage_gb', String(toNumber(storageGb)));
      if (hostCount > 0) form.append('host_count', String(toNumber(hostCount)));
      if (maxCpt > 0) form.append('max_cpt', String(toNumber(maxCpt)));

      const { data } = await api('POST', '/api/tasks/upload', form);
      if (!data.success) {
        throw new Error(data.message || data.status_message || 'Task upload failed');
      }

      setZipFile(null);
      setStatus(`任務已提交: ${data.message || 'UUID task created'}`);
      await refreshTasks();
    } catch (err) {
      setStatus(`提交失敗: ${err.message}`);
    } finally {
      setLoading(false);
    }
  }

  async function viewTaskLog(task) {
    if (!token) return;
    const rawId = task?.task_id || task?.TaskID || '';
    if (!String(rawId).trim()) return;
    const validatedTaskId = validateTaskId(rawId);
    if (!validatedTaskId.ok) {
      setTaskLog(validatedTaskId.message);
      return;
    }
    const id = validatedTaskId.taskId;

    try {
      const { data } = await api('GET', `/api/tasks/${encodeURIComponent(id)}/log`);
      if (data.success) {
        setTaskLog(data.log || task?.output || task?.status_message || '(無日誌)');
      } else {
        setTaskLog(task?.output || task?.status_message || '(無日誌)');
      }
    } catch {
      setTaskLog(task?.output || task?.status_message || '(無日誌)');
    }
    setSelectedTask(id);
  }

  async function viewTaskResult(task) {
    if (!token) return;
    const rawId = task?.task_id || task?.TaskID || '';
    if (!String(rawId).trim()) return;
    const validatedTaskId = validateTaskId(rawId);
    if (!validatedTaskId.ok) {
      setTaskResult(validatedTaskId.message);
      return;
    }
    const id = validatedTaskId.taskId;

    try {
      const { data } = await api('GET', `/api/tasks/${encodeURIComponent(id)}/result`);
      if (data.success) {
        setTaskResult(JSON.stringify(data, null, 2));
      } else {
        setTaskResult(data.message || data.status_message || '(無結果)');
      }
    } catch {
      setTaskResult('(無結果)');
    }
    setSelectedTask(id);
  }

  async function cancelTask(task) {
    if (!token) return;
    const rawId = task?.task_id || task?.TaskID || '';
    if (!String(rawId).trim()) return;
    const validatedTaskId = validateTaskId(rawId);
    if (!validatedTaskId.ok) {
      setStatus(validatedTaskId.message);
      return;
    }
    const id = validatedTaskId.taskId;

    try {
      await api('POST', `/api/tasks/${encodeURIComponent(id)}/stop`);
      await refreshTasks();
      setStatus(`已送出取消請求: ${id}`);
    } catch (err) {
      setStatus(`取消失敗: ${err.message}`);
    }
  }

  async function downloadArtifact(task) {
    if (!token) return;
    const rawId = task?.task_id || task?.TaskID || selectedTask || '';
    if (!String(rawId).trim()) return;
    const validatedTaskId = validateTaskId(rawId);
    if (!validatedTaskId.ok) {
      setStatus(validatedTaskId.message);
      return;
    }
    const id = validatedTaskId.taskId;

    try {
      const res = await fetch(`${apiBase}/api/tasks/${encodeURIComponent(id)}/artifact/download`, {
        headers: { Authorization: `Bearer ${token}` },
      });

      if (!res.ok) {
        const data = await readJson(res);
        throw new Error(data.message || data.status_message || `HTTP ${res.status}`);
      }

      const blob = await res.blob();
      const disposition = res.headers.get('content-disposition') || '';
      const filename = artifactFilenameFromContentDisposition(disposition, id);
      const url = window.URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      a.remove();
      window.URL.revokeObjectURL(url);
      setStatus(`artifact 已下載: ${filename}`);
    } catch (err) {
      setStatus(`下載失敗: ${err.message}`);
    }
  }

  function logout() {
    setToken('');
    setTasks([]);
    setSelectedTask('');
    setTaskLog('');
    setTaskResult('');
    setStatus('請先登入');
    setZipFile(null);
  }

  return (
    <main
      style={{
        minHeight: '100vh',
        padding: '32px 20px 40px',
        fontFamily: 'Inter, system-ui, sans-serif',
        color: '#16202a',
        background: 'linear-gradient(180deg, #f4f7fb 0%, #fafcff 100%)',
      }}
    >
      <div style={{ maxWidth: 1120, margin: '0 auto' }}>
        <header style={{ display: 'flex', justifyContent: 'space-between', gap: 16, alignItems: 'center', flexWrap: 'wrap' }}>
          <div>
            <h1 style={{ margin: 0, fontSize: 30 }}>Hivemind Master</h1>
            <p style={{ margin: '6px 0 0', color: '#5e6c7a' }}>Submit ZIP tasks and follow your own queue</p>
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
              style={{ ...buttonStyle, alignSelf: 'end', background: '#1769aa', color: '#fff' }}
            >
              {loading ? 'Working...' : 'Login'}
            </button>
          </form>
          <div style={{ marginTop: 12, padding: '10px 12px', borderRadius: 10, background: '#edf4fa', color: '#27465d' }}>
            {status}
          </div>
        </section>

        {token ? (
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: 18, marginTop: 18 }}>
            <section style={panelStyle}>
              <h2 style={{ margin: '0 0 12px', fontSize: 20 }}>Upload ZIP Task</h2>
              <label style={{ display: 'block' }}>
                ZIP file
                <input
                  type="file"
                  accept=".zip,application/zip"
                  onChange={(e) => {
                    const file = e.target.files?.[0] || null;
                    setZipFile(file);
                  }}
                  style={{ ...fieldStyle, paddingTop: 9 }}
                />
              </label>
              <div style={{ marginTop: 12, display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(140px, 1fr))', gap: 12 }}>
                <label>
                  CPU score
                  <input type="number" min="0" value={cpuScore} onChange={(e) => setCpuScore(e.target.value)} style={fieldStyle} />
                </label>
                <label>
                  GPU score
                  <input type="number" min="0" value={gpuScore} onChange={(e) => setGpuScore(e.target.value)} style={fieldStyle} />
                </label>
                <label>
                  Memory GB
                  <input type="number" min="0" value={memoryGb} onChange={(e) => setMemoryGb(e.target.value)} style={fieldStyle} />
                </label>
                <label>
                  GPU memory GB
                  <input type="number" min="0" value={gpuMemoryGb} onChange={(e) => setGpuMemoryGb(e.target.value)} style={fieldStyle} />
                </label>
                <label>
                  Storage GB
                  <input type="number" min="0" value={storageGb} onChange={(e) => setStorageGb(e.target.value)} style={fieldStyle} />
                </label>
                <label>
                  Host count
                  <input type="number" min="1" value={hostCount} onChange={(e) => setHostCount(e.target.value)} style={fieldStyle} />
                </label>
                <label>
                  Max CPT
                  <input type="number" min="0" value={maxCpt} onChange={(e) => setMaxCpt(e.target.value)} style={fieldStyle} />
                </label>
              </div>
              <div style={{ marginTop: 14, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                <button
                  type="button"
                  onClick={submitTask}
                  disabled={loading || !zipFile}
                  style={{ ...buttonStyle, background: '#1f7a4d', color: '#fff' }}
                >
                  {loading ? 'Uploading...' : 'Upload ZIP'}
                </button>
                <button
                  type="button"
                  onClick={() => refreshTasks().then(() => setStatus('任務列表已更新')).catch((err) => setStatus(`更新失敗: ${err.message}`))}
                  style={{ ...buttonStyle, background: '#e7edf3', color: '#22313f' }}
                >
                  Refresh tasks
                </button>
              </div>
            </section>

            <section style={panelStyle}>
              <h2 style={{ margin: '0 0 12px', fontSize: 20 }}>Your Tasks</h2>
              {tasks.length === 0 ? (
                <p style={{ color: '#5e6c7a' }}>沒有任務。</p>
              ) : (
                <ul style={{ listStyle: 'none', padding: 0, margin: 0, display: 'grid', gap: 12 }}>
                  {tasks.map((task) => {
                    const id = task.task_id || task.TaskID || '';
                    const statusText = task.status || task.Status || '';
                    const message = task.status_message || task.StatusMessage || '';
                    const wallTimeMs = Number(task.wall_time_ms || 0);
                    const billedAmount = Number(task.billed_amount || 0);
                    const statusColor =
                      statusText === 'COMPLETED' ? '#2e7d32'
                        : statusText === 'FAILED' ? '#c62828'
                          : statusText === 'RUNNING' ? '#1565c0'
                            : '#666';

                    return (
                      <li key={id} style={{ border: '1px solid #dfe5ec', borderRadius: 12, padding: 12 }}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, alignItems: 'center' }}>
                          <strong>{id}</strong>
                          <span style={{ color: statusColor, fontWeight: 700, fontSize: 12 }}>{statusText}</span>
                        </div>
                        <div style={{ fontSize: 12, color: '#5e6c7a', marginTop: 4 }}>{message}</div>
                        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', marginTop: 6, fontSize: 12, color: '#66717d' }}>
                          <span>wall: {(wallTimeMs / 1000).toFixed(1)}s</span>
                          <span>billed: {billedAmount} CPT</span>
                          {task.retry_count ? <span>retries: {task.retry_count}</span> : null}
                        </div>
                        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 10 }}>
                          <button type="button" onClick={() => viewTaskLog(task)} style={{ ...buttonStyle, background: '#eef2f6', color: '#24313f' }}>
                            Log
                          </button>
                          <button type="button" onClick={() => viewTaskResult(task)} style={{ ...buttonStyle, background: '#eef2f6', color: '#24313f' }}>
                            Result
                          </button>
                          <button type="button" onClick={() => downloadArtifact(task)} style={{ ...buttonStyle, background: '#eef2f6', color: '#24313f' }}>
                            Download
                          </button>
                          <button type="button" onClick={() => cancelTask(task)} style={{ ...buttonStyle, background: '#f4d7d7', color: '#8d1d1d' }}>
                            Cancel
                          </button>
                        </div>
                      </li>
                    );
                  })}
                </ul>
              )}
            </section>

            <section style={{ ...panelStyle, gridColumn: '1 / -1' }}>
              <h2 style={{ margin: '0 0 12px', fontSize: 20 }}>
                Task Detail {selectedTask ? `(${selectedTask})` : ''}
              </h2>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))', gap: 16 }}>
                <div>
                  <strong>Log</strong>
                  <pre style={{ whiteSpace: 'pre-wrap', background: '#fafcff', border: '1px solid #e3e8ef', padding: 12, borderRadius: 10, minHeight: 160 }}>
                    {taskLog || '(empty)'}
                  </pre>
                </div>
                <div>
                  <strong>Result</strong>
                  <pre style={{ whiteSpace: 'pre-wrap', background: '#fafcff', border: '1px solid #e3e8ef', padding: 12, borderRadius: 10, minHeight: 160 }}>
                    {taskResult || '(empty)'}
                  </pre>
                </div>
              </div>
            </section>
          </div>
        ) : null}
      </div>
    </main>
  );
}
