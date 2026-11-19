/**
 * Zero-dependency JavaScript/TypeScript SDK for the Hivemind Master HTTP API.
 * Supports unified OpenAI-compatible LLM inference (sync + SSE streaming with
 * Auto/Manual/Private-Mesh routing headers), WASM/WASI tasks, and DAG pipelines.
 */

export class HivemindError extends Error {
  constructor(status, message, code = null) {
    super(status ? `${status}: ${message}` : message);
    this.name = 'HivemindError';
    this.status = status;
    this.code = code;
  }
}

export function buildRoutingHeaders(routing = {}) {
  const headers = {};
  if (!routing || typeof routing !== 'object') return headers;
  if (routing.mode) headers['X-Hivemind-Route-Mode'] = String(routing.mode);
  if (routing.targetWorker) headers['X-Hivemind-Target-Worker'] = String(routing.targetWorker);
  if (routing.deviceTag) headers['X-Hivemind-Device-Tag'] = String(routing.deviceTag);
  if (routing.hardware) headers['X-Hivemind-Hardware'] = String(routing.hardware);
  if (routing.ownDevicesOnly !== undefined) {
    headers['X-Hivemind-Own-Devices-Only'] = routing.ownDevicesOnly ? 'true' : 'false';
  }
  if (routing.trustTier) headers['X-Hivemind-Trust-Tier'] = String(routing.trustTier);
  if (routing.overflowPolicy) headers['X-Hivemind-Overflow-Policy'] = String(routing.overflowPolicy);
  if (Array.isArray(routing.targetWorkers) && routing.targetWorkers.length > 0) {
    headers['X-Hivemind-Target-Workers'] = routing.targetWorkers.join(',');
  }
  return headers;
}

export async function* parseSseStream(stream) {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split(/\r?\n/);
      buffer = lines.pop() ?? '';
      for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed || !trimmed.startsWith('data:')) continue;
        const payload = trimmed.slice(5).trim();
        if (payload === '[DONE]') return;
        yield JSON.parse(payload);
      }
    }
    const tail = buffer.trim();
    if (tail.startsWith('data:')) {
      const payload = tail.slice(5).trim();
      if (payload && payload !== '[DONE]') yield JSON.parse(payload);
    }
  } finally {
    reader.releaseLock();
  }
}

export class HivemindClient {
  constructor(baseUrl, token, { fetchImpl = globalThis.fetch } = {}) {
    if (!baseUrl || typeof baseUrl !== 'string') {
      throw new HivemindError(0, 'baseUrl is required');
    }
    const resolved = String(token || '').trim();
    if (!resolved) {
      throw new HivemindError(0, 'token is required');
    }
    this.baseUrl = baseUrl.replace(/\/+$/, '');
    this._token = resolved;
    this._fetch = fetchImpl;
  }

  async _request(method, path, { body, extraHeaders = {} } = {}) {
    const headers = {
      Authorization: `Bearer ${this._token}`,
      Accept: 'application/json',
      ...extraHeaders,
    };
    const init = { method, headers };
    if (body !== undefined) {
      headers['Content-Type'] = 'application/json';
      init.body = JSON.stringify(body);
    }
    const res = await this._fetch(`${this.baseUrl}${path}`, init);
    const text = await res.text();
    let data = {};
    if (text) {
      try {
        data = JSON.parse(text);
      } catch {
        data = {};
      }
    }
    if (!res.ok) {
      const msg = data?.error?.message || data?.message || res.statusText || 'request failed';
      const code = data?.error?.code || null;
      throw new HivemindError(res.status, msg, code);
    }
    return {
      data,
      headers: {
        privacy: res.headers.get('x-hivemind-data-privacy'),
        trustTier: res.headers.get('x-hivemind-trust-tier'),
        executionMode: res.headers.get('x-hivemind-execution-mode'),
        workerId: res.headers.get('x-hivemind-worker-id'),
      },
    };
  }

  // Unified OpenAI-compatible LLM endpoints

