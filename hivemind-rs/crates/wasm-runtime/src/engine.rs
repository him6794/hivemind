//! Bounded fuel-metered WebAssembly / WASI-subset execution engine.

use crate::parser::{
    parse_and_validate, AllowedHostFunc, Cursor, ParsedModule, ValType, WasmError, WASM_PAGE_BYTES,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmLimits {
    /// Maximum instructions executed before trapping with `FuelExhausted`.
    pub max_fuel: u64,
    /// Maximum linear memory pages (64 KiB each).
    pub max_memory_pages: u32,
    /// Maximum wall-clock execution time.
    pub timeout_ms: u64,
    /// Maximum captured stdout/output bytes.
    pub max_output_bytes: usize,
    /// Maximum call stack depth.
    pub max_call_depth: usize,
}

impl Default for WasmLimits {
    fn default() -> Self {
        Self {
            max_fuel: 10_000_000,
            max_memory_pages: 256, // 16 MiB
            timeout_ms: 5_000,
            max_output_bytes: 256 * 1024,
            max_call_depth: 64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmExecutionReceipt {
    pub module_sha256: String,
    pub entrypoint: String,
    pub exit_code: i32,
    pub return_value: Option<i64>,
    pub stdout: Vec<u8>,
    pub fuel_consumed: u64,
    pub peak_memory_pages: u32,
    pub wall_time_ms: u64,
}

#[derive(Debug, Clone, Copy)]
enum Val {
    I32(i32),
    I64(i64),
}

impl Val {
    fn as_i32(self) -> Result<i32, WasmError> {
        match self {
            Val::I32(v) => Ok(v),
            Val::I64(_) => Err(WasmError::Trap("type mismatch: expected i32".into())),
        }
    }
    fn as_i64(self) -> Result<i64, WasmError> {
        match self {
            Val::I64(v) => Ok(v),
            Val::I32(_) => Err(WasmError::Trap("type mismatch: expected i64".into())),
        }
    }
}

struct BlockFrame {
    is_loop: bool,
    start_pc: usize,
    end_pc: usize,
    stack_height: usize,
}

struct VmState<'a> {
    module: &'a ParsedModule,
    limits: WasmLimits,
    started: Instant,
    fuel_used: u64,
    memory: Vec<u8>,
    peak_pages: u32,
    globals: Vec<Val>,
    input: &'a [u8],
    stdout: Vec<u8>,
    proc_exit_code: Option<i32>,
}

/// Validates and executes a WebAssembly binary inside strict fuel, memory,
/// timeout, and deny-all FS/network bounds.
pub fn execute_wasm(
    wasm_bytes: &[u8],
    entrypoint: &str,
    args: &[i64],
    input: &[u8],
    limits: WasmLimits,
) -> Result<WasmExecutionReceipt, WasmError> {
    let module_sha256 = hex::encode(Sha256::digest(wasm_bytes));
    let module = parse_and_validate(wasm_bytes, limits.max_memory_pages)?;
    let &(kind, func_idx) = module
        .exports
        .get(entrypoint)
        .ok_or_else(|| WasmError::MissingExport(entrypoint.to_owned()))?;
    if kind != 0x00 {
        return Err(WasmError::MissingExport(entrypoint.to_owned()));
    }

    let mut memory = vec![0u8; (module.initial_memory_pages as usize) * WASM_PAGE_BYTES];
    for seg in &module.data_segments {
        let start = seg.offset as usize;
        let end = start
            .checked_add(seg.bytes.len())
            .ok_or_else(|| WasmError::Trap("data segment offset overflow".into()))?;
        if end > memory.len() {
            return Err(WasmError::Trap(
                "data segment out of initial linear memory bounds".into(),
            ));
        }
        memory[start..end].copy_from_slice(&seg.bytes);
    }

    let globals = module
        .globals
        .iter()
        .map(|g| match g.val_type {
            ValType::I32 => Val::I32(g.init_i64 as i32),
            ValType::I64 => Val::I64(g.init_i64),
        })
        .collect();

    let mut vm = VmState {
        module: &module,
        limits,
        started: Instant::now(),
        fuel_used: 0,
        memory,
        peak_pages: module.initial_memory_pages,
        globals,
        input,
        stdout: Vec::new(),
        proc_exit_code: None,
    };

    let func_type = vm.func_type_for(func_idx)?;
    if func_type.params.len() != args.len() {
        return Err(WasmError::Trap(format!(
            "entrypoint `{entrypoint}` expects {} args, got {}",
            func_type.params.len(),
            args.len()
        )));
    }
    let typed_args: Vec<Val> = func_type
        .params
        .iter()
        .zip(args.iter())
        .map(|(vt, &v)| match vt {
            ValType::I32 => Val::I32(v as i32),
            ValType::I64 => Val::I64(v),
        })
        .collect();

    let result = vm.call_function(func_idx, typed_args, 0)?;
    let return_value = result.first().map(|v| match *v {
        Val::I32(x) => i64::from(x),
        Val::I64(x) => x,
    });

    Ok(WasmExecutionReceipt {
        module_sha256,
        entrypoint: entrypoint.to_owned(),
        exit_code: vm.proc_exit_code.unwrap_or(0),
        return_value,
        stdout: vm.stdout,
        fuel_consumed: vm.fuel_used,
        peak_memory_pages: vm.peak_pages,
        wall_time_ms: vm.started.elapsed().as_millis() as u64,
    })
}

impl<'a> VmState<'a> {
    fn func_type_for(&self, func_idx: u32) -> Result<&'a crate::parser::FuncType, WasmError> {
        let idx = func_idx as usize;
        let type_idx = if idx < self.module.imports.len() {
            self.module.imports[idx].1 as usize
        } else {
            let local_idx = idx - self.module.imports.len();
            *self
                .module
                .func_type_indices
                .get(local_idx)
                .ok_or_else(|| WasmError::Trap(format!("invalid function index {func_idx}")))?
                as usize
        };
        self.module
            .types
            .get(type_idx)
            .ok_or_else(|| WasmError::Trap(format!("invalid type index {type_idx}")))
    }

    fn charge_fuel(&mut self) -> Result<(), WasmError> {
        self.fuel_used = self.fuel_used.saturating_add(1);
        if self.fuel_used > self.limits.max_fuel {
            return Err(WasmError::FuelExhausted {
                executed: self.fuel_used,
                limit: self.limits.max_fuel,
            });
        }
        // Check wall-clock timeout periodically to keep overhead tiny.
        if (self.fuel_used & 0x0FFF) == 0 {
            let elapsed = self.started.elapsed();
            if elapsed > Duration::from_millis(self.limits.timeout_ms) {
                return Err(WasmError::Timeout {
                    elapsed_ms: elapsed.as_millis() as u64,
                    limit_ms: self.limits.timeout_ms,
                });
            }
        }
        Ok(())
    }

    fn read_mem(&self, addr: usize, len: usize) -> Result<&[u8], WasmError> {
        let end = addr
            .checked_add(len)
            .ok_or_else(|| WasmError::Trap("memory address overflow".into()))?;
        if end > self.memory.len() {
            return Err(WasmError::Trap("out-of-bounds memory read".into()));
        }
        Ok(&self.memory[addr..end])
    }

    fn write_mem(&mut self, addr: usize, data: &[u8]) -> Result<(), WasmError> {
        let end = addr
            .checked_add(data.len())
            .ok_or_else(|| WasmError::Trap("memory address overflow".into()))?;
        if end > self.memory.len() {
            return Err(WasmError::Trap("out-of-bounds memory write".into()));
        }
        self.memory[addr..end].copy_from_slice(data);
        Ok(())
    }

    fn append_stdout(&mut self, bytes: &[u8]) -> Result<(), WasmError> {
        if self.stdout.len().saturating_add(bytes.len()) > self.limits.max_output_bytes {
            return Err(WasmError::OutputLimitExceeded(self.limits.max_output_bytes));
        }
        self.stdout.extend_from_slice(bytes);
        Ok(())
    }

    fn call_host(
        &mut self,
        host_fn: &AllowedHostFunc,
        args: &[Val],
    ) -> Result<Vec<Val>, WasmError> {
        match host_fn {
            AllowedHostFunc::WasiProcExit => {
                let code = args
                    .first()
                    .copied()
                    .ok_or_else(|| WasmError::Trap("missing proc_exit arg".into()))?
                    .as_i32()?;
                self.proc_exit_code = Some(code);
                Ok(Vec::new())
            }
            AllowedHostFunc::WasiFdWrite => {
                // (fd: i32, iovs_ptr: i32, iovs_len: i32, nwritten_ptr: i32) -> errno: i32
                if args.len() != 4 {
                    return Err(WasmError::Trap("fd_write requires 4 args".into()));
                }
                let fd = args[0].as_i32()?;
                let iovs_ptr = args[1].as_i32()? as usize;
                let iovs_len = args[2].as_i32()? as usize;
                let nwritten_ptr = args[3].as_i32()? as usize;
                if fd != 1 && fd != 2 {
                    // Only stdout/stderr allowed; all other FDs return EBADF (8).
                    return Ok(vec![Val::I32(8)]);
                }
                let mut total_written = 0u32;
                for i in 0..iovs_len {
                    let entry = self.read_mem(iovs_ptr + i * 8, 8)?.to_vec();
                    let buf_ptr =
                        u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]) as usize;
                    let buf_len =
                        u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]) as usize;
                    let chunk = self.read_mem(buf_ptr, buf_len)?.to_vec();
                    self.append_stdout(&chunk)?;
                    total_written = total_written.saturating_add(chunk.len() as u32);
                }
                self.write_mem(nwritten_ptr, &total_written.to_le_bytes())?;
                Ok(vec![Val::I32(0)])
            }
            AllowedHostFunc::HivemindReadInput => {
                // (dst_ptr: i32, max_len: i32) -> bytes_read: i32
                let dst = args[0].as_i32()? as usize;
                let max_len = (args[1].as_i32()? as usize).min(self.input.len());
                let slice = &self.input[..max_len];
                self.write_mem(dst, slice)?;
                Ok(vec![Val::I32(max_len as i32)])
            }
            AllowedHostFunc::HivemindWriteOutput => {
                // (src_ptr: i32, len: i32) -> bytes_written: i32
                let src = args[0].as_i32()? as usize;
                let len = args[1].as_i32()? as usize;
                let bytes = self.read_mem(src, len)?.to_vec();
                self.append_stdout(&bytes)?;
                Ok(vec![Val::I32(bytes.len() as i32)])
            }
        }
    }

    fn call_function(
        &mut self,
        func_idx: u32,
        args: Vec<Val>,
        depth: usize,
    ) -> Result<Vec<Val>, WasmError> {
        if depth > self.limits.max_call_depth {
            return Err(WasmError::Trap("call stack depth exceeded".into()));
        }
        let idx = func_idx as usize;
        if idx < self.module.imports.len() {
            let host_fn = self.module.imports[idx].0.clone();
            return self.call_host(&host_fn, &args);
        }

        let local_fn_idx = idx - self.module.imports.len();
        let body = self
            .module
            .bodies
            .get(local_fn_idx)
            .ok_or_else(|| WasmError::Trap(format!("missing body for func {func_idx}")))?;
        let ftype = self.func_type_for(func_idx)?;

        let mut locals = args;
        for vt in &body.locals {
            locals.push(match vt {
                ValType::I32 => Val::I32(0),
                ValType::I64 => Val::I64(0),
            });
        }

        let mut stack: Vec<Val> = Vec::new();
        let mut blocks: Vec<BlockFrame> = Vec::new();
        let mut cur = Cursor::new(&body.code);

        while !cur.is_eof() {
            if self.proc_exit_code.is_some() {
                break;
            }
            self.charge_fuel()?;
            let op = cur.read_u8()?;
            match op {
                0x00 => return Err(WasmError::Trap("unreachable executed".into())),
                0x01 => {} // nop
                0x02 | 0x03 => {
                    // block / loop
                    let _block_type = cur.read_u8()?;
                    let start_pc = cur.pos();
                    let end_pc = find_matching_end(&body.code, start_pc)?;
                    blocks.push(BlockFrame {
                        is_loop: op == 0x03,
                        start_pc,
                        end_pc,
                        stack_height: stack.len(),
                    });
                }
                0x04 => {
                    // if
                    let _block_type = cur.read_u8()?;
                    let cond = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("stack underflow".into()))?
                        .as_i32()?;
                    let start_pc = cur.pos();
                    let (else_pc, end_pc) = find_if_targets(&body.code, start_pc)?;
                    if cond != 0 {
                        blocks.push(BlockFrame {
                            is_loop: false,
                            start_pc,
                            end_pc,
                            stack_height: stack.len(),
                        });
                    } else if let Some(ep) = else_pc {
                        blocks.push(BlockFrame {
                            is_loop: false,
                            start_pc: ep,
                            end_pc,
                            stack_height: stack.len(),
                        });
                        cur.set_pos(ep);
                    } else {
                        cur.set_pos(end_pc);
                    }
                }
                0x05 => {
                    // else -> jump to end of current if block
                    if let Some(frame) = blocks.pop() {
                        cur.set_pos(frame.end_pc);
                    }
                }
                0x0B => {
                    // end
                    if blocks.pop().is_none() {
                        break; // end of function
                    }
                }
                0x0C => {
                    // br label_idx
                    let depth_arg = cur.read_u32_leb()? as usize;
                    branch_to(&mut cur, &mut blocks, &mut stack, depth_arg)?;
                }
                0x0D => {
                    // br_if label_idx
                    let depth_arg = cur.read_u32_leb()? as usize;
                    let cond = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("stack underflow".into()))?
                        .as_i32()?;
                    if cond != 0 {
                        branch_to(&mut cur, &mut blocks, &mut stack, depth_arg)?;
                    }
                }
                0x0F => break, // return
                0x10 => {
                    // call func_idx
                    let callee = cur.read_u32_leb()?;
                    let callee_type = self.func_type_for(callee)?;
                    let n_params = callee_type.params.len();
                    if stack.len() < n_params {
                        return Err(WasmError::Trap("stack underflow in call".into()));
                    }
                    let call_args = stack.split_off(stack.len() - n_params);
                    let rets = self.call_function(callee, call_args, depth + 1)?;
                    stack.extend(rets);
                }
                0x1A => {
                    stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("stack underflow".into()))?;
                }
                0x20 => {
                    // local.get
                    let lidx = cur.read_u32_leb()? as usize;
                    let val = *locals
                        .get(lidx)
                        .ok_or_else(|| WasmError::Trap("local index out of bounds".into()))?;
                    stack.push(val);
                }
                0x21 => {
                    // local.set
                    let lidx = cur.read_u32_leb()? as usize;
                    let val = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("stack underflow".into()))?;
                    *locals
                        .get_mut(lidx)
                        .ok_or_else(|| WasmError::Trap("local index out of bounds".into()))? = val;
                }
                0x22 => {
                    // local.tee
                    let lidx = cur.read_u32_leb()? as usize;
                    let val = *stack
                        .last()
                        .ok_or_else(|| WasmError::Trap("stack underflow".into()))?;
                    *locals
                        .get_mut(lidx)
                        .ok_or_else(|| WasmError::Trap("local index out of bounds".into()))? = val;
                }
                0x23 => {
                    // global.get
                    let gidx = cur.read_u32_leb()? as usize;
                    let val = *self
                        .globals
                        .get(gidx)
                        .ok_or_else(|| WasmError::Trap("global index out of bounds".into()))?;
                    stack.push(val);
                }
                0x24 => {
                    // global.set
                    let gidx = cur.read_u32_leb()? as usize;
                    let val = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("stack underflow".into()))?;
                    *self
                        .globals
                        .get_mut(gidx)
                        .ok_or_else(|| WasmError::Trap("global index out of bounds".into()))? = val;
                }
                0x28 => {
                    // i32.load
                    let _align = cur.read_u32_leb()?;
                    let offset = cur.read_u32_leb()? as usize;
                    let base = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("stack underflow".into()))?
                        .as_i32()? as u32 as usize;
                    let b = self.read_mem(base.saturating_add(offset), 4)?;
                    stack.push(Val::I32(i32::from_le_bytes([b[0], b[1], b[2], b[3]])));
                }
                0x36 => {
                    // i32.store
                    let _align = cur.read_u32_leb()?;
                    let offset = cur.read_u32_leb()? as usize;
                    let val = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("stack underflow".into()))?
                        .as_i32()?;
                    let base = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("stack underflow".into()))?
                        .as_i32()? as u32 as usize;
                    self.write_mem(base.saturating_add(offset), &val.to_le_bytes())?;
                }
                0x3F => {
                    // memory.size
                    let _mem_idx = cur.read_u8()?;
                    let pages = (self.memory.len() / WASM_PAGE_BYTES) as i32;
                    stack.push(Val::I32(pages));
                }
                0x40 => {
                    // memory.grow
                    let _mem_idx = cur.read_u8()?;
                    let delta = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("stack underflow".into()))?
                        .as_i32()? as u32;
                    let cur_pages = (self.memory.len() / WASM_PAGE_BYTES) as u32;
                    let new_pages = cur_pages.saturating_add(delta);
                    let cap = self
                        .module
                        .max_memory_pages
                        .map_or(self.limits.max_memory_pages, |m| {
                            m.min(self.limits.max_memory_pages)
                        });
                    if new_pages > cap {
                        return Err(WasmError::MemoryLimitExceeded {
                            requested_pages: new_pages,
                            max_pages: cap,
                        });
                    }
                    self.memory
                        .resize((new_pages as usize) * WASM_PAGE_BYTES, 0);
                    self.peak_pages = self.peak_pages.max(new_pages);
                    stack.push(Val::I32(cur_pages as i32));
                }
                0x41 => stack.push(Val::I32(cur.read_i32_leb()?)),
                0x42 => stack.push(Val::I64(cur.read_i64_leb()?)),
                0x45 => {
                    let a = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("underflow".into()))?
                        .as_i32()?;
                    stack.push(Val::I32(i32::from(a == 0)));
                }
                0x46..=0x4F => {
                    let b = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("underflow".into()))?
                        .as_i32()?;
                    let a = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("underflow".into()))?
                        .as_i32()?;
                    let res = match op {
                        0x46 => a == b,
                        0x47 => a != b,
                        0x48 => a < b,
                        0x49 => (a as u32) < (b as u32),
                        0x4A => a > b,
                        0x4B => (a as u32) > (b as u32),
                        0x4C => a <= b,
                        0x4D => (a as u32) <= (b as u32),
                        0x4E => a >= b,
                        0x4F => (a as u32) >= (b as u32),
                        _ => unreachable!(),
                    };
                    stack.push(Val::I32(i32::from(res)));
                }
                0x6A..=0x73 => {
                    let b = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("underflow".into()))?
                        .as_i32()?;
                    let a = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("underflow".into()))?
                        .as_i32()?;
                    let res = match op {
                        0x6A => a.wrapping_add(b),
                        0x6B => a.wrapping_sub(b),
                        0x6C => a.wrapping_mul(b),
                        0x6D => {
                            if b == 0 {
                                return Err(WasmError::Trap("integer divide by zero".into()));
                            }
                            a.wrapping_div(b)
                        }
                        0x6E => {
                            if b == 0 {
                                return Err(WasmError::Trap("integer divide by zero".into()));
                            }
                            ((a as u32) / (b as u32)) as i32
                        }
                        0x6F => {
                            if b == 0 {
                                return Err(WasmError::Trap("integer divide by zero".into()));
                            }
                            a.wrapping_rem(b)
                        }
                        0x70 => {
                            if b == 0 {
                                return Err(WasmError::Trap("integer divide by zero".into()));
                            }
                            ((a as u32) % (b as u32)) as i32
                        }
                        0x71 => a & b,
                        0x72 => a | b,
                        0x73 => a ^ b,
                        _ => unreachable!(),
                    };
                    stack.push(Val::I32(res));
                }
                0x7C..=0x7E => {
                    let b = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("underflow".into()))?
                        .as_i64()?;
                    let a = stack
                        .pop()
                        .ok_or_else(|| WasmError::Trap("underflow".into()))?
                        .as_i64()?;
                    let res = match op {
                        0x7C => a.wrapping_add(b),
                        0x7D => a.wrapping_sub(b),
                        0x7E => a.wrapping_mul(b),
                        _ => unreachable!(),
                    };
                    stack.push(Val::I64(res));
                }
                other => {
                    return Err(WasmError::Trap(format!(
                        "unsupported wasm opcode 0x{other:02x}"
                    )));
                }
            }
        }

        let n_results = ftype.results.len();
        if self.proc_exit_code.is_some() || n_results == 0 {
            return Ok(Vec::new());
        }
        if stack.len() < n_results {
            return Err(WasmError::Trap("missing return value on stack".into()));
        }
        Ok(stack.split_off(stack.len() - n_results))
    }
}

