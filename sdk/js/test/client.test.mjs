import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import { HivemindClient, HivemindError, buildRoutingHeaders } from '../index.mjs';

test('buildRoutingHeaders maps private mesh and distributed routing options', () => {
  const headers = buildRoutingHeaders({
    mode: 'distributed',
    targetWorkers: ['pc-1', 'tablet-2'],
    ownDevicesOnly: true,
    trustTier: 'tier0_private',
    overflowPolicy: 'strict_private',
    hardware: 'gpu',
  });
  assert.equal(headers['X-Hivemind-Route-Mode'], 'distributed');
  assert.equal(headers['X-Hivemind-Target-Workers'], 'pc-1,tablet-2');
  assert.equal(headers['X-Hivemind-Own-Devices-Only'], 'true');
  assert.equal(headers['X-Hivemind-Trust-Tier'], 'tier0_private');
  assert.equal(headers['X-Hivemind-Overflow-Policy'], 'strict_private');
  assert.equal(headers['X-Hivemind-Hardware'], 'gpu');
});

test('HivemindClient handles OpenAI chat, SSE streaming, WASM tasks, and DAG pipelines', async () => {
  const received = [];
  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (chunk) => {
      raw += chunk;
    });
    req.on('end', () => {
      const body = raw ? JSON.parse(raw) : null;
      received.push({ method: req.method, url: req.url, headers: req.headers, body });

      if (req.method === 'GET' && req.url === '/v1/models') {
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ object: 'list', data: [{ id: 'qwen2.5-7b-q4', object: 'model' }] }));
        return;
      }
      if (req.method === 'POST' && req.url === '/v1/chat/completions' && body?.stream) {
        res.writeHead(200, {
          'Content-Type': 'text/event-stream',
          'X-Hivemind-Data-Privacy': 'private-tier0',
        });
        res.write('data: {"choices":[{"delta":{"content":"Hello"}}]}\n\n');
        res.write('data: {"choices":[{"delta":{"content":" world"}}]}\n\n');
        res.write('data: [DONE]\n\n');
        res.end();
        return;
      }
      if (req.method === 'POST' && req.url === '/v1/chat/completions') {
        res.writeHead(200, {
          'Content-Type': 'application/json',
          'X-Hivemind-Data-Privacy': 'public-unencrypted-to-worker',
          'X-Hivemind-Trust-Tier': 'tier1_public',
        });
        res.end(JSON.stringify({ id: 'chatcmpl-1', choices: [{ message: { role: 'assistant', content: 'Hi' } }] }));
        return;
      }
      if (req.method === 'POST' && req.url === '/api/tasks') {
        res.writeHead(201, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ success: true, task_id: body.task_id }));
        return;
      }
      if (req.method === 'POST' && req.url === '/api/pipelines') {
        res.writeHead(201, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ success: true, pipeline_id: 'dag-1' }));
        return;
      }
      res.writeHead(401, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: { message: 'Invalid API key', code: 'invalid_api_key' } }));
    });
  });

  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  const client = new HivemindClient(`http://127.0.0.1:${port}`, 'hmk_test_token');

  try {
    const models = await client.listModels();
    assert.equal(models[0].id, 'qwen2.5-7b-q4');

    const chat = await client.chatCompletions({
      model: 'qwen2.5-7b-q4',
      messages: [{ role: 'user', content: 'hi' }],
      routing: { mode: 'auto', trustTier: 'tier1_public' },
    });
    assert.equal(chat.data.id, 'chatcmpl-1');
    assert.equal(chat.headers.privacy, 'public-unencrypted-to-worker');

    let streamed = '';
    for await (const chunk of client.streamChatCompletions({
      model: 'qwen2.5-7b-q4',
      messages: [{ role: 'user', content: 'stream' }],
      routing: { ownDevicesOnly: true },
    })) {
      streamed += chunk.choices?.[0]?.delta?.content ?? '';
    }
    assert.equal(streamed, 'Hello world');

    const wasm = await client.submitWasmTask({
      taskId: 'wasm-1',
      wasmBase64: 'AGFzbQEAAAA=',
      maxFuel: 50000,
    });
    assert.equal(wasm.task_id, 'wasm-1');

    const dag = await client.createDagPipeline({
      name: 'map-reduce',
      budgetCpt: 200,
      nodes: [{ id: 'map-1', depends_on: [] }, { id: 'reduce', depends_on: ['map-1'] }],
    });
    assert.equal(dag.pipeline_id, 'dag-1');

    await assert.rejects(
      () => client.networkStatus(),
      (err) => err instanceof HivemindError && err.status === 401 && err.code === 'invalid_api_key',
    );
  } finally {
    server.close();
  }
});
