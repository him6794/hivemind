/**
 * Standard non-LLM task templates and visual batch/DAG pipeline graph helpers
 * for Master UI (Milestone 6.2 #2).
 */

export const PIPELINE_TEMPLATES = [
  {
    id: 'onnx-batch-inference',
    name: 'ONNX Batch Inference',
    category: 'ML / Inference',
    runtime: 'onnx',
    description: 'Run batched ONNX model inference across multiple inputs with map/reduce aggregation.',
    defaultBudgetCpt: 150,
    nodes: [
      { node_id: 'prep', kind: 'wasm', depends_on: [], max_cpt: 20 },
      { node_id: 'infer-0', kind: 'onnx', depends_on: ['prep'], max_cpt: 50 },
      { node_id: 'infer-1', kind: 'onnx', depends_on: ['prep'], max_cpt: 50 },
      { node_id: 'merge', kind: 'reduce', depends_on: ['infer-0', 'infer-1'], max_cpt: 30 },
    ],
  },
  {
    id: 'whisper-transcribe',
    name: 'Whisper Audio Transcription',
    category: 'Audio / Speech',
    runtime: 'onnx',
    description: 'Split audio segments, transcribe chunks in parallel, and stitch timestamped subtitles.',
    defaultBudgetCpt: 240,
    nodes: [
      { node_id: 'chunk-audio', kind: 'wasm', depends_on: [], max_cpt: 20 },
      { node_id: 'transcribe-a', kind: 'onnx', depends_on: ['chunk-audio'], max_cpt: 90 },
      { node_id: 'transcribe-b', kind: 'onnx', depends_on: ['chunk-audio'], max_cpt: 90 },
      { node_id: 'stitch-srt', kind: 'reduce', depends_on: ['transcribe-a', 'transcribe-b'], max_cpt: 40 },
    ],
  },
  {
    id: 'embedding-batch',
    name: 'Batch Text Embeddings',
    category: 'Vector / RAG',
    runtime: 'llm',
    description: 'Compute normalized document embeddings in parallel shards and emit a combined JSONL index.',
    defaultBudgetCpt: 120,
    nodes: [
      { node_id: 'tokenize', kind: 'wasm', depends_on: [], max_cpt: 15 },
      { node_id: 'embed-shard-0', kind: 'llm', depends_on: ['tokenize'], max_cpt: 45 },
      { node_id: 'embed-shard-1', kind: 'llm', depends_on: ['tokenize'], max_cpt: 45 },
      { node_id: 'pack-index', kind: 'reduce', depends_on: ['embed-shard-0', 'embed-shard-1'], max_cpt: 15 },
    ],
  },
  {
    id: 'wasm-image-transform',
    name: 'Batch Image Transform (WASM)',
    category: 'Media / WASM',
    runtime: 'wasm',
    description: 'Resize, normalize, and convert image batches inside deterministic zero-permission WASM sandboxes.',
    defaultBudgetCpt: 80,
    nodes: [
      { node_id: 'batch-0', kind: 'wasm', depends_on: [], max_cpt: 30 },
      { node_id: 'batch-1', kind: 'wasm', depends_on: [], max_cpt: 30 },
      { node_id: 'manifest', kind: 'reduce', depends_on: ['batch-0', 'batch-1'], max_cpt: 20 },
    ],
  },
  {
    id: 'monte-carlo-sim',
    name: 'Monte Carlo Simulation',
    category: 'Scientific / Finance',
    runtime: 'hcode',
    description: 'Fan out deterministic seeded Monte Carlo trials across Workers and aggregate percentile statistics.',
    defaultBudgetCpt: 100,
    nodes: [
      { node_id: 'sim-0', kind: 'hcode', depends_on: [], max_cpt: 25 },
      { node_id: 'sim-1', kind: 'hcode', depends_on: [], max_cpt: 25 },
      { node_id: 'sim-2', kind: 'hcode', depends_on: [], max_cpt: 25 },
      { node_id: 'aggregate-stats', kind: 'reduce', depends_on: ['sim-0', 'sim-1', 'sim-2'], max_cpt: 25 },
    ],
  },
];

export function buildDagPipelineGraph(nodes = [], totalBudgetCpt = 0) {
  const byId = new Map();
  for (const raw of nodes) {
    const id = String(raw.node_id ?? raw.id ?? '').trim();
    if (!id) continue;
    byId.set(id, {
      id,
      kind: String(raw.kind ?? 'map'),
      dependsOn: Array.isArray(raw.depends_on) ? raw.depends_on.map(String) : [],
      state: String(raw.state ?? 'pending').toLowerCase(),
      maxCpt: Number(raw.max_cpt ?? 0) || 0,
      billedCpt: Number(raw.billed_cpt ?? 0) || 0,
    });
  }

  const depthMemo = new Map();
  const visiting = new Set();
  function depthOf(id) {
    if (depthMemo.has(id)) return depthMemo.get(id);
    if (visiting.has(id)) return 0; // guard against cyclic preview input
    visiting.add(id);
    const node = byId.get(id);
    let depth = 0;
    if (node) {
      for (const dep of node.dependsOn) {
        if (byId.has(dep)) {
          depth = Math.max(depth, depthOf(dep) + 1);
        }
      }
    }
    visiting.delete(id);
    depthMemo.set(id, depth);
    return depth;
  }

  const columnsMap = new Map();
  const counts = {
    total: byId.size,
    completed: 0,
    running: 0,
    ready: 0,
    pending: 0,
    failed: 0,
    cancelled: 0,
  };
  let totalBilledCpt = 0;

  for (const node of byId.values()) {
    const col = depthOf(node.id);
    if (!columnsMap.has(col)) columnsMap.set(col, []);
    columnsMap.get(col).push(node);
    if (Object.prototype.hasOwnProperty.call(counts, node.state)) {
      counts[node.state] += 1;
    } else {
      counts.pending += 1;
    }
    totalBilledCpt += node.billedCpt;
  }

  const stages = Array.from(columnsMap.entries())
    .sort((a, b) => a[0] - b[0])
    .map(([stageIndex, stageNodes]) => ({
      stageIndex,
      nodes: stageNodes.sort((a, b) => a.id.localeCompare(b.id)),
    }));

  const progressPct = counts.total > 0 ? Math.round((counts.completed / counts.total) * 100) : 0;
  const budgetCap = Number(totalBudgetCpt) || 0;

  return {
    stages,
    counts,
    progressPct,
    totalBilledCpt,
    budgetCapCpt: budgetCap,
    budgetRemainingCpt: Math.max(0, budgetCap - totalBilledCpt),
  };
}
