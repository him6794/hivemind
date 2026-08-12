use crate::supervisor::Cancellation;
use num_bigint::BigUint;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Inc {
        register: usize,
        next: usize,
    },
    DecJump {
        register: usize,
        if_nonzero: usize,
        if_zero: usize,
    },
    Halt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramError {
    Empty,
    InvalidTarget { instruction: usize, target: usize },
}

impl fmt::Display for ProgramError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("reference program must contain an instruction"),
            Self::InvalidTarget { instruction, target } => write!(
                formatter,
                "instruction {instruction} targets missing instruction {target}"
            ),
        }
    }
}

impl std::error::Error for ProgramError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinskyProgram {
    instructions: Vec<Instruction>,
    register_count: usize,
}

impl MinskyProgram {
    pub fn new(instructions: Vec<Instruction>) -> Result<Self, ProgramError> {
        if instructions.is_empty() {
            return Err(ProgramError::Empty);
        }

        let instruction_count = instructions.len();
        for (index, instruction) in instructions.iter().enumerate() {
            let targets = match instruction {
                Instruction::Inc { next, .. } => [*next, 0, 0],
                Instruction::DecJump {
                    if_nonzero, if_zero, ..
                } => [*if_nonzero, *if_zero, 0],
                Instruction::Halt => [0, 0, 0],
            };
            let target_count = match instruction {
                Instruction::Inc { .. } => 1,
                Instruction::DecJump { .. } => 2,
                Instruction::Halt => 0,
            };
            for target in targets.into_iter().take(target_count) {
                if target >= instruction_count {
                    return Err(ProgramError::InvalidTarget {
                        instruction: index,
                        target,
                    });
                }
            }
        }

        let register_count = instructions
            .iter()
            .map(|instruction| match instruction {
                Instruction::Inc { register, .. } | Instruction::DecJump { register, .. } => register.saturating_add(1),
                Instruction::Halt => 0,
            })
            .max()
            .unwrap_or(0);

        Ok(Self {
            instructions,
            register_count,
        })
    }

    pub fn register_count(&self) -> usize {
        self.register_count
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterpreterLimits {
    pub max_steps: u64,
}

impl InterpreterLimits {
    pub fn new(max_steps: u64) -> Self {
        Self { max_steps }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpreterStatus {
    Halted,
    ResourceExhausted,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpreterResult {
    pub status: InterpreterStatus,
    pub steps: u64,
    pub program_counter: usize,
    pub registers: Vec<BigUint>,
}

#[derive(Debug, Clone)]
pub struct ReferenceInterpreter {
    program: MinskyProgram,
}

impl ReferenceInterpreter {
    pub fn new(program: MinskyProgram) -> Self {
        Self { program }
    }

    pub fn run(&self, limits: InterpreterLimits, cancellation: &Cancellation) -> InterpreterResult {
        let mut registers = vec![BigUint::from(0u8); self.program.register_count];
        let mut program_counter = 0usize;
        let mut steps = 0u64;

        loop {
            if cancellation.is_cancelled() {
                return InterpreterResult {
                    status: InterpreterStatus::Cancelled,
                    steps,
                    program_counter,
                    registers,
                };
            }
            if steps >= limits.max_steps {
                return InterpreterResult {
                    status: InterpreterStatus::ResourceExhausted,
                    steps,
                    program_counter,
                    registers,
                };
            }

            let instruction = &self.program.instructions[program_counter];
            steps += 1;
            match instruction {
                Instruction::Inc { register, next } => {
                    registers[*register] += 1u8;
                    program_counter = *next;
                }
                Instruction::DecJump {
                    register,
                    if_nonzero,
                    if_zero,
                } => {
                    if registers[*register] == BigUint::from(0u8) {
                        program_counter = *if_zero;
                    } else {
                        registers[*register] -= 1u8;
                        program_counter = *if_nonzero;
                    }
                }
                Instruction::Halt => {
                    return InterpreterResult {
                        status: InterpreterStatus::Halted,
                        steps,
                        program_counter,
                        registers,
                    };
                }
            }
        }
    }
}
