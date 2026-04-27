import os
import random
import time

# 直接用 task/main.py（確保載入同一份 prime_sieve.dll）
import main as m


def _get_primes_small(start: int, end: int, threads: int) -> list[int]:
    # 走原本的 get_primes_parallel（會回傳完整陣列，僅用於小範圍驗證）
    return m.segmented_sieve(start, end, num_threads=threads)


def validate(iterations: int = 50, max_n: int = 5_000_000, threads: int | None = None) -> None:
    if threads is None:
        threads = os.cpu_count() or 4

    random.seed(0)
    t0 = time.time()
    for i in range(iterations):
        # 隨機抽一段小範圍，避免記憶體爆炸
        a = random.randint(1, max_n - 100_000)
        b = a + random.randint(1_000, 200_000)

        primes = _get_primes_small(a, b, threads=threads)
        c1 = len(primes)
        mp1 = primes[-1] if primes else 0

        c2, mp2 = m.count_primes_and_max(a, b, num_threads=threads)

        if c1 != c2 or mp1 != mp2:
            raise SystemExit(
                f"Mismatch at iter={i} range=[{a},{b}): "
                f"list_count={c1} max={mp1} vs count_max_count={c2} max={mp2}"
            )

        if (i + 1) % 10 == 0:
            print(f"ok {i+1}/{iterations}")

    dt = time.time() - t0
    print(f"All OK: iterations={iterations}, max_n={max_n}, threads={threads}, sec={dt:.2f}")


if __name__ == "__main__":
    # 注意：這是 correctness 驗證，不是效能測試
    os.environ["OMP_NUM_THREADS"] = str(os.cpu_count() or 4)
    validate()
