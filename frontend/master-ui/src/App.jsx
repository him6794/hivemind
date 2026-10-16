import { useEffect, useRef, useState } from 'react';
import {
  Activity,
  CheckCircle2,
  CircleDollarSign,
  Download,
  FileText,
  LogOut,
  Moon,
  RefreshCw,
  Send,
  Sun,
  Workflow,
} from 'lucide-react';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Separator } from '@/components/ui/separator';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import { cn } from '@/lib/shadcn-utils';
import './console.css';
import { LocaleSelect, useLocale } from './LocaleProvider.jsx';
import { errorMessage, localeError, message } from './locale.mjs';
import { artifactFilenameFromContentDisposition } from './artifactDownloadPolicy.mjs';
import {
  clearStoredSession,
  readStoredSession,
  saveStoredSession,
  shouldLogoutForUnauthorizedRequest,
} from './authSession.mjs';
import { createTaskId, validateTaskId } from './taskIdPolicy.mjs';
import {
  isManagedGpuResult,
  isManagedLogGuidanceResult,
  taskListResponseError,
  taskResponseError,
} from './taskResponsePolicy.mjs';
import { normalizeTaskObservability } from './taskObservability.mjs';
import { createBalanceRefreshController, parseCptBalance } from './accountBalance.mjs';
import { readThemePreference, saveThemePreference } from './themePreference.mjs';
import {
  createRequestTimeout,
  createStartupStatusController,
  createTokenSetupController,
  getTaskListView,
} from './startupOrchestrator.mjs';

