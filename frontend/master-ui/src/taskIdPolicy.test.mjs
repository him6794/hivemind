import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { createTaskId, isSafeTaskId, taskIdFromFileName, validateTaskId } from './taskIdPolicy.mjs';

describe('task id policy', () => {
  it('matches server-safe task id rules', () => {
    assert.equal(isSafeTaskId('task-123_ok.1'), true);
    assert.equal(isSafeTaskId(''), false);
    assert.equal(isSafeTaskId('   '), false);
    assert.equal(isSafeTaskId('.'), false);
    assert.equal(isSafeTaskId('..'), false);
    assert.equal(isSafeTaskId('task..bad'), false);
    assert.equal(isSafeTaskId('../escape'), false);
    assert.equal(isSafeTaskId('bad task'), false);
    assert.equal(isSafeTaskId('bad/task'), false);
  });

  it('trims valid ids and rejects invalid ids with a stable message', () => {
    assert.deepEqual(validateTaskId(' task-123_ok.1 '), {
      ok: true,
      taskId: 'task-123_ok.1',
      message: '',
    });

    assert.deepEqual(validateTaskId('task..bad'), {
      ok: false,
      taskId: '',
      message: "task_id must be non-empty ASCII alphanumeric, '.', '-', or '_' and cannot contain '..'",
    });
  });

  it('derives only safe task ids from filenames', () => {
    assert.equal(taskIdFromFileName('render job.zip'), 'render-job');
    assert.equal(taskIdFromFileName('..zip'), '');
    assert.equal(taskIdFromFileName('bad/path.zip'), 'bad-path');
  });

  it('generates default task ids as uuids', () => {
    const generated = createTaskId(() => '7f50f8b2-a963-49a1-bca0-d79f209991d4');

    assert.equal(generated, '7f50f8b2-a963-49a1-bca0-d79f209991d4');
  });

  it('falls back to a safe id when randomUUID is unavailable on plain HTTP', () => {
    const generated = createTaskId(
      () => undefined,
      () => 'task-http-fallback-001',
    );

    assert.equal(generated, 'task-http-fallback-001');
  });
});
