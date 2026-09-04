import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  isManagedGpuResult,
  isManagedLogGuidanceResult,
  taskRequestFailureText,
  taskResponseFailureMessage,
} from './taskResponsePolicy.mjs';

describe('task response policy', () => {
  it('does not invent an error for a successful response', () => {
    assert.equal(
      taskResponseFailureMessage({ success: true, message: 'Task cancellation recorded' }, 'Cancel rejected'),
      '',
    );
  });

  it('uses an explicit server message for a logical failure', () => {
    assert.equal(
      taskResponseFailureMessage({ success: false, message: 'Task cannot be cancelled' }, 'Cancel rejected'),
      'Task cannot be cancelled',
    );
    assert.equal(
      taskResponseFailureMessage({ success: false, status_message: 'Not authorized' }, 'Result unavailable'),
      'Not authorized',
    );
    assert.equal(
      taskResponseFailureMessage({ success: false, log: 'Not found' }, 'Log unavailable'),
      'Not found',
    );
  });

  it('uses a stable fallback for a logical failure without a usable message', () => {
    assert.equal(
      taskResponseFailureMessage({ success: false, message: '   ' }, 'Log unavailable'),
      'Log unavailable',
    );
    assert.equal(
      taskResponseFailureMessage({}, 'Result unavailable'),
      'Result unavailable',
    );
  });

  it('treats a non-OK HTTP response as failure even if its payload claims success', () => {
    assert.equal(
      taskResponseFailureMessage(
        { success: true, message: 'Request rejected by gateway' },
        'Log unavailable',
        false,
      ),
      'Request rejected by gateway',
    );
    assert.equal(
      taskResponseFailureMessage({ success: true }, 'Result unavailable', false),
      'Result unavailable',
    );
  });

  it('formats transport failures as explicit controlled task-detail errors', () => {
    assert.equal(
      taskRequestFailureText('Log', new Error('Cannot reach Hivemind API'), 'Log unavailable'),
      'Log failed: Cannot reach Hivemind API',
    );
    assert.equal(
      taskRequestFailureText('Result', null, 'Result unavailable'),
      'Result failed: Result unavailable',
    );
  });
});

describe('managed log guidance detection', () => {
  const MANAGED_GUIDANCE =
    'This managed-function task stores its output as a task log, not a result torrent; ' +
    'retrieve it from the task log or the artifact download endpoint';

  it('recognizes the managed-output contract response', () => {
    assert.equal(
      isManagedLogGuidanceResult({
        success: false,
        status_message: MANAGED_GUIDANCE,
        result_torrent: '',
      }),
      true,
    );
  });

  it('keeps other logical failures as errors', () => {
    assert.equal(
      isManagedLogGuidanceResult({ success: false, status_message: 'Not authorized' }),
      false,
    );
    // A legacy task with an actual torrent is a genuine success path.
    assert.equal(
      isManagedLogGuidanceResult({
        success: true,
        status_message: 'OK',
        result_torrent: 'btih:result',
      }),
      false,
    );
  });

  it('requires both the guidance text and an empty torrent', () => {
    assert.equal(isManagedLogGuidanceResult({ success: false }), false);
    assert.equal(
      isManagedLogGuidanceResult({
        success: false,
        status_message: MANAGED_GUIDANCE,
        result_torrent: 'btih:something',
      }),
      false,
    );
    // A failed task's message mentions status but not the log guidance.
    assert.equal(
      isManagedLogGuidanceResult({
        success: false,
        status_message: 'Task did not complete successfully (status: FAILED); no result is available',
      }),
      false,
    );
  });
});

describe('managed GPU result detection', () => {
  it('recognizes typed GPU-v1 JSON regardless of terminal status', () => {
    assert.equal(
      isManagedGpuResult({
        success: true,
        result_torrent: '',
        managed_gpu_result: {
          runtime_version: 'managed-function-gpu-v1',
          status: 'completed',
        },
      }),
      true,
    );
    assert.equal(
      isManagedGpuResult({
        success: false,
        managed_gpu_result: {
          runtime_version: 'managed-function-gpu-v1',
          status: 'failed',
        },
      }),
      true,
    );
  });

  it('does not mistake torrents, arrays, or other runtimes for GPU results', () => {
    assert.equal(isManagedGpuResult({ result_torrent: 'btih:result' }), false);
    assert.equal(
      isManagedGpuResult({ managed_gpu_result: { runtime_version: 'managed-function-v0' } }),
      false,
    );
    assert.equal(
      isManagedGpuResult({ managed_gpu_result: [{ runtime_version: 'managed-function-gpu-v1' }] }),
      false,
    );
    assert.equal(isManagedGpuResult({ managed_gpu_result: null }), false);
  });
});
