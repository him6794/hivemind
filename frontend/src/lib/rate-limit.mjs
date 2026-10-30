// Sliding-window limiter kept in process memory.
//
// Counts are per running server process. Run a single website backend, or put
// a shared limiter in front of several, if the limit must hold across instances.
export function createRateLimiter({ limit, windowMs, now = () => Date.now(), maxKeys = 10000 }) {
  if (!Number.isInteger(limit) || limit < 1) {
    throw new RangeError('limit must be a positive integer');
  }
  if (!Number.isFinite(windowMs) || windowMs <= 0) {
    throw new RangeError('windowMs must be a positive number');
  }

  const hits = new Map();

  function forgetExpired(current) {
    const cutoff = current - windowMs;
    for (const [key, times] of hits) {
      if (times[times.length - 1] <= cutoff) {
        hits.delete(key);
      }
    }
  }

  return {
    /** Records an attempt for `key` and reports whether it is within the limit. */
    check(key) {
      const current = now();
      const cutoff = current - windowMs;
      const recent = (hits.get(key) ?? []).filter((time) => time > cutoff);

      if (recent.length >= limit) {
        hits.set(key, recent);
        const retryAfterMs = recent[0] + windowMs - current;
        return { allowed: false, retryAfterSeconds: Math.max(1, Math.ceil(retryAfterMs / 1000)) };
      }

      recent.push(current);
      hits.set(key, recent);

      if (hits.size > maxKeys) {
        forgetExpired(current);
        // Still over the bound under a flood of distinct keys: drop the oldest
        // entries. Those keys start again with a fresh count.
        for (const oldest of hits.keys()) {
          if (hits.size <= maxKeys) break;
          hits.delete(oldest);
        }
      }
      return { allowed: true, retryAfterSeconds: 0 };
    },
  };
}
