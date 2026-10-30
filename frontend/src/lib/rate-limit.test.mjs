import assert from 'node:assert/strict';
import test from 'node:test';
import { createRateLimiter } from './rate-limit.mjs';

function manualClock(start = 0) {
  let time = start;
  return {
    now: () => time,
    advance: (ms) => {
      time += ms;
    },
  };
}

test('allows the limit inside the window and reports the wait once exceeded', () => {
  const clock = manualClock(1_000);
  const limiter = createRateLimiter({ limit: 2, windowMs: 10_000, now: clock.now });

  assert.deepEqual(limiter.check('a'), { allowed: true, retryAfterSeconds: 0 });
  clock.advance(1_000);
  assert.deepEqual(limiter.check('a'), { allowed: true, retryAfterSeconds: 0 });
  clock.advance(1_000);
  // The first attempt leaves the window 8 seconds from now.
  assert.deepEqual(limiter.check('a'), { allowed: false, retryAfterSeconds: 8 });
  // Limits are per key.
  assert.deepEqual(limiter.check('b'), { allowed: true, retryAfterSeconds: 0 });
});

test('frees a slot once the oldest attempt is a full window old', () => {
  const clock = manualClock(0);
  const limiter = createRateLimiter({ limit: 1, windowMs: 5_000, now: clock.now });

  assert.equal(limiter.check('a').allowed, true);
  clock.advance(4_000);
  assert.equal(limiter.check('a').allowed, false);
  clock.advance(1_000);
  assert.equal(limiter.check('a').allowed, true);
});

test('bounds stored keys by dropping the oldest entries under a flood of distinct keys', () => {
  const clock = manualClock(0);
  const limiter = createRateLimiter({ limit: 1, windowMs: 60_000, now: clock.now, maxKeys: 3 });

  for (const key of ['a', 'b', 'c', 'd', 'e']) {
    assert.equal(limiter.check(key).allowed, true);
  }
  // 'a' was dropped to stay within the bound, so it starts again with a fresh count.
  assert.equal(limiter.check('a').allowed, true);
});

test('rejects invalid configuration', () => {
  assert.throws(() => createRateLimiter({ limit: 0, windowMs: 1_000 }), RangeError);
  assert.throws(() => createRateLimiter({ limit: 1, windowMs: 0 }), RangeError);
});
