//! Bounded WebAssembly binary parser & validator with strict import allowlisting.
//!
//! Enforces deny-all filesystem and network isolation at module load time by
//! rejecting any import outside the tiny bounded WASI/Hivemind host surface.

use std::collections::HashMap;

pub const WASM_MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6D];
pub const WASM_VERSION: [u8; 4] = [0x01, 0x00, 0x00, 0x00];
pub const WASM_PAGE_BYTES: usize = 65_536;
pub const MAX_MODULE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WasmError {
    #[error("module size {0} exceeds maximum {MAX_MODULE_BYTES} bytes")]
    ModuleTooLarge(usize),
    #[error("invalid WebAssembly binary: {0}")]
    InvalidBinary(String),
    #[error("forbidden host import `{module}::{name}` (filesystem and network access are denied)")]
    ForbiddenImport { module: String, name: String },
    #[error("linear memory limit exceeded: requested {requested_pages} pages, limit {max_pages}")]
    MemoryLimitExceeded {
        requested_pages: u32,
        max_pages: u32,
    },
    #[error("fuel exhausted after {executed} instructions (limit {limit})")]
    FuelExhausted { executed: u64, limit: u64 },
    #[error("execution timed out after {elapsed_ms} ms (limit {limit_ms} ms)")]
    Timeout { elapsed_ms: u64, limit_ms: u64 },
    #[error("output limit exceeded: max {0} bytes")]
    OutputLimitExceeded(usize),
    #[error("runtime trap: {0}")]
    Trap(String),
    #[error("missing required exported function `{0}`")]
    MissingExport(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValType {
    I32,
    I64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllowedHostFunc {
    WasiFdWrite,
    WasiProcExit,
    HivemindReadInput,
    HivemindWriteOutput,
}

#[derive(Debug, Clone)]
pub struct FunctionBody {
    pub locals: Vec<ValType>,
    pub code: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DataSegment {
    pub offset: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct GlobalDef {
    pub val_type: ValType,
    pub mutable: bool,
    pub init_i64: i64,
}

#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub types: Vec<FuncType>,
    pub imports: Vec<(AllowedHostFunc, u32)>,
    pub func_type_indices: Vec<u32>,
    pub initial_memory_pages: u32,
    pub max_memory_pages: Option<u32>,
    pub globals: Vec<GlobalDef>,
    pub exports: HashMap<String, (u8, u32)>,
    pub bodies: Vec<FunctionBody>,
    pub data_segments: Vec<DataSegment>,
}

pub fn parse_and_validate(bytes: &[u8], max_allowed_pages: u32) -> Result<ParsedModule, WasmError> {
    if bytes.len() > MAX_MODULE_BYTES {
        return Err(WasmError::ModuleTooLarge(bytes.len()));
    }
    if bytes.len() < 8 || bytes[0..4] != WASM_MAGIC || bytes[4..8] != WASM_VERSION {
        return Err(WasmError::InvalidBinary(
            "missing WebAssembly v1 magic header".into(),
        ));
    }

    let mut cur = Cursor::new(&bytes[8..]);
    let mut module = ParsedModule {
        types: Vec::new(),
        imports: Vec::new(),
        func_type_indices: Vec::new(),
        initial_memory_pages: 0,
        max_memory_pages: None,
        globals: Vec::new(),
        exports: HashMap::new(),
        bodies: Vec::new(),
        data_segments: Vec::new(),
    };

    while !cur.is_eof() {
        let section_id = cur.read_u8()?;
        let section_len = cur.read_u32_leb()? as usize;
        let payload = cur.read_bytes(section_len)?;
        let mut sc = Cursor::new(payload);

        match section_id {
            0 => {} // custom section ignored
            1 => {
                // Type section
                let count = sc.read_u32_leb()?;
                for _ in 0..count {
                    let form = sc.read_u8()?;
                    if form != 0x60 {
                        return Err(WasmError::InvalidBinary("expected func type 0x60".into()));
                    }
                    let n_params = sc.read_u32_leb()?;
                    let mut params = Vec::with_capacity(n_params as usize);
                    for _ in 0..n_params {
                        params.push(read_val_type(&mut sc)?);
                    }
                    let n_results = sc.read_u32_leb()?;
                    let mut results = Vec::with_capacity(n_results as usize);
                    for _ in 0..n_results {
                        results.push(read_val_type(&mut sc)?);
                    }
                    module.types.push(FuncType { params, results });
                }
            }
            2 => {
                // Import section (STRICT allowlist; denies FS/network/clock/etc.)
                let count = sc.read_u32_leb()?;
                for _ in 0..count {
                    let mod_name = sc.read_string()?;
                    let field_name = sc.read_string()?;
                    let kind = sc.read_u8()?;
                    if kind != 0x00 {
                        return Err(WasmError::ForbiddenImport {
                            module: mod_name,
                            name: field_name,
                        });
                    }
                    let type_idx = sc.read_u32_leb()?;
                    let host_fn = match (mod_name.as_str(), field_name.as_str()) {
                        ("wasi_snapshot_preview1", "fd_write") => AllowedHostFunc::WasiFdWrite,
                        ("wasi_snapshot_preview1", "proc_exit") => AllowedHostFunc::WasiProcExit,
                        ("hivemind", "read_input") => AllowedHostFunc::HivemindReadInput,
                        ("hivemind", "write_output") => AllowedHostFunc::HivemindWriteOutput,
                        _ => {
                            return Err(WasmError::ForbiddenImport {
                                module: mod_name,
                                name: field_name,
                            });
                        }
                    };
                    module.imports.push((host_fn, type_idx));
                }
            }
            3 => {
                // Function section
                let count = sc.read_u32_leb()?;
                for _ in 0..count {
                    module.func_type_indices.push(sc.read_u32_leb()?);
                }
            }
            5 => {
                // Memory section
                let count = sc.read_u32_leb()?;
                if count > 1 {
                    return Err(WasmError::InvalidBinary(
                        "only single linear memory is supported".into(),
                    ));
                }
                if count == 1 {
                    let flags = sc.read_u8()?;
                    let initial = sc.read_u32_leb()?;
                    let max = if flags == 0x01 {
                        Some(sc.read_u32_leb()?)
                    } else {
                        None
                    };
                    if initial > max_allowed_pages {
                        return Err(WasmError::MemoryLimitExceeded {
                            requested_pages: initial,
                            max_pages: max_allowed_pages,
                        });
                    }
                    module.initial_memory_pages = initial;
                    module.max_memory_pages = max;
                }
            }
            6 => {
                // Global section
                let count = sc.read_u32_leb()?;
                for _ in 0..count {
                    let val_type = read_val_type(&mut sc)?;
                    let mutable = sc.read_u8()? != 0;
                    let init_op = sc.read_u8()?;
                    let init_i64 = match init_op {
                        0x41 => i64::from(sc.read_i32_leb()?),
                        0x42 => sc.read_i64_leb()?,
                        _ => {
                            return Err(WasmError::InvalidBinary(
                                "unsupported global init expr".into(),
                            ))
                        }
                    };
                    let end = sc.read_u8()?;
                    if end != 0x0B {
                        return Err(WasmError::InvalidBinary(
                            "missing end opcode in global init".into(),
                        ));
                    }
                    module.globals.push(GlobalDef {
                        val_type,
                        mutable,
                        init_i64,
                    });
                }
            }
            7 => {
                // Export section
                let count = sc.read_u32_leb()?;
                for _ in 0..count {
                    let name = sc.read_string()?;
                    let kind = sc.read_u8()?;
                    let idx = sc.read_u32_leb()?;
                    module.exports.insert(name, (kind, idx));
                }
            }
            10 => {
                // Code section
                let count = sc.read_u32_leb()?;
                for _ in 0..count {
                    let body_len = sc.read_u32_leb()? as usize;
                    let body_bytes = sc.read_bytes(body_len)?;
                    let mut bc = Cursor::new(body_bytes);
                    let local_decl_count = bc.read_u32_leb()?;
                    let mut locals = Vec::new();
                    for _ in 0..local_decl_count {
                        let repeat = bc.read_u32_leb()?;
                        let vt = read_val_type(&mut bc)?;
                        for _ in 0..repeat {
                            locals.push(vt);
                        }
                    }
                    let code = bc.remaining().to_vec();
                    module.bodies.push(FunctionBody { locals, code });
                }
            }
            11 => {
                // Data section
                let count = sc.read_u32_leb()?;
                for _ in 0..count {
                    let mode = sc.read_u32_leb()?;
                    if mode != 0 {
                        return Err(WasmError::InvalidBinary(
                            "only active memory-0 data segments are supported".into(),
                        ));
                    }
                    let op = sc.read_u8()?;
                    if op != 0x41 {
                        return Err(WasmError::InvalidBinary(
                            "data segment offset must be i32.const".into(),
                        ));
                    }
                    let offset = sc.read_i32_leb()? as u32;
                    if sc.read_u8()? != 0x0B {
                        return Err(WasmError::InvalidBinary(
                            "missing end opcode in data offset".into(),
                        ));
                    }
                    let data_len = sc.read_u32_leb()? as usize;
                    let seg_bytes = sc.read_bytes(data_len)?.to_vec();
                    module.data_segments.push(DataSegment {
                        offset,
                        bytes: seg_bytes,
                    });
                }
            }
            _ => {
                // Skip unknown/optional sections safely
            }
        }
    }

    if module.func_type_indices.len() != module.bodies.len() {
        return Err(WasmError::InvalidBinary(
            "function section and code section counts do not match".into(),
        ));
    }

    Ok(module)
}

fn read_val_type(cur: &mut Cursor<'_>) -> Result<ValType, WasmError> {
    match cur.read_u8()? {
        0x7F => Ok(ValType::I32),
        0x7E => Ok(ValType::I64),
        other => Err(WasmError::InvalidBinary(format!(
            "unsupported value type 0x{other:02x}"
        ))),
    }
}

pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    pub fn read_u8(&mut self) -> Result<u8, WasmError> {
        if self.pos >= self.data.len() {
            return Err(WasmError::InvalidBinary("unexpected EOF".into()));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    pub fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], WasmError> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| WasmError::InvalidBinary("length overflow".into()))?;
        if end > self.data.len() {
            return Err(WasmError::InvalidBinary("unexpected EOF in slice".into()));
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    pub fn read_string(&mut self) -> Result<String, WasmError> {
        let len = self.read_u32_leb()? as usize;
        let bytes = self.read_bytes(len)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| WasmError::InvalidBinary("invalid UTF-8 string".into()))
    }

    pub fn read_u32_leb(&mut self) -> Result<u32, WasmError> {
        let mut result = 0u32;
        let mut shift = 0u32;
        for _ in 0..5 {
            let byte = self.read_u8()?;
            result |= u32::from(byte & 0x7F) << shift;
            if (byte & 0x80) == 0 {
                return Ok(result);
            }
            shift += 7;
        }
        Err(WasmError::InvalidBinary("malformed u32 LEB128".into()))
    }

    pub fn read_i32_leb(&mut self) -> Result<i32, WasmError> {
        let mut result = 0i32;
        let mut shift = 0u32;
        for _ in 0..5 {
            let byte = self.read_u8()?;
            result |= i32::from(byte & 0x7F) << shift;
            shift += 7;
            if (byte & 0x80) == 0 {
                if shift < 32 && (byte & 0x40) != 0 {
                    result |= !0i32 << shift;
                }
                return Ok(result);
            }
        }
        Err(WasmError::InvalidBinary("malformed i32 LEB128".into()))
    }

    pub fn read_i64_leb(&mut self) -> Result<i64, WasmError> {
        let mut result = 0i64;
        let mut shift = 0u32;
        for _ in 0..10 {
            let byte = self.read_u8()?;
            result |= i64::from(byte & 0x7F) << shift;
            shift += 7;
            if (byte & 0x80) == 0 {
                if shift < 64 && (byte & 0x40) != 0 {
                    result |= !0i64 << shift;
                }
                return Ok(result);
            }
        }
        Err(WasmError::InvalidBinary("malformed i64 LEB128".into()))
    }
}
