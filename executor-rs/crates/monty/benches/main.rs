#[cfg(codspeed)]
use codspeed_criterion_compat::{Bencher, Criterion, black_box, criterion_group, criterion_main};
#[cfg(not(codspeed))]
use criterion::{Bencher, Criterion, black_box, criterion_group, criterion_main};
use monty::MontyRun;
#[cfg(not(codspeed))]
use pprof::criterion::{Output, PProfProfiler};

/// Runs a benchmark using the Monty interpreter.
/// Parses once, then benchmarks repeated execution.
fn run_monty(bench: &mut Bencher, code: &str, expected: i64) {
    let ex = MontyRun::new(code.to_owned(), "test.py", vec![]).unwrap();
    let r = ex.run_no_limits(vec![]).unwrap();
    let int_value: i64 = r.as_ref().try_into().unwrap();
    assert_eq!(int_value, expected);

    bench.iter(|| {
        let r = ex.run_no_limits(vec![]).unwrap();
        let int_value: i64 = r.as_ref().try_into().unwrap();
        black_box(int_value);
    });
}

const ADD_TWO: &str = "1 + 2";

const LIST_APPEND: &str = "
a = []
a.append(42)
a[0]
";

const LOOP_MOD_13: &str = "
v = ''
for i in range(1_000):
    if i % 13 == 0:
        v += 'x'
len(v)
";

/// Comprehensive benchmark exercising most supported Python features.
/// Code is shared with test_cases/bench__kitchen_sink.monty
const KITCHEN_SINK: &str = include_str!("../test_cases/bench__kitchen_sink.monty");

const FUNC_CALL_KWARGS: &str = "
def add(a, b=2):
    return a + b

add(a=1)
";

const LIST_APPEND_STR: &str = "
a = []
for i in range(100_000):
    a.append(str(i))
len(a)
";

const LIST_APPEND_INT: &str = "
a = []
for i in range(100_000):
    a.append(i)
sum(a)
";

const FIB_25: &str = "
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

fib(25)
";

/// List comprehension benchmark - creates 1000 elements.
const LIST_COMP: &str = "len([x * 2 for x in range(1000)])";

/// Dict comprehension benchmark - creates 500 unique keys (i // 2 deduplicates pairs).
const DICT_COMP: &str = "len({i // 2: i * 2 for i in range(1000)})";

/// Empty tuple creation benchmark - creates 100,000 empty tuples in a list.
const EMPTY_TUPLES: &str = "len([() for _ in range(100_000)])";

/// 2-tuple creation benchmark - creates 100,000 2-tuples in a list.
const PAIR_TUPLES: &str = "len([(i, i + 1) for i in range(100_000)])";

/// Benchmarks end-to-end execution (parsing + running) using Monty.
/// This is different from other benchmarks as it includes parsing in the loop.
fn end_to_end_monty(bench: &mut Bencher) {
    bench.iter(|| {
        let ex = MontyRun::new(black_box("1 + 2").to_owned(), "test.py", vec![]).unwrap();
        let r = ex.run_no_limits(vec![]).unwrap();
        let int_value: i64 = r.as_ref().try_into().unwrap();
        black_box(int_value);
    });
}

/// Configures all benchmarks in a single group.
fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("add_two__monty", |b| run_monty(b, ADD_TWO, 3));

    c.bench_function("list_append__monty", |b| run_monty(b, LIST_APPEND, 42));

    c.bench_function("loop_mod_13__monty", |b| run_monty(b, LOOP_MOD_13, 77));

    c.bench_function("end_to_end__monty", end_to_end_monty);

    c.bench_function("kitchen_sink__monty", |b| run_monty(b, KITCHEN_SINK, 373));

    c.bench_function("func_call_kwargs__monty", |b| run_monty(b, FUNC_CALL_KWARGS, 3));

    c.bench_function("list_append_str__monty", |b| run_monty(b, LIST_APPEND_STR, 100_000));

    c.bench_function("list_append_int__monty", |b| {
        run_monty(b, LIST_APPEND_INT, 4_999_950_000);
    });

    c.bench_function("fib__monty", |b| run_monty(b, FIB_25, 75_025));

    c.bench_function("list_comp__monty", |b| run_monty(b, LIST_COMP, 1000));

    c.bench_function("dict_comp__monty", |b| run_monty(b, DICT_COMP, 500));

    c.bench_function("empty_tuples__monty", |b| run_monty(b, EMPTY_TUPLES, 100_000));

    c.bench_function("pair_tuples__monty", |b| run_monty(b, PAIR_TUPLES, 100_000));
}

// Use pprof flamegraph profiler when running locally (not on CodSpeed)
#[cfg(not(codspeed))]
criterion_group!(
    name = benches;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = criterion_benchmark
);

// Use default config when running on CodSpeed (pprof's Profiler trait is incompatible)
#[cfg(codspeed)]
criterion_group!(benches, criterion_benchmark);

criterion_main!(benches);
