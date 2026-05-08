print("=" * 60)
print("HiveMind 測試任務 - 數學計算")
print("=" * 60)
print()

# 計算質數
def is_prime(n):
    if n < 2:
        return False
    for i in range(2, int(n ** 0.5) + 1):
        if n % i == 0:
            return False
    return True

print("計算 1-100 之間的質數...")
primes = [n for n in range(1, 101) if is_prime(n)]
print(f"找到 {len(primes)} 個質數")
print(f"質數列表: {primes[:10]}... (顯示前 10 個)")
print()

# 費波那契數列
def fibonacci(n):
    if n <= 1:
        return n
    a, b = 0, 1
    for _ in range(n - 1):
        a, b = b, a + b
    return b

print("計算費波那契數列...")
fib_numbers = [fibonacci(i) for i in range(15)]
print(f"前 15 個費波那契數: {fib_numbers}")
print()

# 階乘計算
def factorial(n):
    if n <= 1:
        return 1
    result = 1
    for i in range(2, n + 1):
        result *= i
    return result

print("計算階乘...")
for i in [5, 10, 15]:
    print(f"{i}! = {factorial(i)}")
print()

# 統計計算
numbers = list(range(1, 101))
print("統計分析 (1-100):")
print(f"  總和: {sum(numbers)}")
print(f"  平均: {sum(numbers) / len(numbers)}")
print(f"  最小值: {min(numbers)}")
print(f"  最大值: {max(numbers)}")
print()

print("=" * 60)
print("數學計算完成！")
print("=" * 60)
