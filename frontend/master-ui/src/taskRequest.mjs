// Parts of a task request that more than one action builds, so the dry run, the price check,
// and the submit ask for the same resources.

function positiveNumber(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
}

// The resource fields the form sends. A field left at zero is not sent at all.
export function resourceFields({ cpuScore, gpuScore, memoryGb, gpuMemoryGb, storageGb, hostCount } = {}) {
  const fields = {};
  if (positiveNumber(cpuScore)) fields.cpu_score = positiveNumber(cpuScore);
  if (positiveNumber(gpuScore)) fields.gpu_score = positiveNumber(gpuScore);
  if (positiveNumber(memoryGb)) fields.memory_gb = positiveNumber(memoryGb);
  if (positiveNumber(gpuMemoryGb)) fields.gpu_memory_gb = positiveNumber(gpuMemoryGb);
  if (positiveNumber(storageGb)) fields.storage_gb = positiveNumber(storageGb);
  if (positiveNumber(hostCount)) fields.host_count = positiveNumber(hostCount);
  return fields;
}

export const QUOTE_PATH = '/api/tasks/quote';

// A quote prices the reserved resources only, but the checks it shares with task creation still
// need an input and a positive budget, so the request carries a placeholder of 1 CPT.
export function resourceQuoteRequest(source, input, resources) {
  return {
    runtime: 'hcode',
    task_source: source,
    torrent: input,
    max_cpt: 1,
    ...resourceFields(resources),
  };
}

// The price of the reserved resources from a quote answer, or null when there is none to trust.
export function resourcePriceFromQuote({ ok, data } = {}) {
  if (!ok || data?.success !== true) return null;
  return Number.isSafeInteger(data.quoted_cpt) && data.quoted_cpt >= 0 ? data.quoted_cpt : null;
}
