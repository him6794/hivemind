import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
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