fn branch_to(
    cur: &mut Cursor<'_>,
    blocks: &mut Vec<BlockFrame>,
    stack: &mut Vec<Val>,
    depth: usize,
) -> Result<(), WasmError> {
    if depth >= blocks.len() {
        // Branch out of function
        blocks.clear();
        cur.set_pos(usize::MAX / 2);
        return Ok(());
    }
    let target_idx = blocks.len() - 1 - depth;
    let is_loop = blocks[target_idx].is_loop;
    let target_pc = if is_loop {
        blocks[target_idx].start_pc
    } else {
        blocks[target_idx].end_pc
    };
    let stack_height = blocks[target_idx].stack_height;
    if is_loop {
        blocks.truncate(target_idx + 1);
    } else {
        blocks.truncate(target_idx);
    }
    if stack.len() > stack_height {
        stack.truncate(stack_height);
    }
    cur.set_pos(target_pc);
    Ok(())
}

fn skip_immediate(cur: &mut Cursor<'_>, op: u8) -> Result<(), WasmError> {
    match op {
        0x02..=0x04 => {
            cur.read_u8()?;
        }
        0x0C | 0x0D | 0x10 | 0x20..=0x24 => {
            cur.read_u32_leb()?;
        }
        0x28 | 0x36 => {
            cur.read_u32_leb()?;
            cur.read_u32_leb()?;
        }
        0x3F | 0x40 => {
            cur.read_u8()?;
        }
        0x41 => {
            cur.read_i32_leb()?;
        }
        0x42 => {
            cur.read_i64_leb()?;
        }
        _ => {}
    }
    Ok(())
}

