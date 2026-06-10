def fibonacci(n):
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a


def is_prime(value):
    if value < 2:
        return False
    for divisor in range(2, int(value ** 0.5) + 1):
        if value % divisor == 0:
            return False
    return True


def main():
    primes = [value for value in range(2, 50) if is_prime(value)]
    print(f"fib(20)={fibonacci(20)}")
    print(f"primes_under_50={primes}")


if __name__ == "__main__":
    main()
