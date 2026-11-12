import { useEffect, useRef, useState } from 'react';
import { useLocale } from '../LocaleProvider.jsx';
import { buildSharingRequest, sharingView } from '../workerSharing.mjs';
import { fetchWithDeadline } from '../workerStartup.mjs';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

function fetchJsonWithDeadline(url, options) {
  return fetchWithDeadline(fetch, url, options, {
    timeoutMs: 15000,
    consumeResponse: async (response) => ({ ok: response.ok, status: response.status, data: await response.json() }),
  });
}

export function WorkerSharing({ workerControlBase, token, backendReady, onUnauthorized }) {
  const [sharing, setSharing] = useState(null);
  const [settings, setSettings] = useState(null);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const generation = useRef(0);
  const unauthorized = useRef(onUnauthorized);
  unauthorized.current = onUnauthorized;

  useEffect(() => {
    const current = ++generation.current;
    setSharing(null);
    setSettings(null);
    setError('');
    setSaving(false);
    if (!token || !backendReady) return undefined;
    const controller = new AbortController();
    let pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try {
        const request = buildSharingRequest(workerControlBase, token);
        const response = await fetchJsonWithDeadline(request.url, { ...request.options, signal: controller.signal });
        if (generation.current !== current) return;
        if (response.status === 401) { unauthorized.current?.(); return; }
        if (!response.ok || !response.data.success) throw new Error('Sharing status is unavailable.');
        setSharing(response.data);
        setSettings((existing) => existing || response.data.settings);
        setError('');
      } catch {
        if (generation.current === current) {
          setSharing(null);
          setError('Sharing status is unavailable.');
        }
      } finally { pending = false; }
    };
    void refresh();
    const timer = setInterval(refresh, 10000);
    return () => { ++generation.current; controller.abort(); clearInterval(timer); };
  }, [token, workerControlBase, backendReady]);

  async function save(event) {
    event.preventDefault();
    const current = generation.current;
    setSaving(true);
    setError('');
    try {
      const request = buildSharingRequest(workerControlBase, token, settings);
      const response = await fetchJsonWithDeadline(request.url, request.options);
      if (generation.current !== current) return;
      if (response.status === 401) { unauthorized.current?.(); return; }
      if (!response.ok || !response.data.success) throw new Error('Sharing settings could not be saved.');
      setSharing(response.data);
      setSettings(response.data.settings);
    } catch {
      if (generation.current === current) setError('Sharing settings could not be saved.');
    } finally {
      if (generation.current === current) setSaving(false);
    }
  }

  return <SharingPanel sharing={sharing} settings={settings} error={error} saving={saving} onSave={save} onChange={setSettings} />;
}

export function SharingPanel({ sharing, settings, error, saving, onSave, onChange }) {
  const { t } = useLocale();
  const view = sharingView(sharing);
  return <Card>
    <CardHeader>
      <CardTitle className="text-base">{t('Sharing availability')}</CardTitle>
      <CardDescription>{t('Choose when this computer accepts new tasks. Running tasks finish normally.')}</CardDescription>
    </CardHeader>
    <CardContent>
      <p role="status">{t(view.status)}</p>
      {sharing && <p className="text-sm text-muted-foreground">{t(view.explanation)}</p>}
      {error && <p role="alert">{t(error)}</p>}
      {settings && <form className="grid gap-4 mt-4" onSubmit={onSave}>
        <div className="grid gap-2">
          <Label htmlFor="sharing-mode">{t('Sharing mode')}</Label>
          <select id="sharing-mode" className="h-9 rounded-md border bg-background px-3 text-sm" value={settings.mode} disabled={saving} onChange={(event) => onChange({ ...settings, mode: event.target.value })}>
            <option value="always">{t('Always share')}</option>
            <option value="idle">{t('Share only while idle')}</option>
          </select>
        </div>
        <div className="grid gap-2">
          <Label htmlFor="sharing-seconds">{t('Idle period (seconds)')}</Label>
          <Input id="sharing-seconds" type="number" min="1" max="86400" step="1" required value={settings.idle_seconds} disabled={saving || settings.mode === 'always'} onChange={(event) => onChange({ ...settings, idle_seconds: event.target.value })} />
        </div>
        {sharing?.source === 'cpu_usage' && <div className="grid gap-2">
          <Label htmlFor="sharing-cpu">{t('CPU usage threshold (%)')}</Label>
          <Input id="sharing-cpu" type="number" min="0" max="100" step="0.1" required value={settings.cpu_percent_threshold} disabled={saving || settings.mode === 'always'} onChange={(event) => onChange({ ...settings, cpu_percent_threshold: event.target.value })} />
        </div>}
        <Button type="submit" disabled={saving || !sharing}>{t(saving ? 'Saving sharing settings…' : 'Save sharing settings')}</Button>
      </form>}
    </CardContent>
  </Card>;
}
