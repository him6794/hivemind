import assert from 'node:assert/strict';
import test from 'node:test';
import { normalizeTaskObservability } from './taskObservability.mjs';

test('normalizes managed-v1 retry counter and task billing fields separately', () => {
  assert.deepEqual(
    normalizeTaskObservability({
      worker_id: 'worker-a',
      provider_user: 'alice',
      runtime: 'hcode',
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
      runtime: 'hcode',
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

test('keeps unknown, invalid, and negative metrics distinct from genuine zero', () => {
  // The server contract defines an omitted proto3 retry_count as its zero default.
  assert.equal(normalizeTaskObservability({}).retryCount, 0);

  for (const invalid of [undefined, null, '', '  ', 'invalid', -1, NaN, Infinity, true]) {
    const observability = normalizeTaskObservability({
      runtime: 'hcode',
      retry_count: invalid,
      usage_units: invalid,
      managed_executed_ops: invalid,
      max_cpt: invalid,
      billed_amount: invalid,
      billing_settled: true,
    });

    assert.equal(observability.retryCount, null);
    assert.equal(observability.dispatchStatus, 'UNKNOWN');
    assert.equal(observability.usageUnits, null);
    assert.equal(observability.chargeCapCpt, null);
    assert.equal(observability.billedAmount, null);
    assert.equal(observability.settledRemainderCpt, null);
    assert.equal(observability.historicalOverCap, false);
  }
});

test('falls back from unavailable usage_units to valid managed execution ops', () => {
  assert.equal(
    normalizeTaskObservability({ usage_units: null, managed_executed_ops: 7 }).usageUnits,
    7,
  );
  assert.equal(
    normalizeTaskObservability({ usage_units: undefined, managed_executed_ops: '7' }).usageUnits,
    7,
  );
  assert.equal(
    normalizeTaskObservability({ usage_units: null, managed_executed_ops: -1 }).usageUnits,
    null,
  );
  assert.equal(
    normalizeTaskObservability({ usage_units: null, managed_executed_ops: null }).usageUnits,
    null,
  );
});

test('preserves genuine zero usage, cap, and settled charge values', () => {
  const observability = normalizeTaskObservability({
    runtime: 'hcode',
    status: 'COMPLETED',
    retry_count: 0,
    usage_units: 0,
    managed_executed_ops: 7,
    max_cpt: 0,
    billed_amount: 0,
    billing_settled: true,
  });

  assert.equal(observability.retryCount, 0);
  assert.equal(observability.usageUnits, 0);
  assert.equal(observability.chargeCapCpt, 0);
  assert.equal(observability.billedAmount, 0);
  assert.equal(observability.settledRemainderCpt, 0);
  assert.equal(observability.historicalOverCap, false);
});

test('distinguishes pending charges, settled unknown charges, and charged zero', () => {
  const pending = normalizeTaskObservability({ billing_settled: false, billed_amount: 0 });
  const settledUnknown = normalizeTaskObservability({ billing_settled: true });
  const chargedZero = normalizeTaskObservability({ billing_settled: true, billed_amount: 0 });

  assert.equal(pending.billingSettled, false);
  assert.equal(pending.billedAmount, 0);
  assert.equal(settledUnknown.billingSettled, true);
  assert.equal(settledUnknown.billedAmount, null);
  assert.equal(chargedZero.billingSettled, true);
  assert.equal(chargedZero.billedAmount, 0);
});

test('flags historical v1 charges above the new cap without clamping actual billing', () => {
  const observability = normalizeTaskObservability({
    runtime: 'hcode',
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
    normalizeTaskObservability({ runtime: 'hcode', status: 'FAILED' }).dispatchStatus,
    'UNKNOWN'
  );
  assert.equal(normalizeTaskObservability({ runtime: 'hcode' }).dispatchStatus, 'UNKNOWN');
  assert.equal(
    normalizeTaskObservability({
      runtime: 'hcode',
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

test('does not infer settled remainder if cap or charge is unavailable or invalid', () => {
  assert.equal(normalizeTaskObservability({ billing_settled: true, billed_amount: 10 }).settledRemainderCpt, null);
  assert.equal(normalizeTaskObservability({ billing_settled: true, max_cpt: 10 }).settledRemainderCpt, null);
  assert.equal(normalizeTaskObservability({ billing_settled: true, max_cpt: -1, billed_amount: 10 }).settledRemainderCpt, null);
  assert.equal(normalizeTaskObservability({ billing_settled: true, max_cpt: 10, billed_amount: -1 }).settledRemainderCpt, null);
});
