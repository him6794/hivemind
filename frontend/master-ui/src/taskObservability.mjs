function text(value) {
  return typeof value === 'string' ? value : '';
}

function nonNegativeNumber(value, fallback = 0) {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallback;
}

function finiteNumberOrNull(value) {
  if (value === undefined || value === null || value === '') return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export function normalizeTaskObservability(task = {}) {
  const retryCount = nonNegativeNumber(task.retry_count, 0);
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
    if (retryCount > 0) {
      dispatchStatus = isManagedV1 ? 'RETRY_COUNTER_RECORDED' : 'REDISPATCHED';
    } else if (workerId) {
      dispatchStatus = 'DISPATCHED';
    } else if (isManagedV1 && !isNotStarted) {
      dispatchStatus = 'UNKNOWN';
    } else {
      dispatchStatus = 'NOT_DISPATCHED';
    }
  }
  const chargeCapCpt = nonNegativeNumber(task.max_cpt, 0);
  const billedAmount = nonNegativeNumber(task.billed_amount, 0);
  const billingSettled = task.billing_settled === true;
  const rawChargeCap = finiteNumberOrNull(task.max_cpt);
  const rawBilledAmount = finiteNumberOrNull(task.billed_amount);
  const settledRemainderCpt = billingSettled && rawChargeCap !== null && rawBilledAmount !== null
    ? rawChargeCap - rawBilledAmount
    : null;
  const historicalOverCap = isManagedV1 && billingSettled && chargeCapCpt > 0 && billedAmount > chargeCapCpt;

  return {
    workerId: text(task.worker_id),
    providerUser: text(task.provider_user),
    dispatchStatus,
    retryCount,
    usageUnits: nonNegativeNumber(task.usage_units, nonNegativeNumber(task.managed_executed_ops, 0)),
    chargeCapCpt,
    billedAmount,
    billingSettled,
    settledRemainderCpt,
    historicalOverCap,
  };
}
