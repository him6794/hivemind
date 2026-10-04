import {
  Activity,
  CircleDollarSign,
  Cpu,
  HardDrive,
  Monitor,
  RefreshCw,
} from 'lucide-react';
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
import {
  deriveUsedPercent,
  hasKnownDashboardMeasurement,
  PROVIDER_CREDITS_DETAIL,
  PROVIDER_CREDITS_LABEL,
} from '../workerDashboard.mjs';

function formatMetric(value, unit = '', maximumFractionDigits = 1) {
  if (value === null || value === undefined || !Number.isFinite(value)) return 'Unknown';
  const formatted = new Intl.NumberFormat(undefined, { maximumFractionDigits }).format(value);
  return unit ? `${formatted} ${unit}` : formatted;
}

function formatSampledAt(value) {
  if (!value) return 'Unknown';
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(date);
}

function ResourceValue({ label, value }) {
  return (
    <div className="resource-value">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function ResourceMeter({ label, value }) {
  const known = hasKnownDashboardMeasurement(value);
  const boundedValue = known ? Math.max(0, Math.min(100, value)) : 0;
  return (
    <div className="resource-meter">
      <div className="resource-meter-label">
        <span>{label}</span>
        <strong>{formatMetric(value, '%')}</strong>
      </div>
      {known ? (
        <div
          className="resource-meter-track"
          role="meter"
          aria-label={label}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={boundedValue}
          aria-valuetext={formatMetric(value, '%')}
        >
          <span className="resource-meter-fill" style={{ width: `${boundedValue}%` }} />
        </div>
      ) : (
        <p className="resource-meter-unavailable">Telemetry unavailable</p>
      )}
    </div>
  );
}

function ResourceCard({ title, icon: Icon, children }) {
  return (
    <Card className="resource-card">
      <CardHeader className="resource-card-header">
        <span className="resource-icon" aria-hidden="true"><Icon /></span>
        <CardTitle className="resource-card-title">{title}</CardTitle>
      </CardHeader>
      <CardContent className="resource-card-content">{children}</CardContent>
    </Card>
  );
}

function CreditHeadline({ dashboard }) {
  const currency = dashboard.currency || 'CPT';
  const hasCredits = hasKnownDashboardMeasurement(dashboard.settled_provider_credits_cpt);
  const value = formatMetric(dashboard.settled_provider_credits_cpt, '', 2);
  return (
    <Card className="credit-headline">
      <CardContent className="credit-headline-content">
        <div className="credit-copy">
          <div className="credit-heading">
            <CircleDollarSign aria-hidden="true" />
            <p>{PROVIDER_CREDITS_LABEL}</p>
            <Badge variant="secondary" className="credit-account-badge">Account-wide</Badge>
          </div>
          <p className="credit-description">{PROVIDER_CREDITS_DETAIL}</p>
          <div className="credit-value-row">
            <p className="credit-value">{value}</p>
            {hasCredits ? <span className="credit-currency">{currency}</span> : null}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function AssignmentValue({ label, children }) {
  return (
    <div className="assignment-value">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function AssignmentList({ assignments }) {
  return (
    <section className="assignment-section" aria-labelledby="worker-assignments-title">
      <div className="section-heading">
        <div>
          <h3 id="worker-assignments-title">Current assignments</h3>
          <p className="text-sm text-muted-foreground">Worker-reported usage is observational, not settled billing.</p>
        </div>
        <Badge variant="outline">{assignments.length} listed</Badge>
      </div>
      {assignments.length === 0 ? (
        <div className="empty-assignments">No assignments are currently listed.</div>
      ) : (
        <ul className="assignment-list">
          {assignments.map((assignment, index) => (
            <li className="assignment-card" key={assignment.task_id || `assignment-${index}`}>
              <div className="assignment-header">
                <div className="assignment-id-wrap">
                  <span className="assignment-caption">Task ID</span>
                  <strong className="assignment-id">{assignment.task_id || 'Unknown task'}</strong>
                </div>
                <Badge variant="outline" className="assignment-status">{assignment.status || 'Unknown'}</Badge>
              </div>
              <Separator className="assignment-separator" />
              <dl className="assignment-details">
                <AssignmentValue label="Submitter">{assignment.submitter || 'Unknown'}</AssignmentValue>
                <AssignmentValue label="Task max CPT">{formatMetric(assignment.max_cpt, 'CPT', 2)}</AssignmentValue>
                <AssignmentValue label="Worker-reported usage">{formatMetric(assignment.reported_usage_cpt, 'CPT', 2)}</AssignmentValue>
                <AssignmentValue label="Usage basis">{assignment.usage_basis || 'Unknown'}</AssignmentValue>
                <AssignmentValue label="Usage last updated">{formatSampledAt(assignment.usage_updated_at)}</AssignmentValue>
              </dl>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export function DashboardSkeleton() {
  return (
    <div className="dashboard-skeleton" aria-hidden="true">
      <Skeleton className="credit-skeleton" />
      <div className="resource-grid">
        {Array.from({ length: 4 }, (_, index) => <Skeleton className="resource-skeleton" key={index} />)}
      </div>
      <Skeleton className="assignment-skeleton" />
    </div>
  );
}

export function WorkerDashboard({ dashboard, dashboardError, dashboardLoading, onRefresh }) {
  return (
    <section className="dashboard-view" aria-labelledby="worker-dashboard-title">
      <div className="dashboard-heading">
        <div>
          <p className="section-kicker">Worker</p>
          <h2 id="worker-dashboard-title">Overview</h2>
          <p className="dashboard-worker-id">Computer ID <strong>{dashboard?.worker_id || 'Unknown'}</strong></p>
        </div>
        <Button type="button" variant="outline" onClick={onRefresh} disabled={dashboardLoading}>
          <RefreshCw className={dashboardLoading ? 'animate-spin motion-reduce:animate-none' : ''} aria-hidden="true" />
          {dashboardLoading ? 'Refreshing' : 'Refresh'}
        </Button>
      </div>

      {!dashboard ? (
        dashboardLoading ? (
          <>
            <div className="dashboard-loading-note" role="status">Loading worker dashboard…</div>
            <DashboardSkeleton />
          </>
        ) : (
          <div className="dashboard-message" role="status">{dashboardError || 'Worker dashboard is unavailable.'}</div>
        )
      ) : (
        <>
          {dashboardError ? (
            <div className="dashboard-warning" role="status">
              <Activity aria-hidden="true" />
              <span>Refresh failed; showing the last received snapshot.</span>
            </div>
          ) : null}
          <div className={`dashboard-freshness${dashboard.stale || dashboardError ? ' is-stale' : ''}`} role="status">
            <span className="freshness-indicator" aria-hidden="true" />
            <strong>{dashboard.stale || dashboardError ? 'Stale snapshot' : 'Current snapshot'}</strong>
            <span>Sampled {formatSampledAt(dashboard.sampled_at)}</span>
          </div>

          <CreditHeadline dashboard={dashboard} />

          <section className="resource-section" aria-labelledby="resource-heading">
            <div className="section-heading resource-section-heading">
              <div>
                <h3 id="resource-heading">Capacity and utilization</h3>
                <p className="text-sm text-muted-foreground">Current local computer telemetry</p>
              </div>
            </div>
            <div className="resource-grid">
              <ResourceCard title="Processor" icon={Cpu}>
                <ResourceValue label="CPU cores" value={formatMetric(dashboard.host.cpu_cores, 'cores', 0)} />
                <ResourceMeter label="CPU usage" value={dashboard.host.cpu_usage_percent} />
              </ResourceCard>
              <ResourceCard title="Memory" icon={Activity}>
                <div className="resource-values">
                  <ResourceValue label="Total" value={formatMetric(dashboard.host.memory_total_gb, 'GB')} />
                  <ResourceValue label="Available" value={formatMetric(dashboard.host.memory_available_gb, 'GB')} />
                </div>
                <ResourceMeter label="Memory usage" value={dashboard.host.memory_usage_percent} />
              </ResourceCard>
              <ResourceCard title="Graphics" icon={Monitor}>
                <div className="resource-values">
                  <ResourceValue label="GPUs" value={formatMetric(dashboard.host.gpu_count, '', 0)} />
                  <ResourceValue label="VRAM total" value={formatMetric(dashboard.host.vram_total_mb, 'MB', 0)} />
                  <ResourceValue label="VRAM available" value={formatMetric(dashboard.host.vram_available_mb, 'MB', 0)} />
                </div>
                <ResourceMeter label="GPU utilization" value={dashboard.host.gpu_utilization_percent} />
                <ResourceMeter label="VRAM usage" value={deriveUsedPercent(dashboard.host.vram_total_mb, dashboard.host.vram_available_mb)} />
              </ResourceCard>
              <ResourceCard title="Storage" icon={HardDrive}>
                <div className="resource-values">
                  <ResourceValue label="Total" value={formatMetric(dashboard.host.storage_total_gb, 'GB')} />
                  <ResourceValue label="Available" value={formatMetric(dashboard.host.storage_available_gb, 'GB')} />
                </div>
                <ResourceMeter label="Storage usage" value={deriveUsedPercent(dashboard.host.storage_total_gb, dashboard.host.storage_available_gb)} />
              </ResourceCard>
            </div>
          </section>

          <AssignmentList assignments={dashboard.assignments} />
        </>
      )}
    </section>
  );
}
