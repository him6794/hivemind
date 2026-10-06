function text(value) {
  return typeof value === 'string' ? value : '';
}

function finiteNumberOrNull(value) {
  if (typeof value !== 'number' && typeof value !== 'string') return null;
  if (typeof value === 'string' && value.trim() === '') return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function nonNegativeNumberOrNull(value) {
  const parsed = finiteNumberOrNull(value);
  return parsed !== null && parsed >= 0 ? parsed : null;
}

export function normalizeTaskObservability(task = {}) {
  // retry_count is a proto3 int32 and the persisted task counter defaults to zero.
  const retryCount = Object.prototype.hasOwnProperty.call(task, 'retry_count')
    ? nonNegativeNumberOrNull(task.retry_count)
    : 0;
  const runtime = text(task.runtime || task.runtime_version || task.Runtime).trim();
  const isManagedV1 = runtime === 'managed-function-v1';
  const reportedDispatchStatus = text(task.dispatch_status);
  const workerId = text(task.worker_id);
  const taskStatus = text(task.status || task.Status).toUpperCase();
  const consensusState = text(task.managed_consensus?.state).toUpperCase();
  const consensusProgress = consensusState === 'NO_QUORUM'
    ? 'NO_QUORUM'
    : consensusState === 'STOP_PENDING'
      ? 'STOP_PENDING'
      : '';
  const isNotStarted = ['PENDING', 'QUEUED', 'SUBMITTED', 'CREATED'].includes(taskStatus);
  let dispatchStatus = reportedDispatchStatus;
  if (isManagedV1 && consensusProgress) {
    dispatchStatus = consensusProgress;
  } else if (isManagedV1 && reportedDispatchStatus === 'REDISPATCHED') {
    dispatchStatus = 'RETRY_COUNTER_RECORDED';
  } else if (!dispatchStatus) {
    if (retryCount !== null && retryCount > 0) {
      dispatchStatus = isManagedV1 ? 'RETRY_COUNTER_RECORDED' : 'REDISPATCHED';
    } else if (workerId) {
      dispatchStatus = 'DISPATCHED';
    } else if (retryCount === null || (isManagedV1 && !isNotStarted)) {
      dispatchStatus = 'UNKNOWN';
    } else {
      dispatchStatus = 'NOT_DISPATCHED';
    }
  }
  const chargeCapCpt = nonNegativeNumberOrNull(task.max_cpt);
  const billedAmount = nonNegativeNumberOrNull(task.billed_amount);
  const billingSettled = task.billing_settled === true;
  const settledRemainderCpt = billingSettled && chargeCapCpt !== null && billedAmount !== null
    ? chargeCapCpt - billedAmount
    : null;
  const historicalOverCap = isManagedV1 && billingSettled && chargeCapCpt !== null && billedAmount !== null
    && chargeCapCpt > 0 && billedAmount > chargeCapCpt;

  return {
    workerId: text(task.worker_id),
    providerUser: text(task.provider_user),
    dispatchStatus,
    retryCount,
    usageUnits: nonNegativeNumberOrNull(task.usage_units)
      ?? nonNegativeNumberOrNull(task.managed_executed_ops),
    chargeCapCpt,
    billedAmount,
    billingSettled,
    settledRemainderCpt,
    historicalOverCap,
  };
}
