import test from 'node:test';
import assert from 'node:assert/strict';
import { PIPELINE_TEMPLATES, buildDagPipelineGraph } from './pipelineTemplates.mjs';

test('PIPELINE_TEMPLATES covers ONNX, Whisper, Embeddings, WASM image, and Monte Carlo', () => {
  const ids = PIPELINE_TEMPLATES.map((t) => t.id);
  assert.ok(ids.includes('onnx-batch-inference'));
  assert.ok(ids.includes('whisper-transcribe'));
  assert.ok(ids.includes('embedding-batch'));
  assert.ok(ids.includes('wasm-image-transform'));
  assert.ok(ids.includes('monte-carlo-sim'));

  for (const tpl of PIPELINE_TEMPLATES) {
    assert.ok(tpl.nodes.length >= 3, `${tpl.id} should have at least 3 nodes`);
    const graph = buildDagPipelineGraph(tpl.nodes, tpl.defaultBudgetCpt);
    assert.ok(graph.stages.length >= 2, `${tpl.id} should have at least 2 topological stages`);
    assert.equal(graph.counts.total, tpl.nodes.length);
  }
});

test('buildDagPipelineGraph computes topological stages, progress, and CPT budget', () => {
  const graph = buildDagPipelineGraph(
    [
      { node_id: 'prep', kind: 'wasm', depends_on: [], state: 'completed', max_cpt: 20, billed_cpt: 12 },
      { node_id: 'map-a', kind: 'onnx', depends_on: ['prep'], state: 'completed', max_cpt: 50, billed_cpt: 38 },
      { node_id: 'map-b', kind: 'onnx', depends_on: ['prep'], state: 'running', max_cpt: 50, billed_cpt: 0 },
      { node_id: 'reduce', kind: 'reduce', depends_on: ['map-a', 'map-b'], state: 'pending', max_cpt: 30, billed_cpt: 0 },
    ],
    150,
  );

  assert.equal(graph.stages.length, 3);
  assert.deepEqual(graph.stages[0].nodes.map((n) => n.id), ['prep']);
  assert.deepEqual(graph.stages[1].nodes.map((n) => n.id), ['map-a', 'map-b']);
  assert.deepEqual(graph.stages[2].nodes.map((n) => n.id), ['reduce']);
  assert.equal(graph.counts.completed, 2);
  assert.equal(graph.counts.running, 1);
  assert.equal(graph.counts.pending, 1);
  assert.equal(graph.progressPct, 50);
  assert.equal(graph.totalBilledCpt, 50);
  assert.equal(graph.budgetRemainingCpt, 100);
});