function toNumber(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

const SESSION_KEY = 'hivemind.master.session.v1';
const API_REQUEST_TIMEOUT_MS = 20_000;
const COLD_NETWORK_REQUEST_TIMEOUT_MS = 90_000;
const ARTIFACT_REQUEST_TIMEOUT_MS = 60_000;

function SummaryCard({ icon: Icon, label, value, caption }) {
  return (
    <Card className="summary-card">
      <CardContent className="summary-card-content">
        <div className="summary-card-label">
          <span>{label}</span>
          <Icon aria-hidden="true" className="size-4 text-muted-foreground" />
        </div>
        <div className="summary-card-value">{value}</div>
        <p className="summary-card-caption">{caption}</p>
      </CardContent>
    </Card>
  );
}

export default function MasterApp() {
  const { t } = useLocale();
  const apiBase = String(import.meta.env.VITE_API_BASE || '').trim().replace(/\/$/, '');

  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [token, setToken] = useState('');
  const [status, setStatus] = useState('Please log in to manage tasks');
  const [startupStatus, setStartupStatus] = useState({ state: 'checking', phase: 'starting' });
  const [sessionReady, setSessionReady] = useState(false);
  const [setupPhase, setSetupPhase] = useState('');
  const [setupErrorPhase, setSetupErrorPhase] = useState('');
  const [setupRetry, setSetupRetry] = useState(0);
  const [loadedTasksToken, setLoadedTasksToken] = useState('');
  const [tasksLoadError, setTasksLoadError] = useState('');
  const [loginLoading, setLoginLoading] = useState(false);
  const [submitLoading, setSubmitLoading] = useState(false);
  const [logLoading, setLogLoading] = useState(null);
  const [resultLoading, setResultLoading] = useState(null);
  const [cancelLoading, setCancelLoading] = useState(null);
  const [downloadLoading, setDownloadLoading] = useState(null);
  const [lastRefresh, setLastRefresh] = useState(null);
  const [balance, setBalance] = useState(null);
  const [balanceError, setBalanceError] = useState('');
  const [sourceError, setSourceError] = useState(null);
  const vpnReadyToken = useRef('');
  const tokenRef = useRef('');
  const balanceRefresh = useRef(null);
  const startupController = useRef(null);
  const setupController = useRef(null);
  const startupEffectEpoch = useRef(0);
  const sessionApplied = useRef(false);

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
  const [activeTab, setActiveTab] = useState('tasks');
  const [pendingCancel, setPendingCancel] = useState(null);
  const [theme, setTheme] = useState(readThemePreference);
  const cancelConfirmationInFlight = useRef(false);

  async function readJson(res) {
    const text = await res.text();
    if (!text) return {};
    try {
      return JSON.parse(text);
    } catch {
      return {};
    }
  }

  async function api(method, path, body, authToken = token, parentSignal, timeoutMs = API_REQUEST_TIMEOUT_MS) {
    const headers = {};
    if (authToken) headers.Authorization = `Bearer ${authToken}`;
    if (body !== undefined && !(body instanceof FormData)) {
      headers['Content-Type'] = 'application/json';
    }

    const request = createRequestTimeout(parentSignal, timeoutMs);
    let res;
    let data;
    try {
      res = await fetch(`${apiBase}${path}`, {
        method,
        headers,
        body: body instanceof FormData ? body : body !== undefined ? JSON.stringify(body) : undefined,
        signal: request.signal,
      });
      data = await readJson(res);
    } catch {
      if (request.didTimeout()) {
        throw localeError(method === 'GET'
          ? 'The request timed out. Please try again.'
          : 'The request timed out; its outcome may be unknown. It was not retried.');
      }
      if (parentSignal?.aborted) throw localeError('Request cancelled');
      throw localeError(method === 'GET'
        ? 'Hivemind could not be reached. Check your connection and try again.'
        : 'The connection dropped; the request outcome may be unknown. It was not retried.');
    } finally {
      request.dispose();
    }

    if (!res.ok) {
      if (res.status === 401) {
        if (path === '/api/login' && !authToken) {
          throw localeError('Username or password was not accepted.');
        }
        if (shouldLogoutForUnauthorizedRequest(authToken, tokenRef.current)) logout();
        throw localeError('Session expired. Please log in again.');
      }
      if (res.status >= 500) {
        throw localeError('Server error ({status}). Please try again later.', { status: res.status });
      }
    }
    return { ok: res.ok, status: res.status, data };
  }

  if (!startupController.current) {
    startupController.current = createStartupStatusController({
      checkStatus: async (signal) => {
        const response = await api('GET', '/api/startup-status', undefined, '', signal);
        if (!response.ok && response.status === 404) return { legacy: true };
        if (!response.ok) throw localeError('Startup status is not available yet');
        return response;
      },
      onState: setStartupStatus,
    });
  }

  async function refreshTasks(authToken = token, signal) {
    if (!authToken) return false;
    const { data } = await api('GET', '/api/tasks', undefined, authToken, signal);
    if (tokenRef.current !== authToken) return false;
    const failureMessage = taskListResponseError(data);
    if (failureMessage) throw failureMessage;

    setTasks(data.tasks);
    setLoadedTasksToken(authToken);
    setTasksLoadError('');
    return true;
  }

  function getBalanceRefreshController() {
    if (!balanceRefresh.current) {
      balanceRefresh.current = createBalanceRefreshController({
        loadBalance: async (authToken) => {
          const { ok, data } = await api('GET', '/api/balance', undefined, authToken);
          if (!ok || data.success === false) {
            throw new Error(data.message || data.status_message || 'Failed to load account balance');
          }
          return parseCptBalance(data);
        },
        getCurrentToken: () => tokenRef.current,
        setBalance,
        setError: setBalanceError,
      });
    }
    return balanceRefresh.current;
  }

  function refreshBalance(authToken = token) {
    if (!authToken) return Promise.resolve(false);
    return getBalanceRefreshController().refresh(authToken);
  }

  function refreshBalanceAfterSubmit(authToken) {
    return getBalanceRefreshController().refreshAfter(authToken);
  }

  async function bootstrapVpn(authToken = token, signal) {
    if (!authToken) throw localeError('Login is required before VPN bootstrap');
    if (vpnReadyToken.current === authToken) return { success: true, state: 'ready' };

    const { ok, data } = await api(
      'POST',
      '/api/vpn/bootstrap',
      undefined,
      authToken,
      signal,
      COLD_NETWORK_REQUEST_TIMEOUT_MS,
    );
    const state = String(data.state || '').trim();
    if (!ok || !data.success || !['ready', 'disabled'].includes(state)) {
      throw localeError('The network is not ready. Please try again.');
    }
    if (signal?.aborted || tokenRef.current !== authToken) throw localeError('Request cancelled');
    vpnReadyToken.current = authToken;
    return data;
  }

  if (!setupController.current) {
    setupController.current = createTokenSetupController({
      bootstrapVpn,
      loadTasks: refreshTasks,
      getCurrentToken: () => tokenRef.current,
      onPhase: ({ token: activeToken, phase }) => {
        if (tokenRef.current !== activeToken) return;
        setSetupErrorPhase('');
        setSetupPhase(phase);
        setStatus(phase === 'connecting' ? 'Connecting to the network' : 'Loading your tasks');
        if (phase === 'tasks') setTasksLoadError('');
      },
      onSuccess: ({ token: activeToken }) => {
        if (tokenRef.current !== activeToken) return;
        setSetupPhase('');
        setSetupErrorPhase('');
        setStatus('Signed in and ready');
        setLastRefresh(Date.now());
      },
      onError: ({ token: activeToken, phase, error }) => {
        if (tokenRef.current !== activeToken) return;
        setSetupPhase('');
        setSetupErrorPhase(phase);
        setTasksLoadError(errorMessage(error, phase === 'tasks' ? 'Failed to load tasks' : 'Network connection failed'));
        setStatus(message('Connection failed: {detail}', { detail: errorMessage(error, 'Please try again.') }));
      },
    });
  }

  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle('dark', theme === 'dark');
    root.style.colorScheme = theme;
    saveThemePreference(theme);
  }, [theme]);

  useEffect(() => {
    document.getElementById('startup-fallback')?.remove();
    const epoch = ++startupEffectEpoch.current;
    void startupController.current.start();
    return () => {
      queueMicrotask(() => {
        if (startupEffectEpoch.current === epoch) startupController.current?.cancel();
      });
    };
  }, []);

  useEffect(() => {
    if (startupStatus.state !== 'ready' || sessionApplied.current) return;
    sessionApplied.current = true;
    const savedSession = readStoredSession(window.sessionStorage, SESSION_KEY);
    tokenRef.current = savedSession.token;
    setUsername(savedSession.username);
    setToken(savedSession.token);
    setStatus(savedSession.token ? 'Session restored' : 'Please log in to manage tasks');
    setSessionReady(true);
  }, [startupStatus.state]);

  useEffect(() => {
    if (!token) return undefined;
    const setup = setupController.current.acquire(token);
    return () => setup.release();
  }, [token, setupRetry]);

  useEffect(() => {
    if (!token || loadedTasksToken !== token) return undefined;
    const id = setInterval(() => {
      if (tokenRef.current !== token || vpnReadyToken.current !== token) return;
      refreshTasks(token).then((loaded) => {
        if (loaded && tokenRef.current === token) setLastRefresh(Date.now());
      }).catch((error) => {
        if (tokenRef.current === token) {
          setTasksLoadError(errorMessage(error, 'Failed to load tasks'));
        }
      });
    }, 5000);
    return () => clearInterval(id);
  }, [token, loadedTasksToken]);

  useEffect(() => {
    if (!token) {
      setBalance(null);
      setBalanceError('');
      return undefined;
    }

    let cancelled = false;
    const loadBalance = () => {
      if (!cancelled) void refreshBalance(token);
    };
    loadBalance();
    const id = setInterval(loadBalance, 30000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [token]);

  async function handleLogin(e) {
    e.preventDefault();
    setLoginLoading(true);
    setStatus('Signing in and connecting');
    setupController.current?.cancel();
    vpnReadyToken.current = '';
    tokenRef.current = '';
    balanceRefresh.current?.clear();
    setToken('');
    setTasks([]);
    setLoadedTasksToken('');
    setTasksLoadError('');
    setSetupPhase('');
    setSetupErrorPhase('');
    setBalance(null);
    setBalanceError('');

    try {
      const { data } = await api(
        'POST',
        '/api/login',
        { username, password },
        '',
        undefined,
        COLD_NETWORK_REQUEST_TIMEOUT_MS,
      );
      if (!data.success || !data.token) {
        throw taskResponseError(data, 'Login failed', false);
      }

      const ownerUsername = username.trim();
      // The bearer JWT lives only in tab session storage: closing the Master
      // console discards it, so no reusable credential persists in the
      // browser profile.
      saveStoredSession(window.sessionStorage, SESSION_KEY, {
        token: data.token,
        username: ownerUsername,
      });
      tokenRef.current = data.token;
      setToken(data.token);
      setUsername(ownerUsername);
    } catch (err) {
      setStatus(message('Login failed: {detail}', { detail: errorMessage(err) }));
    } finally {
      setLoginLoading(false);
    }
  }

  function retrySetup() {
    setSetupErrorPhase('');
    setTasksLoadError('');
    setSetupRetry((attempt) => attempt + 1);
  }

  async function retryTaskRefresh() {
    if (!token) return;
    setTasksLoadError('');
    setStatus('Loading your tasks');
    try {
      await refreshTasks(token);
      if (tokenRef.current !== token) return;
      setLastRefresh(Date.now());
      setStatus('Tasks refreshed');
    } catch (error) {
      if (tokenRef.current !== token) return;
      setTasksLoadError(errorMessage(error, 'Failed to load tasks'));
      setStatus(message('Refresh failed: {detail}', { detail: errorMessage(error, 'Failed to load tasks') }));
    }
  }

  async function submitTask() {
    if (!token || loadedTasksToken !== token || !taskSource.trim()) return;
    setSubmitLoading(true);
    setStatus('Submitting task...');

    try {
      if (!taskSource.trim()) {
        throw localeError('Task instructions are required');
      }
      if (!taskInput.trim()) {
        throw localeError('Input JSON is required');
      }
      try {
        JSON.parse(taskInput);
      } catch {
        throw localeError('Input must be valid JSON');
      }
      if (toNumber(maxCpt) <= 0) {
        throw localeError('Maximum total charge (CPT, including platform fee) must be greater than 0 for managed-function tasks');
      }

      const effectiveTaskId = taskId.trim() || createTaskId();
      if (!effectiveTaskId) {
        throw localeError('task_id is required');
      }
      const validatedTaskId = validateTaskId(effectiveTaskId);
      if (!validatedTaskId.ok) {
        throw localeError(validatedTaskId.message);
      }

      const body = {
        task_id: validatedTaskId.taskId,
        runtime: 'hcode',
        task_source: taskSource,
      };
      // hcode tasks carry their JSON input in the `torrent` field.
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
        throw taskResponseError(data, 'Task submission failed', false);
      }

      setTaskId('');
      setTaskSource('');
      setTaskInput('');
      setSourceError(null);
      setStatus(message('Task submitted: {id}', { id: validatedTaskId.taskId }));
      void refreshBalanceAfterSubmit(token);
      if (tokenRef.current === token) {
        await refreshTasks();
        setLastRefresh(Date.now());
      }
    } catch (err) {
      setStatus(message('Submission failed: {detail}', { detail: errorMessage(err) }));
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
      setTaskLog(message(validatedTaskId.message));
      setLogLoading(null);
      return;
    }
    const id = validatedTaskId.taskId;

    try {
      const { ok, data } = await api('GET', `/api/tasks/${encodeURIComponent(id)}/log`);
      const failureMessage = taskResponseError(data, 'Log unavailable', ok);
      if (failureMessage) {
        throw failureMessage;
      }
      setTaskLog(data.log ? { raw: data.log } : message('(No output yet)'));
    } catch (err) {
      setTaskLog(message('{operation} failed: {detail}', { operation: message('Log'), detail: errorMessage(err, 'Log unavailable') }));
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
      setTaskResult(message(validatedTaskId.message));
      setResultLoading(null);
      return;
    }
    const id = validatedTaskId.taskId;

    try {
      const { ok, data } = await api('GET', `/api/tasks/${encodeURIComponent(id)}/result`);
      if (ok && isManagedGpuResult(data)) {
        // GPU-v1 results are Nodepool-validated typed JSON, never torrents or
        // managed log guidance. Render the envelope for both success and failure.
        setTaskResult({ raw: JSON.stringify(data.managed_gpu_result, null, 2) });
        setSelectedTask(id);
        return;
      }
      // Managed tasks deliberately persist output as a task log instead of a
      // legacy result torrent; the endpoint reports that with success=false
      // plus guidance. Surface the log inline rather than showing the
      // contract message as an error.
      if (ok && isManagedLogGuidanceResult(data)) {
        const { ok: logOk, data: logData } = await api(
          'GET',
          `/api/tasks/${encodeURIComponent(id)}/log`
        );
        const logFailure = taskResponseError(
          logData,
          'Managed output unavailable',
          logOk
        );
        if (logFailure) {
          throw logFailure;
        }
        const output = String(logData?.log ?? '');
        setTaskResult(
          message('{output}\n\n— Managed task: output is served from the task log above.', { output: output ? { raw: output } : message('(No managed output yet)') })
        );
        setSelectedTask(id);
        return;
      }
      const failureMessage = taskResponseError(data, 'Result unavailable', ok);
      if (failureMessage) {
        throw failureMessage;
      }
      setTaskResult({ raw: JSON.stringify(data, null, 2) });
    } catch (err) {
      setTaskResult(message('{operation} failed: {detail}', { operation: message('Result'), detail: errorMessage(err, 'Result unavailable') }));
    } finally {
      setResultLoading(null);
    }
    setSelectedTask(id);
  }

  async function cancelTask(task) {
    if (!token) return;
    const rawId = task?.task_id || task?.TaskID || '';
    if (!String(rawId).trim()) return;
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
      const failureMessage = taskResponseError(data, 'Task cancellation was rejected', ok);
      if (failureMessage) {
        throw failureMessage;
      }
      await refreshTasks();
      setLastRefresh(Date.now());
      setStatus(message('Task cancelled: {id}', { id }));
    } catch (err) {
      setStatus(message('Cancel failed: {detail}', { detail: errorMessage(err) }));
    } finally {
      setCancelLoading(null);
    }
  }

  function requestTaskCancellation(task) {
    if (!token) return;
    const rawId = task?.task_id || task?.TaskID || '';
    if (!String(rawId).trim()) return;
    setPendingCancel({ id: String(rawId), task });
  }

  async function confirmTaskCancellation() {
    const pending = pendingCancel;
    if (!pending || !token || cancelConfirmationInFlight.current) return;
    cancelConfirmationInFlight.current = true;
    setPendingCancel(null);
    try {
      await cancelTask(pending.task);
    } finally {
      cancelConfirmationInFlight.current = false;
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

    const request = createRequestTimeout(undefined, ARTIFACT_REQUEST_TIMEOUT_MS);
    try {
      const res = await fetch(`${apiBase}/api/tasks/${encodeURIComponent(id)}/artifact/download`, {
        headers: { Authorization: `Bearer ${token}` },
        signal: request.signal,
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
      setStatus(message('Artifact downloaded: {filename}', { filename }));
    } catch (err) {
      const detail = request.didTimeout() ? message('The download timed out. Please try again.') : errorMessage(err);
      setStatus(message('Download failed: {detail}', { detail }));
    } finally {
      request.dispose();
      setDownloadLoading(null);
    }
  }

  function logout() {
    clearStoredSession(window.sessionStorage, SESSION_KEY);
    setupController.current?.cancel();
    vpnReadyToken.current = '';
    tokenRef.current = '';
    balanceRefresh.current?.clear();
    setToken('');
    setBalance(null);
    setBalanceError('');
    setTasks([]);
    setLoadedTasksToken('');
    setTasksLoadError('');
    setSetupPhase('');
    setSetupErrorPhase('');
    setSelectedTask('');
    setTaskLog('');
    setTaskResult('');
    setPendingCancel(null);
    setStatus('Please log in to manage tasks');
    setTaskSource('');
    setTaskInput('');
    setSourceError(null);
    setLastRefresh(null);
  }

  function isTerminalStatus(statusText) {
    const normalized = String(statusText || '').toUpperCase();
    return normalized === 'COMPLETED' || normalized === 'FAILED' || normalized === 'CANCELLED';
  }

  const taskSnapshotReady = !!token && loadedTasksToken === token;
  const taskSummary = taskSnapshotReady
    ? tasks.reduce((summary, task) => {
        const status = String(task.status || task.Status || '').toUpperCase();
        if (!isTerminalStatus(status)) summary.open += 1;
        if (status === 'COMPLETED') summary.completed += 1;
        return summary;
      }, { open: 0, completed: 0 })
    : null;

  const taskListView = getTaskListView({
    tasks,
    initialLoading: !!token && loadedTasksToken !== token && !tasksLoadError,
    error: tasksLoadError,
  });

  const startupBusy = startupStatus.state === 'checking' || (startupStatus.state === 'ready' && !sessionReady);
  const phaseLabels = {
    starting: 'Preparing the console',
    connecting: 'Preparing network services',
    resources: 'Loading runtime resources',
    services: 'Starting services',
    ready: 'Preparing your session',
  };
  const startupLabel = startupStatus.state === 'failed'
    ? message('Hivemind could not start{code}. Close and reopen the application.', { code: startupStatus.code ? ` (${startupStatus.code})` : '' })
    : startupStatus.state === 'error'
      ? startupStatus.message
      : !sessionReady && startupStatus.state === 'ready'
        ? phaseLabels.ready
        : phaseLabels[startupStatus.phase] || phaseLabels.starting;

  if (startupStatus.state !== 'ready' || !sessionReady) {
    return (
      <main className="startup-screen">
        <Card className="startup-card" aria-busy={startupBusy}>
          <CardContent className="startup-content">
            <span className="startup-icon" aria-hidden="true"><Workflow /></span>
            <p className="eyebrow">Hivemind</p>
            <h1>{t("Master console")}</h1>
            <LocaleSelect />
            <p className="startup-message" role="status" aria-live="polite">{t(startupLabel)}</p>
            {startupBusy ? <Skeleton className="h-2 w-20" aria-hidden="true" /> : null}
            {startupStatus.state === 'error' ? (
              <Button type="button" onClick={() => void startupController.current.start()}>{t("Check again")}</Button>
            ) : null}
          </CardContent>
        </Card>
      </main>
    );
  }

  const isStatusError = /failed|expired/i.test(typeof status === 'string' ? status : status.key);
  const showStatus = !!token || loginLoading || isStatusError;
  const taskCountsCaption = tasksLoadError
    ? 'Last loaded snapshot'
    : taskSnapshotReady
      ? 'From the latest task list'
      : 'Loading task list';

  return (
    <main className="app-shell">
      <div className="app-container">
        <header className="app-header">
          <div className="brand-lockup">
            <span className="brand-mark" aria-hidden="true"><Workflow /></span>
            <div>
              <p className="eyebrow">Hivemind</p>
              <h1>{t("Master console")}</h1>
            </div>
          </div>
          <div className="header-actions">
            <LocaleSelect />
            <Button
              type="button"
              variant="outline"
              size="icon"
              aria-label={t(theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme')}
              title={t(theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme')}
              onClick={() => setTheme((current) => current === 'dark' ? 'light' : 'dark')}
            >
              {theme === 'dark' ? <Sun aria-hidden="true" /> : <Moon aria-hidden="true" />}
            </Button>
            {token ? (
              <Button type="button" variant="outline" onClick={logout}>
                <LogOut aria-hidden="true" />{t("Sign out")}</Button>
            ) : null}
          </div>
        </header>

        {!token ? (
          <Card className="login-card">
            <CardHeader className="compact-card-header">
              <CardTitle className="section-title" role="heading" aria-level="2">{t("Sign in")}</CardTitle>
              <CardDescription>{t("Connect your Master console to your Hivemind account.")}</CardDescription>
            </CardHeader>
            <CardContent>
              <form onSubmit={handleLogin} className="login-form">
                <div className="field-stack">
                  <Label htmlFor="master-username">{t("Username")}</Label>
                  <Input
                    id="master-username"
                    autoComplete="username"
                    value={username}
                    onChange={(event) => setUsername(event.target.value)}
                  />
                </div>
                <div className="field-stack">
                  <Label htmlFor="master-password">{t("Password")}</Label>
                  <Input
                    id="master-password"
                    type="password"
                    autoComplete="current-password"
                    value={password}
                    onChange={(event) => setPassword(event.target.value)}
                  />
                </div>
                <Button type="submit" className="login-submit" disabled={loginLoading}>
                  {loginLoading ? <RefreshCw className="animate-spin" aria-hidden="true" /> : null}
                  {loginLoading ? t("Signing in and connecting…") : t("Sign in")}
                </Button>
              </form>
            </CardContent>
          </Card>
        ) : (
          <>
            <Card className="account-strip">
              <CardContent className="account-strip-content">
                <div className="account-identity">
                  <span className="account-indicator" aria-hidden="true" />
                  <div>
                    <span className="account-label">{t("Signed in")}</span>
                    <strong>{username}</strong>
                  </div>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void retryTaskRefresh()}
                  disabled={setupPhase !== '' || loadedTasksToken !== token}
                >
                  <RefreshCw aria-hidden="true" />{t("Refresh tasks")}</Button>
              </CardContent>
            </Card>

            <section className="summary-grid" aria-label={t("Account and task overview")}>
              <SummaryCard
                icon={CircleDollarSign}
                label={t("CPT balance")}
                value={balance === null
                  ? balanceError ? t("Unavailable") : <Skeleton className="h-7 w-28" aria-label={t("Loading balance")} />
                  : `${balance.toFixed(2)} CPT`}
                caption={balanceError
                  ? balance === null ? t("Could not load account balance") : t("Showing last received value")
                  : t("Current account balance")}
              />
              <SummaryCard
                icon={Activity}
                label={t("Open tasks")}
                value={taskSummary ? taskSummary.open : '—'}
                caption={t(taskCountsCaption)}
              />
              <SummaryCard
                icon={CheckCircle2}
                label={t("Completed tasks")}
                value={taskSummary ? taskSummary.completed : '—'}
                caption={t(taskCountsCaption)}
              />
            </section>

            {balanceError && balance !== null ? (
              <p className="balance-note" role="note">{t("Could not refresh the balance; showing the last received value.")}</p>
            ) : null}
          </>
        )}

        {showStatus ? (
          <div
            className={cn('status-banner', isStatusError && 'status-banner-error')}
            role="status"
            aria-live="polite"
          >
            <span>{t(status)}</span>
            {token && setupErrorPhase === 'connecting' ? (
              <Button type="button" variant="outline" size="sm" onClick={retrySetup}>{t("Retry connection")}</Button>
            ) : null}
          </div>
        ) : null}

        {token ? (
          <Tabs value={activeTab} onValueChange={setActiveTab} className="workspace-tabs">
            <TabsList className="workspace-tabs-list" aria-label={t("Master workspace")}>
              <TabsTrigger value="tasks"><FileText aria-hidden="true" />{t("Tasks")}</TabsTrigger>
              <TabsTrigger value="submit"><Send aria-hidden="true" />{t("Submit")}</TabsTrigger>
            </TabsList>

            <TabsContent value="tasks" className="workspace-panel animate-in fade-in-0 slide-in-from-bottom-1 duration-200">
              <Card className="tasks-card" aria-busy={taskListView === 'loading'}>
                <CardHeader className="section-card-header">
                  <div>
                    <CardTitle className="section-title" role="heading" aria-level="2">{t("Your tasks")}</CardTitle>
                    <CardDescription>
                      {lastRefresh
                        ? t('Updated {seconds}s ago', { seconds: Math.max(0, Math.round((Date.now() - lastRefresh) / 1000)) })
                        : t("Task status, output, and settled usage")}
                    </CardDescription>
                  </div>
                  <Badge variant="outline">
                    {taskSnapshotReady ? t(tasks.length === 1 ? '{count} task' : '{count} tasks', { count: tasks.length }) : t("Task list")}
                  </Badge>
                </CardHeader>
                <CardContent className="task-content">
                  {taskListView === 'loading' ? (
                    <div className="task-skeleton" aria-label={t("Loading tasks")} role="status">
                      <Skeleton className="h-28 w-full" />
                      <Skeleton className="h-28 w-full" />
                    </div>
                  ) : taskListView === 'error' ? (
                    <div className="empty-state" role="alert">
                      <p>{tasksLoadError ? t(tasksLoadError) : t("Tasks could not be loaded.")}</p>
                      {setupErrorPhase === 'connecting' ? null : (
                        <Button type="button" variant="outline" onClick={() => void retryTaskRefresh()} disabled={setupPhase !== ''}>
                          <RefreshCw aria-hidden="true" />{t("Retry task loading")}</Button>
                      )}
                    </div>
                  ) : taskListView === 'empty' ? (
                    <div className="empty-state">
                      <span className="empty-state-icon" aria-hidden="true"><FileText /></span>
                      <p>{t("No tasks yet.")}</p>
                      <Button type="button" variant="outline" onClick={() => setActiveTab('submit')}>
                        <Send aria-hidden="true" />{t("Submit a task")}</Button>
                    </div>
                  ) : (
                    <>
                      {tasksLoadError ? (
                        <div className="task-refresh-note" role="note">
                          <span>{t(tasksLoadError)}{' '}{t("Showing the last received task list.")}</span>
                          <Button type="button" variant="outline" size="sm" onClick={() => void retryTaskRefresh()} disabled={setupPhase !== ''}>{t("Retry")}</Button>
                        </div>
                      ) : null}
                      <ul className="task-list">
                        {tasks.map((task, index) => {
                          const id = task.task_id || task.TaskID || '';
                          const statusText = String(task.status || task.Status || '').trim();
                          const normalizedStatus = statusText.toUpperCase();
                          const message = task.status_message || task.StatusMessage || '';
                          const runtime = String(task.runtime || task.runtime_version || task.Runtime || '').trim();
                          const isManagedTask = runtime === 'managed-function-v0' || runtime === 'hcode';
                          const wallTimeValue = task.wall_time_ms ?? task.WallTimeMs;
                          const wallTimeMs = Number(wallTimeValue);
                          const hasWallTime = wallTimeValue !== null && wallTimeValue !== undefined && wallTimeValue !== '' && Number.isFinite(wallTimeMs);
                          const observability = normalizeTaskObservability(task);
                          const historicalOverCap = observability.historicalOverCap;
                          const terminal = isTerminalStatus(statusText);
                          const statusVariant = normalizedStatus === 'FAILED' || normalizedStatus === 'CANCELLED'
                            ? 'destructive'
                            : terminal ? 'outline' : 'secondary';
                          const isLogLoading = logLoading === id;
                          const isResultLoading = resultLoading === id;
                          const isCancelLoading = cancelLoading === id;
                          const isDownloadLoading = downloadLoading === id;

                          return (
                            <li key={id || `task-${index}`}>
                              <Card className="task-row">
                                <CardContent className="task-row-content">
                                  <div className="task-row-heading">
                                    <strong className="task-id">{id || t("Task ID unavailable")}</strong>
                                    <Badge variant={statusVariant}>{t(statusText || 'Unknown status')}</Badge>
                                  </div>
                                  {message ? <p className="task-message">{message}</p> : null}
                                  <div className="task-meta">
                                    <span>{t('Wall {time}', { time: hasWallTime ? `${(wallTimeMs / 1000).toFixed(1)}s` : '—' })}</span>
                                    <span>{observability.billingSettled
                                      ? observability.billedAmount === null
                                        ? t("Charge settled; amount unknown")
                                        : t('Charged {amount} CPT{note}', { amount: observability.billedAmount, note: t(historicalOverCap ? ' (above recorded max_cpt; fee included)' : ' (fee included)') })
                                      : t("Charge pending")}</span>
                                    {observability.retryCount === null
                                      ? <span>{t("Retry counter unavailable")}</span>
                                      : observability.retryCount > 0
                                        ? <span>{t('Retry counter {count}', { count: observability.retryCount })}</span>
                                        : null}
                                  </div>
                                  <Separator className="task-separator" />
                                  <dl className="observability-grid">
                                    <dt>{t("Computer")}</dt>
                                    <dd>{observability.workerId || t("Not selected yet")}</dd>
                                    <dt>{t("Shared by")}</dt>
                                    <dd>{observability.providerUser || t("Not available yet")}</dd>
                                    <dt>{t("Progress")}</dt>
                                    <dd>{t(observability.dispatchStatus)}</dd>
                                    <dt>{t("Aggregate replica usage")}</dt>
                                    <dd>{observability.usageUnits === null ? t("Unknown") : t('{usage} CPT before fee', { usage: observability.usageUnits })}</dd>
                                    <dt>{runtime === 'hcode' ? t("Submitted max_cpt") : t("Task charge cap")}</dt>
                                    <dd>{observability.chargeCapCpt === null ? '—' : observability.chargeCapCpt} CPT{runtime === 'hcode' ? t(" (hcode: task-wide, fee included)") : t(", fee included")}</dd>
                                    <dt>{t("Settled charge-cap remainder")}</dt>
                                    <dd>
                                      {observability.settledRemainderCpt === null
                                        ? observability.billingSettled ? t("Unknown") : t("Available after settlement")
                                        : `${observability.settledRemainderCpt} CPT`}
                                    </dd>
                                  </dl>
                                  {historicalOverCap ? (
                                    <p className="task-note" role="note">{t("This charge exceeds the recorded max_cpt. Earlier v1 billing used max_cpt per replica, but this view does not show the billing version; review the task ledger before attributing the difference. Today’s task-wide cap is not retroactive.")}</p>
                                  ) : null}
                                  <div className="task-actions">
                                    <Button type="button" variant="outline" size="sm" onClick={() => void viewTaskLog(task)} disabled={isLogLoading}>
                                      <FileText aria-hidden="true" />{isLogLoading ? t("Loading…") : t("Log")}
                                    </Button>
                                    {isManagedTask ? (
                                      <span className="task-output-note">{t("Output in Log")}</span>
                                    ) : (
                                      <Button type="button" variant="outline" size="sm" onClick={() => void viewTaskResult(task)} disabled={isResultLoading}>
                                        {isResultLoading ? t("Loading…") : t("Result")}
                                      </Button>
                                    )}
                                    <Button type="button" variant="outline" size="sm" onClick={() => void downloadArtifact(task)} disabled={isDownloadLoading}>
                                      <Download aria-hidden="true" />{isDownloadLoading ? t("Downloading…") : t("Download")}
                                    </Button>
                                    <Button type="button" variant="destructive" size="sm" onClick={() => requestTaskCancellation(task)} disabled={isCancelLoading || terminal}>
                                      {isCancelLoading ? t("Cancelling…") : t("Cancel")}
                                    </Button>
                                  </div>
                                </CardContent>
                              </Card>
                            </li>
                          );
                        })}
                      </ul>
                    </>
                  )}
                </CardContent>
              </Card>

              <Card className="detail-card">
                <CardHeader className="section-card-header">
                  <div>
                    <CardTitle className="section-title" role="heading" aria-level="2">{t("Task detail")}</CardTitle>
                    <CardDescription>{selectedTask || t("Select a task to inspect its output")}</CardDescription>
                  </div>
                </CardHeader>
                <CardContent className="detail-grid">
                  <section className="detail-pane" aria-label={t("Task log")}>
                    <h3><FileText aria-hidden="true" />{t("Log")}</h3>
                    <pre>{logLoading ? t("Loading log…") : t(taskLog || '(No output yet)')}</pre>
                  </section>
                  <section className="detail-pane" aria-label={t("Task result")}>
                    <h3><Activity aria-hidden="true" />{t("Result")}</h3>
                    <pre>{resultLoading ? t("Loading result…") : t(taskResult || '(No result yet)')}</pre>
                  </section>
                </CardContent>
              </Card>
            </TabsContent>

            <TabsContent value="submit" className="workspace-panel animate-in fade-in-0 slide-in-from-bottom-1 duration-200">
              <Card className="submit-card">
                <CardHeader className="section-card-header">
                  <div>
                    <CardTitle className="section-title" role="heading" aria-level="2">{t("Submit a task")}</CardTitle>
                    <CardDescription>{t("hcode runs as a deterministic, replicated task.")}</CardDescription>
                  </div>
                </CardHeader>
                <CardContent>
                  <form className="submit-form" onSubmit={(event) => { event.preventDefault(); void submitTask(); }}>
                    <div className="field-stack">
                      <Label htmlFor="task-id">{t("Task ID")}</Label>
                      <Input
                        id="task-id"
                        value={taskId}
                        onChange={(event) => setTaskId(event.target.value)}
                        placeholder={t("Leave blank to assign an ID")}
                      />
                    </div>
                    <div className="field-stack">
                      <Label htmlFor="task-source">{t("Task instructions")}</Label>
                      <Textarea
                        id="task-source"
                        value={taskSource}
                        onChange={(event) => {
                          setTaskSource(event.target.value);
                          setSourceError(event.target.value.trim() ? null : 'Task instructions are required');
                        }}
                        placeholder={t("Describe the small, self-contained task to run")}
                        rows={8}
                        aria-invalid={!!sourceError}
                        aria-describedby={sourceError ? 'task-source-error' : undefined}
                        className={cn(sourceError && 'border-destructive focus-visible:ring-destructive')}
                      />
                      {sourceError ? <p id="task-source-error" className="field-error" role="alert">{t(sourceError)}</p> : null}
                    </div>
                    <div className="field-stack">
                      <Label htmlFor="task-input">{t("Input data (JSON)")}</Label>
                      <Textarea
                        id="task-input"
                        value={taskInput}
                        onChange={(event) => setTaskInput(event.target.value)}
                        placeholder={t("Required, e.g. {\"n\": 42}")}
                        rows={5}
                        spellCheck={false}
                      />
                    </div>

                    <div className="resource-fields">
                      <div className="field-stack">
                        <Label htmlFor="cpu-score">{t("CPU score")}</Label>
                        <Input id="cpu-score" type="number" min="0" value={cpuScore} onChange={(event) => setCpuScore(event.target.value)} />
                      </div>
                      <div className="field-stack">
                        <Label htmlFor="gpu-score">{t("GPU score")}</Label>
                        <Input id="gpu-score" type="number" min="0" value={gpuScore} onChange={(event) => setGpuScore(event.target.value)} />
                      </div>
                      <div className="field-stack">
                        <Label htmlFor="memory-gb">{t("Memory GB")}</Label>
                        <Input id="memory-gb" type="number" min="0" value={memoryGb} onChange={(event) => setMemoryGb(event.target.value)} />
                      </div>
                      <div className="field-stack">
                        <Label htmlFor="gpu-memory-gb">{t("GPU memory GB")}</Label>
                        <Input id="gpu-memory-gb" type="number" min="0" value={gpuMemoryGb} onChange={(event) => setGpuMemoryGb(event.target.value)} />
                      </div>
                      <div className="field-stack">
                        <Label htmlFor="storage-gb">{t("Storage GB")}</Label>
                        <Input id="storage-gb" type="number" min="0" value={storageGb} onChange={(event) => setStorageGb(event.target.value)} />
                      </div>
                      <div className="field-stack">
                        <Label htmlFor="host-count">{t("Host count")}</Label>
                        <Input id="host-count" type="number" min="1" value={hostCount} onChange={(event) => setHostCount(event.target.value)} />
                      </div>
                      <div className="field-stack charge-cap-field">
                        <Label htmlFor="max-cpt">{t("Task charge cap (CPT, fee included)")}</Label>
                        <Input id="max-cpt" type="number" min="0" value={maxCpt} onChange={(event) => setMaxCpt(event.target.value)} />
                      </div>
                    </div>
                    <p className="form-note">{t("Three Worker replicas share this task-wide cap through a deterministic split. It includes the platform fee; automatic paid retries are disabled until a funded platform treasury is available, so retry work is not charged to you.")}</p>
                    <div className="submit-actions">
                      <Button type="submit" disabled={submitLoading || loadedTasksToken !== token || !taskSource.trim() || !!sourceError}>
                        <Send aria-hidden="true" />{submitLoading ? t("Sending…") : t("Send task")}
                      </Button>
                    </div>
                  </form>
                </CardContent>
              </Card>
            </TabsContent>
          </Tabs>
        ) : null}

        <AlertDialog
          open={!!pendingCancel}
          onOpenChange={(open) => { if (!open) setPendingCancel(null); }}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>{t("Cancel this task?")}</AlertDialogTitle>
              <AlertDialogDescription>{t("A stop request will be sent for")}{' '}<strong className="dialog-task-id">{pendingCancel?.id}</strong>{t(". This cannot be undone.")}</AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>{t("Keep task")}</AlertDialogCancel>
              <AlertDialogAction onClick={() => void confirmTaskCancellation()}>{t("Cancel task")}</AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </div>
    </main>
  );
}
