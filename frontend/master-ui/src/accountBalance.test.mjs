import assert from 'node:assert/strict';
import test from 'node:test';
import { parseCptBalance } from './accountBalance.mjs';

test('parses the supported account balance fields', () => {
  assert.equal(parseCptBalance({ balance: 12.5 }), 12.5);
  assert.equal(parseCptBalance({ cpt_balance: '42' }), 42);
});

test('rejects missing and malformed account balances', () => {
  assert.throws(() => parseCptBalance({}), /invalid balance/i);
  assert.throws(() => parseCptBalance({ balance: 'not-a-number' }), /invalid balance/i);
  assert.throws(() => parseCptBalance({ balance: Number.POSITIVE_INFINITY }), /invalid balance/i);
});
