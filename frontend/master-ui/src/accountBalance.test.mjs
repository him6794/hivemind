import assert from 'node:assert/strict';
import test from 'node:test';
import { createBalanceRefreshController, parseCptBalance } from './accountBalance.mjs';

test('parses the supported account balance fields', () => {
  assert.equal(parseCptBalance({ balance: 12.5 }), 12.5);
  assert.equal(parseCptBalance({ cpt_balance: '42' }), 42);
});

test('rejects missing and malformed account balances', () => {
  assert.throws(() => parseCptBalance({}), /invalid balance/i);
  assert.throws(() => parseCptBalance({ balance: 'not-a-number' }), /invalid balance/i);
  assert.throws(() => parseCptBalance({ balance: Number.POSITIVE_INFINITY }), /invalid balance/i);
});

test('coalesces independent balance refreshes and applies the current account value', async () => {
  let resolveBalance;
  let calls = 0;
  let currentToken = 'account-token';
  const balances = [];
  const errors = [];
  const refresh = createBalanceRefreshController({
    loadBalance: async (authToken) => {
      assert.equal(authToken, 'account-token');
      calls += 1;
      return new Promise((resolve) => { resolveBalance = resolve; });
    },
    getCurrentToken: () => currentToken,
    setBalance: (value) => balances.push(value),
    setError: (value) => errors.push(value),
  });

  const first = refresh.refresh('account-token');
  const second = refresh.refresh('account-token');
  assert.equal(first, second);
  await Promise.resolve();
  assert.equal(calls, 1);

  resolveBalance(12.5);
  assert.equal(await first, true);
  assert.deepEqual(balances, [12.5]);
  assert.deepEqual(errors, ['']);
});

test('refreshes after task submission and clears late responses on logout', async () => {
  let resolveBalance;
  let currentToken = 'account-token';
  const balances = [];
  const errors = [];
  const refresh = createBalanceRefreshController({
    loadBalance: async () => new Promise((resolve) => { resolveBalance = resolve; }),
    getCurrentToken: () => currentToken,
    setBalance: (value) => balances.push(value),
    setError: (value) => errors.push(value),
  });

  const pendingRefresh = refresh.refresh('account-token');
  await Promise.resolve();
  currentToken = '';
  refresh.clear();
  assert.deepEqual(balances, [null]);
  assert.deepEqual(errors, ['']);

  resolveBalance(90);
  assert.equal(await pendingRefresh, false);
  assert.deepEqual(balances, [null]);
});

test('performs a fresh balance read after a pending refresh settles', async () => {
  let calls = 0;
  let currentToken = 'account-token';
  const balances = [];
  const refresh = createBalanceRefreshController({
    loadBalance: async () => {
      calls += 1;
      return 100 - calls;
    },
    getCurrentToken: () => currentToken,
    setBalance: (value) => balances.push(value),
    setError: () => {},
  });

  const initial = refresh.refresh('account-token');
  const afterSubmit = refresh.refreshAfter('account-token');
  await Promise.all([initial, afterSubmit]);

  assert.equal(calls, 2);
  assert.deepEqual(balances, [99, 98]);
});
