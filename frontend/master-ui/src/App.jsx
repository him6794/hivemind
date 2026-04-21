import React, { useEffect, useState } from 'react';

export default function MasterApp() {
  const apiBase = (import.meta.env.VITE_API_BASE || 'http://localhost:8082').replace(/\/$/, '');

  const [username, setUsername] = useState('testuser');
  const [password, setPassword] = useState('testpass123');
  const [status, setStatus] = useState('尚未登入');
  const [token, setToken] = useState('');
  const [loading, setLoading] = useState(false);
  const [balance, setBalance] = useState(null);
  const [tasks, setTasks] = useState([]);
  const [taskId, setTaskId] = useState('');
  const [torrent, setTorrent] = useState('magnet:?xt=urn:btih:demo');
  const [memoryGb, setMemoryGb] = useState(4);
  const [gpuMemoryGb, setGpuMemoryGb] = useState(2);
  const [hostCount, setHostCount] = useState(1);
  const [creatingTorrent, setCreatingTorrent] = useState(false);
  const [lastTorrentFilePath, setLastTorrentFilePath] = useState('');
  const [transfers, setTransfers] = useState([]);
  const [transferSummary, setTransferSummary] = useState(null);
  const [selectedTask, setSelectedTask] = useState(null);
  const [taskLog, setTaskLog] = useState('');
  const [taskResult, setTaskResult] = useState('');
  const [showTaskDetail, setShowTaskDetail] = useState(false);
  const [taskFilterStatus, setTaskFilterStatus] = useState('');
  const [taskQuery, setTaskQuery] = useState('');
  const [taskLimit, setTaskLimit] = useState(20);
  const [taskOffset, setTaskOffset] = useState(0);
  const [taskTotal, setTaskTotal] = useState(0);
  const [selectedTaskIds, setSelectedTaskIds] = useState([]);

  const parseDispatchCode = (msg) => {
    const m = String(msg || '').match(/^\[([A-Z_]+)\]\s*/);
    return m ? m[1] : '';
  };

  const stripDispatchCode = (msg) => String(msg || '').replace(/^\[[A-Z_]+\]\s*/, '');

  const refreshDashboard = async (tk) => {
    const q = new URLSearchParams();
    if (taskFilterStatus) q.set('status', taskFilterStatus);
    if (taskQuery.trim()) q.set('q', taskQuery.trim());
    q.set('limit', String(Number(taskLimit) || 20));
    q.set('offset', String(Number(taskOffset) || 0));
    const [bRes, tRes] = await Promise.all([
      fetch(`${apiBase}/api/balance`, {
        headers: { Authorization: `Bearer ${tk}` },
      }),
      fetch(`${apiBase}/api/tasks?${q.toString()}`, {
        headers: { Authorization: `Bearer ${tk}` },
      }),
    ]);
    const b = await bRes.json();
    const t = await tRes.json();
    if (b.success) setBalance(b.balance);
    if (t.success) {
      setTasks(t.tasks || []);
      setTaskTotal(Number(t.total || 0));
    }
  };

  const refreshTransfers = async (tk) => {
    const res = await fetch(`${apiBase}/api/transfers?limit=20&aggregate=1`, {
      headers: { Authorization: `Bearer ${tk}` },
    });
    const data = await res.json();
    if (data.success) {
      setTransfers(data.transfers || []);
      setTransferSummary(data.summary || null);
    }
  };

  useEffect(() => {
    if (!token) return undefined;
    const id = setInterval(() => {
      refreshDashboard(token).catch(() => {});
    }, 5000);
    return () => clearInterval(id);
  }, [token, taskFilterStatus, taskQuery, taskLimit, taskOffset]);

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
          await refreshTransfers(tk);
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
          host_count: Number(hostCount),
        }),
      });
      const data = await res.json();
      if (data.success) {
        setStatus(`任務建立成功：${data.task_id}`);
        setTaskId('');
        await refreshDashboard(token);
        await refreshTransfers(token);
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
        setSelectedTaskIds((prev) => prev.filter((id) => id !== taskId));
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
    setTransfers([]);
    setTransferSummary(null);
    setStatus('已登出');
    setUsername('testuser');
    setPassword('testpass123');
    setShowTaskDetail(false);
    setTaskResult('');
    setSelectedTaskIds([]);
  };

  const toggleTaskSelection = (id) => {
    setSelectedTaskIds((prev) => {
      if (prev.includes(id)) return prev.filter((x) => x !== id);
      return [...prev, id];
    });
  };

  const stopSelectedTasks = async () => {
    if (!token || selectedTaskIds.length === 0) return;
    try {
      const res = await fetch(`${apiBase}/api/stop-tasks`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({ task_ids: selectedTaskIds }),
      });
      const data = await res.json();
      if (data.success) {
        setStatus(`批次停止完成：${data.stopped}/${data.total}`);
      } else {
        setStatus(`批次停止失敗：${data.status_message || '未知錯誤'}`);
      }
      await refreshDashboard(token);
      setSelectedTaskIds([]);
    } catch (err) {
      setStatus(`批次停止失敗：${err.message}`);
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
      a.download = `${Date.now()}.torrent`;
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
    <div style={{ maxWidth: 600, margin: '40px auto', fontFamily: 'Arial, sans-serif', padding: '0 20px' }}>
      <h1>Hivemind - 任務管理中心</h1>
      <p style={{ color: '#666' }}>提交和管理分佈式任務</p>

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
              <strong>已登入</strong> ({username}) / 餘額: {balance ?? '-'} 
            </div>
            <button onClick={logout} style={{ padding: '5px 10px', background: '#ff6b6b', color: 'white', border: 'none', cursor: 'pointer', borderRadius: 4 }}>
              登出
            </button>
          </div>

          <div style={{ marginTop: 20 }}>
            <h2>任務管理</h2>
            <div style={{ padding: '12px', background: '#f5f5f5', borderRadius: 8 }}>
              <label style={{ display: 'block', marginBottom: 12 }}>
                <strong>上傳 ZIP 文件（自動生成 Torrent）</strong>
                <input
                  type="file"
                  accept=".zip"
                  disabled={creatingTorrent}
                  onChange={(e) => {
                    if (e.target.files?.[0]) {
                      handleZipUpload(e.target.files[0]);
                    }
                  }}
                  style={{ width: '100%', padding: 8, marginTop: 4, boxSizing: 'border-box' }}
                />
              </label>

              {lastTorrentFilePath ? (
                <button type="button" onClick={downloadLastTorrent} style={{ width: '100%', padding: 8, marginBottom: 12, background: '#4CAF50', color: 'white', border: 'none', borderRadius: 4, cursor: 'pointer' }}>
                  下載最近生成的 .torrent
                </button>
              ) : null}

              <div style={{ display: 'grid', gap: 8 }}>
                <input
                  placeholder="任務 ID（留空自動生成）"
                  value={taskId}
                  onChange={(e) => setTaskId(e.target.value)}
                  style={{ padding: 8, borderRadius: 4, border: '1px solid #ddd' }}
                />
                <input
                  placeholder="Torrent / Magnet URI"
                  value={torrent}
                  onChange={(e) => setTorrent(e.target.value)}
                  style={{ padding: 8, borderRadius: 4, border: '1px solid #ddd' }}
                />
                <input
                  type="number"
                  placeholder="所需內存 (GB)"
                  value={memoryGb}
                  onChange={(e) => setMemoryGb(e.target.value)}
                  style={{ padding: 8, borderRadius: 4, border: '1px solid #ddd' }}
                />
                <input
                  type="number"
                  placeholder="所需 GPU 內存 (GB)"
                  value={gpuMemoryGb}
                  onChange={(e) => setGpuMemoryGb(e.target.value)}
                  style={{ padding: 8, borderRadius: 4, border: '1px solid #ddd' }}
                />
                <input
                  type="number"
                  min="1"
                  placeholder="需要 worker 數量 (host_count)"
                  value={hostCount}
                  onChange={(e) => setHostCount(e.target.value)}
                  style={{ padding: 8, borderRadius: 4, border: '1px solid #ddd' }}
                />
                <button type="button" onClick={createTask} style={{ padding: 8, background: '#007bff', color: 'white', border: 'none', borderRadius: 4, cursor: 'pointer' }}>
                  建立任務
                </button>
              </div>
            </div>

            <h3 style={{ marginTop: 20 }}>轉帳概況（最近 20 筆）</h3>
            <div style={{ padding: 12, background: '#f7f7f7', borderRadius: 8, border: '1px solid #eee' }}>
              <button
                type="button"
                onClick={() => refreshTransfers(token)}
                style={{ padding: '6px 10px', background: '#4c6ef5', color: '#fff', border: 'none', borderRadius: 4, cursor: 'pointer' }}
              >
                重新整理轉帳
              </button>
              <div style={{ marginTop: 8, fontSize: 13, color: '#333' }}>
                收入: {transferSummary?.total_in ?? 0} / 支出: {transferSummary?.total_out ?? 0} / 淨額: {transferSummary?.net ?? 0} / 筆數: {transferSummary?.count ?? 0}
              </div>
              {transfers.length === 0 ? (
                <p style={{ color: '#999', marginTop: 8 }}>暫無轉帳資料</p>
              ) : (
                <ul style={{ listStyle: 'none', padding: 0, marginTop: 8 }}>
                  {transfers.slice(0, 5).map((tr) => (
                    <li key={tr.id} style={{ padding: '6px 0', borderBottom: '1px dashed #ddd', fontSize: 12 }}>
                      #{tr.id} task={tr.task_id} {tr.payer} -&gt; {tr.payee} amount={tr.amount}
                    </li>
                  ))}
                </ul>
              )}
            </div>

            <h3 style={{ marginTop: 20 }}>我的任務</h3>
            <div style={{ display: 'grid', gap: 8, marginBottom: 10, background: '#f6f6f6', padding: 10, borderRadius: 6 }}>
              <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                <select value={taskFilterStatus} onChange={(e) => { setTaskOffset(0); setTaskFilterStatus(e.target.value); }} style={{ padding: 6 }}>
                  <option value="">全部狀態</option>
                  <option value="PENDING">PENDING</option>
                  <option value="DISPATCHED">DISPATCHED</option>
                  <option value="RUNNING">RUNNING</option>
                  <option value="COMPLETED">COMPLETED</option>
                  <option value="FAILED">FAILED</option>
                  <option value="STOPPED">STOPPED</option>
                </select>
                <input
                  value={taskQuery}
                  onChange={(e) => { setTaskOffset(0); setTaskQuery(e.target.value); }}
                  placeholder="搜尋 task_id/worker/訊息"
                  style={{ padding: 6, flex: 1, minWidth: 180 }}
                />
                <input
                  type="number"
                  min="1"
                  max="100"
                  value={taskLimit}
                  onChange={(e) => { setTaskOffset(0); setTaskLimit(Number(e.target.value || 20)); }}
                  style={{ width: 90, padding: 6 }}
                />
              </div>
              <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                <button type="button" onClick={() => refreshDashboard(token)} style={{ padding: '6px 10px', border: '1px solid #ccc', background: '#fff', borderRadius: 4, cursor: 'pointer' }}>
                  套用篩選
                </button>
                <button type="button" onClick={stopSelectedTasks} disabled={selectedTaskIds.length === 0} style={{ padding: '6px 10px', border: 'none', background: selectedTaskIds.length === 0 ? '#ccc' : '#e53935', color: '#fff', borderRadius: 4, cursor: selectedTaskIds.length === 0 ? 'not-allowed' : 'pointer' }}>
                  批次停止 ({selectedTaskIds.length})
                </button>
                <button type="button" onClick={() => setTaskOffset(Math.max(0, Number(taskOffset) - Number(taskLimit)))} disabled={taskOffset <= 0} style={{ padding: '6px 10px' }}>
                  上一頁
                </button>
                <button type="button" onClick={() => setTaskOffset(Number(taskOffset) + Number(taskLimit))} disabled={Number(taskOffset) + Number(taskLimit) >= Number(taskTotal)} style={{ padding: '6px 10px' }}>
                  下一頁
                </button>
                <span style={{ alignSelf: 'center', fontSize: 12, color: '#555' }}>目前 {tasks.length} / 總數 {taskTotal}</span>
              </div>
            </div>
            {tasks.length === 0 ? (
              <p style={{ color: '#999' }}>無任務</p>
            ) : (
              <ul style={{ listStyle: 'none', padding: 0 }}>
                {tasks.map((t) => (
                  <li key={t.TaskID || t.task_id} style={{ marginBottom: '12px', padding: '12px', background: '#f9f9f9', borderRadius: '4px', border: '1px solid #eee' }}>
                    <div>
                      <input
                        type="checkbox"
                        checked={selectedTaskIds.includes(t.TaskID || t.task_id)}
                        onChange={() => toggleTaskSelection(t.TaskID || t.task_id)}
                      />
                    </div>
                    <div style={{ fontWeight: 'bold' }}>{t.TaskID || t.task_id}</div>
                    <div style={{ marginTop: '4px', fontSize: '14px', color: '#333' }}>
                      <strong>狀態：</strong> {(t.Status || t.status)}
                    </div>
                    <div style={{ marginTop: '4px', fontSize: '12px', color: '#666', display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
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
                    <div style={{ marginTop: '8px', display: 'flex', gap: '4px' }}>
                      <button
                        type="button"
                        onClick={() => viewTaskLog(t.TaskID || t.task_id)}
                        style={{ padding: '4px 8px', fontSize: '12px', cursor: 'pointer', background: '#e3f2fd', border: '1px solid #90caf9', borderRadius: 4 }}
                      >
                        查看日誌
                      </button>
                      <button
                        type="button"
                        onClick={() => viewTaskResult(t.TaskID || t.task_id)}
                        style={{ padding: '4px 8px', fontSize: '12px', cursor: 'pointer', background: '#e8f5e9', border: '1px solid #81c784', borderRadius: 4 }}
                      >
                        查看結果
                      </button>
                      <button
                        type="button"
                        onClick={() => stopTask(t.TaskID || t.task_id)}
                        style={{ padding: '4px 8px', fontSize: '12px', cursor: 'pointer', background: '#ffebee', color: '#c62828', border: '1px solid #ef5350', borderRadius: 4 }}
                      >
                        停止
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>

          {showTaskDetail && (
            <div style={{ marginTop: 20, padding: '12px', background: '#f9f9f9', borderRadius: '4px', border: '1px solid #eee' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <strong>任務詳情 ({selectedTask}):</strong>
                <button
                  onClick={() => setShowTaskDetail(false)}
                  style={{ padding: '4px 8px', cursor: 'pointer', background: '#f0f0f0', border: '1px solid #ccc', borderRadius: 4 }}
                >
                  關閉
                </button>
              </div>
              <div style={{ marginTop: '8px' }}>
                <strong>日誌：</strong>
                <div style={{ marginTop: '4px', padding: '8px', background: 'white', borderRadius: '4px', minHeight: '100px', maxHeight: '300px', overflow: 'auto', whiteSpace: 'pre-wrap', fontFamily: 'monospace', fontSize: '12px', border: '1px solid #ddd' }}>
                  {taskLog}
                </div>
              </div>
              <div style={{ marginTop: '12px' }}>
                <strong>結果：</strong>
                <div style={{ marginTop: '4px', padding: '8px', background: 'white', borderRadius: '4px', minHeight: '40px', maxHeight: '180px', overflow: 'auto', whiteSpace: 'pre-wrap', fontFamily: 'monospace', fontSize: '12px', border: '1px solid #ddd' }}>
                  {taskResult || '(尚未查詢)'}
                </div>
              </div>
            </div>
          )}
        </>
      ) : null}
    </div>
  );
}
