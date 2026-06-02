import React, { useEffect, useState } from "react";

export default function MasterApp() {
  const apiBase = String(import.meta.env.VITE_API_BASE || "http://localhost:8082").trim().replace(/\/$/, "");

  const [username, setUsername] = useState("testuser");
  const [password, setPassword] = useState("testpass123");
  const [token, setToken] = useState("");
  const [status, setStatus] = useState("請先登入");
  const [loading, setLoading] = useState(false);

  const [tasks, setTasks] = useState([]);
  const [taskTotal, setTaskTotal] = useState(0);
  const [cacheMetrics, setCacheMetrics] = useState({
    total_completed_tasks: 0,
    total_cache_hits: 0,
    cache_hit_rate: 0,
    top_workers: [],
  });
  const [cacheAlert, setCacheAlert] = useState({
    severity: "normal",
    message: "Cache Hit Rate 在可接受範圍。",
    cache_hit_rate: 0,
  });
  const [lowHitRateThreshold, setLowHitRateThreshold] = useState(0.3);
  const [highHitRateThreshold, setHighHitRateThreshold] = useState(3.0);

  const [taskId, setTaskId] = useState("");
  const [torrent, setTorrent] = useState("magnet:?xt=urn:btih:demo");
  const [zipPath, setZipPath] = useState("");
  const [memoryGb, setMemoryGb] = useState(1);
  const [gpuMemoryGb, setGpuMemoryGb] = useState(0);
  const [hostCount, setHostCount] = useState(1);
  const [maxCpt, setMaxCpt] = useState(0);

  const [selectedTask, setSelectedTask] = useState("");
  const [taskLog, setTaskLog] = useState("");
  const [taskResult, setTaskResult] = useState("");
  const [trustWorkerId, setTrustWorkerId] = useState("");
  const [trustScore, setTrustScore] = useState(100);
  const [trustBanned, setTrustBanned] = useState(false);
  const [trustProfile, setTrustProfile] = useState(null);
  const [trustEntries, setTrustEntries] = useState([]);
  const [auditLogs, setAuditLogs] = useState([]);
  const [auditLimit, setAuditLimit] = useState(20);
  const [cacheAnomalies, setCacheAnomalies] = useState([]);
  const [anomalyLimit, setAnomalyLimit] = useState(20);

  async function api(method, path, body, tk = token) {
    const headers = {};
    if (tk) headers.Authorization = `Bearer ${tk}`;
    if (body !== undefined) headers["Content-Type"] = "application/json";

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
    const t = await api("GET", "/api/tasks", undefined, tk);
    const cm = await api("GET", "/api/admin/scheduling/cache-metrics", undefined, tk);
    const alert = await api(
      "GET",
      `/api/admin/scheduling/cache-alert?low=${encodeURIComponent(lowHitRateThreshold)}&high=${encodeURIComponent(highHitRateThreshold)}`,
      undefined,
      tk,
    );

    if (t.data.success) {
      setTasks(t.data.tasks || []);
      setTaskTotal(Number(t.data.total || (t.data.tasks || []).length));
    }
    if (cm.data.success) {
      setCacheMetrics({
        total_completed_tasks: Number(cm.data.total_completed_tasks || 0),
        total_cache_hits: Number(cm.data.total_cache_hits || 0),
        cache_hit_rate: Number(cm.data.cache_hit_rate || 0),
        top_workers: Array.isArray(cm.data.top_workers) ? cm.data.top_workers : [],
      });
    }
    if (alert.data && alert.data.success) {
      setCacheAlert({
        severity: String(alert.data.severity || "normal"),
        message: String(alert.data.message || "Cache hit alert unavailable"),
        cache_hit_rate: Number(alert.data.cache_hit_rate || 0),
      });
    } else {
      setCacheAlert({
        severity: String(alert?.data?.severity || "error"),
        message: String(alert?.data?.message || "Cache alert API failed"),
        cache_hit_rate: Number(cacheMetrics.cache_hit_rate || 0),
      });
    }
  }

  useEffect(() => {
    if (!token) return;
    const id = setInterval(() => {
      refreshDashboard().catch(() => {});
    }, 5000);
    return () => clearInterval(id);
  }, [token]);

  useEffect(() => {
    if (!token) return;
    refreshDashboard().catch(() => {});
  }, [token, lowHitRateThreshold, highHitRateThreshold]);

  const login = async (e) => {
    e.preventDefault();
    setLoading(true);
    setStatus("登入中...");
    try {
      const { data } = await api("POST", "/api/login", { username, password }, "");
      if (data.success && data.token) {
        setToken(data.token);
        setStatus("登入成功");
        await refreshDashboard(data.token);
      } else {
        setStatus(`登入失敗: ${data.status_message || "unknown error"}`);
      }
    } catch (err) {
      setStatus(`連線失敗: ${err.message}`);
    } finally {
      setLoading(false);
    }
  };

  const createTask = async () => {
    if (!token) return;
    setStatus("提交任務中...");
    try {
      const payload = {
        task_id: taskId,
        torrent: zipPath.trim() ? undefined : torrent,
        zip_path: zipPath.trim() || undefined,
        memory_gb: Number(memoryGb),
        gpu_memory_gb: Number(gpuMemoryGb),
        host_count: Number(hostCount),
        max_cpt: Number(maxCpt),
      };
      const { data } = await api("POST", "/api/tasks", payload);
      if (data.success) {
        setStatus(`任務已提交: ${data.task?.task_id || taskId}`);
      } else {
        setStatus(`提交失敗: ${data.message || data.status_message || "unknown error"}`);
      }
      await refreshDashboard();
    } catch (err) {
      setStatus(`提交失敗: ${err.message}`);
    }
  };

  const viewTaskLog = async (task) => {
    if (!token) return;
    const id = task?.TaskID || task?.task_id || "";
    const fallback = task?.Output || task?.output || task?.StatusMessage || task?.status_message || "(無日誌)";
    if (!id) {
      setTaskLog(fallback);
      setSelectedTask(id);
      return;
    }
    try {
      const { data } = await api("GET", `/api/tasks/${encodeURIComponent(id)}/log`);
      if (data.success) {
        setTaskLog(data.output || data.status_message || "(無日誌)");
      } else {
        setTaskLog(fallback);
      }
    } catch {
      setTaskLog(fallback);
    }
    setSelectedTask(id);
  };

  const viewTaskResult = async (task) => {
    if (!token) return;
    const id = task?.TaskID || task?.task_id || "";
    if (!id) {
      setTaskResult("(無結果)");
      return;
    }
    try {
      const { data } = await api("GET", `/api/tasks/${encodeURIComponent(id)}/result`);
      if (data.success) {
        setTaskResult(JSON.stringify(data, null, 2));
      } else {
        setTaskResult(data.message || data.status_message || "(無結果)");
      }
    } catch {
      setTaskResult("(無結果)");
    }
    setSelectedTask(id);
  };

  const stopTask = async (task) => {
    if (!token) return;
    const id = task?.TaskID || task?.task_id || "";
    if (!id) return;
    try {
      await api("POST", `/api/tasks/${encodeURIComponent(id)}/stop`);
      await refreshDashboard();
    } catch (err) {
      setStatus(`停止失敗: ${err.message}`);
    }
  };

  const loadWorkerTrust = async () => {
    if (!token || !trustWorkerId.trim()) return;
    try {
      const { data } = await api("GET", `/api/provider/workers/${encodeURIComponent(trustWorkerId.trim())}/trust`);
      if (data.success && data.trust) {
        setTrustProfile(data.trust);
        setTrustScore(Number(data.trust.score || 0));
        setTrustBanned(Boolean(data.trust.banned));
        setStatus(`已載入 worker trust: ${trustWorkerId.trim()}`);
      } else {
        setStatus(`讀取 trust 失敗: ${data.message || data.status_message || "unknown error"}`);
      }
    } catch (err) {
      setStatus(`讀取 trust 失敗: ${err.message}`);
    }
  };

  const loadWorkerTrustList = async () => {
    if (!token) return;
    try {
      const { data } = await api("GET", "/api/admin/workers/trust");
      if (data.success) {
        setTrustEntries(data.entries || []);
      } else {
        setStatus(`讀取 trust 列表失敗: ${data.message || data.status_message || "unknown error"}`);
      }
    } catch (err) {
      setStatus(`讀取 trust 列表失敗: ${err.message}`);
    }
  };

  const applyWorkerTrustControl = async () => {
    if (!token || !trustWorkerId.trim()) return;
    try {
      const payload = { banned: trustBanned, score: Number(trustScore) };
      const { data } = await api(
        "PUT",
        `/api/admin/workers/${encodeURIComponent(trustWorkerId.trim())}/trust-control`,
        payload,
      );
      if (data.success) {
        setStatus(`已更新 worker trust: ${trustWorkerId.trim()} (banned=${String(data.banned)}, score=${data.score})`);
        await loadWorkerTrust();
      } else {
        setStatus(`更新 trust 失敗: ${data.message || data.status_message || "unknown error"}`);
      }
    } catch (err) {
      setStatus(`更新 trust 失敗: ${err.message}`);
    }
  };

  const loadAuditLogs = async () => {
    if (!token) return;
    try {
      const { data } = await api("GET", `/api/admin/audit/logs?limit=${Number(auditLimit)}`);
      if (data.success) {
        setAuditLogs(data.entries || []);
      }
    } catch (err) {
      setStatus(`audit logs 讀取失敗: ${err.message}`);
    }
  };

  const loadCacheAnomalies = async () => {
    if (!token) return;
    try {
      const { data } = await api("GET", `/api/admin/scheduling/cache-anomalies?limit=${Number(anomalyLimit)}`);
      if (data.success) {
        setCacheAnomalies(data.anomalies || []);
      }
    } catch (err) {
      setStatus(`cache anomalies 讀取失敗: ${err.message}`);
    }
  };

  const resetDashboard = () => {
    setTasks([]);
    setTaskTotal(0);
    setCacheMetrics({ total_completed_tasks: 0, total_cache_hits: 0, cache_hit_rate: 0, top_workers: [] });
    setCacheAlert({ severity: "normal", message: "Cache Hit Rate 在可接受範圍。", cache_hit_rate: 0 });
  };

  const alertStyle = cacheAlert.severity === "low"
    ? { padding: 8, background: "#fff3e0", borderLeft: "4px solid #e65100", borderRadius: 4 }
    : cacheAlert.severity === "high"
    ? { padding: 8, background: "#e8f5e9", borderLeft: "4px solid #2e7d32", borderRadius: 4 }
    : cacheAlert.severity === "normal"
    ? { padding: 8, background: "#e3f2fd", borderLeft: "4px solid #1565c0", borderRadius: 4 }
    : { padding: 8, background: "#ffebee", borderLeft: "4px solid #c62828", borderRadius: 4 };

  return (
    <div style={{ maxWidth: 960, margin: "0 auto", padding: 24, fontFamily: "system-ui, sans-serif" }}>
      <h1 style={{ margin: 0 }}>Hivemind Master</h1>

      {!token ? (
        <form onSubmit={login} style={{ marginTop: 16, display: "flex", flexDirection: "column", gap: 10, maxWidth: 320 }}>
          <input placeholder="username" value={username} onChange={(e) => setUsername(e.target.value)} />
          <input type="password" placeholder="password" value={password} onChange={(e) => setPassword(e.target.value)} />
          <button type="submit" disabled={loading}>{loading ? "登入中..." : "登入"}</button>
          <div style={{ fontSize: 13, color: "#666" }}>{status}</div>
        </form>
      ) : (
        <>
          <div style={{ marginTop: 8, fontSize: 13, color: "#888" }}>{status}</div>

          <div style={{ marginTop: 16, display: "flex", flexDirection: "column", gap: 10, maxWidth: 480 }}>
            <h3>建立任務</h3>
            <input placeholder="task_id" value={taskId} onChange={(e) => setTaskId(e.target.value)} />
            <input placeholder="magnet / btih" value={torrent} onChange={(e) => setTorrent(e.target.value)} />
            <input placeholder="zip_path (optional)" value={zipPath} onChange={(e) => setZipPath(e.target.value)} />
            <div style={{ display: "flex", gap: 8 }}>
              <input type="number" placeholder="記憶體 GB" value={memoryGb} onChange={(e) => setMemoryGb(e.target.value)} style={{ width: 100 }} />
              <input type="number" placeholder="GPU 記憶體 GB" value={gpuMemoryGb} onChange={(e) => setGpuMemoryGb(e.target.value)} style={{ width: 120 }} />
              <input type="number" placeholder="host count" value={hostCount} onChange={(e) => setHostCount(e.target.value)} style={{ width: 100 }} />
              <input type="number" placeholder="max CPT" value={maxCpt} onChange={(e) => setMaxCpt(e.target.value)} style={{ width: 100 }} />
            </div>
            <button onClick={createTask}>提交任務</button>
          </div>

          <div style={{ marginTop: 16 }}>
            <h3>排程觀測</h3>
            <div style={{ display: "flex", gap: 12, flexWrap: "wrap", marginBottom: 10 }}>
              <div>Completed: <strong>{cacheMetrics.total_completed_tasks}</strong></div>
              <div>Cache Hits: <strong>{cacheMetrics.total_cache_hits}</strong></div>
              <div>Hit Rate: <strong>{cacheMetrics.cache_hit_rate.toFixed(2)}</strong></div>
            </div>
            <div style={alertStyle}>
              <strong>告警:</strong> {cacheAlert.message}（rate={cacheAlert.cache_hit_rate.toFixed(2)}）
            </div>
            <div style={{ marginTop: 8, display: "flex", gap: 8, alignItems: "center" }}>
              <span>low threshold:</span>
              <input type="number" step="0.1" value={lowHitRateThreshold} onChange={(e) => setLowHitRateThreshold(e.target.value)} style={{ width: 80 }} />
              <span>high threshold:</span>
              <input type="number" step="0.1" value={highHitRateThreshold} onChange={(e) => setHighHitRateThreshold(e.target.value)} style={{ width: 80 }} />
            </div>
            <div style={{ marginTop: 8 }}>
              <strong>Top Workers by Cache Hits</strong>
              {cacheMetrics.top_workers.length === 0 ? (
                <p style={{ marginTop: 6 }}>尚無 cache 資料</p>
              ) : (
                <table style={{ borderCollapse: "collapse", width: "100%", marginTop: 6, background: "#fff" }}>
                  <thead>
                    <tr>
                      <th style={{ textAlign: "left", borderBottom: "1px solid #ddd", padding: 6 }}>Worker</th>
                      <th style={{ textAlign: "right", borderBottom: "1px solid #ddd", padding: 6 }}>Cache Hits</th>
                    </tr>
                  </thead>
                  <tbody>
                    {cacheMetrics.top_workers.map((w) => (
                      <tr key={w.worker_id}>
                        <td style={{ borderBottom: "1px solid #eee", padding: 6 }}>{w.worker_id}</td>
                        <td style={{ textAlign: "right", borderBottom: "1px solid #eee", padding: 6 }}>{w.cache_hits}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          </div>

          <div style={{ marginTop: 16 }}>
            <h3>Cache Anomalies</h3>
            <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 6 }}>
              <span>limit:</span>
              <input type="number" value={anomalyLimit} onChange={(e) => setAnomalyLimit(e.target.value)} style={{ width: 70 }} />
              <button onClick={loadCacheAnomalies}>刷新</button>
            </div>
            {cacheAnomalies.length === 0 ? (
              <p>尚無 anomalies</p>
            ) : (
              <table style={{ borderCollapse: "collapse", width: "100%", background: "#fff" }}>
                <thead>
                  <tr>
                    <th style={{ textAlign: "left", borderBottom: "1px solid #ddd", padding: 6 }}>Time</th>
                    <th style={{ textAlign: "left", borderBottom: "1px solid #ddd", padding: 6 }}>Severity</th>
                    <th style={{ textAlign: "left", borderBottom: "1px solid #ddd", padding: 6 }}>Message</th>
                  </tr>
                </thead>
                <tbody>
                  {cacheAnomalies.map((a, i) => (
                    <tr key={i}>
                      <td style={{ borderBottom: "1px solid #eee", padding: 6, fontSize: 12 }}>{a.created_at || ""}</td>
                      <td style={{ borderBottom: "1px solid #eee", padding: 6 }}>{a.severity || ""}</td>
                      <td style={{ borderBottom: "1px solid #eee", padding: 6, fontSize: 12 }}>{a.message || ""}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>

          <div style={{ marginTop: 16 }}>
            <h3>Admin Audit Logs</h3>
            <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 6 }}>
              <span>limit:</span>
              <input type="number" value={auditLimit} onChange={(e) => setAuditLimit(e.target.value)} style={{ width: 70 }} />
              <button onClick={loadAuditLogs}>刷新</button>
            </div>
            {auditLogs.length === 0 ? (
              <p>尚無 audit logs</p>
            ) : (
              <table style={{ borderCollapse: "collapse", width: "100%", background: "#fff" }}>
                <thead>
                  <tr>
                    <th style={{ textAlign: "left", borderBottom: "1px solid #ddd", padding: 6 }}>Time</th>
                    <th style={{ textAlign: "left", borderBottom: "1px solid #ddd", padding: 6 }}>Admin</th>
                    <th style={{ textAlign: "left", borderBottom: "1px solid #ddd", padding: 6 }}>Action</th>
                    <th style={{ textAlign: "left", borderBottom: "1px solid #ddd", padding: 6 }}>Target</th>
                  </tr>
                </thead>
                <tbody>
                  {auditLogs.map((entry, i) => (
                    <tr key={i}>
                      <td style={{ borderBottom: "1px solid #eee", padding: 6, fontSize: 12 }}>{entry.created_at || ""}</td>
                      <td style={{ borderBottom: "1px solid #eee", padding: 6 }}>{entry.admin_user || ""}</td>
                      <td style={{ borderBottom: "1px solid #eee", padding: 6 }}>{entry.action || ""}</td>
                      <td style={{ borderBottom: "1px solid #eee", padding: 6, fontSize: 12 }}>{entry.target_type}/{entry.target_id}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>

          <div style={{ marginTop: 16 }}>
            <h3>Worker Trust 管理</h3>
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
              <input
                value={trustWorkerId}
                onChange={(e) => setTrustWorkerId(e.target.value)}
                placeholder="worker_id"
                style={{ minWidth: 240 }}
              />
              <input
                type="number"
                value={trustScore}
                onChange={(e) => setTrustScore(e.target.value)}
                placeholder="score"
                style={{ width: 100 }}
              />
              <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <input
                  type="checkbox"
                  checked={trustBanned}
                  onChange={(e) => setTrustBanned(e.target.checked)}
                />
                banned
              </label>
              <button onClick={loadWorkerTrust}>讀取</button>
              <button onClick={loadWorkerTrustList}>列表</button>
              <button onClick={applyWorkerTrustControl}>套用</button>
            </div>
            <div style={{ marginTop: 10 }}>
              {trustProfile ? (
                <div style={{ fontSize: 13 }}>
                  <div>worker_id: <strong>{trustProfile.worker_id}</strong></div>
                  <div>score: <strong>{trustProfile.score}</strong></div>
                  <div>banned: <strong>{String(trustProfile.banned)}</strong></div>
                  <div>successful: <strong>{trustProfile.successful_tasks}</strong> / failed: <strong>{trustProfile.failed_tasks}</strong></div>
                </div>
              ) : (
                <div style={{ fontSize: 13, color: "#666" }}>尚未載入 worker trust</div>
              )}
            </div>
            <div style={{ marginTop: 12 }}>
              <strong>Worker Trust 列表</strong>
              {trustEntries.length === 0 ? (
                <p style={{ marginTop: 6 }}>目前沒有 trust 列表資料</p>
              ) : (
                <div style={{ overflowX: "auto", marginTop: 6 }}>
                  <table style={{ borderCollapse: "collapse", width: "100%", background: "#fff" }}>
                    <thead>
                      <tr>
                        <th style={{ textAlign: "left", borderBottom: "1px solid #ddd", padding: 6 }}>Worker</th>
                        <th style={{ textAlign: "left", borderBottom: "1px solid #ddd", padding: 6 }}>Owner</th>
                        <th style={{ textAlign: "left", borderBottom: "1px solid #ddd", padding: 6 }}>Status</th>
                        <th style={{ textAlign: "right", borderBottom: "1px solid #ddd", padding: 6 }}>Score</th>
                        <th style={{ textAlign: "center", borderBottom: "1px solid #ddd", padding: 6 }}>Banned</th>
                      </tr>
                    </thead>
                    <tbody>
                      {trustEntries.map((entry) => (
                        <tr
                          key={entry.worker_id}
                          onClick={() => {
                            setTrustWorkerId(entry.worker_id || "");
                            setTrustScore(Number(entry.score || 0));
                            setTrustBanned(Boolean(entry.banned));
                          }}
                          style={{ cursor: "pointer" }}
                        >
                          <td style={{ borderBottom: "1px solid #eee", padding: 6 }}>{entry.worker_id}</td>
                          <td style={{ borderBottom: "1px solid #eee", padding: 6 }}>{entry.username}</td>
                          <td style={{ borderBottom: "1px solid #eee", padding: 6 }}>{entry.worker_status}</td>
                          <td style={{ textAlign: "right", borderBottom: "1px solid #eee", padding: 6 }}>{entry.score}</td>
                          <td style={{ textAlign: "center", borderBottom: "1px solid #eee", padding: 6 }}>{String(entry.banned)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </div>
          </div>

          <div style={{ marginTop: 16 }}>
            <h3>任務列表</h3>
            {tasks.length === 0 ? (
              <p>目前沒有任務</p>
            ) : (
              <ul style={{ listStyle: "none", padding: 0, display: "grid", gap: 10 }}>
                {tasks.map((t) => {
                  const id = t.TaskID || t.task_id;
                  const st = t.Status || t.status;
                  const msg = t.StatusMessage || t.status_message || "";
                  const rt = Number(t.retry_count || 0);
                  const wt = Number(t.wall_time_ms || 0);
                  const pm = Number(t.peak_memory_mb || 0);
                  const ba = Number(t.billed_amount || 0);
                  const bs = t.billing_settled;
                  const dt = t.deterministic;
                  const statusColor = st === "COMPLETED" ? "#2e7d32" : st === "FAILED" ? "#c62828" : st === "RUNNING" ? "#1565c0" : "#666";
                  return (
                    <li key={id} style={{ border: "1px solid #ddd", borderRadius: 8, padding: 10 }}>
                      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                        <strong>{id}</strong>
                        <span style={{ fontSize: 12, color: statusColor, fontWeight: 600 }}>{st}</span>
                      </div>
                      <div style={{ fontSize: 12, color: "#555", marginTop: 4 }}>{msg}</div>
                      <div style={{ fontSize: 11, color: "#888", marginTop: 4, display: "flex", gap: 12 }}>
                        <span>wall: {(wt / 1000).toFixed(1)}s</span>
                        <span>mem: {pm}MB</span>
                        <span>billed: {ba} CPT {bs ? "(settled)" : "(pending)"}</span>
                        {rt > 0 && <span>retries: {rt}</span>}
                        {dt && <span style={{ color: "#7b1fa2" }}>deterministic</span>}
                      </div>
                      <div style={{ marginTop: 6, display: "flex", gap: 8 }}>
                        <button onClick={() => viewTaskLog(t)}>日誌</button>
                        <button onClick={() => viewTaskResult(t)}>結果</button>
                        <button onClick={() => stopTask(t)}>停止</button>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>

          <div style={{ marginTop: 16, padding: 12, background: "#fafafa", borderRadius: 8, border: "1px solid #ddd" }}>
            <h3>任務詳情 {selectedTask ? `(${selectedTask})` : ""}</h3>
            <div>
              <strong>日誌:</strong>
              <pre style={{ whiteSpace: "pre-wrap", background: "#fff", padding: 8, border: "1px solid #eee" }}>{taskLog || "(空)"}</pre>
            </div>
            <div>
              <strong>結果:</strong>
              <pre style={{ whiteSpace: "pre-wrap", background: "#fff", padding: 8, border: "1px solid #eee" }}>{taskResult || "(空)"}</pre>
            </div>
          </div>
        </>
      )}
    </div>
  );
}