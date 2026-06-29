//! Metered managed function runtime.

use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    String(String),
    List(Vec<Self>),
    Dict(BTreeMap<String, Self>),
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
    pub max_loop_iterations: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_ops: 1_000_000,
            max_call_depth: 64,
            max_output_bytes: 1_048_576,
            max_loop_iterations: 100_000,
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
    line: Option<usize>,
    column: Option<usize>,
}

impl RuntimeError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    #[must_use]
    pub fn line(&self) -> Option<usize> {
        self.line
    }

    #[must_use]
    pub fn column(&self) -> Option<usize> {
        self.column
    }

    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            line: None,
            column: None,
        }
    }

    fn at(mut self, span: Span) -> Self {
        self.line = Some(span.line);
        self.column = Some(span.column);
        self
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

    pub fn execute_json_input(
        &self,
        source: &str,
        limits: ExecutionLimits,
        input_json: &str,
    ) -> Result<ExecutionResult, RuntimeError> {
        let input = Value::from_json_str(input_json)?;
        let tokens = Lexer::new(source).tokenize()?;
        let program = Parser::new(tokens).parse_program()?;
        let mut evaluator = Evaluator::new(limits);
        evaluator.current_scope().insert("input".to_string(), input);
        evaluator.eval_program(&program)
    }
}

impl Value {
    pub fn from_json_str(input: &str) -> Result<Self, RuntimeError> {
        let value = serde_json::from_str::<serde_json::Value>(input)
            .map_err(|e| RuntimeError::new("input_error", format!("invalid JSON input: {e}")))?;
        Self::from_json_value(&value)
    }

