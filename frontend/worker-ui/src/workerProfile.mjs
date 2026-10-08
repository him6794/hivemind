export const emptyProfile = {
  worker_id: '',
  ip: '',
  location: 'local',
  cpu_cores: 0,
  memory_gb: 0,
  cpu_score: null,
  gpu_score: null,
  gpu_memory_gb: 0,
  storage_total_gb: 0,
  storage_available_gb: 0,
  gpu_name: '',
};

export function toNumber(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

// Missing / empty / non-numeric values stay null instead of being coerced to
// 0, mirroring the dashboard's presence-aware handling. A real measured 0 is
// preserved; an absent value is not invented.
export function nullableNumber(value) {
  if (value === null || value === undefined || value === '') return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

// A performance score of 0 is the "not scored in this profile" placeholder, not
// a measured result — a real measured score is always positive, so 0
// unambiguously means "unmeasured." Treated as absent so the UI and the
// registration body do not present it as a real rating.
function nullableScore(value) {
  const parsed = nullableNumber(value);
  return parsed === null || parsed === 0 ? null : parsed;
}

export function normalizeWorkerProfile(next, fallbackIp = '') {
  return {
    worker_id: String(next.worker_id || next.workerId || ''),
    ip: String(next.ip || next.IP || fallbackIp),
    location: String(next.location || next.Location || 'local'),
    cpu_cores: toNumber(next.cpu_cores ?? next.cpuCores ?? 0),
    memory_gb: toNumber(next.memory_gb ?? next.memoryGb ?? 0),
    cpu_score: nullableScore(next.cpu_score ?? next.cpuScore),
    gpu_score: nullableScore(next.gpu_score ?? next.gpuScore),
    gpu_memory_gb: toNumber(next.gpu_memory_gb ?? next.gpuMemoryGb ?? 0),
    storage_total_gb: toNumber(next.storage_total_gb ?? next.storageTotalGb ?? 0),
    storage_available_gb: toNumber(next.storage_available_gb ?? next.storageAvailableGb ?? 0),
    gpu_name: String(next.gpu_name || next.gpuName || ''),
  };
}

function validateWorkerCapacity(values) {
  // null is "unmeasured/absent" and skips the non-negative check; a real value
  // (including a measured 0) must still be non-negative.
  for (const value of Object.values(values)) {
    if (value !== null && value < 0) {
      throw new Error('capacity values must be non-negative');
    }
  }
  if (!Number.isInteger(values.cpu_cores)) {
    throw new Error('cpu_cores must be an integer');
  }
  if (
    values.storage_available_gb !== null
    && values.storage_total_gb !== null
    && values.storage_available_gb > values.storage_total_gb
  ) {
    throw new Error('storage_available_gb cannot exceed storage_total_gb');
  }
}

export function buildRegisterWorkerBody(username, workerProfile, endpoint) {
  const workerId = String(workerProfile.worker_id || '').trim();
  // The callback endpoint is optional: workers that deliver results through
  // the outbound session register without one. A blank value registers the
  // worker as session-only instead of failing.
  const workerEndpoint = String(endpoint || '').trim();
  // Performance scores are a measurement, not capacity. An unmeasured score
  // stays null; a measured score must be non-negative like the other figures.
  const capacity = {
    cpu_cores: toNumber(workerProfile.cpu_cores),
    memory_gb: toNumber(workerProfile.memory_gb),
    cpu_score: nullableScore(workerProfile.cpu_score),
    gpu_score: nullableScore(workerProfile.gpu_score),
    gpu_memory_gb: toNumber(workerProfile.gpu_memory_gb),
    storage_total_gb: toNumber(workerProfile.storage_total_gb),
    storage_available_gb: toNumber(workerProfile.storage_available_gb),
  };
  validateWorkerCapacity(capacity);
  return {
    username: username.trim(),
    ...(workerId ? { worker_id: workerId } : {}),
    ...(workerEndpoint ? { ip: workerEndpoint } : {}),
    cpu_cores: capacity.cpu_cores,
    memory_gb: capacity.memory_gb,
    // cpu_score is a required scalar, so an unmeasured CPU registers as the
    // "not scored" 0; gpu_score is optional and omitted when unmeasured.
    cpu_score: capacity.cpu_score ?? 0,
    ...(capacity.gpu_score !== null ? { gpu_score: capacity.gpu_score } : {}),
    gpu_memory_gb: capacity.gpu_memory_gb,
    gpu_name: String(workerProfile.gpu_name || ''),
    storage_total_gb: capacity.storage_total_gb,
    storage_available_gb: capacity.storage_available_gb,
    location: workerProfile.location || 'local',
  };
}

export function buildRegisterWorkerRequest(workerControlBase, authToken, body) {
  const base = String(workerControlBase || '').trim().replace(/\/+$/, '');
  return {
    url: `${base}/api/register-worker`,
    options: {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${authToken}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(body),
    },
  };
}

export function registrationOwnerUsername(authenticatedUsername, formUsername) {
  return String(authenticatedUsername || formUsername || '').trim();
}
