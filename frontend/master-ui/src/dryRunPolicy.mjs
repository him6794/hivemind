// How the Submit tab reads the Master's dry-run answer (POST /api/tasks/dry-run).
//
// A dry run is an estimate made on this machine. The network settles on what the
// Workers report, so the panel shows these numbers as an estimate and never as a charge.

import { message } from './locale.mjs';
import { resourceFields } from './taskRequest.mjs';

export const DRY_RUN_PATH = '/api/tasks/dry-run';

export function dryRunRequest(source, input) {
  return { task_source: source, torrent: input };
}

// The answer belongs to one exact source, input, and set of resources. Editing any of them makes it
// stale, because the resources change the price the cap has to cover.
export function dryRunSignature(source, input, resources) {
  return JSON.stringify([String(source ?? ''), String(input ?? ''), resourceFields(resources)]);
}

function positiveInteger(value) {
  return Number.isSafeInteger(value) && value > 0 ? value : null;
}

function serverText(data) {
  return typeof data?.message === 'string' && data.message.trim() ? { raw: data.message } : null;
}

function unexpected() {
  return { kind: 'rejected', detail: message('The dry run returned an answer this console cannot read.') };
}

function interpretQuote(data) {
  const quote = data.quote;
  const usageUnits = positiveInteger(quote?.replica_usage_units);
  const replicaCount = positiveInteger(quote?.replica_count);
  const reservedUsageCpt = positiveInteger(quote?.reserved_usage_cpt);
  const requiredMaxCpt = positiveInteger(quote?.required_max_cpt);
  const platformFeeCpt = Number.isSafeInteger(quote?.platform_fee_cpt) && quote.platform_fee_cpt >= 0
    ? quote.platform_fee_cpt
    : null;
  if (
    usageUnits === null || replicaCount === null || reservedUsageCpt === null
    || requiredMaxCpt === null || platformFeeCpt === null
  ) {
    return unexpected();
  }
  return {
    kind: 'quote',
    output: typeof data.output === 'string' ? data.output : '',
    outputBytes: Number.isSafeInteger(data.output_bytes) ? data.output_bytes : 0,
    outputTruncated: data.output_truncated === true,
    exceedsConsensusLimit: data.exceeds_consensus_result_limit === true,
    consensusLimitBytes: positiveInteger(data.consensus_result_limit_bytes),
    usageUnits,
    replicaCount,
    replicaCountIsDefault: quote.replica_count_source === 'default',
    reservedUsageCpt,
    platformFeeCpt,
    requiredMaxCpt,
    interpreter: typeof data.semantics_manifest_sha256 === 'string'
      ? data.semantics_manifest_sha256.slice(0, 12)
      : '',
  };
}

function interpretFailure(error) {
  const cause = error.cause;
  if (cause === 'usage_limit' || cause === 'timed_out') {
    return { kind: 'limit', cause };
  }
  if (cause !== 'program' || typeof error.code !== 'string') return unexpected();
  return {
    kind: 'program-error',
    code: error.code,
    detail: typeof error.message === 'string' ? error.message : '',
    line: positiveInteger(error.line),
    column: positiveInteger(error.column),
    usageUnits: Number.isSafeInteger(error.usage_units) ? error.usage_units : null,
  };
}

// Turns the HTTP outcome into what the panel renders:
//   quote          the function finished; the numbers price the task
//   program-error  the function or its input is wrong
//   limit          the dry run stopped at its own limit, so nothing is known about the price
//   busy           the Master is running its maximum number of dry runs
//   rejected       the request was refused or the answer was unusable
export function interpretDryRun({ ok, status, data } = {}) {
  const body = data && typeof data === 'object' ? data : {};
  if (status === 429) return { kind: 'busy' };
  if (!ok) {
    return {
      kind: 'rejected',
      detail: serverText(body) ?? message('The dry run was not accepted.'),
    };
  }
  if (body.success === true && body.quote && typeof body.quote === 'object') {
    return interpretQuote(body);
  }
  if (body.error && typeof body.error === 'object') return interpretFailure(body.error);
  if (body.success === false) {
    return { kind: 'rejected', detail: serverText(body) ?? message('The dry run failed.') };
  }
  return unexpected();
}

// The cap a task needs: enough for the work, and never less than the price of the resources it
// reserves, because the Master refuses a cap below that price. The resource price is absent when
// it could not be read, and then only the work is counted.
export function suggestedCap(view) {
  if (view?.kind !== 'quote') return null;
  return Math.max(view.requiredMaxCpt, view.resourcePriceCpt ?? 0);
}

// Warnings that compare what the task needs with what the user has entered or holds.
export function estimateWarnings({ requiredMaxCpt, resourcePriceCpt = null, maxCpt, balance }) {
  const warnings = [];
  const cap = Number(maxCpt);
  const needed = Math.max(requiredMaxCpt, resourcePriceCpt ?? 0);
  if (Number.isFinite(cap) && cap > 0 && cap < requiredMaxCpt) {
    warnings.push(message(
      'Your charge cap of {cap} CPT is below this estimate of {required} CPT. The task would run out of budget.',
      { cap, required: requiredMaxCpt },
    ));
  } else if (Number.isFinite(cap) && cap > 0 && cap < needed) {
    warnings.push(message(
      'Your charge cap of {cap} CPT is below the {price} CPT price of the computer requirements you chose. The Master will refuse the task.',
      { cap, price: needed },
    ));
  }
  if (typeof balance === 'number' && Number.isFinite(balance) && balance < needed) {
    warnings.push(message(
      'Your balance of {balance} CPT is below this estimate of {required} CPT.',
      { balance: balance.toFixed(2), required: needed },
    ));
  }
  return warnings;
}

// The cap to fill in after a dry run: only when the user has not entered one.
export function capToFill(view, currentMaxCpt) {
  const cap = suggestedCap(view);
  if (cap === null) return null;
  const current = Number(currentMaxCpt);
  return Number.isFinite(current) && current > 0 ? null : cap;
}
