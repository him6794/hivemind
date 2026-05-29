import React, { useEffect, useState } from 'react';

export default function WorkerApp() {
  const apiBase = String(import.meta.env.VITE_API_BASE || 'http://localhost:8082').trim().replace(/\/$/, '');
  const workerControlBase = String(import.meta.env.VITE_WORKER_CONTROL_BASE || 'http://localhost:18080').trim().replace(/\/$/, '');

  const [username, setUsername] = useState('worker1');
  const [password, setPassword] = useState('worker123');
  const [status, setStatus] = useState('尚未登入');
  const [token, setToken] = useState('');
  const [loading, setLoading] = useState(false);
  const [workerIp, setWorkerIp] = useState(`${window.location.hostname || '127.0.0.1'}:50053`);
  const [workerInfo, setWorkerInfo] = useState({
    cpuCores: 8,
    memoryGb: 16,
    cpuScore: 100,
    gpuScore: 80,
    gpuMemoryGb: 8,
    location: '',
  });
  const [currentTask, setCurrentTask] = useState(null);
  const [taskLog, setTaskLog] = useState('');
  const [showTaskDetail, setShowTaskDetail] = useState(false);
  const [clusterWorkers, setClusterWorkers] = useState([]);

  const removeWorker = async (workerId) => {
    if (!token || !workerId) return;
    try {
      const res = await fetch(`${apiBase}/api/remove-worker`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({ worker_id: workerId }),
      });
      const data = await res.json();
      if (data.success) {
        setStatus(`已移除 worker：${workerId}`);
        await refreshClusterWorkers();
      } else {
        setStatus(`移除 worker 失敗：${data.status_message || '未知錯誤'}`);
      }
    } catch (err) {
      setStatus(`移除 worker 失敗：${err.message}`);
    }
  };

  const refreshClusterWorkers = async (tk) => {
    const authToken = tk || token;
    if (!authToken) return;
    try {
      const res = await fetch(`${apiBase}/api/workers?include_offline=1`, {
        headers: { Authorization: `Bearer ${authToken}` },
      });
      const data = await res.json();
      if (data.success) {
        setClusterWorkers(data.workers || []);
      }
    } catch (_) {
      // ignore panel refresh failure to avoid interrupting main flow
    }
  };

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
        await refreshClusterWorkers(data.token || '');
      } else {
        setStatus(`登入失敗：${data.status_message || '未知錯誤'}`);
      }
    } catch (err) {
      setStatus(`連線失敗：${err.message}`);
    } finally {
      setLoading(false);
    }
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
          ip: workerIp,
          cpu_cores: Number(workerInfo.cpuCores),
          memory_gb: Number(workerInfo.memoryGb),
          cpu_score: Number(workerInfo.cpuScore),
          gpu_score: Number(workerInfo.gpuScore),
          gpu_memory_gb: Number(workerInfo.gpuMemoryGb),
          location: workerInfo.location,
        }),
      });
      const data = await res.json();
      if (data.success) {
        setStatus(`worker 節點註冊成功 (CPU: ${workerInfo.cpuCores} cores, 內存: ${workerInfo.memoryGb}GB)`);
        await refreshClusterWorkers();
      } else {
        setStatus(`worker 註冊失敗：${data.status_message || '未知錯誤'}`);
      }
    } catch (err) {
      setStatus(`worker 註冊失敗：${err.message}`);
    }
  };

  const refreshWorkerStatus = async () => {
    if (!token) return;
    try {
      await collectWorkerInfo();
      await refreshClusterWorkers();
      setStatus('worker 狀態已更新');
    } catch (err) {
      setStatus(`更新失敗：${err.message}`);
    }
  };

  const logout = () => {
    setToken('');
    setStatus('已登出');
    setUsername('worker1');
    setPassword('worker123');
    setClusterWorkers([]);
    setShowTaskDetail(false);
  };

  return (
    <div style={{ maxWidth: 600, margin: '40px auto', fontFamily: 'Arial, sans-serif', padding: '0 20px' }}>
      <h1>Hivemind - Worker 節點管理</h1>
      <p style={{ color: '#666' }}>管理和監控計算節點</p>

      <form onSubmit={onSubmit} style={{ display: 'grid', gap: 12, background: '#f9f9f9', padding: 16, borderRadius: 8 }}>
        <label>
          使用者名稱
          <input
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            style={{ width: '100%', padding: 8, marginTop: 4, boxSizing: 'border-box' }}
          />
        </label>

        <label>
          密碼
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            style={{ width: '100%', padding: 8, marginTop: 4, boxSizing: 'border-box' }}
          />
        </label>

        <button type="submit" disabled={loading} style={{ padding: 10, background: loading ? '#ccc' : '#007bff', color: 'white', border: 'none', borderRadius: 4, cursor: 'pointer' }}>
          {loading ? '登入中...' : '登入'}
        </button>
      </form>

      <div style={{ marginTop: 16, padding: 12, background: '#e8f4f8', borderRadius: 4 }}>
        <strong>狀態：</strong> {status}
      </div>

      {token ? (
        <>
          <div style={{ marginTop: 10, display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: 8, background: '#f0f0f0', borderRadius: 4 }}>
            <div>
              <strong>已登入</strong> ({username})
            </div>
            <button onClick={logout} style={{ padding: '5px 10px', background: '#ff6b6b', color: 'white', border: 'none', cursor: 'pointer', borderRadius: 4 }}>
              登出
            </button>
          </div>

          <div style={{ marginTop: 20 }}>
            <h2>Worker 節點資訊</h2>
            <div style={{ padding: '12px', background: '#f5f5f5', borderRadius: 8 }}>
              <div style={{ marginBottom: 12 }}>
                <label>
                  Worker IP:Port
                  <input
                    value={workerIp}
                    onChange={(e) => setWorkerIp(e.target.value)}
                    style={{ width: '100%', padding: 8, marginTop: 4, boxSizing: 'border-box', borderRadius: 4, border: '1px solid #ddd' }}
                  />
                </label>
              </div>

              <div style={{ padding: 12, background: 'white', borderRadius: 4, marginBottom: 12, border: '1px solid #ddd' }}>
                <h3 style={{ margin: '0 0 8px 0' }}>硬體配置</h3>
                <div style={{ display: 'grid', gap: 8, fontSize: 14 }}>
                  <div><strong>CPU 核心數：</strong> {workerInfo.cpuCores}</div>
                  <div><strong>內存容量：</strong> {workerInfo.memoryGb} GB</div>
                  <div><strong>CPU 評分：</strong> {workerInfo.cpuScore}</div>
                  <div><strong>GPU 評分：</strong> {workerInfo.gpuScore}</div>
                  <div><strong>GPU 內存：</strong> {workerInfo.gpuMemoryGb} GB</div>
                  <div><strong>位置：</strong> {workerInfo.location || '未指定'}</div>
                </div>
              </div>

              <div style={{ display: 'grid', gap: 8 }}>
                <button type="button" onClick={refreshWorkerStatus} style={{ padding: 8, background: '#4CAF50', color: 'white', border: 'none', borderRadius: 4, cursor: 'pointer' }}>
                  刷新 Worker 狀態
                </button>
                <button type="button" onClick={registerWorker} style={{ padding: 8, background: '#FF9800', color: 'white', border: 'none', borderRadius: 4, cursor: 'pointer' }}>
                  一鍵註冊 Worker 節點
                </button>
              </div>
            </div>
          </div>

          <div style={{ marginTop: 20 }}>
            <h2>叢集 Worker 節點</h2>
            <div style={{ padding: '12px', background: '#f5f5f5', borderRadius: 8, border: '1px solid #ddd' }}>
              <button
                type="button"
                onClick={() => refreshClusterWorkers()}
                style={{ padding: 8, background: '#6c757d', color: 'white', border: 'none', borderRadius: 4, cursor: 'pointer', marginBottom: 8 }}
              >
                重新整理叢集清單
              </button>
              {clusterWorkers.length === 0 ? (
                <p style={{ color: '#999', margin: 0 }}>目前沒有可顯示的 worker</p>
              ) : (
                <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
                  {clusterWorkers.map((w) => (
                    <li key={`${w.id}-${w.addr}`} style={{ padding: '8px 0', borderBottom: '1px dashed #ddd', fontSize: 13 }}>
                      <strong>{w.id}</strong> @ {w.addr} | 狀態: {w.status || 'UNKNOWN'}
                      <button
                        type="button"
                        onClick={() => removeWorker(w.id)}
                        style={{ marginLeft: 8, padding: '2px 8px', fontSize: 12, background: '#ffe5e5', color: '#b00020', border: '1px solid #ffb3b3', borderRadius: 4, cursor: 'pointer' }}
                      >
                        移除
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>

          <div style={{ marginTop: 20 }}>
            <h2>當前任務</h2>
            {currentTask ? (
              <div style={{ padding: '12px', background: '#f5f5f5', borderRadius: 8, border: '1px solid #ddd' }}>
                <div><strong>任務 ID：</strong> {currentTask.id}</div>
                <div><strong>狀態：</strong> {currentTask.status}</div>
                <div><strong>進度：</strong> {currentTask.progress}%</div>
                <button
                  type="button"
                  onClick={() => setShowTaskDetail(!showTaskDetail)}
                  style={{ padding: '4px 8px', marginTop: 8, background: '#2196F3', color: 'white', border: 'none', borderRadius: 4, cursor: 'pointer' }}
                >
                  {showTaskDetail ? '隱藏' : '查看'}詳情
                </button>
              </div>
            ) : (
              <p style={{ color: '#999', padding: '12px', background: '#f5f5f5', borderRadius: 8 }}>無當前任務</p>
            )}
          </div>

          <div style={{ marginTop: 20, padding: 12, background: '#f9f9f9', borderRadius: 8, border: '1px solid #eee' }}>
            <h3 style={{ margin: '0 0 8px 0' }}>自動更新</h3>
            <p style={{ margin: 0, fontSize: 12, color: '#666' }}>Worker 狀態每 5 秒自動更新一次</p>
          </div>
        </>
      ) : null}
    </div>
  );
}
