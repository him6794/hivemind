// Presence-aware view of the Worker hardware benchmark. The frozen suite
// reports raw samples and a server-derived score; this module only projects
// that payload into the shape the Computer tab renders. Unknown values stay
// `null` (rendered as "Unknown"), never coerced to 0 — an unmeasured CPU or
// GPU is not a zero score.

function record(value) {
  return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}

function nullableText(value) {
  if (value === null || value === undefined) return null;
  const normalized = String(value).trim();
  return normalized ? normalized : null;
}

function nullableNumber(value) {
  if (value === null || value === undefined || value === '') return null;
  const normalized = Number(value);
  return Number.isFinite(normalized) ? normalized : null;
}

// Display projection of one benchmark case's raw samples. This mirrors the
// frozen GFLOP/s formula in the models crate (2*N^3*reps*threads / elapsed_ns,
// median across batches); the server already validated the sample bounds and
// derives the authoritative score, so this only surfaces the throughput the
// user actually measured. A case with no valid batches yields null, not 0.
function medianGflops(caseEntry) {
  const batches = Array.isArray(caseEntry?.batches) ? caseEntry.batches : [];
  const dimension = Number(caseEntry?.dimension);
  const threads = Number(caseEntry?.threads);
  if (!Number.isFinite(dimension) || !Number.isFinite(threads)) return null;
  const rates = [];
  for (const batch of batches) {
    const repetitions = Number(batch?.repetitions);
    const elapsedNs = Number(batch?.elapsed_ns);
    if (!Number.isFinite(repetitions) || repetitions <= 0) return null;
    if (!Number.isFinite(elapsedNs) || elapsedNs <= 0) return null;
    rates.push((2 * dimension * dimension * dimension * repetitions * threads) / elapsedNs);
  }
  if (rates.length === 0) return null;
  rates.sort((a, b) => a - b);
  const median = rates[Math.floor(rates.length / 2)];
  return Number.isFinite(median) && median >= 0 ? median : null;
}

function normalizeCase(caseEntry) {
  const source = record(caseEntry);
  return {
    name: nullableText(source.name) || 'Unknown case',
    dimension: nullableNumber(source.dimension),
    threads: nullableNumber(source.threads),
    median_gflops: medianGflops(source),
  };
}

// Server-derived score: `cpu_score` is omitted entirely when unmeasured
// (serde skip-if-none), while `gpu_scores` is a fixed-length array with null
// entries, so index alignment with `report.gpus` is preserved either way.
function normalizeMeasurement(measurement, score, index) {
  const source = record(measurement);
  const scores = Array.isArray(score) ? score : null;
  const resolved = scores ? nullableNumber(scores[index]) : nullableNumber(score);
  return {
    state: nullableText(source.state),
    score: resolved,
    threads: nullableNumber(source.threads),
    measured_at: nullableText(source.measured_at),
    identity: nullableText(source.identity),
    correctness_passed: source.correctness_passed === true,
    reason: nullableText(source.reason),
    cases: Array.isArray(source.cases) ? source.cases.map(normalizeCase) : [],
  };
}

function normalizeGpuDevice(deviceEntry, score, index) {
  const source = record(deviceEntry);
  const device = record(source.device);
  const measurement = normalizeMeasurement(source.measurement, score, index);
  return {
    name: nullableText(device.name) || 'Unknown GPU',
    backend: nullableText(device.backend),
    device_type: nullableText(device.device_type),
    driver: nullableText(device.driver),
    pci_bus_id: nullableText(device.pci_bus_id),
    ...measurement,
  };
}

export function normalizeBenchmark(payload) {
  const source = record(payload);
  const report = record(source.report);
  const gpus = Array.isArray(report.gpus) ? report.gpus : [];
  const gpuScores = Array.isArray(source.gpu_scores) ? source.gpu_scores : null;
  return {
    success: source.success === true,
    busy: source.busy === true,
    threads: nullableNumber(source.threads),
    suite: nullableText(report.suite),
    build_id: nullableText(report.build_id),
    release_build: report.release_build === true,
    cpu: normalizeMeasurement(report.cpu, source.cpu_score, 0),
    gpu: {
      state: nullableText(report.gpu_state),
      reason: nullableText(report.gpu_reason),
      devices: gpus.map((device, index) => normalizeGpuDevice(device, gpuScores, index)),
    },
  };
}

function buildBenchmarkUrl(base, path) {
  const root = String(base || '').trim().replace(/\/+$/, '');
  return `${root}${path}`;
}

export function buildBenchmarkStatusRequest(workerControlBase, authToken) {
  const token = String(authToken || '').trim();
  if (!token) throw new Error('A signed-in session is required');
  return {
    url: buildBenchmarkUrl(workerControlBase, '/api/benchmark'),
    options: {
      method: 'GET',
      headers: { Authorization: `Bearer ${token}` },
    },
  };
}

export function buildBenchmarkRerunRequest(workerControlBase, authToken, component) {
  const token = String(authToken || '').trim();
  if (!token) throw new Error('A signed-in session is required');
  const selector = String(component || '').trim().toLowerCase();
  if (!['all', 'cpu', 'gpu'].includes(selector)) throw new Error('Unknown benchmark component');
  return {
    url: buildBenchmarkUrl(workerControlBase, '/api/benchmark/rerun'),
    options: {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ component: selector }),
    },
  };
}
