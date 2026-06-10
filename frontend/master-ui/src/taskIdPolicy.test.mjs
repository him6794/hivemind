import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { isSafeTaskId, taskIdFromFileName, validateTaskId } from './taskIdPolicy.mjs';

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
});
