export const PROVIDER_CREDITS_LABEL = 'Account-wide settled provider credits';
export const PROVIDER_CREDITS_DETAIL = 'Account-wide total; not a per-worker payout.';

const HOST_NUMBER_FIELDS = [
  'cpu_cores',
  'cpu_usage_percent',
  'memory_total_gb',
  'memory_available_gb',
  'memory_usage_percent',
  'gpu_count',
  'gpu_utilization_percent',
  'vram_total_mb',
  'vram_available_mb',
  'storage_total_gb',
  'storage_available_gb',
];

function record(value) {
  return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

function text(value) {
  return value === null || value === undefined ? '' : String(value);
}

function nullableText(value) {
  const normalized = text(value).trim();
  return normalized ? normalized : null;
}

function nullableNumber(value) {
  if (value === null || value === undefined || value === '') return null;
  const normalized = Number(value);
  return Number.isFinite(normalized) ? normalized : null;
}

export function hasKnownDashboardMeasurement(value) {
  return typeof value === 'number' && Number.isFinite(value);
}

export function deriveUsedPercent(total, available) {
  if (!hasKnownDashboardMeasurement(total) || !hasKnownDashboardMeasurement(available)) return null;
  if (total <= 0 || available < 0 || available > total) return null;
  return ((total - available) / total) * 100;
}

export function normalizeWorkerDashboard(payload = {}) {
  const source = record(payload);
  const hostSource = record(source.host);
  const host = Object.fromEntries(
    HOST_NUMBER_FIELDS.map((field) => [field, nullableNumber(hostSource[field])]),
  );

  return {
    success: source.success === true,
    worker_id: text(source.worker_id),
    sampled_at: nullableText(source.sampled_at),
    stale: source.stale === true,
    host,
    assignments: Array.isArray(source.assignments)
      ? source.assignments.map((value) => {
          const assignment = record(value);
          return {
            task_id: text(assignment.task_id),
            submitter: text(assignment.submitter),
            status: text(assignment.status),
            max_cpt: nullableNumber(assignment.max_cpt),
            reported_usage_cpt: nullableNumber(assignment.reported_usage_cpt),
            usage_basis: text(assignment.usage_basis),
            usage_updated_at: nullableText(assignment.usage_updated_at),
          };
        })
      : [],
    settled_provider_credits_cpt: nullableNumber(source.settled_provider_credits_cpt),
    currency: text(source.currency),
  };
}

export function buildWorkerDashboardRequest(workerControlBase, authToken) {
  const base = String(workerControlBase || '').trim().replace(/\/+$/, '');
  const token = String(authToken || '').trim();
  if (!token) throw new Error('A signed-in session is required');

  return {
    url: `${base}/api/worker-dashboard`,
    options: {
      method: 'GET',
      headers: { Authorization: `Bearer ${token}` },
    },
  };
}

export function createNonOverlappingPoll(callback) {
  let inFlight = false;
  return async (...args) => {
    if (inFlight) return false;
    inFlight = true;
    try {
      await callback(...args);
      return true;
    } finally {
      inFlight = false;
    }
  };
}
