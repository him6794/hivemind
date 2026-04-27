"""
Simple smoke-test task for pydantic-monty execution path.
Requirements:
  - Pure Python (stdlib only)
  - No requirements.txt needed
  - Should complete in under 5 seconds
"""
import math
import time
import json


def sieve_of_eratosthenes(limit: int) -> list:
    """Return list of primes up to limit."""
    if limit < 2:
        return []
    composite = [False] * (limit + 1)
    for i in range(2, int(limit ** 0.5) + 1):
        if not composite[i]:
            for j in range(i * i, limit + 1, i):
                composite[j] = True
    return [i for i in range(2, limit + 1) if not composite[i]]


def fib(n: int) -> int:
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a


print("=== HiveMind Monty Smoke Test ===")
t0 = time.time()

# 1. Sieve of Eratosthenes
limit = 5000
primes = sieve_of_eratosthenes(limit)
print(f"[sieve] primes up to {limit}: {len(primes)} found")
print(f"[sieve] first 5: {primes[:5]}  last 5: {primes[-5:]}")

# 2. Fibonacci
fib_n = 30
fib_result = fib(fib_n)
print(f"[fib] fib({fib_n}) = {fib_result}")

# 3. Basic math
pi_approx = sum((-1) ** k / (2 * k + 1) for k in range(100000)) * 4
print(f"[math] pi approx (100k terms) = {pi_approx:.8f}  (real: {math.pi:.8f})")

elapsed = round(time.time() - t0, 4)
print(f"[time] elapsed = {elapsed}s")
print("=== DONE ===")

result = {
    "prime_count": len(primes),
    "first_primes": primes[:5],
    "fib_30": fib_result,
    "pi_approx": round(pi_approx, 8),
    "elapsed_s": elapsed,
    "status": "ok",
}
print(f"[result] {json.dumps(result, ensure_ascii=False)}")
