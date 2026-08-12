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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapInstruction {
    Set {
        register: usize,
        value: u64,
        next: usize,
    },
    Allocate {
        cells: usize,
        destination: usize,
        next: usize,
    },
    Store {
        pointer: usize,
        offset: usize,
        source: usize,
        next: usize,
    },
    Load {
        pointer: usize,
        offset: usize,
        destination: usize,
        next: usize,
    },
    Halt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapProgramError {
    Empty,
    InvalidTarget { instruction: usize, target: usize },
    RegisterOutOfRange { instruction: usize, register: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeapProgram {
    instructions: Vec<HeapInstruction>,
    register_count: usize,
}

impl HeapProgram {
    pub fn new(instructions: Vec<HeapInstruction>, register_count: usize) -> Result<Self, HeapProgramError> {
        if instructions.is_empty() {
            return Err(HeapProgramError::Empty);
        }
        let instruction_count = instructions.len();
        for (index, instruction) in instructions.iter().enumerate() {
            let (registers, targets): (Vec<usize>, Vec<usize>) = match instruction {
                HeapInstruction::Set { register, next, .. } => (vec![*register], vec![*next]),
                HeapInstruction::Allocate { destination, next, .. } => (vec![*destination], vec![*next]),
                HeapInstruction::Store {
                    pointer, source, next, ..
                } => (vec![*pointer, *source], vec![*next]),
                HeapInstruction::Load {
                    pointer,
                    destination,
                    next,
                    ..
                } => (vec![*pointer, *destination], vec![*next]),
                HeapInstruction::Halt => (Vec::new(), Vec::new()),
            };
            if let Some(register) = registers.into_iter().find(|register| *register >= register_count) {
                return Err(HeapProgramError::RegisterOutOfRange {
                    instruction: index,
                    register,
                });
            }
            if let Some(target) = targets.into_iter().find(|target| *target >= instruction_count) {
                return Err(HeapProgramError::InvalidTarget {
                    instruction: index,
                    target,
                });
            }
        }
        Ok(Self {
            instructions,
            register_count,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapLimits {
    pub max_steps: u64,
    pub max_cells: usize,
}

impl HeapLimits {
    pub fn new(max_steps: u64, max_cells: usize) -> Self {
        Self { max_steps, max_cells }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeapValue(BigUint);

impl HeapValue {
    pub fn integer(value: u64) -> Self {
        Self(BigUint::from(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapStatus {
    Halted,
    ResourceExhausted,
    Cancelled,
    InvalidMemoryAccess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeapResult {
    pub status: HeapStatus,
    pub steps: u64,
    pub registers: Vec<HeapValue>,
    pub heap_cells: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HeapInterpreter {
    program: HeapProgram,
}

impl HeapInterpreter {
    pub fn new(program: HeapProgram) -> Self {
        Self { program }
    }

    pub fn run(&self, limits: HeapLimits, cancellation: &Cancellation) -> HeapResult {
        let mut registers = vec![HeapValue::integer(0); self.program.register_count];
        let mut heap = Vec::<HeapValue>::new();
        let mut pointers = vec![None; self.program.register_count];
        let mut pc = 0usize;
        let mut steps = 0u64;

        loop {
            if cancellation.is_cancelled() {
                return HeapResult {
                    status: HeapStatus::Cancelled,
                    steps,
                    registers,
                    heap_cells: heap.len(),
                    error: None,
                };
            }
            if steps >= limits.max_steps {
                return HeapResult {
                    status: HeapStatus::ResourceExhausted,
                    steps,
                    registers,
                    heap_cells: heap.len(),
                    error: None,
                };
            }
            steps += 1;
            match &self.program.instructions[pc] {
                HeapInstruction::Set { register, value, next } => {
                    registers[*register] = HeapValue::integer(*value);
                    pc = *next;
                }
                HeapInstruction::Allocate {
                    cells,
                    destination,
                    next,
                } => {
                    let Some(new_len) = heap.len().checked_add(*cells) else {
                        return memory_error(steps, registers, heap.len(), "heap size overflow");
                    };
                    if new_len > limits.max_cells {
                        return HeapResult {
                            status: HeapStatus::ResourceExhausted,
                            steps,
                            registers,
                            heap_cells: heap.len(),
                            error: None,
                        };
                    }
                    let pointer = heap.len();
                    heap.resize(new_len, HeapValue::integer(0));
                    pointers[*destination] = Some(pointer);
                    pc = *next;
                }
                HeapInstruction::Store {
                    pointer,
                    offset,
                    source,
                    next,
                } => {
                    let Some(base) = pointers[*pointer] else {
                        return memory_error(steps, registers, heap.len(), "null heap pointer");
                    };
                    let Some(index) = base.checked_add(*offset) else {
                        return memory_error(steps, registers, heap.len(), "heap offset overflow");
                    };
                    let Some(slot) = heap.get_mut(index) else {
                        return memory_error(steps, registers, heap.len(), "heap index out of bounds");
                    };
                    *slot = registers[*source].clone();
                    pc = *next;
                }
                HeapInstruction::Load {
                    pointer,
                    offset,
                    destination,
                    next,
                } => {
                    let Some(base) = pointers[*pointer] else {
                        return memory_error(steps, registers, heap.len(), "null heap pointer");
                    };
                    let Some(index) = base.checked_add(*offset) else {
                        return memory_error(steps, registers, heap.len(), "heap offset overflow");
                    };
                    let Some(value) = heap.get(index) else {
                        return memory_error(steps, registers, heap.len(), "heap index out of bounds");
                    };
                    registers[*destination] = value.clone();
                    pc = *next;
                }
                HeapInstruction::Halt => {
                    return HeapResult {
                        status: HeapStatus::Halted,
                        steps,
                        registers,
                        heap_cells: heap.len(),
                        error: None,
                    };
                }
            }
        }
    }
}

fn memory_error(steps: u64, registers: Vec<HeapValue>, heap_cells: usize, message: &str) -> HeapResult {
    HeapResult {
        status: HeapStatus::InvalidMemoryAccess,
        steps,
        registers,
        heap_cells,
        error: Some(message.to_owned()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecursionInstruction {
    Set {
        register: usize,
        value: u64,
        next: usize,
    },
    DecJump {
        register: usize,
        if_nonzero: usize,
        if_zero: usize,
    },
    Call {
        target: usize,
        return_pc: usize,
    },
    Return,
    Halt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecursionProgramError {
    Empty,
    InvalidTarget { instruction: usize, target: usize },
    RegisterOutOfRange { instruction: usize, register: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecursionProgram {
    instructions: Vec<RecursionInstruction>,
    register_count: usize,
}

impl RecursionProgram {
    pub fn new(instructions: Vec<RecursionInstruction>, register_count: usize) -> Result<Self, RecursionProgramError> {
        if instructions.is_empty() {
            return Err(RecursionProgramError::Empty);
        }
        let instruction_count = instructions.len();
        for (index, instruction) in instructions.iter().enumerate() {
            let (registers, targets): (Vec<usize>, Vec<usize>) = match instruction {
                RecursionInstruction::Set { register, next, .. } => (vec![*register], vec![*next]),
                RecursionInstruction::DecJump {
                    register,
                    if_nonzero,
                    if_zero,
                } => (vec![*register], vec![*if_nonzero, *if_zero]),
                RecursionInstruction::Call { target, return_pc } => (Vec::new(), vec![*target, *return_pc]),
                RecursionInstruction::Return | RecursionInstruction::Halt => (Vec::new(), Vec::new()),
            };
            if let Some(register) = registers.into_iter().find(|register| *register >= register_count) {
                return Err(RecursionProgramError::RegisterOutOfRange {
                    instruction: index,
                    register,
                });
            }
            if let Some(target) = targets.into_iter().find(|target| *target >= instruction_count) {
                return Err(RecursionProgramError::InvalidTarget {
                    instruction: index,
                    target,
                });
            }
        }
        Ok(Self {
            instructions,
            register_count,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecursionLimits {
    pub max_steps: u64,
    pub max_depth: usize,
}

impl RecursionLimits {
    pub fn new(max_steps: u64, max_depth: usize) -> Self {
        Self { max_steps, max_depth }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecursionStatus {
    Halted,
    ResourceExhausted,
    Cancelled,
    StackUnderflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecursionResult {
    pub status: RecursionStatus,
    pub steps: u64,
    pub registers: Vec<HeapValue>,
    pub stack_depth: usize,
    pub max_depth: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RecursionInterpreter {
    program: RecursionProgram,
}

impl RecursionInterpreter {
    pub fn new(program: RecursionProgram) -> Self {
        Self { program }
    }

    pub fn run(&self, limits: RecursionLimits, cancellation: &Cancellation) -> RecursionResult {
        let mut registers = vec![HeapValue::integer(0); self.program.register_count];
        let mut stack = Vec::<usize>::new();
        let mut pc = 0usize;
        let mut steps = 0u64;
        let mut max_depth = 0usize;

        loop {
            if cancellation.is_cancelled() {
                return recursion_result(
                    RecursionStatus::Cancelled,
                    steps,
                    registers,
                    stack.len(),
                    max_depth,
                    None,
                );
            }
            if steps >= limits.max_steps || stack.len() > limits.max_depth {
                return recursion_result(
                    RecursionStatus::ResourceExhausted,
                    steps,
                    registers,
                    stack.len(),
                    max_depth,
                    None,
                );
            }
            steps += 1;
            match &self.program.instructions[pc] {
                RecursionInstruction::Set { register, value, next } => {
                    registers[*register] = HeapValue::integer(*value);
                    pc = *next;
                }
                RecursionInstruction::DecJump {
                    register,
                    if_nonzero,
                    if_zero,
                } => {
                    if registers[*register] == HeapValue::integer(0) {
                        pc = *if_zero;
                    } else {
                        registers[*register].0 -= 1u8;
                        pc = *if_nonzero;
                    }
                }
                RecursionInstruction::Call { target, return_pc } => {
                    if stack.len() >= limits.max_depth {
                        return recursion_result(
                            RecursionStatus::ResourceExhausted,
                            steps,
                            registers,
                            stack.len(),
                            max_depth,
                            None,
                        );
                    }
                    stack.push(*return_pc);
                    max_depth = max_depth.max(stack.len());
                    pc = *target;
                }
                RecursionInstruction::Return => {
                    let Some(return_pc) = stack.pop() else {
                        return recursion_result(
                            RecursionStatus::StackUnderflow,
                            steps,
                            registers,
                            stack.len(),
                            max_depth,
                            Some("return with empty call stack".into()),
                        );
                    };
                    pc = return_pc;
                }
                RecursionInstruction::Halt => {
                    return recursion_result(RecursionStatus::Halted, steps, registers, stack.len(), max_depth, None);
                }
            }
        }
    }
}

fn recursion_result(
    status: RecursionStatus,
    steps: u64,
    registers: Vec<HeapValue>,
    stack_depth: usize,
    max_depth: usize,
    error: Option<String>,
) -> RecursionResult {
    RecursionResult {
        status,
        steps,
        registers,
        stack_depth,
        max_depth,
        error,
    }
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
