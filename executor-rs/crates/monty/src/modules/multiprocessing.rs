//! Implementation of the `multiprocessing` module.
//!
//! This is a compatibility-focused subset that exposes process metadata helpers
//! while keeping sandbox behavior deterministic and safe.
//!
//! Supported APIs:
//! - `multiprocessing.cpu_count()`
//! - `multiprocessing.get_start_method()`

use crate::{
    args::ArgValues,
    exception_private::RunResult,
    heap::{Heap, HeapData, HeapId},
    intern::{Interns, StaticStrings},
    modules::ModuleFunctions,
    resource::{ResourceError, ResourceTracker},
    types::{AttrCallResult, Module},
    value::Value,
};

/// Multiprocessing module functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, serde::Serialize, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum MultiprocessingFunctions {
    CpuCount,
    GetStartMethod,
}

/// Creates the `multiprocessing` module and allocates it on the heap.
///
/// # Returns
/// A HeapId pointing to the newly allocated module.
///
/// # Panics
/// Panics if required strings are not pre-interned during prepare phase.
pub fn create_module(heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
    let mut module = Module::new(StaticStrings::Multiprocessing);

    module.set_attr(
        StaticStrings::CpuCount,
        Value::ModuleFunction(ModuleFunctions::Multiprocessing(MultiprocessingFunctions::CpuCount)),
        heap,
        interns,
    );
    module.set_attr(
        StaticStrings::GetStartMethod,
        Value::ModuleFunction(ModuleFunctions::Multiprocessing(
            MultiprocessingFunctions::GetStartMethod,
        )),
        heap,
        interns,
    );

    heap.allocate(HeapData::Module(module))
}

/// Dispatches a call to a `multiprocessing` module function.
pub(super) fn call(
    heap: &mut Heap<impl ResourceTracker>,
    functions: MultiprocessingFunctions,
    args: ArgValues,
) -> RunResult<AttrCallResult> {
    let value = match functions {
        MultiprocessingFunctions::CpuCount => cpu_count(heap, args)?,
        MultiprocessingFunctions::GetStartMethod => get_start_method(heap, args)?,
    };

    Ok(AttrCallResult::Value(value))
}

/// Implements `multiprocessing.cpu_count()`.
///
/// Returns `1` in sandbox mode to provide deterministic behavior and avoid
/// leaking host topology details to untrusted code.
fn cpu_count(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("multiprocessing.cpu_count", heap)?;
    Ok(Value::Int(1))
}

/// Implements `multiprocessing.get_start_method()`.
///
/// Returns `'spawn'` as the only supported strategy in the sandbox model.
fn get_start_method(heap: &mut Heap<impl ResourceTracker>, args: ArgValues) -> RunResult<Value> {
    args.check_zero_args("multiprocessing.get_start_method", heap)?;
    Ok(StaticStrings::Spawn.into())
}
