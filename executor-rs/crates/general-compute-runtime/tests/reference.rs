use general_compute_runtime::reference::{
    Instruction, InterpreterLimits, InterpreterStatus, MinskyProgram, ReferenceInterpreter,
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
