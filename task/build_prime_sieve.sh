#!/bin/bash
# Linux/macOS 編譯腳本

set -e

echo "開始編譯 prime_sieve.c..."

# Linux 產生 .so
gcc -shared -O3 -march=native -fPIC -o prime_sieve.so prime_sieve.c -lm

echo "✓ 編譯成功: prime_sieve.so"
ls -lh prime_sieve.so

echo ""
echo "測試編譯結果..."
python3 -c "import ctypes; lib = ctypes.CDLL('./prime_sieve.so'); print(f'count_primes(1, 100) = {lib.count_primes(1, 100)}')"
