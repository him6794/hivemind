import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  buildBenchmarkRerunRequest,
  buildBenchmarkStatusRequest,
  normalizeBenchmark,
} from './workerBenchmark.mjs';

function readyCase(name, dimension, threads, elapsedNs) {
  // Five batches with the same repetitions; the median is the middle elapsed.
  return {
    name,
    dimension,
    threads,
    batches: elapsedNs.map((value) => ({ repetitions: 1, elapsed_ns: value })),
  };
}

describe('worker benchmark projection', () => {
  it('leaves an unmeasured report unknown rather than zero', () => {
    const normalized = normalizeBenchmark({
      success: true,
      busy: false,
      threads: 8,
      report: {
        suite: 'hivemind-hardware-v1',
        build_id: 'deadbeef',
        release_build: false,
        cpu: { state: 'not-run', identity: '', threads: 8, correctness_passed: false, cases: [] },
        gpu_state: 'not-run',
        gpu_reason: null,
        gpus: [],
      },
    });

    assert.equal(normalized.cpu.state, 'not-run');
    assert.equal(normalized.cpu.score, null);
    assert.equal(normalized.gpu.state, 'not-run');
    assert.deepEqual(normalized.gpu.devices, []);
    assert.equal(normalized.release_build, false);
  });

  it('surfaces server-derived scores and raw throughput for a ready report', () => {
    const cpuCase = readyCase('cpu-f32-allocated', 128, 8, [100, 200, 300, 400, 500]);
    const gpuCase = readyCase('gpu-f32-1024', 1024, 1, [100, 200, 300, 400, 500]);
    const expectedGpu = (2 * 1024 ** 3 * 1 * 1) / 300; // median = the 300ns batch

    const normalized = normalizeBenchmark({
      success: true,
      busy: false,
      threads: 8,
      cpu_score: 123,
      gpu_scores: [456],
      report: {
        suite: 'hivemind-hardware-v1',
        build_id: 'deadbeef',
        release_build: true,
        cpu: {
          state: 'ready',
          measured_at: '2026-10-08T09:00:00Z',
          identity: 'Intel(R) Xeon(R)',
          threads: 8,
          correctness_passed: true,
          cases: [cpuCase],
        },
        gpu_state: 'ready',
        gpu_reason: null,
        gpus: [
          {
            device: {
              backend: 'dx12',
              vendor: 0x10de,
              device: 0x1e04,
              name: 'NVIDIA GeForce GTX 960',
              driver: 'NVIDIA',
              driver_info: '551.86',
              device_type: 'discrete-gpu',
              pci_bus_id: '0000:01:00.0',
            },
            measurement: {
              state: 'ready',
              measured_at: '2026-10-08T09:00:05Z',
              identity: 'GTX 960',
              threads: 1,
              correctness_passed: true,
              cases: [gpuCase],
            },
          },
        ],
      },
    });

    assert.equal(normalized.cpu.score, 123);
    assert.equal(normalized.cpu.state, 'ready');
    assert.equal(normalized.cpu.measured_at, '2026-10-08T09:00:00Z');
    assert.equal(normalized.cpu.cases[0].median_gflops, (2 * 128 ** 3 * 1 * 8) / 300);

    const gpu = normalized.gpu.devices[0];
    assert.equal(gpu.name, 'NVIDIA GeForce GTX 960');
    assert.equal(gpu.backend, 'dx12');
    assert.equal(gpu.device_type, 'discrete-gpu');
    assert.equal(gpu.score, 456);
    assert.equal(gpu.cases[0].median_gflops, expectedGpu);
  });

  it('keeps a real 0 score distinct from an unknown score', () => {
    const normalized = normalizeBenchmark({
      success: true,
      busy: false,
      threads: 1,
      cpu_score: 0,
      gpu_scores: [null],
      report: {
        suite: 'hivemind-hardware-v1',
        build_id: 'x',
        release_build: false,
        cpu: { state: 'stale', identity: '', threads: 1, correctness_passed: false, cases: [] },
        gpu_state: 'failed',
        gpu_reason: 'no supported hardware GPU adapter',
        gpus: [
          {
            device: {
              backend: 'dx12', vendor: 1, device: 2, name: 'Card', driver: '',
              driver_info: '', device_type: 'discrete-gpu', pci_bus_id: null,
            },
            measurement: { state: 'no-device', identity: '', threads: 1, correctness_passed: false, cases: [] },
          },
        ],
      },
    });

    assert.equal(normalized.cpu.score, 0);
    assert.equal(normalized.gpu.devices[0].score, null);
    assert.equal(normalized.gpu.reason, 'no supported hardware GPU adapter');
  });

  it('builds the authenticated status and rerun requests', () => {
    const status = buildBenchmarkStatusRequest('http://127.0.0.1:18080/', 'token');
    assert.equal(status.url, 'http://127.0.0.1:18080/api/benchmark');
    assert.equal(status.options.headers.Authorization, 'Bearer token');

    const rerun = buildBenchmarkRerunRequest('http://127.0.0.1:18080', 'token', ' GPU ');
    assert.equal(rerun.url, 'http://127.0.0.1:18080/api/benchmark/rerun');
    assert.equal(rerun.options.method, 'POST');
    assert.equal(rerun.options.headers.Authorization, 'Bearer token');
    assert.deepEqual(JSON.parse(rerun.options.body), { component: 'gpu' });

    assert.throws(() => buildBenchmarkRerunRequest('http://127.0.0.1:18080', 'token', 'disk'), /component/);
    assert.throws(() => buildBenchmarkStatusRequest('http://127.0.0.1:18080', ''), /signed-in/);
  });
});
