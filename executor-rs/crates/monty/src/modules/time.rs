//! Implementation of the `time` module.
//!
//! This module provides a small, sandbox-safe subset of Python's `time` module:
//! - `time()` returns wall-clock seconds since UNIX epoch as a float.
//! - `monotonic()` returns a process-monotonic clock as a float.
//! - `sleep(seconds)` blocks for a non-negative duration and returns `None`.
//!
//! Only the above functions are implemented for now. The goal is to support
//! common timing workflows while keeping implementation complexity bounded.

use std::{
    sync::OnceLock,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    args::ArgValues,
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Module, PyTrait},
    value::Value,
};

/// Time module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum TimeFunctions {
    Time,
    Monotonic,
    Sleep,
}

/// Monotonic clock epoch used for `time.monotonic()`.
static MONOTONIC_EPOCH: OnceLock<Instant> = OnceLock::new();

/// Creates the `time` module and allocates it on the heap.
///
/// # Returns
/// A HeapId pointing to the newly allocated module.
///
/// # Panics
/// Panics if required strings are not pre-interned during prepare phase.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Time);

    module.set_attr(
        StaticStrings::Time,
        Value::ModuleFunction(ModuleFunctions::Time(TimeFunctions::Time)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Monotonic,
        Value::ModuleFunction(ModuleFunctions::Time(TimeFunctions::Monotonic)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::Sleep,
        Value::ModuleFunction(ModuleFunctions::Time(TimeFunctions::Sleep)),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a `time` module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    functions: TimeFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match functions {
        TimeFunctions::Time => py_time(heap, args)?,
        TimeFunctions::Monotonic => py_monotonic(heap, args)?,
        TimeFunctions::Sleep => py_sleep(heap, args)?,
    };

    Ok(AttrCallResult::Value(value))
}

/// Implements `time.time()`.
fn py_time(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.time", heap)?;
    let now = SystemTime::now();
    let since_epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    Ok(Value::Float(duration_to_secs_f64(since_epoch)))
}

/// Implements `time.monotonic()`.
fn py_monotonic(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("time.monotonic", heap)?;
    let base = MONOTONIC_EPOCH.get_or_init(Instant::now);
    Ok(Value::Float(base.elapsed().as_secs_f64()))
}

/// Implements `time.sleep(seconds)`.
///
/// This currently performs a direct blocking sleep on the running thread.
fn py_sleep(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    let seconds = args.get_one_arg("time.sleep", heap)?;
    let duration_secs = number_to_duration_secs(seconds, heap)?;

    thread::sleep(Duration::from_secs_f64(duration_secs));
    Ok(Value::None)
}

/// Converts a positive integer/float value into seconds for `time.sleep()`.
fn number_to_duration_secs(value: Value, heap: &mut Heap<impl ResourceTracker>) -> RunResult<f64> {
    let secs = match value {
        Value::Int(i) => i as f64,
        Value::Float(f) => f,
        other => {
            let type_name = other.py_type(heap);
            other.drop_with_heap(heap);
            return Err(ExcType::type_error(format!(
                "'{type_name}' object cannot be interpreted as a float"
            )));
        }
    };

    if secs.is_nan() {
        return Err(SimpleException::new_msg(ExcType::ValueError, "Invalid value NaN (not a number)").into());
    }
    if secs < 0.0 {
        return Err(SimpleException::new_msg(ExcType::ValueError, "sleep length must be non-negative").into());
    }
    if !secs.is_finite() {
        return Err(
            SimpleException::new_msg(ExcType::OverflowError, "timestamp out of range for platform time_t").into(),
        );
    }
    Ok(secs)
}

/// Converts duration to fractional seconds as `f64`.
fn duration_to_secs_f64(duration: Duration) -> f64 {
    duration.as_secs_f64()
}
