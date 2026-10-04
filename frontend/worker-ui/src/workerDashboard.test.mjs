import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  buildWorkerDashboardRequest,
  createNonOverlappingPoll,
  deriveUsedPercent,
  hasKnownDashboardMeasurement,
  normalizeWorkerDashboard,
  PROVIDER_CREDITS_DETAIL,
  PROVIDER_CREDITS_LABEL,
} from './workerDashboard.mjs';

describe('worker dashboard contract', () => {
  it('preserves known zero values and leaves optional telemetry unknown', () => {
    const dashboard = normalizeWorkerDashboard({
      success: true,
      worker_id: 'worker-a',
      sampled_at: '2026-09-26T10:00:00Z',
      stale: true,
      host: {
        cpu_cores: 0,
        cpu_usage_percent: null,
        memory_total_gb: '32',
        memory_available_gb: 8,
        memory_usage_percent: undefined,
        gpu_count: 0,
        gpu_utilization_percent: null,
        vram_total_mb: null,
        vram_available_mb: null,
        storage_total_gb: 1000,
        storage_available_gb: 700,
      },
      assignments: [{
        task_id: 'task-1',
        submitter: 'alice',
        status: 'RUNNING',
        max_cpt: 80,
        reported_usage_cpt: null,
        usage_basis: 'reported_ops',
        usage_updated_at: null,
      }],
      settled_provider_credits_cpt: '125.5',
      currency: 'CPT',
    });

    assert.equal(dashboard.success, true);
    assert.equal(dashboard.stale, true);
    assert.equal(dashboard.host.cpu_cores, 0);
    assert.equal(dashboard.host.cpu_usage_percent, null);
    assert.equal(dashboard.host.memory_total_gb, 32);
    assert.equal(dashboard.host.memory_usage_percent, null);
    assert.equal(dashboard.host.gpu_count, 0);
    assert.equal(dashboard.host.vram_total_mb, null);
    assert.equal(dashboard.assignments[0].submitter, 'alice');
    assert.equal(dashboard.assignments[0].status, 'RUNNING');
    assert.equal(dashboard.assignments[0].max_cpt, 80);
    assert.equal(dashboard.assignments[0].reported_usage_cpt, null);
    assert.equal(dashboard.assignments[0].usage_basis, 'reported_ops');
    assert.equal(dashboard.assignments[0].usage_updated_at, null);
    assert.equal(dashboard.settled_provider_credits_cpt, 125.5);
    assert.match(PROVIDER_CREDITS_LABEL, /account-wide/i);
    assert.match(PROVIDER_CREDITS_DETAIL, /not a per-worker payout/i);
  });

  it('keeps an unsupported GPU probe unknown instead of showing zero', () => {
    const dashboard = normalizeWorkerDashboard({ success: true, host: { gpu_count: null } });
    assert.equal(dashboard.host.gpu_count, null);
  });

  it('treats null meter readings as unknown while retaining a known zero reading', () => {
    assert.equal(hasKnownDashboardMeasurement(null), false);
    assert.equal(hasKnownDashboardMeasurement(undefined), false);
    assert.equal(hasKnownDashboardMeasurement(0), true);
    assert.equal(hasKnownDashboardMeasurement(25), true);
  });

  it('derives storage and VRAM utilization only from valid capacity readings', () => {
    assert.equal(deriveUsedPercent(100, 25), 75);
    assert.equal(deriveUsedPercent(100, 0), 100);
    assert.equal(deriveUsedPercent(null, 0), null);
    assert.equal(deriveUsedPercent(0, 0), null);
    assert.equal(deriveUsedPercent(100, 120), null);
  });

  it('builds an authenticated GET request for the local dashboard endpoint', () => {
    assert.deepEqual(
      buildWorkerDashboardRequest('http://127.0.0.1:18080/', 'session-token'),
      {
        url: 'http://127.0.0.1:18080/api/worker-dashboard',
        options: {
          method: 'GET',
          headers: { Authorization: 'Bearer session-token' },
        },
      },
    );
  });

  it('builds a same-origin dashboard request when the configured base is root', () => {
    assert.equal(buildWorkerDashboardRequest('/', 'session-token').url, '/api/worker-dashboard');
  });

  it('skips overlapping polls and resumes after the active request settles', async () => {
    let releaseFirst;
    let calls = 0;
    const poll = createNonOverlappingPoll(async () => {
      calls += 1;
      if (calls === 1) {
        await new Promise((resolve) => { releaseFirst = resolve; });
      }
    });

    const firstRun = poll();
    assert.equal(await poll(), false);
    assert.equal(calls, 1);

    releaseFirst();
    assert.equal(await firstRun, true);
    assert.equal(await poll(), true);
    assert.equal(calls, 2);
  });
});
