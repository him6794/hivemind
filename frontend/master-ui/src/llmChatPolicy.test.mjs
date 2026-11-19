import test from 'node:test';
import assert from 'node:assert/strict';
import {
  buildLlmRoutingHeaders,
  formatOpenAiSnippets,
  normalizeApiBaseUrl,
  parseSseChatChunk,
  requiresPublicPrivacyWarning,
  summarizeOnlineModels,
} from './llmChatPolicy.mjs';

test('normalizeApiBaseUrl appends /v1 once', () => {
  assert.equal(normalizeApiBaseUrl('http://localhost:8080'), 'http://localhost:8080/v1');
  assert.equal(normalizeApiBaseUrl('http://localhost:8080/v1/'), 'http://localhost:8080/v1');
  assert.equal(normalizeApiBaseUrl(''), 'http://127.0.0.1:8080/v1');
});

test('buildLlmRoutingHeaders emits only populated X-Hivemind-* headers', () => {
  const headers = buildLlmRoutingHeaders({
    routeMode: 'manual',
    targetWorker: 'worker-gpu-1',
    targetWorkers: ['worker-a', 'worker-b'],
    deviceTag: 'studio-mac',
    hardware: 'gpu',
    ownDevicesOnly: true,
    trustTier: 'tier0_private',
    overflowPolicy: 'strict_private',
  });
  assert.deepEqual(headers, {
    'X-Hivemind-Route-Mode': 'manual',
    'X-Hivemind-Target-Worker': 'worker-gpu-1',
    'X-Hivemind-Target-Workers': 'worker-a,worker-b',
    'X-Hivemind-Device-Tag': 'studio-mac',
    'X-Hivemind-Hardware': 'gpu',
    'X-Hivemind-Own-Devices-Only': 'true',
    'X-Hivemind-Trust-Tier': 'tier0_private',
    'X-Hivemind-Overflow-Policy': 'strict_private',
  });

  assert.deepEqual(buildLlmRoutingHeaders({}), {});
});

test('requiresPublicPrivacyWarning warns whenever public pool can be used', () => {
  assert.equal(requiresPublicPrivacyWarning({ trustTier: 'tier1_public' }), true);
  assert.equal(
    requiresPublicPrivacyWarning({ trustTier: 'tier0_private', overflowPolicy: 'prefer_private_then_public' }),
    true,
  );
  assert.equal(
    requiresPublicPrivacyWarning({ trustTier: 'tier0_private', overflowPolicy: 'strict_private' }),
    false,
  );
  assert.equal(requiresPublicPrivacyWarning({ ownDevicesOnly: true }), false);
  assert.equal(requiresPublicPrivacyWarning({ trustTier: 'tier2_tee' }), false);
});

test('parseSseChatChunk handles deltas, [DONE], and comments', () => {
  assert.equal(parseSseChatChunk(': keepalive'), null);
  assert.deepEqual(parseSseChatChunk('data: [DONE]'), { done: true });

  const chunk = parseSseChatChunk(
    'data: {"id":"cmpl-1","model":"qwen2.5-7b","choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}',
  );
  assert.equal(chunk.done, false);
  assert.equal(chunk.delta, 'Hello');
  assert.equal(chunk.model, 'qwen2.5-7b');
  assert.equal(chunk.finishReason, null);
});

test('formatOpenAiSnippets includes baseUrl, key, and model', () => {
  const snippets = formatOpenAiSnippets({
    baseUrl: 'http://127.0.0.1:8080',
    apiKey: 'hmk_secret123',
    model: 'llama-3.1-8b-q4_k_m',
  });
  assert.equal(snippets.baseUrl, 'http://127.0.0.1:8080/v1');
  assert.match(snippets.python, /hmk_secret123/);
  assert.match(snippets.python, /llama-3\.1-8b-q4_k_m/);
  assert.match(snippets.javascript, /http:\/\/127\.0\.0\.1:8080\/v1/);
  assert.match(snippets.curl, /Authorization: Bearer hmk_secret123/);
});

test('summarizeOnlineModels sorts online models first', () => {
  const rows = summarizeOnlineModels({
    data: [
      { id: 'model-b', online_workers: 0 },
      { id: 'model-a', online_workers: 4, min_vram_mb: 6144 },
      { id: 'model-c', online_workers: 2, min_vram_mb: 4096 },
    ],
  });
  assert.deepEqual(
    rows.map((r) => [r.id, r.onlineWorkers, r.status]),
    [
      ['model-a', 4, 'online'],
      ['model-c', 2, 'online'],
      ['model-b', 0, 'offline'],
    ],
  );
});
