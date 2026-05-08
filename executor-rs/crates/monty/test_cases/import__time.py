import time
from time import monotonic as imported_monotonic

# === import time and call module functions ===
wall_1 = time.time()
wall_2 = time.time()
assert isinstance(wall_1, float), 'time.time returns float'
assert isinstance(wall_2, float), 'time.time returns float on repeated calls'
assert wall_2 >= wall_1, 'time.time is non-decreasing for immediate repeated calls'

# === monotonic clock semantics ===
mono_1 = time.monotonic()
mono_2 = time.monotonic()
assert isinstance(mono_1, float), 'time.monotonic returns float'
assert mono_2 >= mono_1, 'time.monotonic is non-decreasing'

# === from-import support ===
mono_3 = imported_monotonic()
assert isinstance(mono_3, float), 'from time import monotonic provides callable function'

# === sleep behavior ===
before_sleep = time.monotonic()
time.sleep(0.001)
after_sleep = time.monotonic()
assert after_sleep >= before_sleep, 'time.sleep waits for a non-negative duration'
