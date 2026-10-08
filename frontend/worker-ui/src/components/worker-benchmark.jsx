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

function formatScore(value) {
  if (value === null || value === undefined || !Number.isFinite(value)) return 'Unknown';
  return new Intl.NumberFormat(undefined).format(value);
}

function formatGflops(value) {
  if (value === null || value === undefined || !Number.isFinite(value)) return 'Unknown';
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)} GFLOP/s`;
}

function formatMeasuredAt(value) {
  if (!value) return 'Unknown';
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(date);
}

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
  const key = state || 'not-run';
  return <Badge variant={stateVariant(key)}>{STATE_LABELS[key] || key}</Badge>;
}

function CaseList({ cases }) {
  if (!cases.length) return null;
  return (
    <ul className="benchmark-case-list">
      {cases.map((caseEntry, index) => (
        <li className="benchmark-case" key={caseEntry.name || index}>
          <span className="benchmark-case-name">{caseEntry.name}</span>
          <span className="benchmark-case-detail">
            {caseEntry.threads !== null ? `${caseEntry.threads} thread${caseEntry.threads === 1 ? '' : 's'}` : 'Unknown'}
            {' · '}
            {formatGflops(caseEntry.median_gflops)}
          </span>
        </li>
      ))}
    </ul>
  );
}

function MeasurementRow({ label, icon: Icon, measurement }) {
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
          <dt>Score</dt>
          <dd>{formatScore(measurement.score)}</dd>
        </div>
        <div>
          <dt>Threads</dt>
          <dd>{measurement.threads === null ? 'Unknown' : measurement.threads}</dd>
        </div>
        <div>
          <dt>Measured</dt>
          <dd>{measurement.state === 'ready' || measurement.state === 'stale' ? formatMeasuredAt(measurement.measured_at) : '—'}</dd>
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
  return (
    <Card className="benchmark-card">
      <CardHeader className="computer-card-header">
        <div className="computer-card-heading">
          <span className="resource-icon" aria-hidden="true"><Cpu /></span>
          <div>
            <CardTitle className="text-base">Hardware benchmark</CardTitle>
            <CardDescription>
              {benchmark?.suite ? `${benchmark.suite} · measured locally` : 'Measured performance, not an estimate'}
            </CardDescription>
          </div>
        </div>
        <div className="benchmark-rerun" role="group" aria-label="Rerun benchmark">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onRerun('all')}
            disabled={rerunLoading || benchmark?.busy}
          >
            <RefreshCw className={rerunLoading ? 'animate-spin motion-reduce:animate-none' : ''} aria-hidden="true" />
            Rerun all
          </Button>
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
            <div className="benchmark-loading" aria-label="Loading benchmark" role="status">
              <Skeleton className="benchmark-skeleton-line" />
              <Skeleton className="benchmark-skeleton-line" />
            </div>
          ) : (
            <div className="benchmark-message" role="status">{benchmarkError || 'Benchmark is unavailable.'}</div>
          )
        ) : (
          <>
            <div className={`benchmark-freshness${benchmark.busy ? ' is-busy' : ''}`} role="status">
              <strong>{benchmark.busy ? 'Measuring… this computer is not taking new tasks' : benchmark.release_build ? 'Release measurements' : 'Debug measurements (not for comparison)'}</strong>
            </div>
            <Separator className="benchmark-separator" />
            <MeasurementRow label="Processor" icon={Cpu} measurement={benchmark.cpu} />
            {benchmark.gpu.devices.length > 0 ? (
              benchmark.gpu.devices.map((device) => (
                <div key={device.pci_bus_id || device.name} className="benchmark-gpu">
                  <p className="benchmark-gpu-title">{device.name}
                    {device.backend ? ` · ${device.backend}` : ''}
                    {device.device_type ? ` · ${device.device_type}` : ''}
                  </p>
                  <MeasurementRow label="Graphics" icon={Cpu} measurement={device} />
                </div>
              ))
            ) : (
              <MeasurementRow
                label="Graphics"
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
