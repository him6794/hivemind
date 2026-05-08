//! Built-in module implementations.
//!
//! This module provides implementations for Python built-in modules like `sys`, `typing`,
//! and `asyncio`. These are created on-demand when import statements are executed.

use std::fmt::{self, Write};

use crate::{
    args::ArgValues,
    exception_private::RunResult,
    heap::{Heap, HeapId},
    intern::{Interns, StaticStrings, StringId},
    resource::{ResourceError, ResourceTracker},
    types::AttrCallResult,
};

pub(crate) mod asyncio;
pub(crate) mod multiprocessing;
pub(crate) mod os;
pub(crate) mod pathlib;
pub(crate) mod re;
pub(crate) mod sys;
pub(crate) mod time;
pub(crate) mod typing;

/// Registry entry describing one importable built-in module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BuiltinModuleEntry {
    name: StaticStrings,
    module: BuiltinModule,
}

/// Canonical module registry used by compiler and VM for stable module-id mapping.
///
/// Keeping this as a single table avoids coupling bytecode module IDs to enum
/// discriminants and makes it easier to extend the module list safely.
const BUILTIN_MODULE_REGISTRY: [BuiltinModuleEntry; 8] = [
    BuiltinModuleEntry {
        name: StaticStrings::Sys,
        module: BuiltinModule::Sys,
    },
    BuiltinModuleEntry {
        name: StaticStrings::Typing,
        module: BuiltinModule::Typing,
    },
    BuiltinModuleEntry {
        name: StaticStrings::Asyncio,
        module: BuiltinModule::Asyncio,
    },
    BuiltinModuleEntry {
        name: StaticStrings::Pathlib,
        module: BuiltinModule::Pathlib,
    },
    BuiltinModuleEntry {
        name: StaticStrings::Os,
        module: BuiltinModule::Os,
    },
    BuiltinModuleEntry {
        name: StaticStrings::Re,
        module: BuiltinModule::Re,
    },
    BuiltinModuleEntry {
        name: StaticStrings::Time,
        module: BuiltinModule::Time,
    },
    BuiltinModuleEntry {
        name: StaticStrings::Multiprocessing,
        module: BuiltinModule::Multiprocessing,
    },
];

/// Resolves built-in modules by name and module ID.
pub(crate) struct BuiltinModuleRegistry;

impl BuiltinModuleRegistry {
    /// Finds a built-in module from its imported name.
    pub fn from_string_id(string_id: StringId) -> Option<BuiltinModule> {
        let static_name = StaticStrings::from_string_id(string_id)?;
        BUILTIN_MODULE_REGISTRY
            .iter()
            .find(|entry| entry.name == static_name)
            .map(|entry| entry.module)
    }

    /// Returns the stable bytecode module ID used by `Opcode::LoadModule`.
    pub fn module_id(module: BuiltinModule) -> u8 {
        BUILTIN_MODULE_REGISTRY
            .iter()
            .position(|entry| entry.module == module)
            .and_then(|index| u8::try_from(index).ok())
            .expect("builtin module missing registry entry")
    }

    /// Resolves a `Opcode::LoadModule` operand back to a module.
    pub fn from_module_id(module_id: u8) -> Option<BuiltinModule> {
        BUILTIN_MODULE_REGISTRY
            .get(usize::from(module_id))
            .map(|entry| entry.module)
    }
}

/// Built-in modules that can be imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinModule {
    /// The `sys` module providing system-specific parameters and functions.
    Sys,
    /// The `typing` module providing type hints support.
    Typing,
    /// The `asyncio` module providing async/await support (only `gather()` implemented).
    Asyncio,
    /// The `pathlib` module providing object-oriented filesystem paths.
    Pathlib,
    /// The `os` module providing operating system interface (only `getenv()` implemented).
    Os,
    /// The `re` module providing regular expression matching.
    Re,
    /// The `time` module providing wall-clock and monotonic time helpers.
    Time,
    /// The `multiprocessing` module providing process utilities.
    Multiprocessing,
}

impl BuiltinModule {
    /// Get the module from a string ID.
    pub fn from_string_id(string_id: StringId) -> Option<Self> {
        BuiltinModuleRegistry::from_string_id(string_id)
    }

