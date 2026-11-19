import { useCallback, useEffect, useMemo, useState } from 'react';
import { Copy, MessageSquare, RefreshCw, Send, ShieldAlert, ShieldCheck } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import {
  buildLlmRoutingHeaders,
  formatOpenAiSnippets,
  normalizeApiBaseUrl,
  parseSseChatChunk,
  requiresPublicPrivacyWarning,
  summarizeOnlineModels,
} from './llmChatPolicy.mjs';

export function LlmChatPanel({ token, apiBaseOrigin }) {
  const [models, setModels] = useState([]);
  const [networkStatus, setNetworkStatus] = useState(null);
  const [selectedModel, setSelectedModel] = useState('qwen2.5-7b-instruct-q4_k_m');
  const [routeMode, setRouteMode] = useState('auto');
  const [targetWorker, setTargetWorker] = useState('');
  const [deviceTag, setDeviceTag] = useState('');
  const [hardware, setHardware] = useState('');
  const [trustTier, setTrustTier] = useState('tier1_public');
  const [overflowPolicy, setOverflowPolicy] = useState('prefer_private_then_public');
  const [ownDevicesOnly, setOwnDevicesOnly] = useState(false);

  const [messages, setMessages] = useState([]);
  const [prompt, setPrompt] = useState('');
  const [streaming, setStreaming] = useState(false);
  const [errorText, setErrorText] = useState('');
  const [lastPrivacyHeader, setLastPrivacyHeader] = useState('');
  const [lastWorkerId, setLastWorkerId] = useState('');
  const [copiedLabel, setCopiedLabel] = useState('');
  const [loadingModels, setLoadingModels] = useState(false);

  const baseUrl = useMemo(() => normalizeApiBaseUrl(apiBaseOrigin || window.location.origin), [apiBaseOrigin]);
  const snippets = useMemo(
    () => formatOpenAiSnippets({ baseUrl, apiKey: token, model: selectedModel }),
    [baseUrl, token, selectedModel],
  );
  const showPublicWarning = useMemo(
    () => requiresPublicPrivacyWarning({ trustTier, overflowPolicy, ownDevicesOnly }),
    [trustTier, overflowPolicy, ownDevicesOnly],
  );

  const refreshModels = useCallback(async () => {
    if (!token) return;
    setLoadingModels(true);
    setErrorText('');
    try {
      const [modelsRes, statusRes] = await Promise.all([
        fetch('/v1/models', { headers: { Authorization: `Bearer ${token}` } }),
        fetch('/api/network/status', { headers: { Authorization: `Bearer ${token}` } }).catch(() => null),
      ]);
      if (modelsRes.ok) {
        const data = await modelsRes.json();
        const rows = summarizeOnlineModels(data);
        setModels(rows);
        if (rows.length > 0 && !rows.some((r) => r.id === selectedModel)) {
          setSelectedModel(rows[0].id);
        }
      }
      if (statusRes && statusRes.ok) {
        setNetworkStatus(await statusRes.json());
      }
    } catch (err) {
      setErrorText(String(err?.message || err));
    } finally {
      setLoadingModels(false);
    }
  }, [token, selectedModel]);

  useEffect(() => {
    void refreshModels();
  }, [refreshModels]);

  async function copyText(label, text) {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedLabel(label);
      setTimeout(() => setCopiedLabel(''), 2000);
    } catch {
      setCopiedLabel('');
    }
  }

  async function sendChat(event) {
    event.preventDefault();
    const content = prompt.trim();
    if (!content || streaming || !token) return;

    const nextMessages = [...messages, { role: 'user', content }];
    setMessages([...nextMessages, { role: 'assistant', content: '' }]);
    setPrompt('');
    setStreaming(true);
    setErrorText('');

    const routingHeaders = buildLlmRoutingHeaders({
      routeMode,
      targetWorker,
      deviceTag,
      hardware,
      ownDevicesOnly,
      trustTier,
      overflowPolicy,
    });

    try {
      const res = await fetch('/v1/chat/completions', {
        method: 'POST',
        headers: {
          Authorization: `Bearer ${token}`,
          'Content-Type': 'application/json',
          ...routingHeaders,
        },
        body: JSON.stringify({
          model: selectedModel,
          messages: nextMessages,
          stream: true,
        }),
      });

      setLastPrivacyHeader(res.headers.get('X-Hivemind-Data-Privacy') || '');
      setLastWorkerId(res.headers.get('X-Hivemind-Worker-Id') || '');

      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body?.error?.message || `HTTP ${res.status}`);
      }

      if (!res.body) {
        const data = await res.json();
        const reply = data?.choices?.[0]?.message?.content ?? '';
        setMessages([...nextMessages, { role: 'assistant', content: reply }]);
        return;
      }

      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';
      let assistantText = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split(/\r?\n/);
        buffer = lines.pop() ?? '';
        for (const line of lines) {
          const parsed = parseSseChatChunk(line);
          if (!parsed) continue;
          if (parsed.done) break;
          if (parsed.delta) {
            assistantText += parsed.delta;
            setMessages([...nextMessages, { role: 'assistant', content: assistantText }]);
          }
          if (parsed.workerId) {
            setLastWorkerId(parsed.workerId);
          }
        }
      }
    } catch (err) {
      setErrorText(String(err?.message || err));
      setMessages(nextMessages);
    } finally {
      setStreaming(false);
    }
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader className="section-card-header">
          <div>
            <CardTitle className="section-title flex items-center gap-2">
              <MessageSquare aria-hidden="true" className="size-4" />
              LLM Chat &amp; Online Model Pool
            </CardTitle>
            <CardDescription>
              OpenAI-compatible streaming chat over your private mesh (Tier 0) or the public mutual-aid pool (Tier 1).
            </CardDescription>
          </div>
          <Button type="button" variant="outline" size="sm" onClick={() => void refreshModels()} disabled={loadingModels}>
            <RefreshCw aria-hidden="true" className="size-4" />
            {loadingModels ? 'Refreshing…' : 'Refresh Models'}
          </Button>
        </CardHeader>
        <CardContent className="space-y-4">
          {showPublicWarning ? (
            <div className="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-sm" role="status">
              <ShieldAlert aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-amber-600" />
              <div>
                <strong>Public Pool Privacy Notice:</strong> Tier 1 public inference sends prompts in plaintext to contributing Worker nodes. Switch to <code>Tier 0 Private (Strict)</code> or enable <code>Own Devices Only</code> for sensitive data.
              </div>
            </div>
          ) : (
            <div className="flex items-start gap-2 rounded-md border border-emerald-500/40 bg-emerald-500/10 p-3 text-sm" role="status">
              <ShieldCheck aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-emerald-600" />
              <div>
                <strong>Private Mesh Protection Active:</strong> Requests are restricted to your own enrolled devices.
              </div>
            </div>
          )}

          <div className="grid gap-3 md:grid-cols-3">
            <div>
              <Label htmlFor="llm-model-select">Model</Label>
              <Input
                id="llm-model-select"
                list="hivemind-online-models"
                value={selectedModel}
                onChange={(e) => setSelectedModel(e.target.value)}
                placeholder="qwen2.5-7b-instruct-q4_k_m"
              />
              <datalist id="hivemind-online-models">
                {models.map((m) => (
                  <option key={m.id} value={m.id}>{`${m.id} (${m.onlineWorkers} workers)`}</option>
                ))}
              </datalist>
            </div>
            <div>
              <Label htmlFor="llm-trust-tier">Trust Tier</Label>
              <select
                id="llm-trust-tier"
                className="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm"
                value={trustTier}
                onChange={(e) => setTrustTier(e.target.value)}
              >
                <option value="tier1_public">Tier 1 — Public Mutual-Aid Pool</option>
                <option value="tier0_private">Tier 0 — Private Own-Device Mesh</option>
                <option value="tier2_tee">Tier 2 — Confidential TEE (Beta)</option>
              </select>
            </div>
            <div>
              <Label htmlFor="llm-overflow">Overflow Policy</Label>
              <select
                id="llm-overflow"
                className="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm"
                value={overflowPolicy}
                onChange={(e) => setOverflowPolicy(e.target.value)}
              >
                <option value="prefer_private_then_public">Prefer Private, Overflow to Public</option>
                <option value="strict_private">Strict Private Only (No Overflow)</option>
              </select>
            </div>
          </div>

          <div className="grid gap-3 md:grid-cols-4">
            <div>
              <Label htmlFor="llm-route-mode">Route Mode</Label>
              <select
                id="llm-route-mode"
                className="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm"
                value={routeMode}
                onChange={(e) => setRouteMode(e.target.value)}
              >
                <option value="auto">Auto (Optimal)</option>
                <option value="manual">Manual Target</option>
              </select>
            </div>
            <div>
              <Label htmlFor="llm-target-worker">Target Worker ID</Label>
              <Input
                id="llm-target-worker"
                value={targetWorker}
                onChange={(e) => setTargetWorker(e.target.value)}
                placeholder="optional worker_id"
              />
            </div>
            <div>
              <Label htmlFor="llm-device-tag">Device Tag</Label>
              <Input
                id="llm-device-tag"
                value={deviceTag}
                onChange={(e) => setDeviceTag(e.target.value)}
                placeholder="e.g. home-rtx4090"
              />
            </div>
            <div>
              <Label htmlFor="llm-hardware">Hardware Filter</Label>
              <select
                id="llm-hardware"
                className="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm"
                value={hardware}
                onChange={(e) => setHardware(e.target.value)}
              >
                <option value="">Any Hardware</option>
                <option value="gpu">GPU Only</option>
                <option value="cpu">CPU Only</option>
                <option value="npu">NPU Only</option>
              </select>
            </div>
          </div>

          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={ownDevicesOnly}
              onChange={(e) => setOwnDevicesOnly(e.target.checked)}
            />
            Restrict to my own enrolled devices only (<code>X-Hivemind-Own-Devices-Only</code>)
          </label>

          {models.length > 0 ? (
            <div className="flex flex-wrap gap-2">
              {models.map((m) => (
                <Badge
                  key={m.id}
                  variant={m.status === 'online' ? 'default' : 'secondary'}
                  className="cursor-pointer"
                  onClick={() => setSelectedModel(m.id)}
                >
                  {m.id} · {m.onlineWorkers} online
                </Badge>
              ))}
            </div>
          ) : (
            <p className="text-xs text-muted-foreground">
              {networkStatus
                ? `Online workers: ${networkStatus.online_workers ?? 0}`
                : 'No online LLM models reported yet — start a Worker with an approved model or enter a model ID.'}
            </p>
          )}

          <div className="max-h-72 space-y-2 overflow-y-auto rounded-md border p-3">
            {messages.length === 0 ? (
              <p className="text-sm text-muted-foreground">Send a prompt to start streaming from Hivemind.</p>
            ) : (
              messages.map((msg, idx) => (
                <div
                  key={`${msg.role}-${idx}`}
                  className={msg.role === 'user' ? 'rounded-md bg-muted/60 p-2 text-sm' : 'rounded-md border p-2 text-sm'}
                >
                  <div className="mb-1 text-xs font-semibold uppercase text-muted-foreground">{msg.role}</div>
                  <div className="whitespace-pre-wrap">{msg.content || (streaming ? '…' : '')}</div>
                </div>
              ))
            )}
          </div>

          {lastWorkerId || lastPrivacyHeader ? (
            <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
              {lastWorkerId ? <span>Worker: <code>{lastWorkerId}</code></span> : null}
              {lastPrivacyHeader ? <span>Privacy header: <code>{lastPrivacyHeader}</code></span> : null}
            </div>
          ) : null}

          {errorText ? (
            <p className="field-error" role="alert">{errorText}</p>
          ) : null}

          <form onSubmit={(e) => void sendChat(e)} className="space-y-2">
            <Textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="Ask anything…"
              rows={3}
            />
            <div className="flex items-center gap-2">
              <Button type="submit" disabled={streaming || !prompt.trim()}>
                <Send aria-hidden="true" className="size-4" />
                {streaming ? 'Streaming…' : 'Send Message'}
              </Button>
              <Button type="button" variant="outline" onClick={() => setMessages([])} disabled={streaming}>
                Clear Chat
              </Button>
            </div>
          </form>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="section-card-header">
          <div>
            <CardTitle className="section-title">OpenAI-Compatible API Integration</CardTitle>
            <CardDescription>
              Use your Master endpoint directly with Python <code>openai</code>, JS/TS SDK, Continue.dev, Cline, or Open WebUI.
            </CardDescription>
          </div>
          {copiedLabel ? <Badge variant="secondary">Copied {copiedLabel}</Badge> : null}
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <code className="rounded bg-muted px-2 py-1 text-xs">{snippets.baseUrl}</code>
            <Button type="button" variant="outline" size="sm" onClick={() => void copyText('Base URL', snippets.baseUrl)}>
              <Copy aria-hidden="true" className="size-3.5" />
              Copy Base URL
            </Button>
            <Button type="button" variant="outline" size="sm" onClick={() => void copyText('Python Snippet', snippets.python)}>
              <Copy aria-hidden="true" className="size-3.5" />
              Copy Python
            </Button>
            <Button type="button" variant="outline" size="sm" onClick={() => void copyText('JS Snippet', snippets.javascript)}>
              <Copy aria-hidden="true" className="size-3.5" />
              Copy JS/TS
            </Button>
            <Button type="button" variant="outline" size="sm" onClick={() => void copyText('curl Command', snippets.curl)}>
              <Copy aria-hidden="true" className="size-3.5" />
              Copy curl
            </Button>
          </div>
          <pre className="overflow-x-auto rounded-md bg-muted p-3 text-xs">{snippets.python}</pre>
        </CardContent>
      </Card>
    </div>
  );
}
