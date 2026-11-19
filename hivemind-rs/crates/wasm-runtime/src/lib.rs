//! Cross-platform bounded WebAssembly / WASI-subset sandbox for Hivemind Workers.

pub mod engine;
pub mod parser;

pub use engine::{execute_wasm, WasmExecutionReceipt, WasmLimits};
pub use parser::{parse_and_validate, WasmError, WASM_PAGE_BYTES};

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_section(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![id];
        encode_u32_leb(&mut out, payload.len() as u32);
        out.extend_from_slice(payload);
        out
    }

    fn encode_u32_leb(out: &mut Vec<u8>, mut val: u32) {
        loop {
            let mut b = (val & 0x7F) as u8;
            val >>= 7;
            if val != 0 {
                b |= 0x80;
            }
            out.push(b);
            if val == 0 {
                break;
            }
        }
    }

    fn encode_str(out: &mut Vec<u8>, s: &str) {
        encode_u32_leb(out, s.len() as u32);
        out.extend_from_slice(s.as_bytes());
    }

    fn build_module(sections: Vec<(u8, Vec<u8>)>) -> Vec<u8> {
        let mut wasm = Vec::new();
        wasm.extend_from_slice(&parser::WASM_MAGIC);
        wasm.extend_from_slice(&parser::WASM_VERSION);
        for (id, payload) in sections {
            wasm.extend_from_slice(&encode_section(id, &payload));
        }
        wasm
    }

    #[test]
    fn wasm_executes_bounded_loop_and_wasi_fd_write_stdout() {
        // Type 0: (i32, i32, i32, i32) -> i32  [wasi fd_write]
        // Type 1: () -> i32                    [_start: sums 1..=10 and prints "HI"]
        let mut type_sec = vec![2];
        type_sec.extend_from_slice(&[0x60, 4, 0x7F, 0x7F, 0x7F, 0x7F, 1, 0x7F]);
        type_sec.extend_from_slice(&[0x60, 0, 1, 0x7F]);

        // Import 0: wasi_snapshot_preview1::fd_write (type 0)
        let mut import_sec = vec![1];
        encode_str(&mut import_sec, "wasi_snapshot_preview1");
        encode_str(&mut import_sec, "fd_write");
        import_sec.push(0x00); // func
        import_sec.push(0x00); // type 0

        // Function 1 (local 0): type 1
        let func_sec = vec![1, 1];
        // Memory: 1 initial page
        let mem_sec = vec![1, 0x00, 1];
        // Export "_start" -> func 1
        let mut exp_sec = vec![1];
        encode_str(&mut exp_sec, "_start");
        exp_sec.push(0x00);
        exp_sec.push(0x01);

        // Data segment: at offset 0 put iov={ptr:16, len:2}, at offset 16 put "HI"
        let mut data_sec = vec![2];
        // seg 0: offset 0, bytes [16,0,0,0, 2,0,0,0]
        data_sec.extend_from_slice(&[0x00, 0x41, 0x00, 0x0B, 8, 16, 0, 0, 0, 2, 0, 0, 0]);
        // seg 1: offset 16, bytes "HI"
        data_sec.extend_from_slice(&[0x00, 0x41, 16, 0x0B, 2, b'H', b'I']);

        // Body of _start:
        // locals: 2 i32s (local 0 = i, local 1 = sum)
        // call fd_write(1, 0, 1, 32); drop;
        // sum 1..=10 -> 55
        let mut body = vec![
            1, 2, 0x7F, // 2 i32 locals
            0x41, 1, // fd=1 (stdout)
            0x41, 0, // iovs_ptr=0
            0x41, 1, // iovs_len=1
            0x41, 32, // nwritten_ptr=32
            0x10, 0,    // call fd_write
            0x1A, // drop errno
            0x41, 1, 0x21, 0, // local.set 0 = 1 (i)
            0x41, 0, 0x21, 1, // local.set 1 = 0 (sum)
            0x03, 0x40, // loop
            0x20, 1, 0x20, 0, 0x6A, 0x21, 1, // sum = sum + i
            0x20, 0, 0x41, 1, 0x6A, 0x22, 0, // i = i + 1 (tee)
            0x41, 10, 0x4C, // i <= 10
            0x0D, 0,    // br_if 0 (loop)
            0x0B, // end loop
            0x20, 1,    // local.get 1 (sum)
            0x0B, // end func
        ];
        let mut code_sec = vec![1];
        encode_u32_leb(&mut code_sec, body.len() as u32);
        code_sec.append(&mut body);

        let wasm = build_module(vec![
            (1, type_sec),
            (2, import_sec),
            (3, func_sec),
            (5, mem_sec),
            (7, exp_sec),
            (10, code_sec),
            (11, data_sec),
        ]);

        let receipt = execute_wasm(&wasm, "_start", &[], b"", WasmLimits::default())
            .expect("valid WASI module should execute");
        assert_eq!(receipt.exit_code, 0);
        assert_eq!(receipt.return_value, Some(55));
        assert_eq!(receipt.stdout, b"HI");
        assert!(receipt.fuel_consumed > 50);
        assert_eq!(receipt.peak_memory_pages, 1);
    }

    #[test]
    fn infinite_loop_is_trapped_by_fuel_limit() {
        let type_sec = vec![1, 0x60, 0, 0];
        let func_sec = vec![1, 0];
        let mut exp_sec = vec![1];
        encode_str(&mut exp_sec, "spin");
        exp_sec.extend_from_slice(&[0x00, 0x00]);
        let body = vec![0, 0x03, 0x40, 0x0C, 0, 0x0B, 0x0B]; // loop { br 0 }
        let mut code_sec = vec![1];
        encode_u32_leb(&mut code_sec, body.len() as u32);
        code_sec.extend_from_slice(&body);

        let wasm = build_module(vec![
            (1, type_sec),
            (3, func_sec),
            (7, exp_sec),
            (10, code_sec),
        ]);

        let err = execute_wasm(
            &wasm,
            "spin",
            &[],
            b"",
            WasmLimits {
                max_fuel: 500,
                ..WasmLimits::default()
            },
        )
        .expect_err("infinite loop must exhaust fuel");
        assert!(matches!(err, WasmError::FuelExhausted { limit: 500, .. }));
    }

    #[test]
    fn forbidden_filesystem_and_network_imports_are_rejected_at_load() {
        let type_sec = vec![1, 0x60, 0, 0];
        let mut import_sec = vec![1];
        encode_str(&mut import_sec, "wasi_snapshot_preview1");
        encode_str(&mut import_sec, "path_open");
        import_sec.extend_from_slice(&[0x00, 0x00]);

        let wasm = build_module(vec![(1, type_sec), (2, import_sec)]);
        let err = parse_and_validate(&wasm, 16).expect_err("path_open must be rejected");
        assert!(matches!(
            err,
            WasmError::ForbiddenImport { module, name }
            if module == "wasi_snapshot_preview1" && name == "path_open"
        ));
    }

    #[test]
    fn memory_grow_beyond_cap_is_trapped() {
        let type_sec = vec![1, 0x60, 0, 1, 0x7F];
        let func_sec = vec![1, 0];
        let mem_sec = vec![1, 0x00, 1];
        let mut exp_sec = vec![1];
        encode_str(&mut exp_sec, "oom");
        exp_sec.extend_from_slice(&[0x00, 0x00]);
        let body = vec![0, 0x41, 10, 0x40, 0x00, 0x0B]; // memory.grow(10)
        let mut code_sec = vec![1];
        encode_u32_leb(&mut code_sec, body.len() as u32);
        code_sec.extend_from_slice(&body);

        let wasm = build_module(vec![
            (1, type_sec),
            (3, func_sec),
            (5, mem_sec),
            (7, exp_sec),
            (10, code_sec),
        ]);

        let err = execute_wasm(
            &wasm,
            "oom",
            &[],
            b"",
            WasmLimits {
                max_memory_pages: 4,
                ..WasmLimits::default()
            },
        )
        .expect_err("memory.grow over cap must trap");
        assert!(matches!(
            err,
            WasmError::MemoryLimitExceeded {
                requested_pages: 11,
                max_pages: 4
            }
        ));
    }
}
