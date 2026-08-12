use general_compute_runtime::reference::{
    HeapInstruction, HeapInterpreter, HeapLimits, HeapProgram, HeapStatus, HeapValue, Instruction, InterpreterLimits,
    InterpreterStatus, MinskyProgram, ReferenceInterpreter,
};
use general_compute_runtime::supervisor::Cancellation;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn minsky_machine_increments_and_decrements_to_zero() {
    let program = MinskyProgram::new(vec![
        Instruction::Inc { register: 0, next: 1 },
        Instruction::Inc { register: 0, next: 2 },
        Instruction::DecJump {
            register: 0,
            if_nonzero: 2,
            if_zero: 3,
        },
        Instruction::Halt,
    ])
    .expect("fixture program should validate");

    let result = ReferenceInterpreter::new(program).run(InterpreterLimits::new(32), &Cancellation::new());

    assert_eq!(result.status, InterpreterStatus::Halted);
    assert_eq!(result.steps, 6);
    assert_eq!(result.registers[0].to_string(), "0");
}

#[test]
fn minsky_machine_stops_nonterminating_program_at_step_budget() {
    let program =
        MinskyProgram::new(vec![Instruction::Inc { register: 0, next: 0 }]).expect("loop fixture should validate");

    let result = ReferenceInterpreter::new(program).run(InterpreterLimits::new(7), &Cancellation::new());

    assert_eq!(result.status, InterpreterStatus::ResourceExhausted);
    assert_eq!(result.steps, 7);
    assert_eq!(result.registers[0].to_string(), "7");
}

#[test]
fn reference_program_rejects_targets_outside_instruction_tape() {
    let error = MinskyProgram::new(vec![Instruction::Inc { register: 0, next: 1 }])
        .expect_err("out-of-range jump must be rejected");

    assert!(matches!(
        error,
        general_compute_runtime::reference::ProgramError::InvalidTarget {
            instruction: 0,
            target: 1
        }
    ));
}

#[test]
fn reference_interpreter_stops_cooperatively_on_cancellation() {
    let program =
        MinskyProgram::new(vec![Instruction::Inc { register: 0, next: 0 }]).expect("loop fixture should validate");
    let cancellation = Arc::new(Cancellation::new());
    let trigger = Arc::clone(&cancellation);
    let thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(1));
        trigger.cancel();
    });

    let result = ReferenceInterpreter::new(program).run(InterpreterLimits::new(u64::MAX), &cancellation);
    thread.join().expect("cancellation trigger should finish");

    assert_eq!(result.status, InterpreterStatus::Cancelled);
    assert!(result.steps > 0, "cancellation should observe executed work");
}

#[test]
fn heap_fixture_mutates_and_reads_a_bigint_backed_cell() {
    let program = HeapProgram::new(
        vec![
            HeapInstruction::Set {
                register: 0,
                value: 41,
                next: 1,
            },
            HeapInstruction::Allocate {
                cells: 1,
                destination: 1,
                next: 2,
            },
            HeapInstruction::Store {
                pointer: 1,
                offset: 0,
                source: 0,
                next: 3,
            },
            HeapInstruction::Load {
                pointer: 1,
                offset: 0,
                destination: 2,
                next: 4,
            },
            HeapInstruction::Halt,
        ],
        3,
    )
    .expect("heap fixture should validate");

    let result = HeapInterpreter::new(program).run(HeapLimits::new(16, 4), &Cancellation::new());

    assert_eq!(result.status, HeapStatus::Halted);
    assert_eq!(result.heap_cells, 1);
    assert_eq!(result.registers[2], HeapValue::integer(41));
}

#[test]
fn heap_fixture_returns_resource_exhausted_before_exceeding_cell_quota() {
    let program = HeapProgram::new(
        vec![
            HeapInstruction::Allocate {
                cells: 2,
                destination: 0,
                next: 1,
            },
            HeapInstruction::Halt,
        ],
        1,
    )
    .expect("heap fixture should validate");

    let result = HeapInterpreter::new(program).run(HeapLimits::new(8, 1), &Cancellation::new());

    assert_eq!(result.status, HeapStatus::ResourceExhausted);
    assert_eq!(result.heap_cells, 0);
    assert!(result.error.is_none(), "quota exhaustion is not a memory fault");
}
