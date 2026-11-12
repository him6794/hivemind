//! Observe allocator requests, not just the eventual value-limit error: a
//! rejected string operation must not first allocate its oversized result.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use managed_function_runtime::{ExecutionLimits, ManagedExecutor, RuntimeError, Value};

struct ObservedAllocator;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}

fn observe_allocation(bytes: usize) {
    let _ = TRACKING.try_with(|tracking| {
        if tracking.get() {
            let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(bytes)));
        }
    });
}

// SAFETY: Every allocation operation is forwarded unchanged to System. The
// thread-local observation uses allocation-free Cells and never touches pointers.
unsafe impl GlobalAlloc for ObservedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        observe_allocation(layout.size());
        // SAFETY: The caller supplied the allocation layout required by System.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        observe_allocation(layout.size());
        // SAFETY: The caller supplied the allocation layout required by System.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: This allocator obtained the pointer from System with this layout.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, bytes: usize) -> *mut u8 {
        observe_allocation(bytes);
        // SAFETY: The pointer/layout and new size have System's required contract.
        unsafe { System.realloc(pointer, layout, bytes) }
    }
}

#[global_allocator]
static ALLOCATOR: ObservedAllocator = ObservedAllocator;

struct Observation;

impl Drop for Observation {
    fn drop(&mut self) {
        TRACKING.with(|tracking| tracking.set(false));
    }
}

fn rejected_allocation(
    source: &str,
    input: &str,
    limits: ExecutionLimits,
) -> (RuntimeError, usize) {
    LARGEST.with(|largest| largest.set(0));
    TRACKING.with(|tracking| tracking.set(true));
    let observation = Observation;
    let result = ManagedExecutor.execute_json_input(source, limits, input);
    drop(observation);
    let largest = LARGEST.with(Cell::get);
    (
        result.expect_err("the operation must reject the result"),
        largest,
    )
}

#[test]
fn join_rejects_multiplicative_output_before_allocating_it() {
    let input = serde_json::json!({"xs": vec![""; 5_000], "sep": "a".repeat(4_096)}).to_string();
    let (error, largest) = rejected_allocation(
        "return join(get(input, \"xs\"), get(input, \"sep\"));",
        &input,
        ExecutionLimits::for_managed_function_budget(100_000_000),
    );
    assert_eq!(error.code(), "value_limit_exceeded");
    assert!(
        largest <= 1_048_576,
        "rejected join allocated {largest} bytes"
    );
}

#[test]
fn split_rejects_excess_pieces_before_allocating_them() {
    let input = serde_json::to_string(&"x".repeat(524_288)).unwrap();
    let (error, largest) = rejected_allocation(
        "return split(input, \"x\");",
        &input,
        ExecutionLimits::for_managed_function_budget(100_000_000),
    );
    assert_eq!(error.code(), "value_limit_exceeded");
    assert!(
        largest <= 1_048_576,
        "rejected split allocated {largest} bytes"
    );
}

#[test]
fn join_checks_remaining_materialization_before_allocating_a_valid_sized_string() {
    let input = serde_json::json!({"xs": vec![""; 100], "sep": "a".repeat(4_096)}).to_string();
    let mut limits = ExecutionLimits::for_managed_function_budget(100_000_000);
    limits.max_value_materialization_bytes = 100_000;
    let (error, largest) = rejected_allocation(
        "return join(get(input, \"xs\"), get(input, \"sep\"));",
        &input,
        limits,
    );
    assert_eq!(error.code(), "value_limit_exceeded");
    assert!(largest <= 16_384, "rejected join allocated {largest} bytes");
}

#[test]
fn split_checks_each_structural_and_remaining_materialization_bound_before_allocation() {
    let input = serde_json::to_string(&",".repeat(10_000)).unwrap();
    for bound in ["value", "collection", "depth", "materialization", "usage"] {
        let mut limits = ExecutionLimits::for_managed_function_budget(100_000_000);
        match bound {
            "value" => limits.max_value_bytes = 20_000,
            "collection" => limits.max_collection_items = 10_000,
            "depth" => limits.max_value_depth = 1,
            // Input copy (10_002) + separator (3) + result (30_004), minus one.
            "materialization" => limits.max_value_materialization_bytes = 40_008,
            "usage" => limits.max_usage_units = Some(15_000),
            _ => unreachable!(),
        }
        let (error, largest) = rejected_allocation("return split(input, \",\");", &input, limits);
        assert_eq!(
            error.code(),
            if bound == "usage" {
                "budget_exhausted"
            } else {
                "value_limit_exceeded"
            },
            "{bound}"
        );
        assert!(
            largest <= 65_536,
            "{bound}: rejected split allocated {largest} bytes"
        );
    }
}

