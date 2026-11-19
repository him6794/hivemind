/**
 * Helpers for the Master UI LLM Chat, Model Online Status dashboard,
 * routing header construction, SSE streaming parser, and OpenAI SDK snippets.
 */

export function normalizeApiBaseUrl(origin) {
  const raw = String(origin ?? '').trim().replace(/\/+$/, '');
  if (!raw) return 'http://127.0.0.1:8080/v1';
  return raw.endsWith('/v1') ? raw : `${raw}/v1`;
}

export function buildLlmRoutingHeaders(options = {}) {
  const headers = {};
  const routeMode = String(options.routeMode ?? '').trim().toLowerCase();
  if (routeMode === 'manual' || routeMode === 'auto') {
    headers['X-Hivemind-Route-Mode'] = routeMode;
  }

  const targetWorker = String(options.targetWorker ?? '').trim();
  if (targetWorker) {
    headers['X-Hivemind-Target-Worker'] = targetWorker;
  }

  const targetWorkers = Array.isArray(options.targetWorkers)
    ? options.targetWorkers.map((w) => String(w).trim()).filter(Boolean)
    : String(options.targetWorkers ?? '')
        .split(',')
        .map((w) => w.trim())
        .filter(Boolean);
  if (targetWorkers.length > 0) {
    headers['X-Hivemind-Target-Workers'] = targetWorkers.join(',');
  }

  const deviceTag = String(options.deviceTag ?? '').trim();
  if (deviceTag) {
    headers['X-Hivemind-Device-Tag'] = deviceTag;
  }

  const hardware = String(options.hardware ?? '').trim().toLowerCase();
  if (hardware === 'gpu' || hardware === 'cpu' || hardware === 'npu') {
    headers['X-Hivemind-Hardware'] = hardware;
  }

  if (options.ownDevicesOnly === true) {
    headers['X-Hivemind-Own-Devices-Only'] = 'true';
  }

  const trustTier = String(options.trustTier ?? '').trim().toLowerCase();
  if (trustTier === 'tier0_private' || trustTier === 'tier1_public' || trustTier === 'tier2_tee') {
    headers['X-Hivemind-Trust-Tier'] = trustTier;
  }

  const overflowPolicy = String(options.overflowPolicy ?? '').trim().toLowerCase();
  if (overflowPolicy === 'strict_private' || overflowPolicy === 'prefer_private_then_public') {
    headers['X-Hivemind-Overflow-Policy'] = overflowPolicy;
  }

  return headers;
}

export function requiresPublicPrivacyWarning(options = {}) {
  if (options.ownDevicesOnly === true) return false;
  const tier = String(options.trustTier ?? 'tier1_public').trim().toLowerCase();
  const overflow = String(options.overflowPolicy ?? 'prefer_private_then_public').trim().toLowerCase();
  if (tier === 'tier0_private' && overflow === 'strict_private') {
    return false;
  }
  return tier !== 'tier2_tee';
}

export function parseSseChatChunk(rawLine) {
  const line = String(rawLine ?? '').trim();
  if (!line || line.startsWith(':') || !line.startsWith('data:')) {
    return null;
  }
  const payload = line.slice(5).trim();
  if (!payload) return null;
  if (payload === '[DONE]') {
    return { done: true };
  }
  try {
    const parsed = JSON.parse(payload);
    const choice = Array.isArray(parsed.choices) ? parsed.choices[0] : null;
    const delta =
      choice?.delta?.content ??
      choice?.text ??
      choice?.message?.content ??
      '';
    return {
      done: false,
      id: parsed.id ?? null,
      model: parsed.model ?? null,
      delta: typeof delta === 'string' ? delta : '',
      finishReason: choice?.finish_reason ?? null,
      usage: parsed.usage ?? null,
      workerId: parsed.hivemind_worker_id ?? null,
    };
  } catch {
    return null;
  }
}

export function formatOpenAiSnippets({ baseUrl, apiKey, model } = {}) {
  const v1Url = normalizeApiBaseUrl(baseUrl);
  const key = String(apiKey ?? '').trim() || 'hmk_your_api_key_here';
  const modelId = String(model ?? '').trim() || 'qwen2.5-7b-instruct-q4_k_m';

  const python = [
    'from openai import OpenAI',
    '',
    `client = OpenAI(base_url="${v1Url}", api_key="${key}")`,
    'stream = client.chat.completions.create(',
    `    model="${modelId}",`,
    '    messages=[{"role": "user", "content": "Hello from Hivemind!"}],',
    '    stream=True,',
    ')',
    'for chunk in stream:',
    '    print(chunk.choices[0].delta.content or "", end="", flush=True)',
  ].join('\n');

  const javascript = [
    'import OpenAI from "openai";',
    '',
    `const client = new OpenAI({ baseURL: "${v1Url}", apiKey: "${key}" });`,
    'const stream = await client.chat.completions.create({',
    `  model: "${modelId}",`,
    '  messages: [{ role: "user", content: "Hello from Hivemind!" }],',
    '  stream: true,',
    '});',
    'for await (const chunk of stream) {',
    '  process.stdout.write(chunk.choices[0]?.delta?.content ?? "");',
    '}',
  ].join('\n');

  const curl = [
    `curl -N ${v1Url}/chat/completions \\`,
    `  -H "Authorization: Bearer ${key}" \\`,
    '  -H "Content-Type: application/json" \\',
    `  -d '{"model":"${modelId}","messages":[{"role":"user","content":"Hello!"}],"stream":true}'`,
  ].join('\n');

  return { baseUrl: v1Url, python, javascript, curl };
}

export function summarizeOnlineModels(modelsPayload) {
  const list = Array.isArray(modelsPayload?.data)
    ? modelsPayload.data
    : Array.isArray(modelsPayload)
      ? modelsPayload
      : [];
  return list
    .map((item) => {
      const onlineWorkers = Number(item.online_workers ?? item.onlineWorkers ?? 0);
      return {
        id: String(item.id ?? item.model_id ?? ''),
        ownedBy: String(item.owned_by ?? 'hivemind'),
        onlineWorkers: Number.isFinite(onlineWorkers) ? onlineWorkers : 0,
        minVramMb: Number(item.min_vram_mb ?? item.minVramMb ?? 0) || 0,
        status: onlineWorkers > 0 ? 'online' : 'offline',
      };
    })
    .filter((entry) => entry.id.length > 0)
    .sort((a, b) => b.onlineWorkers - a.onlineWorkers || a.id.localeCompare(b.id));
}