fn find_matching_end(code: &[u8], start_pc: usize) -> Result<usize, WasmError> {
    let mut cur = Cursor::new(code);
    cur.set_pos(start_pc);
    let mut nesting = 1usize;
    while !cur.is_eof() {
        let op = cur.read_u8()?;
        match op {
            0x02..=0x04 => {
                nesting += 1;
                skip_immediate(&mut cur, op)?;
            }
            0x0B => {
                nesting -= 1;
                if nesting == 0 {
                    return Ok(cur.pos());
                }
            }
            _ => skip_immediate(&mut cur, op)?,
        }
    }
    Err(WasmError::InvalidBinary("unterminated block".into()))
}

fn find_if_targets(code: &[u8], start_pc: usize) -> Result<(Option<usize>, usize), WasmError> {
    let mut cur = Cursor::new(code);
    cur.set_pos(start_pc);
    let mut nesting = 1usize;
    let mut else_pc = None;
    while !cur.is_eof() {
        let op = cur.read_u8()?;
        match op {
            0x02..=0x04 => {
                nesting += 1;
                skip_immediate(&mut cur, op)?;
            }
            0x05 if nesting == 1 => {
                else_pc = Some(cur.pos());
            }
            0x0B => {
                nesting -= 1;
                if nesting == 0 {
                    return Ok((else_pc, cur.pos()));
                }
            }
            _ => skip_immediate(&mut cur, op)?,
        }
    }
    Err(WasmError::InvalidBinary("unterminated if block".into()))
}