  async listModels() {
    const { data } = await this._request('GET', '/v1/models');
    return data.data ?? [];
  }

  async chatCompletions({ model, messages, maxTokens, temperature, topP, stop, routing } = {}) {
    const body = { model, messages, stream: false };
    if (maxTokens !== undefined) body.max_tokens = maxTokens;
    if (temperature !== undefined) body.temperature = temperature;
    if (topP !== undefined) body.top_p = topP;
    if (stop !== undefined) body.stop = stop;
    return this._request('POST', '/v1/chat/completions', {
      body,
      extraHeaders: buildRoutingHeaders(routing),
    });
  }

  async *streamChatCompletions({ model, messages, maxTokens, temperature, topP, stop, routing } = {}) {
    const body = { model, messages, stream: true };
    if (maxTokens !== undefined) body.max_tokens = maxTokens;
    if (temperature !== undefined) body.temperature = temperature;
    if (topP !== undefined) body.top_p = topP;
    if (stop !== undefined) body.stop = stop;

    const res = await this._fetch(`${this.baseUrl}/v1/chat/completions`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${this._token}`,
        Accept: 'text/event-stream',
        'Content-Type': 'application/json',
        ...buildRoutingHeaders(routing),
      },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      const text = await res.text();
      let data = {};
      try {
        data = JSON.parse(text);
      } catch {
        data = {};
      }
      throw new HivemindError(
        res.status,
        data?.error?.message || data?.message || res.statusText || 'stream failed',
        data?.error?.code || null,
      );
    }
    yield* parseSseStream(res.body);
  }

  async completions({ model, prompt, maxTokens, temperature, routing } = {}) {
    const body = { model, prompt, stream: false };
    if (maxTokens !== undefined) body.max_tokens = maxTokens;
    if (temperature !== undefined) body.temperature = temperature;
    return this._request('POST', '/v1/completions', {
      body,
      extraHeaders: buildRoutingHeaders(routing),
    });
  }

  // WASM / WASI general-purpose tasks

  async submitWasmTask({
    taskId,
    wasmBase64,
    entrypoint = '_start',
    inputJson = '{}',
    maxFuel = 10_000_000,
    maxMemoryPages = 256,
    maxCpt = 50,
  }) {
    const payload = {
      task_id: taskId,
      runtime: 'wasm-wasi-v1',
      task_source: wasmBase64,
      torrent: JSON.stringify({
        entrypoint,
        input: inputJson,
        max_fuel: maxFuel,
        max_memory_pages: maxMemoryPages,
      }),
      max_cpt: maxCpt,
    };
    const { data } = await this._request('POST', '/api/tasks', { body: payload });
    return data;
  }

  // Server-side Jobs & DAG Pipeline Orchestration

  async createJob({ name = '', taskSource, inputs, perItemMaxCpt, jobMaxCpt }) {
    const body = {
      name,
      task_source: taskSource,
      inputs,
      per_item_max_cpt: perItemMaxCpt,
    };
    if (jobMaxCpt !== undefined) body.job_max_cpt = jobMaxCpt;
    const { data } = await this._request('POST', '/api/jobs', { body });
    return data;
  }

  async createDagPipeline({ name, budgetCpt, nodes }) {
    const { data } = await this._request('POST', '/api/pipelines', {
      body: { name, budget_cpt: budgetCpt, nodes },
    });
    return data;
  }

  async getDagPipeline(pipelineId) {
    const { data } = await this._request('GET', `/api/pipelines/${encodeURIComponent(pipelineId)}`);
    return data;
  }

  async cancelDagPipeline(pipelineId) {
    const { data } = await this._request(
      'POST',
      `/api/pipelines/${encodeURIComponent(pipelineId)}/cancel`,
    );
    return data;
  }

  // Multi-device private mesh & public status

  async listDevices() {
    const { data } = await this._request('GET', '/api/devices');
    return data.devices ?? [];
  }

  async networkStatus() {
    const { data } = await this._request('GET', '/api/network/status');
    return data;
  }
}
