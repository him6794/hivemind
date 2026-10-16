import { useLocale } from '../LocaleProvider.jsx';
import { Cpu, RefreshCw } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';
import { Separator } from '@/components/ui/separator';
import { Skeleton } from '@/components/ui/skeleton';

const STATE_LABELS = {
  'not-run': 'Not run',
  queued: 'Queued',
  running: 'Measuring',
  ready: 'Measured',
  unsupported: 'Not supported',
  'no-device': 'No device',
  failed: 'Failed',
  timedout: 'Timed out',
  stale: 'Stale',
};

function stateVariant(state) {
  if (state === 'ready' || state === 'running') return 'secondary';
  if (state === 'failed' || state === 'timedout') return 'destructive';
  return 'outline';
}

function StateBadge({ state }) {
  const { t } = useLocale();
  const key = state || 'not-run';
  return <Badge variant={stateVariant(key)}>{Object.hasOwn(STATE_LABELS, key) ? t(STATE_LABELS[key]) : key}</Badge>;
}

function CaseList({ cases }) {
  const { t, formatMetric } = useLocale();
  if (!cases.length) return null;
  return (
    <ul className="benchmark-case-list">
      {cases.map((caseEntry, index) => (
        <li className="benchmark-case" key={caseEntry.name || index}>
          <span className="benchmark-case-name">{caseEntry.name}</span>
          <span className="benchmark-case-detail">
            {caseEntry.threads !== null ? t(caseEntry.threads === 1 ? '{count} thread' : '{count} threads', { count: caseEntry.threads }) : t("Unknown")}
            {' · '}
            {formatMetric(caseEntry.median_gflops, 'GFLOP/s')}
          </span>
        </li>
      ))}
    </ul>
  );
}

function MeasurementRow({ label, icon: Icon, measurement }) {
  const { t, formatMetric, formatTimestamp } = useLocale();
  const measured = measurement.state === 'ready' || measurement.state === 'stale';
  return (
    <div className="benchmark-measurement">
      <div className="benchmark-measurement-head">
        <span className="resource-icon" aria-hidden="true"><Icon /></span>
        <strong className="benchmark-measurement-title">{label}</strong>
        <StateBadge state={measurement.state} />
      </div>
      <dl className="benchmark-measurement-meta">
        <div>
          <dt>{t("Score")}</dt>
          <dd>{formatMetric(measurement.score, '', 3)}</dd>
        </div>
        <div>
          <dt>{t("Threads")}</dt>
          <dd>{measurement.threads === null ? t("Unknown") : measurement.threads}</dd>
        </div>
        <div>
          <dt>{t("Measured")}</dt>
          <dd>{measurement.state === 'ready' || measurement.state === 'stale' ? formatTimestamp(measurement.measured_at) : '—'}</dd>
        </div>
      </dl>
      {measured && measurement.cases.length > 0 ? <CaseList cases={measurement.cases} /> : null}
      {measurement.reason ? (
        <p className="benchmark-reason" role="status">{measurement.reason}</p>
      ) : null}
    </div>
  );
}

export function WorkerBenchmark({
  benchmark,
  benchmarkError,
  benchmarkLoading,
  rerunLoading,
  onRerun,
}) {
  const { t } = useLocale();
  return (
    <Card className="benchmark-card">
      <CardHeader className="computer-card-header">
        <div className="computer-card-heading">
          <span className="resource-icon" aria-hidden="true"><Cpu /></span>
          <div>
            <CardTitle className="text-base">{t("Hardware benchmark")}</CardTitle>
            <CardDescription>
              {benchmark?.suite ? t('{suite} · measured locally', { suite: benchmark.suite }) : t("Measured performance, not an estimate")}
            </CardDescription>
          </div>
        </div>
        <div className="benchmark-rerun" role="group" aria-label={t("Rerun benchmark")}>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onRerun('all')}
            disabled={rerunLoading || benchmark?.busy}
          >
            <RefreshCw className={rerunLoading ? 'animate-spin motion-reduce:animate-none' : ''} aria-hidden="true" />{t("Rerun all")}</Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => onRerun('cpu')}
            disabled={rerunLoading || benchmark?.busy}
          >
            CPU
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => onRerun('gpu')}
            disabled={rerunLoading || benchmark?.busy}
          >
            GPU
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        {!benchmark ? (
          benchmarkLoading ? (
            <div className="benchmark-loading" aria-label={t("Loading benchmark")} role="status">
              <Skeleton className="benchmark-skeleton-line" />
              <Skeleton className="benchmark-skeleton-line" />
            </div>
          ) : (
            <div className="benchmark-message" role="status">{t(benchmarkError || 'Benchmark is unavailable.')}</div>
          )
        ) : (
          <>
            <div className={`benchmark-freshness${benchmark.busy ? ' is-busy' : ''}`} role="status">
              <strong>{benchmark.busy ? t("Measuring… this computer is not taking new tasks") : benchmark.release_build ? t("Release measurements") : t("Debug measurements (not for comparison)")}</strong>
            </div>
            <Separator className="benchmark-separator" />
            <MeasurementRow label={t("Processor")} icon={Cpu} measurement={benchmark.cpu} />
            {benchmark.gpu.devices.length > 0 ? (
              benchmark.gpu.devices.map((device) => (
                <div key={device.pci_bus_id || device.name} className="benchmark-gpu">
                  <p className="benchmark-gpu-title">{device.name}
                    {device.backend ? ` · ${device.backend}` : ''}
                    {device.device_type ? ` · ${device.device_type}` : ''}
                  </p>
                  <MeasurementRow label={t("Graphics")} icon={Cpu} measurement={device} />
                </div>
              ))
            ) : (
              <MeasurementRow
                label={t("Graphics")}
                icon={Cpu}
                measurement={{
                  state: benchmark.gpu.state,
                  score: null,
                  threads: null,
                  measured_at: null,
                  reason: benchmark.gpu.reason,
                  cases: [],
                }}
              />
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}
