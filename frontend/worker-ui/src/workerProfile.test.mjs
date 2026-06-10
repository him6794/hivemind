import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { buildRegisterWorkerBody, normalizeWorkerProfile, registrationOwnerUsername } from './workerProfile.mjs';

describe('worker profile registration payload', () => {
  it('normalizes local worker info before React state updates', () => {
    const normalized = normalizeWorkerProfile(
      {
        workerId: 'local-worker-42',
        IP: '127.0.0.1:50053',
        cpuCores: '8',
        memoryGb: '32',
        cpuScore: '900',
        gpuScore: '1200',
        gpuMemoryGb: '16',
        storageTotalGb: '1000',
        storageAvailableGb: '750',
        gpuName: 'RTX Test',
      },
      'fallback:50053',
    );

    assert.deepEqual(normalized, {
      worker_id: 'local-worker-42',
      ip: '127.0.0.1:50053',
      location: 'local',
      cpu_cores: 8,
      memory_gb: 32,
      cpu_score: 900,
      gpu_score: 1200,
      gpu_memory_gb: 16,
      storage_total_gb: 1000,
      storage_available_gb: 750,
      gpu_name: 'RTX Test',
    });
  });

  it('sends local worker_id and fresh resource values when registering', () => {
    const body = buildRegisterWorkerBody(
      ' provider ',
      {
        worker_id: 'local-worker-42',
        location: 'taipei',
        cpu_cores: 8,
        memory_gb: 32,
        cpu_score: 900,
        gpu_score: 1200,
        gpu_memory_gb: 16,
        gpu_name: 'RTX Test',
        storage_total_gb: 1000,
        storage_available_gb: 750,
      },
      '127.0.0.1:50053',
    );

    assert.deepEqual(body, {
      username: 'provider',
      worker_id: 'local-worker-42',
      ip: '127.0.0.1:50053',
      cpu_cores: 8,
      memory_gb: 32,
      cpu_score: 900,
      gpu_score: 1200,
      gpu_memory_gb: 16,
      gpu_name: 'RTX Test',
      storage_total_gb: 1000,
      storage_available_gb: 750,
      location: 'taipei',
    });
  });

  it('uses authenticated username instead of edited login form username for re-registration', () => {
    assert.equal(registrationOwnerUsername(' alice ', ' bob '), 'alice');
    assert.equal(registrationOwnerUsername('', ' bob '), 'bob');

    const body = buildRegisterWorkerBody(
      registrationOwnerUsername(' alice ', ' bob '),
      {
        worker_id: 'local-worker-42',
        location: 'taipei',
      },
      '127.0.0.1:50053',
    );

    assert.equal(body.username, 'alice');
  });

  it('normalizes endpoint and rejects blank endpoint before registration', () => {
    const body = buildRegisterWorkerBody(
      'provider',
      {
        worker_id: 'local-worker-42',
        location: 'taipei',
      },
      '  localhost:50053  ',
    );

    assert.equal(body.ip, 'localhost:50053');
    assert.throws(
      () => buildRegisterWorkerBody('provider', { worker_id: 'local-worker-42' }, '   '),
      /worker endpoint is required/,
    );
  });

  it('rejects negative worker capacity values before registration', () => {
    assert.throws(
      () => buildRegisterWorkerBody(
        'provider',
        {
          worker_id: 'local-worker-42',
          cpu_cores: -8,
          memory_gb: -32,
          cpu_score: -1,
          gpu_score: -1,
          gpu_memory_gb: -16,
          storage_total_gb: -100,
          storage_available_gb: -50,
        },
        '127.0.0.1:50053',
      ),
      /capacity values must be non-negative/,
    );
  });

  it('rejects fractional cpu cores before registration', () => {
    assert.throws(
      () => buildRegisterWorkerBody(
        'provider',
        {
          worker_id: 'local-worker-42',
          cpu_cores: 1.5,
        },
        '127.0.0.1:50053',
      ),
      /cpu_cores must be an integer/,
    );
  });

  it('rejects impossible worker storage availability', () => {
    assert.throws(
      () => buildRegisterWorkerBody(
        'provider',
        {
          worker_id: 'local-worker-42',
          storage_total_gb: 100,
          storage_available_gb: 150,
        },
        '127.0.0.1:50053',
      ),
      /storage_available_gb cannot exceed storage_total_gb/,
    );
  });
});
