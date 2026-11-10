import test from 'node:test';
import assert from 'node:assert/strict';
import { QUOTE_PATH, resourceFields, resourcePriceFromQuote, resourceQuoteRequest } from './taskRequest.mjs';

test('a resource left at zero is not sent', () => {
  assert.deepEqual(resourceFields({}), {});
  assert.deepEqual(resourceFields(), {});
  assert.deepEqual(
    resourceFields({ cpuScore: 0, gpuScore: '0', memoryGb: '', gpuMemoryGb: null, storageGb: undefined, hostCount: 0 }),
    {},
  );
});

test('form values become numbers, whether they are typed or already numeric', () => {
  assert.deepEqual(
    resourceFields({ cpuScore: '250', gpuScore: 100, memoryGb: '8', gpuMemoryGb: '6', storageGb: 20, hostCount: '2' }),
    { cpu_score: 250, gpu_score: 100, memory_gb: 8, gpu_memory_gb: 6, storage_gb: 20, host_count: 2 },
  );
});

test('negative and non-numeric values are dropped', () => {
  assert.deepEqual(resourceFields({ cpuScore: -5, memoryGb: 'lots', hostCount: 'x' }), {});
});

test('the quote request carries the source, the input, the resources, and a placeholder budget', () => {
  assert.equal(QUOTE_PATH, '/api/tasks/quote');
  assert.deepEqual(resourceQuoteRequest('1;', '{}', { memoryGb: 4, hostCount: 2 }), {
    runtime: 'hcode',
    task_source: '1;',
    torrent: '{}',
    max_cpt: 1,
    memory_gb: 4,
    host_count: 2,
  });
  assert.equal(resourceQuoteRequest('1;', '{}', {}).max_cpt, 1);
});

test('only a successful quote with a whole price is trusted', () => {
  assert.equal(resourcePriceFromQuote({ ok: true, data: { success: true, quoted_cpt: 10 } }), 10);
  assert.equal(resourcePriceFromQuote({ ok: true, data: { success: true, quoted_cpt: 0 } }), 0);
  for (const answer of [
    { ok: false, data: { success: true, quoted_cpt: 10 } },
    { ok: true, data: { success: false, quoted_cpt: 10 } },
    { ok: true, data: { success: true, quoted_cpt: '10' } },
    { ok: true, data: { success: true, quoted_cpt: -1 } },
    { ok: true, data: { success: true, quoted_cpt: 1.5 } },
    { ok: true, data: null },
    {},
    undefined,
  ]) {
    assert.equal(resourcePriceFromQuote(answer), null, JSON.stringify(answer));
  }
});
