import test from 'node:test';
import assert from 'node:assert/strict';
import { OUTPUT_PREVIEW_CHARS, taskOutputFilename, taskOutputView } from './taskOutputView.mjs';

test('a task without output shows nothing', () => {
  assert.equal(taskOutputView({}), null);
  assert.equal(taskOutputView({ output: '' }), null);
  assert.equal(taskOutputView({ output: null }), null);
  assert.equal(taskOutputView({ output: 42 }), null);
  assert.equal(taskOutputView(null), null);
});

test('a short output is shown whole', () => {
  assert.deepEqual(taskOutputView({ output: '60' }), { text: '60', truncated: false, totalChars: 2 });
  const exact = 'x'.repeat(OUTPUT_PREVIEW_CHARS);
  assert.equal(taskOutputView({ output: exact }).truncated, false);
});

test('a long output is cut and says how long it really is', () => {
  const view = taskOutputView({ output: 'x'.repeat(OUTPUT_PREVIEW_CHARS + 5) });
  assert.equal(view.truncated, true);
  assert.equal(view.text.length, OUTPUT_PREVIEW_CHARS);
  assert.equal(view.totalChars, OUTPUT_PREVIEW_CHARS + 5);
});

test('a cut never splits a surrogate pair', () => {
  const output = `${'x'.repeat(OUTPUT_PREVIEW_CHARS - 1)}😀tail`;
  const view = taskOutputView({ output });
  assert.equal(view.truncated, true);
  assert.equal(view.text, 'x'.repeat(OUTPUT_PREVIEW_CHARS - 1));
  assert.doesNotMatch(view.text, /[\ud800-\udbff]$/);
});

test('the saved file is named after the task', () => {
  assert.equal(taskOutputFilename('task-1'), 'task-1-output.txt');
});
