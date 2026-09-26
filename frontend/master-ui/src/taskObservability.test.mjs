import assert from 'node:assert/strict';
import test from 'node:test';
import { normalizeTaskObservability } from './taskObservability.mjs';

test('normalizes managed-v1 retry counter and task billing fields separately', () => {
  assert.deepEqual(
    normalizeTaskObservability({
      worker_id: 'worker-a',
      provider_user: 'alice',
      runtime: 'managed-function-v1',
      managed_consensus: { state: 'no_quorum', votes_received: 1, replica_count: 3 },
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
      dispatchStatus: 'NO_QUORUM',
      retryCount: 2,
      usageUnits: 80,
      chargeCapCpt: 100,
      billedAmount: 81,
      billingSettled: true,
      settledRemainderCpt: 19,
      historicalOverCap: false,
    }
  );
});

test('uses managed-v1 retry-counter terminology when dispatch fields are absent', () => {
  assert.deepEqual(
    normalizeTaskObservability({
      worker_ip: '10.0.0.4',
      runtime: 'managed-function-v1',
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
      dispatchStatus: 'RETRY_COUNTER_RECORDED',
      retryCount: 1,
      usageUnits: 12,
      chargeCapCpt: 20,
      billedAmount: 13,
      billingSettled: false,
      settledRemainderCpt: null,
      historicalOverCap: false,
    }
  );
});

test('flags historical v1 charges above the new cap without clamping actual billing', () => {
  const observability = normalizeTaskObservability({
    runtime: 'managed-function-v1',
    max_cpt: 100,
    billed_amount: 960,
    billing_settled: true,
  });

  assert.equal(observability.chargeCapCpt, 100);
  assert.equal(observability.billedAmount, 960);
  assert.equal(observability.historicalOverCap, true);
});

test('uses neutral progress when a failed v1 task has no consensus details', () => {
  assert.equal(
    normalizeTaskObservability({ runtime: 'managed-function-v1', status: 'FAILED' }).dispatchStatus,
    'UNKNOWN'
  );
  assert.equal(normalizeTaskObservability({ runtime: 'managed-function-v1' }).dispatchStatus, 'UNKNOWN');
  assert.equal(
    normalizeTaskObservability({
      runtime: 'managed-function-v1',
      managed_consensus: { state: 'stop_pending', votes_received: 1, replica_count: 3 },
    }).dispatchStatus,
    'STOP_PENDING'
  );
});

test('exposes charge-cap remainder only after settlement and preserves negative values', () => {
  assert.equal(
    normalizeTaskObservability({ max_cpt: 100, billed_amount: 72, billing_settled: false }).settledRemainderCpt,
    null,
  );
  assert.equal(
    normalizeTaskObservability({ max_cpt: 100, billed_amount: 72, billing_settled: true }).settledRemainderCpt,
    28,
  );
  assert.equal(
    normalizeTaskObservability({ max_cpt: 100, billed_amount: 125, billing_settled: true }).settledRemainderCpt,
    -25,
  );
});

test('does not infer settled remainder if cap or charge is unavailable', () => {
  assert.equal(normalizeTaskObservability({ billing_settled: true, billed_amount: 10 }).settledRemainderCpt, null);
  assert.equal(normalizeTaskObservability({ billing_settled: true, max_cpt: 10 }).settledRemainderCpt, null);
});