#[test]
fn split_exact_bounds_include_empty_pieces_and_json_escapes() {
    let text = "\"\n\u{0000},,雪,";
    let input = serde_json::to_string(text).unwrap();
    let pieces: Vec<_> = text.split(',').collect();
    let expected = Value::List(
        pieces
            .iter()
            .map(|piece| Value::String((*piece).into()))
            .collect(),
    );
    let result_bytes = serde_json::to_string(&pieces).unwrap().len() as u64;
    let mut exact = ExecutionLimits::for_managed_function_budget(1_000);
    exact.max_value_bytes = result_bytes;
    exact.max_collection_items = pieces.len() as u64;
    exact.max_value_depth = 2;
    // One input lookup, one separator literal, and one result materialization.
    exact.max_value_materialization_bytes = input.len() as u64 + 3 + result_bytes;
    let source = r#"return split(input, ",");"#;
    let completed = ManagedExecutor
        .execute_json_input(source, exact.clone(), &input)
        .unwrap();
    assert_eq!(completed.value, expected);

    for bound in ["value", "collection", "depth", "materialization"] {
        let mut too_small = exact.clone();
        match bound {
            "value" => too_small.max_value_bytes -= 1,
            "collection" => too_small.max_collection_items -= 1,
            "depth" => too_small.max_value_depth -= 1,
            "materialization" => too_small.max_value_materialization_bytes -= 1,
            _ => unreachable!(),
        }
        let error = ManagedExecutor
            .execute_json_input(source, too_small, &input)
            .unwrap_err();
        assert_eq!(error.code(), "value_limit_exceeded", "{bound}");
        // All limits fail after the same deterministic usage charges.
        assert_eq!(
            error.partial_receipt().unwrap().usage_units,
            completed.receipt.usage_units
        );
    }
}

#[test]
fn join_exact_bounds_use_escaped_bytes_and_charge_materialization_once() {
    let texts = vec!["雪\u{0000}"; 10];
    let separator = "\"\n\\";
    let joined = texts.join(separator);
    let input = serde_json::to_string(&texts).unwrap();
    let result_bytes = serde_json::to_string(&joined).unwrap().len() as u64;
    let mut exact = ExecutionLimits::for_managed_function_budget(1_000);
    exact.max_value_bytes = result_bytes;
    exact.max_value_materialization_bytes =
        input.len() as u64 + serde_json::to_string(separator).unwrap().len() as u64 + result_bytes;
    let source = r#"return join(input, "\"\n\\");"#;
    let completed = ManagedExecutor
        .execute_json_input(source, exact.clone(), &input)
        .unwrap();
    assert_eq!(completed.value, Value::String(joined));

    for bound in ["value", "materialization"] {
        let mut too_small = exact.clone();
        match bound {
            "value" => too_small.max_value_bytes -= 1,
            "materialization" => too_small.max_value_materialization_bytes -= 1,
            _ => unreachable!(),
        }
        let error = ManagedExecutor
            .execute_json_input(source, too_small, &input)
            .unwrap_err();
        assert_eq!(error.code(), "value_limit_exceeded", "{bound}");
        assert_eq!(
            error.partial_receipt().unwrap().usage_units,
            completed.receipt.usage_units
        );
    }

    // Earlier work consumes the same cumulative budget, even when its result
    // is no longer used: "a,b" (5) + "," (3) + ["a","b"] (9) = 17.
    let after_prior_work = format!(r#"let scratch = split("a,b", ","); {source}"#);
    let error = ManagedExecutor
        .execute_json_input(&after_prior_work, exact.clone(), &input)
        .unwrap_err();
    assert_eq!(error.code(), "value_limit_exceeded");
    exact.max_value_materialization_bytes += 17;
    assert_eq!(
        ManagedExecutor
            .execute_json_input(&after_prior_work, exact, &input)
            .unwrap()
            .value,
        completed.value,
    );
}

#[test]
fn unicode_separators_preserve_values_and_usage_boundaries() {
    let text = "雪💡💡終💡";
    let separator = "💡";
    let pieces: Vec<_> = text.split(separator).collect();
    let split_input = serde_json::json!({"text": text, "separator": separator}).to_string();
    let join_input = serde_json::json!({"pieces": pieces, "separator": separator}).to_string();
    for (source, input, expected) in [
        (
            r#"return split(get(input, "text"), get(input, "separator"));"#,
            split_input,
            Value::List(
                pieces
                    .iter()
                    .map(|piece| Value::String((*piece).into()))
                    .collect(),
            ),
        ),
        (
            r#"return join(get(input, "pieces"), get(input, "separator"));"#,
            join_input,
            Value::String(text.into()),
        ),
    ] {
        let completed = ManagedExecutor
            .execute_json_input(
                source,
                ExecutionLimits::for_managed_function_budget(1_000),
                &input,
            )
            .unwrap();
        assert_eq!(completed.value, expected);
        let exact = ExecutionLimits::for_managed_function_budget(completed.receipt.usage_units);
        assert_eq!(
            ManagedExecutor
                .execute_json_input(source, exact, &input)
                .unwrap()
                .value,
            expected
        );
        let too_small =
            ExecutionLimits::for_managed_function_budget(completed.receipt.usage_units - 1);
        let error = ManagedExecutor
            .execute_json_input(source, too_small, &input)
            .unwrap_err();
        assert_eq!(error.code(), "budget_exhausted");
        assert!(error.partial_receipt().unwrap().usage_units < completed.receipt.usage_units);
    }
}

#[test]
fn split_and_join_remain_unavailable_on_frozen_v0() {
    for source in [
        r#"return split("a,b", ",");"#,
        r#"return join(["a","b"], ",");"#,
    ] {
        let error = ManagedExecutor
            .execute(source, ExecutionLimits::default())
            .unwrap_err();
        assert_eq!(error.code(), "name_error");
    }
}
