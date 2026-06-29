//! Metered managed function runtime.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    String(String),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionLimits {
    pub max_ops: u64,
    pub max_call_depth: usize,
    pub max_output_bytes: usize,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_ops: 1_000_000,
            max_call_depth: 64,
            max_output_bytes: 1_048_576,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionReceipt {
    pub executed_ops: u64,
    pub function_calls: u64,
    pub loop_iterations: u64,
    pub max_call_depth: usize,
    pub output_bytes: usize,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResult {
    pub status: Status,
    pub value: Value,
    pub output: String,
    pub receipt: ExecutionReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    code: &'static str,
    message: String,
}

impl RuntimeError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl Display for RuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl Error for RuntimeError {}

#[derive(Debug, Default)]
pub struct ManagedExecutor;

impl ManagedExecutor {
    pub fn execute(&self, source: &str, limits: ExecutionLimits) -> Result<ExecutionResult, RuntimeError> {
        let tokens = Lexer::new(source).tokenize()?;
        let program = Parser::new(tokens).parse_program()?;
        Evaluator::new(limits).eval_program(&program)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Ident(String),
    Int(i64),
    String(String),
    True,
    False,
    Let,
    Fn,
    Return,
    If,
    Else,
    Plus,
    Minus,
    Star,
    Slash,
    Eq,
    EqEq,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semi,
    Eof,
}

struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            pos: 0,
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>, RuntimeError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            let Some(byte) = self.peek() else {
                tokens.push(Token::Eof);
                return Ok(tokens);
            };
            let token = match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.identifier(),
                b'0'..=b'9' => self.integer()?,
                b'"' => self.string()?,
                b'+' => {
                    self.pos += 1;
                    Token::Plus
                }
                b'-' => {
                    self.pos += 1;
                    Token::Minus
                }
                b'*' => {
                    self.pos += 1;
                    Token::Star
                }
                b'/' => {
                    self.pos += 1;
                    Token::Slash
                }
                b'=' => {
                    self.pos += 1;
                    if self.consume_byte(b'=') {
                        Token::EqEq
                    } else {
                        Token::Eq
                    }
                }
                b'!' => {
                    self.pos += 1;
                    if self.consume_byte(b'=') {
                        Token::BangEq
                    } else {
                        return Err(RuntimeError::new("parse_error", "expected != after !"));
                    }
                }
                b'<' => {
                    self.pos += 1;
                    if self.consume_byte(b'=') {
                        Token::LtEq
                    } else {
                        Token::Lt
                    }
                }
                b'>' => {
                    self.pos += 1;
                    if self.consume_byte(b'=') {
                        Token::GtEq
                    } else {
                        Token::Gt
                    }
                }
                b'(' => {
                    self.pos += 1;
                    Token::LParen
                }
                b')' => {
                    self.pos += 1;
                    Token::RParen
                }
                b'{' => {
                    self.pos += 1;
                    Token::LBrace
                }
                b'}' => {
                    self.pos += 1;
                    Token::RBrace
                }
                b',' => {
                    self.pos += 1;
                    Token::Comma
                }
                b';' => {
                    self.pos += 1;
                    Token::Semi
                }
                _ => {
                    return Err(RuntimeError::new(
                        "parse_error",
                        format!("unexpected character '{}'", char::from(byte)),
                    ));
                }
            };
            tokens.push(token);
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn consume_byte(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn identifier(&mut self) -> Token {
        let start = self.pos;
        while matches!(self.peek(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')) {
            self.pos += 1;
        }
        let text = String::from_utf8_lossy(&self.bytes[start..self.pos]);
        match text.as_ref() {
            "let" => Token::Let,
            "fn" => Token::Fn,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "true" => Token::True,
            "false" => Token::False,
            _ => Token::Ident(text.into_owned()),
        }
    }

    fn integer(&mut self) -> Result<Token, RuntimeError> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| RuntimeError::new("parse_error", "invalid integer"))?;
        let value = text
            .parse::<i64>()
            .map_err(|_| RuntimeError::new("parse_error", "integer is out of range"))?;
        Ok(Token::Int(value))
    }

    fn string(&mut self) -> Result<Token, RuntimeError> {
        self.pos += 1;
        let mut value = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(RuntimeError::new("parse_error", "unterminated string"));
            };
            self.pos += 1;
            match byte {
                b'"' => return Ok(Token::String(value)),
                b'\\' => {
                    let Some(escaped) = self.peek() else {
                        return Err(RuntimeError::new("parse_error", "unterminated string escape"));
                    };
                    self.pos += 1;
                    let ch = match escaped {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        _ => {
                            return Err(RuntimeError::new("parse_error", "unsupported string escape"));
                        }
                    };
                    value.push(ch);
                }
                _ => value.push(char::from(byte)),
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Program {
    statements: Vec<Stmt>,
}

#[derive(Debug, Clone)]
enum Stmt {
    Let(String, Expr),
    Fn(Function),
    Return(Expr),
    Print(Expr),
    Expr(Expr),
}

#[derive(Debug, Clone)]
struct Function {
    name: String,
    params: Vec<String>,
    body: Expr,
}

#[derive(Debug, Clone)]
enum Expr {
    Value(Value),
    Variable(String),
    If {
        condition: Box<Self>,
        then_expr: Box<Self>,
        else_expr: Box<Self>,
    },
    Call {
        name: String,
        args: Vec<Self>,
    },
    Binary {
        left: Box<Self>,
        op: BinaryOp,
        right: Box<Self>,
    },
}

#[derive(Debug, Clone, Copy)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn parse_program(mut self) -> Result<Program, RuntimeError> {
        let mut statements = Vec::new();
        while !matches!(self.peek(), Token::Eof) {
            statements.push(self.statement()?);
        }
        Ok(Program { statements })
    }

    fn statement(&mut self) -> Result<Stmt, RuntimeError> {
        match self.peek() {
            Token::Let => {
                self.advance();
                let name = self.ident()?;
                self.expect(&Token::Eq)?;
                let expr = self.expression()?;
                self.expect(&Token::Semi)?;
                Ok(Stmt::Let(name, expr))
            }
            Token::Fn => self.function().map(Stmt::Fn),
            Token::Return => {
                self.advance();
                let expr = self.expression()?;
                self.expect(&Token::Semi)?;
                Ok(Stmt::Return(expr))
            }
            Token::Ident(name) if name == "print" => {
                self.advance();
                self.expect(&Token::LParen)?;
                let expr = self.expression()?;
                self.expect(&Token::RParen)?;
                self.expect(&Token::Semi)?;
                Ok(Stmt::Print(expr))
            }
            _ => {
                let expr = self.expression()?;
                self.expect(&Token::Semi)?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn function(&mut self) -> Result<Function, RuntimeError> {
        self.expect(&Token::Fn)?;
        let name = self.ident()?;
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                params.push(self.ident()?);
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RParen)?;
        self.expect(&Token::LBrace)?;
        self.expect(&Token::Return)?;
        let body = self.expression()?;
        self.expect(&Token::Semi)?;
        self.expect(&Token::RBrace)?;
        Ok(Function { name, params, body })
    }

    fn expression(&mut self) -> Result<Expr, RuntimeError> {
        self.equality()
    }

    fn equality(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.comparison()?;
        loop {
            let op = match self.peek() {
                Token::EqEq => BinaryOp::Eq,
                Token::BangEq => BinaryOp::NotEq,
                _ => break,
            };
            self.advance();
            let right = self.comparison()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn comparison(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.term()?;
        loop {
            let op = match self.peek() {
                Token::Lt => BinaryOp::Lt,
                Token::LtEq => BinaryOp::LtEq,
                Token::Gt => BinaryOp::Gt,
                Token::GtEq => BinaryOp::GtEq,
                _ => break,
            };
            self.advance();
            let right = self.term()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn term(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.factor()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.factor()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn factor(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.primary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.primary()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr, RuntimeError> {
        match self.peek().clone() {
            Token::Int(value) => {
                self.advance();
                Ok(Expr::Value(Value::Int(value)))
            }
            Token::String(value) => {
                self.advance();
                Ok(Expr::Value(Value::String(value)))
            }
            Token::True => {
                self.advance();
                Ok(Expr::Value(Value::Bool(true)))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Value(Value::Bool(false)))
            }
            Token::If => self.if_expr(),
            Token::Ident(name) => {
                self.advance();
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        loop {
                            args.push(self.expression()?);
                            if matches!(self.peek(), Token::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(Expr::Call { name, args })
                } else {
                    Ok(Expr::Variable(name))
                }
            }
            Token::LParen => {
                self.advance();
                let expr = self.expression()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            token => Err(RuntimeError::new(
                "parse_error",
                format!("expected expression, found {token:?}"),
            )),
        }
    }

    fn if_expr(&mut self) -> Result<Expr, RuntimeError> {
        self.expect(&Token::If)?;
        let condition = self.expression()?;
        self.expect(&Token::LBrace)?;
        let then_expr = self.expression()?;
        self.expect(&Token::RBrace)?;
        self.expect(&Token::Else)?;
        self.expect(&Token::LBrace)?;
        let else_expr = self.expression()?;
        self.expect(&Token::RBrace)?;
        Ok(Expr::If {
            condition: Box::new(condition),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
        })
    }

    fn ident(&mut self) -> Result<String, RuntimeError> {
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                Ok(name)
            }
            token => Err(RuntimeError::new(
                "parse_error",
                format!("expected identifier, found {token:?}"),
            )),
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<(), RuntimeError> {
        let actual = self.peek().clone();
        if std::mem::discriminant(&actual) == std::mem::discriminant(expected) {
            self.advance();
            Ok(())
        } else {
            Err(RuntimeError::new(
                "parse_error",
                format!("expected {expected:?}, found {actual:?}"),
            ))
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) {
        self.pos += 1;
    }
}

struct Evaluator {
    limits: ExecutionLimits,
    receipt: ExecutionReceipt,
    output: String,
    functions: HashMap<String, Function>,
    scopes: Vec<HashMap<String, Value>>,
    call_depth: usize,
}

impl Evaluator {
    fn new(limits: ExecutionLimits) -> Self {
        Self {
            limits,
            receipt: ExecutionReceipt::default(),
            output: String::new(),
            functions: HashMap::new(),
            scopes: vec![HashMap::new()],
            call_depth: 0,
        }
    }

    fn eval_program(mut self, program: &Program) -> Result<ExecutionResult, RuntimeError> {
        for statement in &program.statements {
            if let Stmt::Fn(function) = statement {
                self.functions.insert(function.name.clone(), function.clone());
            }
        }

        let mut last = Value::Null;
        for statement in &program.statements {
            if matches!(statement, Stmt::Fn(_)) {
                continue;
            }
            match self.eval_stmt(statement)? {
                Control::Continue(value) => last = value,
                Control::Return(value) => {
                    last = value;
                    break;
                }
            }
        }

        Ok(ExecutionResult {
            status: Status::Completed,
            value: last,
            output: self.output,
            receipt: self.receipt,
        })
    }

    fn eval_stmt(&mut self, statement: &Stmt) -> Result<Control, RuntimeError> {
        self.charge(1)?;
        match statement {
            Stmt::Let(name, expr) => {
                let value = self.eval_expr(expr)?;
                self.current_scope().insert(name.clone(), value);
                Ok(Control::Continue(Value::Null))
            }
            Stmt::Fn(_) => Ok(Control::Continue(Value::Null)),
            Stmt::Return(expr) => {
                let value = self.eval_expr(expr)?;
                Ok(Control::Return(value))
            }
            Stmt::Print(expr) => {
                self.charge(5)?;
                let value = self.eval_expr(expr)?;
                let text = format!("{}\n", display_value(&value));
                let next_len = self.output.len() + text.len();
                if next_len > self.limits.max_output_bytes {
                    return Err(RuntimeError::new("output_limit_exceeded", "output limit exceeded"));
                }
                self.output.push_str(&text);
                self.receipt.output_bytes = self.output.len();
                Ok(Control::Continue(Value::Null))
            }
            Stmt::Expr(expr) => self.eval_expr(expr).map(Control::Continue),
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        self.charge(1)?;
        match expr {
            Expr::Value(value) => Ok(value.clone()),
            Expr::Variable(name) => self.lookup(name),
            Expr::If {
                condition,
                then_expr,
                else_expr,
            } => {
                let condition = self.eval_expr(condition)?;
                match condition {
                    Value::Bool(true) => self.eval_expr(then_expr),
                    Value::Bool(false) => self.eval_expr(else_expr),
                    _ => Err(RuntimeError::new("type_error", "if condition must be bool")),
                }
            }
            Expr::Call { name, args } => self.call(name, args),
            Expr::Binary { left, op, right } => {
                let left = self.eval_expr(left)?;
                let right = self.eval_expr(right)?;
                eval_binary(left, *op, right)
            }
        }
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Result<Value, RuntimeError> {
        self.charge(5)?;
        let function = self
            .functions
            .get(name)
            .cloned()
            .ok_or_else(|| RuntimeError::new("name_error", format!("unknown function '{name}'")))?;
        if function.params.len() != args.len() {
            return Err(RuntimeError::new(
                "arity_error",
                format!(
                    "function '{}' expected {} arguments, got {}",
                    name,
                    function.params.len(),
                    args.len()
                ),
            ));
        }
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval_expr(arg)?);
        }
        if self.call_depth + 1 > self.limits.max_call_depth {
            return Err(RuntimeError::new("call_depth_exceeded", "call depth exceeded"));
        }
        self.receipt.function_calls += 1;
        self.call_depth += 1;
        self.receipt.max_call_depth = self.receipt.max_call_depth.max(self.call_depth);
        let mut scope = HashMap::new();
        for (param, value) in function.params.iter().zip(values) {
            scope.insert(param.clone(), value);
        }
        self.scopes.push(scope);
        let result = self.eval_expr(&function.body);
        self.scopes.pop();
        self.call_depth -= 1;
        result
    }

    fn lookup(&self, name: &str) -> Result<Value, RuntimeError> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Ok(value.clone());
            }
        }
        Err(RuntimeError::new("name_error", format!("unknown variable '{name}'")))
    }

    fn current_scope(&mut self) -> &mut HashMap<String, Value> {
        self.scopes.last_mut().expect("evaluator always has a current scope")
    }

    fn charge(&mut self, cost: u64) -> Result<(), RuntimeError> {
        let next = self.receipt.executed_ops.saturating_add(cost);
        if next > self.limits.max_ops {
            return Err(RuntimeError::new("op_limit_exceeded", "operation limit exceeded"));
        }
        self.receipt.executed_ops = next;
        Ok(())
    }
}

enum Control {
    Continue(Value),
    Return(Value),
}

fn eval_binary(left: Value, op: BinaryOp, right: Value) -> Result<Value, RuntimeError> {
    match op {
        BinaryOp::Add => match (left, right) {
            (Value::Int(left), Value::Int(right)) => Ok(Value::Int(left + right)),
            (Value::String(left), Value::String(right)) => Ok(Value::String(format!("{left}{right}"))),
            _ => Err(RuntimeError::new("type_error", "+ expects matching ints or strings")),
        },
        BinaryOp::Sub => int_binary(left, right, "-", |left, right| left - right),
        BinaryOp::Mul => int_binary(left, right, "*", |left, right| left * right),
        BinaryOp::Div => match (left, right) {
            (Value::Int(_), Value::Int(0)) => Err(RuntimeError::new("runtime_error", "division by zero")),
            (Value::Int(left), Value::Int(right)) => Ok(Value::Int(left / right)),
            _ => Err(RuntimeError::new("type_error", "/ expects ints")),
        },
        BinaryOp::Eq => Ok(Value::Bool(left == right)),
        BinaryOp::NotEq => Ok(Value::Bool(left != right)),
        BinaryOp::Lt => int_compare(left, right, "<", |left, right| left < right),
        BinaryOp::LtEq => int_compare(left, right, "<=", |left, right| left <= right),
        BinaryOp::Gt => int_compare(left, right, ">", |left, right| left > right),
        BinaryOp::GtEq => int_compare(left, right, ">=", |left, right| left >= right),
    }
}

fn int_binary(
    left: Value,
    right: Value,
    op: &'static str,
    apply: impl FnOnce(i64, i64) -> i64,
) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => Ok(Value::Int(apply(left, right))),
        _ => Err(RuntimeError::new("type_error", format!("{op} expects ints"))),
    }
}

fn int_compare(
    left: Value,
    right: Value,
    op: &'static str,
    apply: impl FnOnce(i64, i64) -> bool,
) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => Ok(Value::Bool(apply(left, right))),
        _ => Err(RuntimeError::new("type_error", format!("{op} expects ints"))),
    }
}

fn display_value(value: &Value) -> String {
    match value {
        Value::Int(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Null => "null".to_string(),
    }
}
