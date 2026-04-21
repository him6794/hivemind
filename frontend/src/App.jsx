import React, { useEffect, useState } from 'react';

export default function App() {
  const apiBase = (import.meta.env.VITE_API_BASE || 'http://localhost:8082').replace(/\/$/, '');
  const workerControlBase = (import.meta.env.VITE_WORKER_CONTROL_BASE || 'http://localhost:18080').replace(/\/$/, '');

  const [username, setUsername] = useState('worker1');
  const [password, setPassword] = useState('worker123');
  const [status, setStatus] = useState('尚未登入');
  const [token, setToken] = useState('');
  const [loading, setLoading] = useState(false);
  const [balance, setBalance] = useState(null);
  const [tasks, setTasks] = useState([]);
  const [taskId, setTaskId] = useState('');
  const [torrent, setTorrent] = useState('magnet:?xt=urn:btih:demo');
  const [memoryGb, setMemoryGb] = useState(4);
  const [gpuMemoryGb, setGpuMemoryGb] = useState(2);
  const [zipFile, setZipFile] = useState(null);
  const [creatingTorrent, setCreatingTorrent] = useState(false);
  const [lastTorrentFilePath, setLastTorrentFilePath] = useState('');
  const [selectedTask, setSelectedTask] = useState(null);
  const [taskLog, setTaskLog] = useState('');
  const [taskResult, setTaskResult] = useState('');
  const [showTaskDetail, setShowTaskDetail] = useState(false);
  const [workerIp, setWorkerIp] = useState(`${window.location.hostname || '127.0.0.1'}:50053`);
  const [workerInfo, setWorkerInfo] = useState({
    cpuCores: 8,
    memoryGb: 16,
    cpuScore: 100,
    gpuScore: 80,
    gpuMemoryGb: 8,
    location: '',
  });

  const parseDispatchCode = (msg) => {
    const m = String(msg || '').match(/^\[([A-Z_]+)\]\s*/);
    return m ? m[1] : '';
  };

  const stripDispatchCode = (msg) => String(msg || '').replace(/^\[[A-Z_]+\]\s*/, '');

  const refreshDashboard = async (tk) => {
    const [bRes, tRes] = await Promise.all([
      fetch(`${apiBase}/api/balance`, {
        headers: { Authorization: `Bearer ${tk}` },
      }),
      fetch(`${apiBase}/api/tasks`, {
        headers: { Authorization: `Bearer ${tk}` },
      }),
    ]);
    const b = await bRes.json();
    const t = await tRes.json();
    if (b.success) setBalance(b.balance);
    if (t.success) setTasks(t.tasks || []);
  };

  useEffect(() => {
    if (!token) return undefined;
    const id = setInterval(() => {
      refreshDashboard(token).catch(() => {});
    }, 5000);
    return () => clearInterval(id);
  }, [token]);

  const onSubmit = async (e) => {
    e.preventDefault();
    setLoading(true);
    setStatus('登入中...');
    setToken('');

    try {
      const res = await fetch(`${apiBase}/api/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, password }),
      });
      const data = await res.json();
      if (data.success) {
        setStatus('登入成功');
        setToken(data.token || '');
        const tk = data.token || '';
        if (tk) {
          await refreshDashboard(tk);
        }
      } else {
        setStatus(`登入失敗：${data.status_message || '未知錯誤'}`);
      }
    } catch (err) {
      setStatus(`連線失敗：${err.message}`);
    } finally {
      setLoading(false);
    }
  };

  const createTask = async () => {
    if (!token) return;
    setStatus('建立任務中...');
    try {
      const res = await fetch(`${apiBase}/api/upload-task`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({
          task_id: taskId,
          torrent,
          memory_gb: Number(memoryGb),
          gpu_memory_gb: Number(gpuMemoryGb),
        }),
      });
      const data = await res.json();
      if (data.success) {
        setStatus(`任務建立成功：${data.task_id}`);
        setTaskId('');
        await refreshDashboard(token);
      } else {
        setStatus(`任務建立失敗：${data.status_message || '未知錯誤'}`);
      }
    } catch (err) {
      setStatus(`任務建立失敗：${err.message}`);
    }
  };

  const handleZipUpload = async (file) => {
    if (!file || !token) return;
    setCreatingTorrent(true);
    setStatus('正在生成種子...');
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
        setStatus(`種子已生成：${data.torrent_name || 'torrent'}（來源：${data.torrent_url ? 'torrent url' : 'magnet'}）`);
        setZipFile(null);
      } else {
        setStatus(`種子生成失敗：${data.status_message || '未知錯誤'}`);
      }
    } catch (err) {
      setStatus(`種子生成失敗：${err.message}`);
    } finally {
      setCreatingTorrent(false);
    }
  };

  const viewTaskLog = async (taskId) => {
    if (!token) return;
    try {
      const res = await fetch(`${apiBase}/api/task/${taskId}/log`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      const data = await res.json();
      if (data.success) {
        setTaskLog(data.log || '(暫無日誌)');
        setSelectedTask(taskId);
        setShowTaskDetail(true);
        setStatus(`任務日誌已載入：${taskId}`);
      } else {
        setStatus(`載入日誌失敗：${data.status_message || '未知錯誤'}`);
      }
    } catch (err) {
      setStatus(`載入日誌失敗：${err.message}`);
    }
  };

  const viewTaskResult = async (taskId) => {
    if (!token) return;
    try {
      const res = await fetch(`${apiBase}/api/task/${taskId}/result`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      const data = await res.json();
      if (data.success) {
        setTaskResult(data.result_torrent || '(暫無結果)');
        setSelectedTask(taskId);
        setShowTaskDetail(true);
        setStatus(`任務結果已載入：${taskId}`);
      } else {
        setStatus(`載入結果失敗：${data.status_message || '未知錯誤'}`);
      }
    } catch (err) {
      setStatus(`載入結果失敗：${err.message}`);
    }
  };

  const stopTask = async (taskId) => {
    if (!token) return;
    try {
      const res = await fetch(`${apiBase}/api/stop-task`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({ task_id: taskId }),
      });
      const data = await res.json();
      if (data.success) {
        setStatus(`任務已停止：${taskId}`);
        await refreshDashboard(token);
      } else {
        setStatus(`停止失敗：${data.status_message || '未知錯誤'}`);
      }
    } catch (err) {
      setStatus(`停止失敗：${err.message}`);
    }
  };

  const logout = () => {
    setToken('');
    setBalance(null);
    setTasks([]);
    setStatus('已登出');
    setUsername('worker1');
    setPassword('worker123');
    setShowTaskDetail(false);
    setTaskResult('');
  };

  const collectWorkerInfo = async () => {
    try {
      const res = await fetch(`${workerControlBase}/api/worker-info`);
      const data = await res.json();
      if (data.success && data.profile) {
        const p = data.profile;
        setWorkerInfo({
          cpuCores: Number(p.cpu_cores ?? p.CpuCores ?? 8),
          memoryGb: Number(p.memory_gb ?? p.MemoryGb ?? 16),
          cpuScore: Number(p.cpu_score ?? p.CpuScore ?? 100),
          gpuScore: Number(p.gpu_score ?? p.GpuScore ?? 80),
          gpuMemoryGb: Number(p.gpu_memory_gb ?? p.GpuMemoryGb ?? 8),
          location: String(p.location ?? p.Location ?? ''),
        });
        setWorkerIp(String(p.ip ?? p.IP ?? workerIp));
        return true;
      }
      setStatus(`取得 worker 資訊失敗：${data.status_message || '未知錯誤'}`);
      return false;
    } catch (err) {
      setStatus(`取得 worker 資訊失敗：${err.message}`);
      return false;
    }
  };

  const registerWorker = async () => {
    if (!token) return;
    setStatus('正在從 worker 程式讀取硬體資訊...');
    const ok = await collectWorkerInfo();
    if (!ok) return;

    let latestInfo = workerInfo;
    let latestIp = workerIp;
    try {
      const latestRes = await fetch(`${workerControlBase}/api/worker-info`);
      const latest = await latestRes.json();
      if (latest.success && latest.profile) {
        const p = latest.profile;
        latestInfo = {
          cpuCores: Number(p.cpu_cores ?? p.CpuCores ?? 8),
          memoryGb: Number(p.memory_gb ?? p.MemoryGb ?? 16),
          cpuScore: Number(p.cpu_score ?? p.CpuScore ?? 100),
          gpuScore: Number(p.gpu_score ?? p.GpuScore ?? 80),
          gpuMemoryGb: Number(p.gpu_memory_gb ?? p.GpuMemoryGb ?? 8),
          location: String(p.location ?? p.Location ?? ''),
        };
        latestIp = String(p.ip ?? p.IP ?? workerIp);
      }
    } catch (_) {
      // ignore; use already collected state
    }

    setStatus('註冊 worker 節點中...');
    try {
      const res = await fetch(`${apiBase}/api/register-worker`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({
          username,
          ip: latestIp,
          cpu_cores: Number(latestInfo.cpuCores),
          memory_gb: Number(latestInfo.memoryGb),
          cpu_score: Number(latestInfo.cpuScore),
          gpu_score: Number(latestInfo.gpuScore),
          gpu_memory_gb: Number(latestInfo.gpuMemoryGb),
          location: latestInfo.location,
        }),
      });
      const data = await res.json();
      if (data.success) {
        setStatus(`worker 節點註冊成功（cpu=${latestInfo.cpuCores}, mem=${latestInfo.memoryGb}GB）`);
      } else {
        setStatus(`worker 註冊失敗：${data.status_message || '未知錯誤'}`);
      }
    } catch (err) {
      setStatus(`worker 註冊失敗：${err.message}`);
    }
  };

  const downloadLastTorrent = async () => {
    if (!token || !lastTorrentFilePath) return;
    try {
      const res = await fetch(`${apiBase}${lastTorrentFilePath}`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (!res.ok) {
        const txt = await res.text();
        setStatus(`下載 torrent 失敗：${txt || res.status}`);
        return;
      }
      const blob = await res.blob();
      const url = window.URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `${(Date.now())}.torrent`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      window.URL.revokeObjectURL(url);
      setStatus('torrent 檔下載成功');
    } catch (err) {
      setStatus(`下載 torrent 失敗：${err.message}`);
    }
  };

  return (
    <div style={{ maxWidth: 420, margin: '40px auto', fontFamily: 'Arial, sans-serif' }}>
      <h1>Hivemind 登入</h1>
      <p>最小可用登入頁（連到 nodepool 的 /api/login）</p>

      <form onSubmit={onSubmit} style={{ display: 'grid', gap: 12 }}>
        <label>
          使用者
          <input
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            style={{ width: '100%', padding: 8 }}
          />
        </label>

        <label>
          密碼
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            style={{ width: '100%', padding: 8 }}
          />
        </label>

        <button type="submit" disabled={loading} style={{ padding: 10 }}>
          {loading ? '登入中...' : '登入'}
        </button>
      </form>

      <div style={{ marginTop: 16 }}>
        <strong>狀態：</strong> {status}
      </div>

      {token ? (
        <>
          <div style={{ marginTop: 10, wordBreak: 'break-all', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <div>
              <strong>Token：</strong> {token.substring(0, 20)}...
            </div>
            <button onClick={logout} style={{ padding: '5px 10px', background: '#ff6b6b', color: 'white', border: 'none', cursor: 'pointer' }}>
              登出
            </button>
          </div>
          <div style={{ marginTop: 10 }}>
            <strong>餘額：</strong> {balance ?? '-'}
          </div>
          <div style={{ marginTop: 12, padding: '10px', background: '#eef6ff', borderRadius: '6px' }}>
            <strong>Worker 節點註冊（需先登入）</strong>
            <div style={{ marginTop: 8, display: 'grid', gap: 6 }}>
              <input
                placeholder="worker ip:port"
                value={workerIp}
                onChange={(e) => setWorkerIp(e.target.value)}
                style={{ padding: 8 }}
              />
              <div style={{ fontSize: 12, background: '#fff', padding: 8, borderRadius: 4 }}>
                CPU Cores: {workerInfo.cpuCores} / Memory: {workerInfo.memoryGb}GB / CPU Score: {workerInfo.cpuScore} / GPU Score: {workerInfo.gpuScore} / GPU Memory: {workerInfo.gpuMemoryGb}GB / Location: {workerInfo.location || '-'}
              </div>
              <button type="button" onClick={registerWorker} style={{ padding: 8 }}>
                一鍵註冊 Worker 節點
              </button>
            </div>
          </div>
          <div style={{ marginTop: 12 }}>
            <strong>任務列表：</strong>
            <div style={{ marginTop: 8, display: 'grid', gap: 8 }}>
              <label style={{ display: 'grid' }}>
                選擇 ZIP 文件自動做種
                <input
                  type="file"
                  accept=".zip"
                  disabled={creatingTorrent}
                  onChange={(e) => {
                    if (e.target.files?.[0]) {
                      setZipFile(e.target.files[0]);
                      handleZipUpload(e.target.files[0]);
                    }
                  }}
                  style={{ padding: 8 }}
                />
              </label>
              {lastTorrentFilePath ? (
                <button type="button" onClick={downloadLastTorrent} style={{ padding: 8 }}>
                  下載最近生成的 .torrent
                </button>
              ) : null}
              <input
                placeholder="task id（可留空自動生成）"
                value={taskId}
                onChange={(e) => setTaskId(e.target.value)}
                style={{ padding: 8 }}
              />
              <input
                placeholder="torrent / magnet"
                value={torrent}
                onChange={(e) => setTorrent(e.target.value)}
                style={{ padding: 8 }}
              />
              <input
                type="number"
                value={memoryGb}
                onChange={(e) => setMemoryGb(e.target.value)}
                style={{ padding: 8 }}
              />
              <input
                type="number"
                value={gpuMemoryGb}
                onChange={(e) => setGpuMemoryGb(e.target.value)}
                style={{ padding: 8 }}
              />
              <button type="button" onClick={createTask} style={{ padding: 8 }}>
                建立任務
              </button>
            </div>
            <ul>
              {tasks.map((t) => (
                <li key={t.TaskID || t.task_id} style={{ marginBottom: '8px', padding: '8px', background: '#f0f0f0', borderRadius: '4px' }}>
                  <div><strong>{(t.TaskID || t.task_id)}</strong> - {(t.Status || t.status)}</div>
                  <div style={{ marginTop: '4px', fontSize: '12px', color: '#333', display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                    {parseDispatchCode(t.StatusMessage || t.status_message) ? (
                      <span style={{ background: '#e9ecff', color: '#334', borderRadius: 10, padding: '1px 8px', fontWeight: 700 }}>
                        {parseDispatchCode(t.StatusMessage || t.status_message)}
                      </span>
                    ) : null}
                    <span>{stripDispatchCode(t.StatusMessage || t.status_message || '-')}</span>
                  </div>
                  <div style={{ marginTop: '4px', fontSize: '12px', color: '#555' }}>
                    CPU {Number(t.CpuUsage ?? t.cpu_usage ?? 0).toFixed(1)}% /
                    MEM {Number(t.MemoryUsage ?? t.memory_usage ?? 0).toFixed(1)}% /
                    GPU {Number(t.GpuUsage ?? t.gpu_usage ?? 0).toFixed(1)}% /
                    GPU-MEM {Number(t.GpuMemoryUsage ?? t.gpu_memory_usage ?? 0).toFixed(1)}%
                  </div>
                  <div style={{ marginTop: '4px', display: 'flex', gap: '4px' }}>
                    <button
                      type="button"
                      onClick={() => viewTaskLog(t.TaskID || t.task_id)}
                      style={{ padding: '4px 8px', fontSize: '12px', cursor: 'pointer' }}
                    >
                      查看日誌
                    </button>
                    <button
                      type="button"
                      onClick={() => viewTaskResult(t.TaskID || t.task_id)}
                      style={{ padding: '4px 8px', fontSize: '12px', cursor: 'pointer' }}
                    >
                      查看結果
                    </button>
                    <button
                      type="button"
                      onClick={() => stopTask(t.TaskID || t.task_id)}
                      style={{ padding: '4px 8px', fontSize: '12px', cursor: 'pointer', background: '#ff9999', color: 'white', border: 'none' }}
                    >
                      停止
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          </div>

          {showTaskDetail && (
            <div style={{ marginTop: 20, padding: '12px', background: '#f9f9f9', borderRadius: '4px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <strong>任務日誌 ({selectedTask}):</strong>
                <button
                  onClick={() => setShowTaskDetail(false)}
                  style={{ padding: '4px 8px', cursor: 'pointer' }}
                >
                  關閉
                </button>
              </div>
              <div style={{ marginTop: '8px', padding: '8px', background: 'white', borderRadius: '4px', minHeight: '100px', maxHeight: '300px', overflow: 'auto', whiteSpace: 'pre-wrap', fontFamily: 'monospace', fontSize: '12px' }}>
                {taskLog}
              </div>
              <div style={{ marginTop: '8px', padding: '8px', background: 'white', borderRadius: '4px', minHeight: '40px', maxHeight: '180px', overflow: 'auto', whiteSpace: 'pre-wrap', fontFamily: 'monospace', fontSize: '12px' }}>
                <strong>結果：</strong> {taskResult || '(尚未查詢)'}
              </div>
            </div>
          )}
        </>
      ) : null}
    </div>
  );
}
