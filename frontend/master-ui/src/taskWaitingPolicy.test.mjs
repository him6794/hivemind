import assert from 'node:assert/strict';
import test from 'node:test';
import { t } from './locale.mjs';
import { waitingForWorkersMessage } from './taskWaitingPolicy.mjs';

test('reads the owner-rule waiting reason and translates it with its counts', () => {
  const entry = waitingForWorkersMessage(
    'Waiting for 3 Workers from different owners; 1 available now.',
  );
  assert.deepEqual(entry, {
    key: 'Waiting for {needed} Workers from different owners; {available} available now.',
    params: { needed: '3', available: '1' },
  });
  assert.equal(t('en', entry), 'Waiting for 3 Workers from different owners; 1 available now.');
  assert.equal(t('zh', entry), '等待 3 個不同擁有者的 Worker；目前可用 1 個。');
});

test('reads the shared-owner waiting reason used by local smoke runs', () => {
  const entry = waitingForWorkersMessage('Waiting for 2 eligible Workers; 0 available now.');
  assert.equal(t('zh', entry), '等待 2 個符合條件的 Worker；目前可用 0 個。');
});

test('leaves every other task message to be shown as it is', () => {
  assert.equal(waitingForWorkersMessage('Redispatched'), null);
  assert.equal(waitingForWorkersMessage(''), null);
  assert.equal(waitingForWorkersMessage(undefined), null);
  assert.equal(waitingForWorkersMessage('Waiting for a while.'), null);
});
