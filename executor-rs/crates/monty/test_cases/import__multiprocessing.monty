import multiprocessing
from multiprocessing import cpu_count, get_start_method

# === module import and exported callables ===
count = multiprocessing.cpu_count()
assert isinstance(count, int), 'multiprocessing.cpu_count returns int'
assert count >= 1, 'multiprocessing.cpu_count returns positive count'

# === from-import support ===
imported_count = cpu_count()
assert isinstance(imported_count, int), 'from multiprocessing import cpu_count returns int'
assert imported_count >= 1, 'from multiprocessing import cpu_count returns positive count'

start_method = get_start_method()
assert start_method in ('spawn', 'fork', 'forkserver'), 'multiprocessing.get_start_method returns known method'