    fn from_json_value(value: &serde_json::Value) -> Result<Self, RuntimeError> {
        match value {
            serde_json::Value::Null => Ok(Self::Null),
            serde_json::Value::Bool(value) => Ok(Self::Bool(*value)),
            serde_json::Value::Number(value) => value
                .as_i64()
                .map(Self::Int)
                .ok_or_else(|| RuntimeError::new("input_error", "only signed 64-bit JSON integers are supported")),
            serde_json::Value::String(value) => Ok(Self::String(value.clone())),
            serde_json::Value::Array(values) => values
                .iter()
                .map(Self::from_json_value)
                .collect::<Result<Vec<_>, _>>()
                .map(Self::List),
            serde_json::Value::Object(values) => values
                .iter()
                .map(|(key, value)| Ok((key.clone(), Self::from_json_value(value)?)))
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map(Self::Dict),
        }
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
    For,
    In,
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
    LBracket,
    RBracket,
    Colon,
    Comma,
    Semi,
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpannedToken {
    token: Token,
    span: Span,
}

struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    fn tokenize(mut self) -> Result<Vec<SpannedToken>, RuntimeError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            let span = self.span();
            let Some(byte) = self.peek() else {
                tokens.push(SpannedToken {
                    token: Token::Eof,
                    span,
                });
                return Ok(tokens);
            };
            let token = match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.identifier(),
                b'0'..=b'9' => self.integer()?,
                b'"' => self.string()?,
                b'+' => {
                    self.bump();
                    Token::Plus
                }
                b'-' => {
                    self.bump();
                    Token::Minus
                }
                b'*' => {
                    self.bump();
                    Token::Star
                }
                b'/' => {
                    self.bump();
                    Token::Slash
                }
                b'=' => {
                    self.bump();
                    if self.consume_byte(b'=') {
                        Token::EqEq
                    } else {
                        Token::Eq
                    }
                }
                b'!' => {
                    self.bump();
                    if self.consume_byte(b'=') {
                        Token::BangEq
                    } else {
                        return Err(RuntimeError::new("parse_error", "expected != after !"));
                    }
                }
                b'<' => {
                    self.bump();
                    if self.consume_byte(b'=') {
                        Token::LtEq
                    } else {
                        Token::Lt
                    }
                }
                b'>' => {
                    self.bump();
                    if self.consume_byte(b'=') {
                        Token::GtEq
                    } else {
                        Token::Gt
                    }
                }
                b'(' => {
                    self.bump();
                    Token::LParen
                }
                b')' => {
                    self.bump();
                    Token::RParen
                }
                b'{' => {
                    self.bump();
                    Token::LBrace
                }
                b'}' => {
                    self.bump();
                    Token::RBrace
                }
                b'[' => {
                    self.bump();
                    Token::LBracket
                }
                b']' => {
                    self.bump();
                    Token::RBracket
                }
                b':' => {
                    self.bump();
                    Token::Colon
                }
                b',' => {
                    self.bump();
                    Token::Comma
                }
                b';' => {
                    self.bump();
                    Token::Semi
                }
                _ => {
                    return Err(RuntimeError::new(
                        "parse_error",
                        format!("unexpected character '{}'", char::from(byte)),
                    ));
                }
            };
            tokens.push(SpannedToken { token, span });
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.bump();
        }
    }

    fn span(&self) -> Span {
        Span {
            line: self.line,
            column: self.column,
        }
    }

    fn bump(&mut self) {
        let byte = self.bytes[self.pos];
        self.pos += 1;
        if byte == b'\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn consume_byte(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn identifier(&mut self) -> Token {
        let start = self.pos;
        while matches!(self.peek(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')) {
            self.bump();
        }
        let text = String::from_utf8_lossy(&self.bytes[start..self.pos]);
        match text.as_ref() {
            "let" => Token::Let,
            "fn" => Token::Fn,
            "return" => Token::Return,
            "for" => Token::For,
            "in" => Token::In,
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
            self.bump();
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| RuntimeError::new("parse_error", "invalid integer"))?;
        let value = text
            .parse::<i64>()
            .map_err(|_| RuntimeError::new("parse_error", "integer is out of range"))?;
        Ok(Token::Int(value))
    }

    fn string(&mut self) -> Result<Token, RuntimeError> {
        self.bump();
        let mut value = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(RuntimeError::new("parse_error", "unterminated string"));
            };
            self.bump();
            match byte {
                b'"' => return Ok(Token::String(value)),
                b'\\' => {
                    let Some(escaped) = self.peek() else {
                        return Err(RuntimeError::new("parse_error", "unterminated string escape"));
                    };
                    self.bump();
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
    For {
        item: String,
        iterable: Expr,
        body: Vec<Self>,
    },
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
    List(Vec<Self>),
    Dict(Vec<(String, Self)>),
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
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<SpannedToken>) -> Self {
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
            Token::For => self.for_statement(),
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

    fn for_statement(&mut self) -> Result<Stmt, RuntimeError> {
        self.expect(&Token::For)?;
        let item = self.ident()?;
        self.expect(&Token::In)?;
        let iterable = self.expression()?;
        self.expect(&Token::LBrace)?;
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            body.push(self.statement()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(Stmt::For { item, iterable, body })
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
            Token::LBracket => self.list_expr(),
            Token::LBrace => self.dict_expr(),
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
            token => {
                Err(RuntimeError::new("parse_error", format!("expected expression, found {token:?}")).at(self.span()))
            }
        }
    }

    fn list_expr(&mut self) -> Result<Expr, RuntimeError> {
        self.expect(&Token::LBracket)?;
        let mut values = Vec::new();
        if !matches!(self.peek(), Token::RBracket) {
            loop {
                values.push(self.expression()?);
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RBracket)?;
        Ok(Expr::List(values))
    }

    fn dict_expr(&mut self) -> Result<Expr, RuntimeError> {
        self.expect(&Token::LBrace)?;
        let mut values = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            loop {
                let key = match self.peek().clone() {
                    Token::String(key) => {
                        self.advance();
                        key
                    }
                    token => {
                        return Err(RuntimeError::new(
                            "parse_error",
                            format!("dict keys must be strings, found {token:?}"),
                        )
                        .at(self.span()));
                    }
                };
                self.expect(&Token::Colon)?;
                values.push((key, self.expression()?));
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::Dict(values))
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
            token => {
                Err(RuntimeError::new("parse_error", format!("expected identifier, found {token:?}")).at(self.span()))
            }
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<(), RuntimeError> {
        let actual = self.peek().clone();
        if std::mem::discriminant(&actual) == std::mem::discriminant(expected) {
            self.advance();
            Ok(())
        } else {
            Err(RuntimeError::new("parse_error", format!("expected {expected:?}, found {actual:?}")).at(self.span()))
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn span(&self) -> Span {
        self.tokens[self.pos].span
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
            Stmt::For { item, iterable, body } => {
                let iterable = self.eval_expr(iterable)?;
                let Value::List(values) = iterable else {
                    return Err(RuntimeError::new("type_error", "for expects a list"));
                };
                let mut last = Value::Null;
                for value in values {
                    if self.receipt.loop_iterations + 1 > self.limits.max_loop_iterations {
                        return Err(RuntimeError::new(
                            "loop_limit_exceeded",
                            "loop iteration limit exceeded",
                        ));
                    }
                    self.receipt.loop_iterations += 1;
                    self.current_scope().insert(item.clone(), value);
                    for statement in body {
                        match self.eval_stmt(statement)? {
                            Control::Continue(value) => last = value,
                            Control::Return(value) => return Ok(Control::Return(value)),
                        }
                    }
                }
                Ok(Control::Continue(last))
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
            Expr::List(values) => values
                .iter()
                .map(|expr| self.eval_expr(expr))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List),
            Expr::Dict(values) => values
                .iter()
                .map(|(key, expr)| Ok((key.clone(), self.eval_expr(expr)?)))
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map(Value::Dict),
            Expr::Binary { left, op, right } => {
                let left = self.eval_expr(left)?;
                let right = self.eval_expr(right)?;
                eval_binary(left, *op, right)
            }
        }
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Result<Value, RuntimeError> {
        self.charge(5)?;
        if let Some(value) = self.builtin_call(name, args)? {
            return Ok(value);
        }
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

    fn builtin_call(&mut self, name: &str, args: &[Expr]) -> Result<Option<Value>, RuntimeError> {
        match name {
            "len" => {
                let [arg] = args else {
                    return Err(RuntimeError::new("arity_error", "len expects 1 argument"));
                };
                let value = self.eval_expr(arg)?;
                let len = match value {
                    Value::String(value) => value.len(),
                    Value::List(value) => value.len(),
                    Value::Dict(value) => value.len(),
                    _ => return Err(RuntimeError::new("type_error", "len expects string, list, or dict")),
                };
                Ok(Some(Value::Int(i64::try_from(len).map_err(|_| {
                    RuntimeError::new("runtime_error", "length is out of range")
                })?)))
            }
            "get" => {
                let [target, key] = args else {
                    return Err(RuntimeError::new("arity_error", "get expects 2 arguments"));
                };
                let target = self.eval_expr(target)?;
                let key = self.eval_expr(key)?;
                let value = match (target, key) {
                    (Value::Dict(values), Value::String(key)) => values.get(&key).cloned().unwrap_or(Value::Null),
                    (Value::List(values), Value::Int(index)) if index >= 0 => values
                        .get(
                            usize::try_from(index)
                                .map_err(|_| RuntimeError::new("runtime_error", "list index is out of range"))?,
                        )
                        .cloned()
                        .unwrap_or(Value::Null),
                    _ => {
                        return Err(RuntimeError::new(
                            "type_error",
                            "get expects dict/string key or list/int index",
                        ));
                    }
                };
                Ok(Some(value))
            }
            "contains" => {
                let [target, key] = args else {
                    return Err(RuntimeError::new("arity_error", "contains expects 2 arguments"));
                };
                let target = self.eval_expr(target)?;
                let key = self.eval_expr(key)?;
                let value = match (target, key) {
                    (Value::Dict(values), Value::String(key)) => values.contains_key(&key),
                    (Value::String(value), Value::String(needle)) => value.contains(&needle),
                    (Value::List(values), needle) => values.iter().any(|value| value == &needle),
                    _ => {
                        return Err(RuntimeError::new(
                            "type_error",
                            "contains expects dict, string, or list",
                        ));
                    }
                };
                Ok(Some(Value::Bool(value)))
            }
            _ => Ok(None),
        }
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
        Value::List(values) => format!("{values:?}"),
        Value::Dict(values) => format!("{values:?}"),
        Value::Null => "null".to_string(),
    }
}
