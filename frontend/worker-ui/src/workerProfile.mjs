export const emptyProfile = {
  worker_id: '',
  ip: '',
  location: 'local',
  cpu_cores: 0,
  memory_gb: 0,
  cpu_score: 0,
  gpu_score: 0,
  gpu_memory_gb: 0,
  storage_total_gb: 0,
  storage_available_gb: 0,
  gpu_name: '',
};

export function toNumber(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function normalizeWorkerProfile(next, fallbackIp = '') {
  return {
    worker_id: String(next.worker_id || next.workerId || ''),
    ip: String(next.ip || next.IP || fallbackIp),
    location: String(next.location || next.Location || 'local'),
    cpu_cores: toNumber(next.cpu_cores ?? next.cpuCores ?? 0),
    memory_gb: toNumber(next.memory_gb ?? next.memoryGb ?? 0),
    cpu_score: toNumber(next.cpu_score ?? next.cpuScore ?? 0),
    gpu_score: toNumber(next.gpu_score ?? next.gpuScore ?? 0),
    gpu_memory_gb: toNumber(next.gpu_memory_gb ?? next.gpuMemoryGb ?? 0),
    storage_total_gb: toNumber(next.storage_total_gb ?? next.storageTotalGb ?? 0),
    storage_available_gb: toNumber(next.storage_available_gb ?? next.storageAvailableGb ?? 0),
    gpu_name: String(next.gpu_name || next.gpuName || ''),
  };
}

function validateWorkerCapacity(values) {
  for (const value of Object.values(values)) {
    if (value < 0) {
      throw new Error('capacity values must be non-negative');
    }
  }
  if (!Number.isInteger(values.cpu_cores)) {
    throw new Error('cpu_cores must be an integer');
  }
  if (values.storage_available_gb > values.storage_total_gb) {
    throw new Error('storage_available_gb cannot exceed storage_total_gb');
  }
}

export function buildRegisterWorkerBody(username, workerProfile, endpoint) {
  const workerId = String(workerProfile.worker_id || '').trim();
  const workerEndpoint = String(endpoint || '').trim();
  if (!workerEndpoint) {
    throw new Error('worker endpoint is required');
  }
  const capacity = {
    cpu_cores: toNumber(workerProfile.cpu_cores),
    memory_gb: toNumber(workerProfile.memory_gb),
    cpu_score: toNumber(workerProfile.cpu_score),
    gpu_score: toNumber(workerProfile.gpu_score),
    gpu_memory_gb: toNumber(workerProfile.gpu_memory_gb),
    storage_total_gb: toNumber(workerProfile.storage_total_gb),
    storage_available_gb: toNumber(workerProfile.storage_available_gb),
  };
  validateWorkerCapacity(capacity);
  return {
    username: username.trim(),
    ...(workerId ? { worker_id: workerId } : {}),
    ip: workerEndpoint,
    cpu_cores: capacity.cpu_cores,
    memory_gb: capacity.memory_gb,
    cpu_score: capacity.cpu_score,
    gpu_score: capacity.gpu_score,
    gpu_memory_gb: capacity.gpu_memory_gb,
    gpu_name: String(workerProfile.gpu_name || ''),
    storage_total_gb: capacity.storage_total_gb,
    storage_available_gb: capacity.storage_available_gb,
    location: workerProfile.location || 'local',
  };
}

export function registrationOwnerUsername(authenticatedUsername, formUsername) {
  return String(authenticatedUsername || formUsername || '').trim();
}
