import test from 'node:test';
import assert from 'node:assert/strict';
import {
  DRY_RUN_PATH,
  capToFill,
  dryRunRequest,
  dryRunSignature,
  estimateWarnings,
  interpretDryRun,
} from './dryRunPolicy.mjs';
import { t } from './locale.mjs';

const quoteAnswer = {
  success: true,
  message: 'The run completed',
  output: '60',
  output_bytes: 2,
  output_truncated: false,
  exceeds_consensus_result_limit: false,
  consensus_result_limit_bytes: 262144,
  usage_units: 25,
  quote: {
    replica_usage_units: 25,
    replica_count: 3,
    replica_count_source: 'default',
    reserved_usage_cpt: 75,
    platform_fee_cpt: 7,
    required_max_cpt: 82,
  },
  error: null,
  semantics_manifest_sha256: 'd82fb2679d7fe4247571206a28614cfaf7449091c49d131f67e08a74766658c0',
};

test('the request carries the source and the input the way a submit does', () => {
  assert.equal(DRY_RUN_PATH, '/api/tasks/dry-run');
  assert.deepEqual(dryRunRequest('1;', '{}'), { task_source: '1;', torrent: '{}' });
});

test('an answer is tied to one exact source and input', () => {
  const base = dryRunSignature('1;', '{}');
  assert.equal(base, dryRunSignature('1;', '{}'));
  assert.notEqual(base, dryRunSignature('2;', '{}'));
  assert.notEqual(base, dryRunSignature('1;', '{ }'));
  // The two fields cannot be shifted into each other.
  assert.notEqual(dryRunSignature('a', 'bc'), dryRunSignature('ab', 'c'));
});

test('a finished run becomes a quote with the numbers the Master computed', () => {
  const view = interpretDryRun({ ok: true, status: 200, data: quoteAnswer });
  assert.equal(view.kind, 'quote');
  assert.equal(view.output, '60');
  assert.equal(view.usageUnits, 25);
  assert.equal(view.replicaCount, 3);
  assert.equal(view.replicaCountIsDefault, true);
  assert.equal(view.reservedUsageCpt, 75);
  assert.equal(view.platformFeeCpt, 7);
  assert.equal(view.requiredMaxCpt, 82);
  assert.equal(view.interpreter, 'd82fb2679d7f');
  assert.equal(view.exceedsConsensusLimit, false);
});

test('a requested replica count is not described as the default', () => {
  const view = interpretDryRun({
    ok: true,
    status: 200,
    data: { ...quoteAnswer, quote: { ...quoteAnswer.quote, replica_count: 5, replica_count_source: 'requested' } },
  });
  assert.equal(view.replicaCount, 5);
  assert.equal(view.replicaCountIsDefault, false);
});

test('a truncated or oversize output is flagged', () => {
  const view = interpretDryRun({
    ok: true,
    status: 200,
    data: { ...quoteAnswer, output_truncated: true, exceeds_consensus_result_limit: true },
  });
  assert.equal(view.outputTruncated, true);
  assert.equal(view.exceedsConsensusLimit, true);
});

test('a syntax error keeps its position and has no price', () => {
  const view = interpretDryRun({
    ok: true,
    status: 200,
    data: {
      success: false,
      message: 'The program failed',
      error: { code: 'parse_error', message: 'unexpected token', line: 2, column: 9, cause: 'program', usage_units: null },
    },
  });
  assert.equal(view.kind, 'program-error');
  assert.equal(view.code, 'parse_error');
  assert.equal(view.detail, 'unexpected token');
  assert.equal(view.line, 2);
  assert.equal(view.column, 9);
});

test('a runtime error without a position still reads as a program error', () => {
  const view = interpretDryRun({
    ok: true,
    status: 200,
    data: {
      success: false,
      error: { code: 'runtime_error', message: 'division by zero', line: null, column: null, cause: 'program', usage_units: 11 },
    },
  });
  assert.equal(view.kind, 'program-error');
  assert.equal(view.line, null);
  assert.equal(view.usageUnits, 11);
});

