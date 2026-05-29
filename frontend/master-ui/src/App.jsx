import React, { useEffect, useState } from 'react';

export default function MasterApp() {
  const apiBase = String(import.meta.env.VITE_API_BASE || 'http://localhost:8082').trim().replace(/\/$/, '');

  const [username, setUsername] = useState('testuser');
  const [password, setPassword] = useState('testpass123');
  const [token, setToken] = useState('');
  const [status, setStatus] = useState('請先登入');
  const [loading, setLoading] = useState(false);

  const [balance, setBalance] = useState(null);
  const [tasks, setTasks] = useState([]);
  const [taskTotal, setTaskTotal] = useState(0);

  const [taskId, setTaskId] = useState('');
  const [torrent, setTorrent] = useState('magnet:?xt=urn:btih:demo');
  const [creatingTorrent, setCreatingTorrent] = useState(false);
  const [lastTorrentFilePath, setLastTorrentFilePath] = useState('');
  const [memoryGb, setMemoryGb] = useState(1);
  const [gpuMemoryGb, setGpuMemoryGb] = useState(0);
  const [hostCount, setHostCount] = useState(1);

  const [selectedTask, setSelectedTask] = useState('');
  const [taskLog, setTaskLog] = useState('');
  const [taskResult, setTaskResult] = useState('');

  async function api(method, path, body, tk = token) {
    const headers = {};
    if (tk) headers.Authorization = `Bearer ${tk}`;
    if (body !== undefined) headers['Content-Type'] = 'application/json';

    const res = await fetch(`${apiBase}${path}`, {
      method,
      headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });

    let data = {};
    try {
      data = await res.json();
    } catch {
      data = { success: false, status_message: `HTTP ${res.status}` };
    }
    return { ok: res.ok, data };
  }

  async function refreshDashboard(tk = token) {
    if (!tk) return;
    const [b, t] = await Promise.all([
      api('GET', '/api/balance', undefined, tk),
      api('GET', '/api/tasks?limit=20&offset=0', undefined, tk),
    ]);

    if (b.data.success) setBalance(b.data.balance);
    if (t.data.success) {
      setTasks(t.data.tasks || []);
      setTaskTotal(Number(t.data.total || 0));
    }
  }

  useEffect(() => {
    if (!token) return;
    const id = setInterval(() => {
      refreshDashboard().catch(() => {});
    }, 5000);
    return () => clearInterval(id);
  }, [token]);

  const login = async (e) => {
    e.preventDefault();
    setLoading(true);
    setStatus('登入中...');
    try {
      const { data } = await api('POST', '/api/login', { username, password }, '');
      if (data.success && data.token) {
        setToken(data.token);
        setStatus('登入成功');
        await refreshDashboard(data.token);
      } else {
        setStatus(`登入失敗: ${data.status_message || 'unknown error'}`);
      }
    } catch (err) {
      setStatus(`連線失敗: ${err.message}`);
    } finally {
      setLoading(false);
    }
  };

  const createTask = async () => {
    if (!token) return;
    setStatus('提交任務中...');
    try {
      const payload = {
        task_id: taskId,
        torrent,
        memory_gb: Number(memoryGb),
        gpu_memory_gb: Number(gpuMemoryGb),
        host_count: Number(hostCount),
      };
      const { data } = await api('POST', '/api/upload-task', payload);
      if (data.success) {
        setStatus(`任務已提交: ${data.task_id}`);
      } else {
        setStatus(`提交失敗: ${data.status_message || 'unknown error'}`);
      }
      await refreshDashboard();
    } catch (err) {
      setStatus(`提交失敗: ${err.message}`);
    }
  };

  const handleZipUpload = async (file) => {
    if (!token || !file) return;
    setCreatingTorrent(true);
    setStatus('上傳 ZIP 並建立 torrent 中...');
    try {
      const formData = new FormData();
      formData.append('file', file);
      const res = await fetch(`${apiBase}/api/create-torrent`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${token}` },
        body: formData,
      });
      const data = await res.json();
      if (data.success) {
        const preferredSource = data.torrent_url || data.torrent || data.magnet || '';
        setTorrent(preferredSource);
        setLastTorrentFilePath(data.torrent_file || '');
        setStatus(`已建立 torrent: ${data.torrent_name || 'torrent'}`);
      } else {
        setStatus(`建立 torrent 失敗: ${data.status_message || 'unknown error'}`);
      }
    } catch (err) {
      setStatus(`建立 torrent 失敗: ${err.message}`);
    } finally {
      setCreatingTorrent(false);
    }
  };

  const viewTaskLog = async (task) => {
    if (!token) return;
    const id = task?.TaskID || task?.task_id || '';
    const fallback = task?.Output || task?.output || task?.StatusMessage || task?.status_message || '(無日誌)';
    if (!id) {
      setTaskLog(fallback);
      setSelectedTask('');
      return;
    }
    try {
      const { data } = await api('GET', `/api/task/${id}/log`);
      setTaskLog(data.success ? (data.log || fallback) : fallback);
      setSelectedTask(id);
      setStatus(data.success ? `任務日誌已載入: ${id}` : `查看日誌失敗: ${data.status_message || 'unknown error'}`);
    } catch (err) {
      setTaskLog(fallback);
      setSelectedTask(id);
      setStatus(`查看日誌失敗: ${err.message}`);
    }
  };

  const viewTaskResult = async (task) => {
    if (!token) return;
    const id = task?.TaskID || task?.task_id || '';
    if (!id) return;
    try {
      const { data } = await api('GET', `/api/task/${id}/result`);
      setTaskResult(data.result_torrent || '(無結果)');
      setSelectedTask(id);
      setStatus(data.success ? `任務結果已載入: ${id}` : `查看結果失敗: ${data.status_message || 'unknown error'}`);
    } catch (err) {
      setStatus(`查看結果失敗: ${err.message}`);
    }
  };

  const stopTask = async (task) => {
    if (!token) return;
    const id = task?.TaskID || task?.task_id || '';
    if (!id) return;
    try {
      const { data } = await api('POST', '/api/stop-task', { task_id: id });
      setStatus(data.success ? `任務已停止: ${id}` : `停止失敗: ${data.status_message || 'unknown error'}`);
      await refreshDashboard();
    } catch (err) {
      setStatus(`停止失敗: ${err.message}`);
    }
  };

  const logout = () => {
    setToken('');
    setBalance(null);
    setTasks([]);
    setTaskTotal(0);
    setSelectedTask('');
    setTaskLog('');
    setTaskResult('');
    setStatus('已登出');
  };

  return (
    <div style={{ maxWidth: 900, margin: '24px auto', fontFamily: 'Arial, sans-serif', padding: '0 16px' }}>
      <h1>Hivemind Master UI</h1>
      <p>API Base: {apiBase}</p>

      {!token && (
        <form onSubmit={login} style={{ display: 'grid', gap: 8, maxWidth: 360, background: '#f5f5f5', padding: 12, borderRadius: 8 }}>
          <input value={username} onChange={(e) => setUsername(e.target.value)} placeholder="username" />
          <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder="password" />
          <button type="submit" disabled={loading}>{loading ? '登入中...' : '登入'}</button>
        </form>
      )}

      <div style={{ marginTop: 12, padding: 10, background: '#eef3ff', borderRadius: 8 }}>{status}</div>

      {token && (
        <>
          <div style={{ marginTop: 12, display: 'flex', gap: 12, alignItems: 'center' }}>
            <strong>Balance: {balance ?? '-'}</strong>
            <strong>Total tasks: {taskTotal}</strong>
            <button onClick={logout}>登出</button>
            <button onClick={() => refreshDashboard()}>刷新</button>
          </div>

          <div style={{ marginTop: 16, padding: 12, background: '#f7f7f7', borderRadius: 8 }}>
            <h3>提交任務</h3>
            <div style={{ display: 'grid', gap: 8 }}>
              <input
                type="file"
                accept=".zip"
                disabled={creatingTorrent}
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (file) handleZipUpload(file);
                }}
              />
              {lastTorrentFilePath ? (
                <div style={{ fontSize: 12, color: '#555' }}>
                  已生成 torrent 檔案：{lastTorrentFilePath}
                </div>
              ) : null}
              <input value={taskId} onChange={(e) => setTaskId(e.target.value)} placeholder="task_id (可空白自動生成)" />
              <input value={torrent} onChange={(e) => setTorrent(e.target.value)} placeholder="torrent/magnet" />
              <input type="number" value={memoryGb} onChange={(e) => setMemoryGb(e.target.value)} placeholder="memory_gb" />
              <input type="number" value={gpuMemoryGb} onChange={(e) => setGpuMemoryGb(e.target.value)} placeholder="gpu_memory_gb" />
              <input type="number" value={hostCount} onChange={(e) => setHostCount(e.target.value)} placeholder="host_count" />
              <button onClick={createTask}>提交</button>
            </div>
          </div>

          <div style={{ marginTop: 16 }}>
            <h3>任務列表</h3>
            {tasks.length === 0 ? (
              <p>目前沒有任務</p>
            ) : (
              <ul style={{ listStyle: 'none', padding: 0, display: 'grid', gap: 10 }}>
                {tasks.map((t) => {
                  const id = t.TaskID || t.task_id;
                  const st = t.Status || t.status;
                  const msg = t.StatusMessage || t.status_message || '';
                  return (
                    <li key={id} style={{ border: '1px solid #ddd', borderRadius: 8, padding: 10 }}>
                      <div><strong>{id}</strong></div>
                      <div>Status: {st}</div>
                      <div style={{ fontSize: 12, color: '#555' }}>{msg}</div>
                      <div style={{ marginTop: 6, display: 'flex', gap: 8 }}>
                        <button onClick={() => viewTaskLog(t)}>查看日誌</button>
                        <button onClick={() => viewTaskResult(t)}>查看結果</button>
                        <button onClick={() => stopTask(t)}>停止</button>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>

          <div style={{ marginTop: 16, padding: 12, background: '#fafafa', borderRadius: 8, border: '1px solid #ddd' }}>
            <h3>任務詳情 {selectedTask ? `(${selectedTask})` : ''}</h3>
            <div>
              <strong>日誌:</strong>
              <pre style={{ whiteSpace: 'pre-wrap', background: '#fff', padding: 8, border: '1px solid #eee' }}>{taskLog || '(空)'}</pre>
            </div>
            <div>
              <strong>結果:</strong>
              <pre style={{ whiteSpace: 'pre-wrap', background: '#fff', padding: 8, border: '1px solid #eee' }}>{taskResult || '(空)'}</pre>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
