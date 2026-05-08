import math
import sys
sys.set_int_max_str_digits(50000000)
def is_prime(n):
    if n <= 1:
        return False
    if n == 2:
        return True
    
    if n % 2 == 0:
        return False
    limit = int(math.sqrt(n)) + 1
    for i in range(3, limit, 2):
        if n % i == 0:
            return False
            
    return True

test_number = 2**136279841
if is_prime(test_number):
    print(f"{test_number} 是質數")
else:
    print(f"{test_number} 不是質數")