    /// Creates a new instance of this module on the heap.
    ///
    /// Returns a HeapId pointing to the newly allocated module.
    ///
    /// # Panics
    ///
    /// Panics if the required strings have not been pre-interned during prepare phase.
    pub fn create(self, heap: &mut Heap<impl ResourceTracker>, interns: &Interns) -> Result<HeapId, ResourceError> {
        match self {
            Self::Sys => sys::create_module(heap, interns),
            Self::Typing => typing::create_module(heap, interns),
            Self::Asyncio => asyncio::create_module(heap, interns),
            Self::Pathlib => pathlib::create_module(heap, interns),
            Self::Os => os::create_module(heap, interns),
            Self::Re => re::create_module(heap, interns),
            Self::Time => time::create_module(heap, interns),
            Self::Multiprocessing => multiprocessing::create_module(heap, interns),
        }
    }
}

/// All stdlib module function (but not builtins).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum ModuleFunctions {
    Asyncio(asyncio::AsyncioFunctions),
    Multiprocessing(multiprocessing::MultiprocessingFunctions),
    Os(os::OsFunctions),
    Re(re::ReFunctions),
    Time(time::TimeFunctions),
}

impl fmt::Display for ModuleFunctions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asyncio(func) => write!(f, "{func}"),
            Self::Multiprocessing(func) => write!(f, "{func}"),
            Self::Os(func) => write!(f, "{func}"),
            Self::Re(func) => write!(f, "{func}"),
            Self::Time(func) => write!(f, "{func}"),
        }
    }
}

impl ModuleFunctions {
    /// Calls the module function with the given arguments.
    ///
    /// Returns `AttrCallResult` to support both immediate values and OS calls that
    /// require host involvement (e.g., `os.getenv()` needs the host to provide environment variables).
    /// The `interns` parameter is needed by modules that must extract string values from
    /// `Value::InternString` arguments (e.g., the `re` module).
    pub fn call(
        self,
        heap: &mut Heap<impl ResourceTracker>,
        args: ArgValues,
        interns: &Interns,
    ) -> RunResult<AttrCallResult> {
        match self {
            Self::Asyncio(functions) => asyncio::call(heap, functions, args),
            Self::Multiprocessing(functions) => multiprocessing::call(heap, functions, args),
            Self::Os(functions) => os::call(heap, functions, args),
            Self::Re(functions) => re::call(heap, functions, args, interns),
            Self::Time(functions) => time::call(heap, functions, args),
        }
    }

    /// Writes the Python repr() string for this function to a formatter.
    pub fn py_repr_fmt<W: Write>(self, f: &mut W, py_id: usize) -> std::fmt::Result {
        write!(f, "<function {self} at 0x{py_id:x}>")
    }
}

#[cfg(test)]
mod tests {
    use crate::intern::StaticStrings;

    use super::{BuiltinModule, BuiltinModuleRegistry};

    #[test]
    fn builtin_module_registry_roundtrip() {
        for module in [
            BuiltinModule::Sys,
            BuiltinModule::Typing,
            BuiltinModule::Asyncio,
            BuiltinModule::Pathlib,
            BuiltinModule::Os,
            BuiltinModule::Re,
            BuiltinModule::Time,
            BuiltinModule::Multiprocessing,
        ] {
            let module_id = BuiltinModuleRegistry::module_id(module);
            let restored = BuiltinModuleRegistry::from_module_id(module_id).expect("module id must be valid");
            assert_eq!(restored, module);
        }
    }

    #[test]
    fn builtin_module_registry_name_lookup() {
        let sys = StaticStrings::Sys.string_id();
        let re = StaticStrings::Re.string_id();
        let time = StaticStrings::Time.string_id();
        let multiprocessing = StaticStrings::Multiprocessing.string_id();
        let bool_name = StaticStrings::Bool.string_id();

        assert_eq!(BuiltinModuleRegistry::from_string_id(sys), Some(BuiltinModule::Sys));
        assert_eq!(BuiltinModuleRegistry::from_string_id(re), Some(BuiltinModule::Re));
        assert_eq!(BuiltinModuleRegistry::from_string_id(time), Some(BuiltinModule::Time));
        assert_eq!(
            BuiltinModuleRegistry::from_string_id(multiprocessing),
            Some(BuiltinModule::Multiprocessing)
        );
        assert_eq!(BuiltinModuleRegistry::from_string_id(bool_name), None);
    }
}
