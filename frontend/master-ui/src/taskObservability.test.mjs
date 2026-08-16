import assert from 'node:assert/strict';
import test from 'node:test';
import { normalizeTaskObservability } from './taskObservability.mjs';

test('normalizes trusted task observability fields and keeps redispatch visible', () => {
  assert.deepEqual(
    normalizeTaskObservability({
      worker_id: 'worker-a',
      provider_user: 'alice',
      dispatch_status: 'REDISPATCHED',
      retry_count: 2,
      usage_units: 80,
      max_cpt: 100,
      billed_amount: 81,
      billing_settled: true,
    }),
    {
      workerId: 'worker-a',
      providerUser: 'alice',
      dispatchStatus: 'REDISPATCHED',
      retryCount: 2,
      usageUnits: 80,
      maxCpt: 100,
      billedAmount: 81,
      billingSettled: true,
    }
  );
});

test('falls back to legacy task fields while the API rolls out observability', () => {
  assert.deepEqual(
    normalizeTaskObservability({
      worker_ip: '10.0.0.4',
      retry_count: 1,
      managed_executed_ops: 12,
      max_cpt: 20,
      billed_amount: 13,
      billing_settled: false,
      status: 'PENDING',
    }),
    {
      workerId: '',
      providerUser: '',
      dispatchStatus: 'REDISPATCHED',
      retryCount: 1,
      usageUnits: 12,
      maxCpt: 20,
      billedAmount: 13,
      billingSettled: false,
    }
  );
});
