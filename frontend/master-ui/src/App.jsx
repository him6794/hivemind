import React, { useEffect, useState } from 'react';
import './console.css';
import { artifactFilenameFromContentDisposition } from './artifactDownloadPolicy.mjs';
import { clearStoredSession, readStoredSession, saveStoredSession } from './authSession.mjs';
import { createTaskId, validateTaskId } from './taskIdPolicy.mjs';
import { taskRequestFailureText, taskResponseFailureMessage } from './taskResponsePolicy.mjs';

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

const SESSION_KEY = 'hivemind.master.session.v1';

export default function MasterApp() {
  const apiBase = String(import.meta.env.VITE_API_BASE || '').trim().replace(/\/$/, '');
  const initialSession = readStoredSession(window.localStorage, SESSION_KEY);

  const [username, setUsername] = useState(initialSession.username);
  const [password, setPassword] = useState('');
  const [token, setToken] = useState(initialSession.token);
  const [status, setStatus] = useState(initialSession.token ? 'Session restored' : 'Please log in to manage tasks');
  const [loginLoading, setLoginLoading] = useState(false);
  const [submitLoading, setSubmitLoading] = useState(false);
  const [logLoading, setLogLoading] = useState(null);
  const [resultLoading, setResultLoading] = useState(null);
  const [cancelLoading, setCancelLoading] = useState(null);
  const [downloadLoading, setDownloadLoading] = useState(null);
  const [lastRefresh, setLastRefresh] = useState(null);
  const [sourceError, setSourceError] = useState(null);

  const [taskId, setTaskId] = useState('');
  const [taskSource, setTaskSource] = useState('');
  const [taskInput, setTaskInput] = useState('');
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

    let res;
    try {
      res = await fetch(`${apiBase}${path}`, {
        method,
        headers,
        body: body instanceof FormData ? body : body !== undefined ? JSON.stringify(body) : undefined,
      });
    } catch {
      throw new Error(`Cannot reach Hivemind API at ${apiBase}. Check your connection and VITE_API_BASE.`);
    }

    const data = await readJson(res);
    if (!res.ok) {
      if (res.status === 401) {
        logout();
        throw new Error('Session expired. Please log in again.');
      }
      if (res.status >= 500) {
        throw new Error(`Server error (${res.status}). Please try again later.`);
      }
    }
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
    refreshTasks()
      .then(() => setLastRefresh(Date.now()))
      .catch((err) => setStatus(`Task load failed: ${err.message}`));
    const id = setInterval(() => {
      refreshTasks().then(() => setLastRefresh(Date.now())).catch(() => {});
    }, 5000);
    return () => clearInterval(id);
  }, [token]);

  async function handleLogin(e) {
    e.preventDefault();
    setLoginLoading(true);
    setStatus('Logging in...');
    setToken('');

    try {
      const { data } = await api('POST', '/api/login', { username, password }, '');
      if (!data.success || !data.token) {
        throw new Error(data.message || data.status_message || 'Login failed');
      }

      const ownerUsername = username.trim();
      saveStoredSession(window.localStorage, SESSION_KEY, {
        token: data.token,
        username: ownerUsername,
      });
      setToken(data.token);
      setUsername(ownerUsername);
      setStatus('Logged in successfully');
      await refreshTasks(data.token);
      setLastRefresh(Date.now());
    } catch (err) {
      setStatus(`Login failed: ${err.message}`);
    } finally {
      setLoginLoading(false);
    }
  }

  async function submitTask() {
    if (!token || !taskSource.trim()) return;
    setSubmitLoading(true);
    setStatus('Submitting task...');

    try {
      if (!taskSource.trim()) {
        throw new Error('Function source is required');
      }
      if (taskInput.trim()) {
        try {
          JSON.parse(taskInput);
        } catch {
          throw new Error('Input must be valid JSON');
        }
      }
      if (toNumber(maxCpt) <= 0) {
        throw new Error('Max CPT must be greater than 0 for managed-function tasks');
      }

      const effectiveTaskId = taskId.trim() || createTaskId();
      if (!effectiveTaskId) {
        throw new Error('task_id is required');
      }
      const validatedTaskId = validateTaskId(effectiveTaskId);
      if (!validatedTaskId.ok) {
        throw new Error(validatedTaskId.message);
      }

      const body = {
        task_id: validatedTaskId.taskId,
        runtime: 'managed-function-v0',
        task_source: taskSource,
      };
      // managed-function-v0 tasks carry their JSON input in the `torrent` field.
      if (taskInput.trim()) body.torrent = taskInput;
      if (cpuScore > 0) body.cpu_score = toNumber(cpuScore);
      if (gpuScore > 0) body.gpu_score = toNumber(gpuScore);
      if (memoryGb > 0) body.memory_gb = toNumber(memoryGb);
      if (gpuMemoryGb > 0) body.gpu_memory_gb = toNumber(gpuMemoryGb);
      if (storageGb > 0) body.storage_gb = toNumber(storageGb);
      if (hostCount > 0) body.host_count = toNumber(hostCount);
      if (maxCpt > 0) body.max_cpt = toNumber(maxCpt);

      const { data } = await api('POST', '/api/tasks', body);
      if (!data.success) {
        throw new Error(data.message || data.status_message || 'Task submission failed');
      }

      setTaskId('');
      setTaskSource('');
      setTaskInput('');
      setSourceError(null);
      setStatus(`Task submitted: ${validatedTaskId.taskId}`);
      await refreshTasks();
      setLastRefresh(Date.now());
    } catch (err) {
      setStatus(`Submission failed: ${err.message}`);
    } finally {
      setSubmitLoading(false);
    }
  }

  async function viewTaskLog(task) {
    if (!token) return;
    const rawId = task?.task_id || task?.TaskID || '';
    setLogLoading(rawId);
    if (!String(rawId).trim()) { setLogLoading(null); return; }
    const validatedTaskId = validateTaskId(rawId);
    if (!validatedTaskId.ok) {
      setTaskLog(validatedTaskId.message);
      setLogLoading(null);
      return;
    }
    const id = validatedTaskId.taskId;

    try {
      const { ok, data } = await api('GET', `/api/tasks/${encodeURIComponent(id)}/log`);
      const failureMessage = taskResponseFailureMessage(data, 'Log unavailable', ok);
      if (failureMessage) {
        throw new Error(failureMessage);
      }
      setTaskLog(data.log || '(No output yet)');
    } catch (err) {
      setTaskLog(taskRequestFailureText('Log', err, 'Log unavailable'));
    } finally {
      setLogLoading(null);
    }
    setSelectedTask(id);
  }

  async function viewTaskResult(task) {
    if (!token) return;
    const rawId = task?.task_id || task?.TaskID || '';
    setResultLoading(rawId);
    if (!String(rawId).trim()) { setResultLoading(null); return; }
    const validatedTaskId = validateTaskId(rawId);
    if (!validatedTaskId.ok) {
      setTaskResult(validatedTaskId.message);
      setResultLoading(null);
      return;
    }
    const id = validatedTaskId.taskId;

    try {
      const { ok, data } = await api('GET', `/api/tasks/${encodeURIComponent(id)}/result`);
      const failureMessage = taskResponseFailureMessage(data, 'Result unavailable', ok);
      if (failureMessage) {
        throw new Error(failureMessage);
      }
      setTaskResult(JSON.stringify(data, null, 2));
    } catch (err) {
      setTaskResult(taskRequestFailureText('Result', err, 'Result unavailable'));
    } finally {
      setResultLoading(null);
    }
    setSelectedTask(id);
  }

  async function cancelTask(task) {
    if (!token) return;
    const rawId = task?.task_id || task?.TaskID || '';
    if (!String(rawId).trim()) return;
    if (!window.confirm(`Cancel task "${rawId}"? This cannot be undone.`)) return;
    setCancelLoading(rawId);
    const validatedTaskId = validateTaskId(rawId);
    if (!validatedTaskId.ok) {
      setStatus(validatedTaskId.message);
      setCancelLoading(null);
      return;
    }
    const id = validatedTaskId.taskId;

    try {
      const { ok, data } = await api('POST', `/api/tasks/${encodeURIComponent(id)}/stop`);
      const failureMessage = taskResponseFailureMessage(data, 'Task cancellation was rejected', ok);
      if (failureMessage) {
        throw new Error(failureMessage);
      }
      await refreshTasks();
      setLastRefresh(Date.now());
      setStatus(`Task cancelled: ${id}`);
    } catch (err) {
      setStatus(`Cancel failed: ${err.message}`);
    } finally {
      setCancelLoading(null);
    }
  }

  async function downloadArtifact(task) {
    if (!token) return;
    const rawId = task?.task_id || task?.TaskID || selectedTask || '';
    if (!String(rawId).trim()) return;
    setDownloadLoading(rawId);
    const validatedTaskId = validateTaskId(rawId);
    if (!validatedTaskId.ok) {
      setStatus(validatedTaskId.message);
      setDownloadLoading(null);
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
      setStatus(`Artifact downloaded: ${filename}`);
    } catch (err) {
      setStatus(`Download failed: ${err.message}`);
    } finally {
      setDownloadLoading(null);
    }
  }

  function logout() {
    clearStoredSession(window.localStorage, SESSION_KEY);
    setToken('');
    setTasks([]);
    setSelectedTask('');
    setTaskLog('');
    setTaskResult('');
    setStatus('Please log in to manage tasks');
    setTaskSource('');
    setTaskInput('');
    setSourceError(null);
    setLastRefresh(null);
  }

  function isTerminalStatus(statusText) {
    return statusText === 'COMPLETED' || statusText === 'FAILED' || statusText === 'CANCELLED';
  }

  return (
    <main className="app-shell">
      <div className="app-container">
        <header className="app-header">
          <div className="brand-lockup">
            <div className="brand-mark" aria-hidden="true" />
            <div>
              <p className="eyebrow">Hivemind Console</p>
              <h1>Master UI</h1>
              <p className="lead">
                Submit managed-function tasks to the local Hivemind runtime, monitor execution, and collect worker output from one account session.
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
                <strong>{username}</strong>
              </div>
              <button
                type="button"
                onClick={() => refreshTasks().then(() => {
                  setLastRefresh(Date.now());
                  setStatus('Tasks refreshed');
                }).catch((err) => setStatus(`Refresh failed: ${err.message}`))}
                className="button"
              >
                Refresh tasks
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
                {loginLoading ? 'Signing in...' : 'Login'}
              </button>
            </form>
          )}
          <div className={`status ${status.toLowerCase().includes('failed') || status.toLowerCase().includes('expired') ? 'error' : ''}`}>
            {status}
          </div>
        </section>

        {token ? (
          <div className="grid two" style={{ marginTop: 18 }}>
            <section className="surface">
              <h2>Submit Task</h2>
              <div className="grid">
                <label>
                  Task ID
                  <input
                    value={taskId}
                    onChange={(e) => setTaskId(e.target.value)}
                    placeholder="optional, defaults to UUID"
                    className="field"
                  />
                </label>
                <label>
                  Function source
                  <textarea
                    value={taskSource}
                    onChange={(e) => {
                      setTaskSource(e.target.value);
                      setSourceError(e.target.value.trim() ? null : 'Function source is required');
                    }}
                    placeholder="managed-function-v0 source code"
                    rows={8}
                    className={`field ${sourceError ? 'error' : ''}`}
                  />
                  {sourceError ? <div className="status error">{sourceError}</div> : null}
                </label>
                <label>
                  Input (JSON)
                  <textarea
                    value={taskInput}
                    onChange={(e) => setTaskInput(e.target.value)}
                    placeholder='optional, e.g. {"n": 42}'
                    rows={4}
                    className="field"
                  />
                </label>
              </div>

              <div className="metric-grid" style={{ marginTop: 12 }}>
                <label>
                  CPU score
                  <input type="number" min="0" value={cpuScore} onChange={(e) => setCpuScore(e.target.value)} className="field" />
                </label>
                <label>
                  GPU score
                  <input type="number" min="0" value={gpuScore} onChange={(e) => setGpuScore(e.target.value)} className="field" />
                </label>
                <label>
                  Memory GB
                  <input type="number" min="0" value={memoryGb} onChange={(e) => setMemoryGb(e.target.value)} className="field" />
                </label>
                <label>
                  GPU memory GB
                  <input type="number" min="0" value={gpuMemoryGb} onChange={(e) => setGpuMemoryGb(e.target.value)} className="field" />
                </label>
                <label>
                  Storage GB
                  <input type="number" min="0" value={storageGb} onChange={(e) => setStorageGb(e.target.value)} className="field" />
                </label>
                <label>
                  Host count
                  <input type="number" min="1" value={hostCount} onChange={(e) => setHostCount(e.target.value)} className="field" />
                </label>
                <label>
                  Max CPT
                  <input type="number" min="0" value={maxCpt} onChange={(e) => setMaxCpt(e.target.value)} className="field" />
                </label>
              </div>
              <div className="actions">
                <button
                  type="button"
                  onClick={submitTask}
                  disabled={submitLoading || !taskSource.trim() || !!sourceError}
                  className="button primary"
                >
                  {submitLoading ? 'Submitting...' : 'Submit Task'}
                </button>
              </div>
            </section>

            <section className="surface">
              <div className="toolbar" style={{ marginBottom: 12 }}>
                <h2 style={{ marginBottom: 0 }}>Your Tasks</h2>
                {lastRefresh ? (
                  <span className="subtle">Updated {Math.round((Date.now() - lastRefresh) / 1000)}s ago</span>
                ) : null}
              </div>
              {tasks.length === 0 ? (
                <p className="subtle">No tasks yet. Upload a task file to get started.</p>
              ) : (
                <ul className="task-list">
                  {tasks.map((task) => {
                    const id = task.task_id || task.TaskID || '';
                    const statusText = task.status || task.Status || '';
                    const statusClass = statusText.toLowerCase();
                    const message = task.status_message || task.StatusMessage || '';
                    const wallTimeMs = Number(task.wall_time_ms || 0);
                    const billedAmount = Number(task.billed_amount || 0);
                    const terminal = isTerminalStatus(statusText);

                    const isLogLoading = logLoading === id;
                    const isResultLoading = resultLoading === id;
                    const isCancelLoading = cancelLoading === id;
                    const isDownloadLoading = downloadLoading === id;

                    return (
                      <li key={id} className="task-row">
                        <div className="row-head">
                          <strong>{id}</strong>
                          <span className={`pill ${statusClass}`}>{statusText}</span>
                        </div>
                        <div className="subtle" style={{ marginTop: 4, fontSize: 12 }}>{message}</div>
                        <div className="meta">
                          <span>wall {(wallTimeMs / 1000).toFixed(1)}s</span>
                          <span>billed {billedAmount} CPT</span>
                          {task.retry_count ? <span>retries {task.retry_count}</span> : null}
                        </div>
                        <div className="actions">
                          <button type="button" onClick={() => viewTaskLog(task)} disabled={isLogLoading} className="button">
                            {isLogLoading ? 'Loading...' : 'Log'}
                          </button>
                          <button type="button" onClick={() => viewTaskResult(task)} disabled={isResultLoading} className="button">
                            {isResultLoading ? 'Loading...' : 'Result'}
                          </button>
                          <button type="button" onClick={() => downloadArtifact(task)} disabled={isDownloadLoading} className="button">
                            {isDownloadLoading ? 'Downloading...' : 'Download'}
                          </button>
                          <button type="button" onClick={() => cancelTask(task)} disabled={isCancelLoading || terminal} className="button danger">
                            {isCancelLoading ? 'Cancelling...' : 'Cancel'}
                          </button>
                        </div>
                      </li>
                    );
                  })}
                </ul>
              )}
            </section>

            <section className="surface" style={{ gridColumn: '1 / -1' }}>
              <h2>Task Detail {selectedTask ? `(${selectedTask})` : ''}</h2>
              <div className="grid two">
                <div>
                  <strong>Log</strong>
                  <pre>{logLoading ? 'Loading log...' : (taskLog || '(No output yet)')}</pre>
                </div>
                <div>
                  <strong>Result</strong>
                  <pre>{resultLoading ? 'Loading result...' : (taskResult || '(No result yet)')}</pre>
                </div>
              </div>
            </section>
          </div>
        ) : null}
      </div>
    </main>
  );
}