test('hitting the dry run own limit is not reported as a program error', () => {
  for (const cause of ['usage_limit', 'timed_out']) {
    const view = interpretDryRun({
      ok: true,
      status: 200,
      data: { success: false, error: { code: 'budget_exhausted', message: 'x', cause } },
    });
    assert.deepEqual(view, { kind: 'limit', cause });
  }
});

test('a busy Master and a refused request are told apart', () => {
  assert.deepEqual(interpretDryRun({ ok: false, status: 429, data: { success: false } }), { kind: 'busy' });
  const refused = interpretDryRun({
    ok: false,
    status: 400,
    data: { success: false, message: 'hcode requires non-empty JSON input' },
  });
  assert.equal(refused.kind, 'rejected');
  assert.deepEqual(refused.detail, { raw: 'hcode requires non-empty JSON input' });
  const silent = interpretDryRun({ ok: false, status: 400, data: {} });
  assert.equal(silent.kind, 'rejected');
  assert.equal(t('en', silent.detail), 'The dry run was not accepted.');
});

test('an answer with unusable numbers is rejected instead of priced', () => {
  for (const quote of [
    { ...quoteAnswer.quote, required_max_cpt: 0 },
    { ...quoteAnswer.quote, required_max_cpt: '82' },
    { ...quoteAnswer.quote, replica_count: null },
    { ...quoteAnswer.quote, platform_fee_cpt: -1 },
    { ...quoteAnswer.quote, reserved_usage_cpt: 1.5 },
  ]) {
    const view = interpretDryRun({ ok: true, status: 200, data: { ...quoteAnswer, quote } });
    assert.equal(view.kind, 'rejected', JSON.stringify(quote));
  }
  assert.equal(interpretDryRun({ ok: true, status: 200, data: null }).kind, 'rejected');
  assert.equal(interpretDryRun({}).kind, 'rejected');
});

test('the estimate is compared with the cap and the balance', () => {
  assert.deepEqual(estimateWarnings({ requiredMaxCpt: 82, maxCpt: 100, balance: 1000 }), []);
  assert.deepEqual(estimateWarnings({ requiredMaxCpt: 82, maxCpt: 0, balance: 1000 }), []);
  assert.deepEqual(estimateWarnings({ requiredMaxCpt: 82, maxCpt: '', balance: null }), []);

  const [cap] = estimateWarnings({ requiredMaxCpt: 82, maxCpt: '60', balance: 1000 });
  assert.equal(
    t('en', cap),
    'Your charge cap of 60 CPT is below this estimate of 82 CPT. The task would run out of budget.',
  );
  const [balance] = estimateWarnings({ requiredMaxCpt: 3663, maxCpt: 4000, balance: 1000 });
  assert.equal(t('en', balance), 'Your balance of 1000.00 CPT is below this estimate of 3663 CPT.');
  assert.equal(estimateWarnings({ requiredMaxCpt: 82, maxCpt: 10, balance: 5 }).length, 2);
});

test('the cap is filled in only when the user has not entered one', () => {
  const view = interpretDryRun({ ok: true, status: 200, data: quoteAnswer });
  assert.equal(capToFill(view, 0), 82);
  assert.equal(capToFill(view, ''), 82);
  assert.equal(capToFill(view, '0'), 82);
  assert.equal(capToFill(view, 500), null);
  assert.equal(capToFill({ kind: 'limit' }, 0), null);
  assert.equal(capToFill(null, 0), null);
});

test('every message the panel can show has a Traditional Chinese translation', () => {
  const refused = interpretDryRun({ ok: false, status: 400, data: {} });
  const unreadable = interpretDryRun({ ok: true, status: 200, data: {} });
  const failed = interpretDryRun({ ok: true, status: 200, data: { success: false } });
  const warnings = estimateWarnings({ requiredMaxCpt: 82, maxCpt: 10, balance: 5 });
  for (const entry of [refused.detail, unreadable.detail, failed.detail, ...warnings]) {
    assert.notEqual(t('zh', entry), t('en', entry), JSON.stringify(entry));
  }
});
