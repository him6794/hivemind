function text(value) {
  return typeof value === 'string' ? value : '';
}

function nonNegativeNumber(value, fallback = 0) {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallback;
}

export function normalizeTaskObservability(task = {}) {
  const retryCount = nonNegativeNumber(task.retry_count, 0);
  const dispatchStatus = text(task.dispatch_status) || (
    retryCount > 0
      ? 'REDISPATCHED'
      : text(task.worker_id)
        ? 'DISPATCHED'
        : 'NOT_DISPATCHED'
  );

  return {
    workerId: text(task.worker_id),
    providerUser: text(task.provider_user),
    dispatchStatus,
    retryCount,
    usageUnits: nonNegativeNumber(task.usage_units, nonNegativeNumber(task.managed_executed_ops, 0)),
    maxCpt: nonNegativeNumber(task.max_cpt, 0),
    billedAmount: nonNegativeNumber(task.billed_amount, 0),
    billingSettled: task.billing_settled === true,
  };
}
