//! Metered managed function runtime.

/// Canonical `managed-function-v0` semantics, metering, billing, and proof
/// compatibility manifest.
///
/// The bytes are deliberately checked into the runtime crate and hash-pinned
/// by contract tests. Changing them is a runtime/proof protocol migration, not
/// an in-place behavior edit.
pub const V0_SEMANTICS_MANIFEST_JSON: &str = include_str!("../managed-function-v0-semantics.json");

/// SHA-256 of the canonical JSON bytes in [`V0_SEMANTICS_MANIFEST_JSON`],
/// excluding the file's trailing newline.
pub const V0_SEMANTICS_MANIFEST_SHA256: &str =
    "8ed716dc07c7bc9abcfc5338b1888e71dd041c3fb397c45d0efb1ff76af1deee";

use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    fmt::{Display, Formatter, Write},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    String(String),
    List(Vec<Self>),
    Dict(BTreeMap<String, Self>),
    Null,
}

/// Render a managed value using the canonical task-output representation.
///
/// Prefer [`render_output_bounded`] for untrusted task output. This legacy
/// convenience function keeps the historical unlimited behavior.
///
/// # Panics
///
/// Panics only if rendering would overflow `usize`; bounded callers should use
/// [`render_output_bounded`] and handle its structured error instead.
#[must_use]
pub fn render_output(value: &Value) -> String {
    render_output_bounded(value, u64::MAX)
        .expect("an unlimited canonical renderer can only fail on length overflow")
}

/// Render a managed value using the canonical task-output representation,
/// rejecting output that would exceed `max_bytes`.
///
/// Strings are emitted raw at the top level; all other values use the same
/// compact JSON representation as [`render_output`]. The renderer appends
/// incrementally and checks every append before allocation, so a rejected
/// value never first creates an unbounded serialized intermediate. `max_bytes`
/// is a fixed-width logical UTF-8 byte count, so the decision is identical on
/// native workers and zkVM guests.
pub fn render_output_bounded(value: &Value, max_bytes: u64) -> Result<String, RuntimeError> {
    let mut output = BoundedOutput::new(max_bytes);
    match value {
        Value::String(value) => output.push_str(value)?,
        Value::Int(value) => output.push_str(&value.to_string())?,
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" })?,
        Value::Null => output.push_str("null")?,
        Value::List(_) | Value::Dict(_) => write_json_value(value, &mut output)?,
    }
    Ok(output.into_inner())
}

struct BoundedOutput {
    value: String,
    rendered_bytes: u64,
    max_bytes: u64,
}

impl BoundedOutput {
    fn new(max_bytes: u64) -> Self {
        Self {
            value: String::new(),
            rendered_bytes: 0,
            max_bytes,
        }
    }

    fn into_inner(self) -> String {
        self.value
    }

    fn push_str(&mut self, text: &str) -> Result<(), RuntimeError> {
        let text_bytes = u64::try_from(text.len()).map_err(|_| output_limit_error())?;
        let next_len = self
            .rendered_bytes
            .checked_add(text_bytes)
            .ok_or_else(output_limit_error)?;
        if next_len > self.max_bytes {
            return Err(output_limit_error());
        }
        self.value.push_str(text);
        self.rendered_bytes = next_len;
        Ok(())
    }

    fn push_char(&mut self, character: char) -> Result<(), RuntimeError> {
        let mut encoded = [0; 4];
        self.push_str(character.encode_utf8(&mut encoded))
    }
}

fn output_limit_error() -> RuntimeError {
    RuntimeError::new("output_limit_exceeded", "output limit exceeded")
}

fn write_json_value(value: &Value, output: &mut BoundedOutput) -> Result<(), RuntimeError> {
    match value {
        Value::Int(value) => output.push_str(&value.to_string()),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::String(value) => write_json_string(value, output),
        Value::Null => output.push_str("null"),
        Value::List(values) => {
            output.push_char('[')?;
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push_char(',')?;
                }
                write_json_value(value, output)?;
            }
            output.push_char(']')
        }
        Value::Dict(values) => {
            output.push_char('{')?;
            for (index, (key, value)) in values.iter().enumerate() {
                if index > 0 {
                    output.push_char(',')?;
                }
                write_json_string(key, output)?;
                output.push_char(':')?;
                write_json_value(value, output)?;
            }
            output.push_char('}')
        }
    }
}

fn write_json_string(value: &str, output: &mut BoundedOutput) -> Result<(), RuntimeError> {
    output.push_char('"')?;
    let mut segment_start = 0;
    for (index, byte) in value.bytes().enumerate() {
        let escaped = match byte {
            b'"' => Some(r#"\""#),
            b'\\' => Some(r"\\"),
            b'\x08' => Some(r"\b"),
            b'\x0c' => Some(r"\f"),
            b'\n' => Some(r"\n"),
            b'\r' => Some(r"\r"),
            b'\t' => Some(r"\t"),
            b'\x00'..=b'\x1f' => None,
            _ => continue,
        };
        output.push_str(&value[segment_start..index])?;
        if let Some(escaped) = escaped {
            output.push_str(escaped)?;
        } else {
            output.push_str(r"\u00")?;
            output.push_char(hex_digit(byte >> 4))?;
            output.push_char(hex_digit(byte & 0x0f))?;
        }
        segment_start = index + 1;
    }
    output.push_str(&value[segment_start..])?;
    output.push_char('"')
}

fn hex_digit(value: u8) -> char {
    char::from(if value < 10 {
        b'0' + value
    } else {
        b'a' + (value - 10)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionLimits {
    pub max_ops: u64,
    pub max_usage_units: Option<u64>,
    pub max_call_depth: usize,
    pub max_output_bytes: u64,
    pub max_loop_iterations: u64,
    /// Maximum canonical-JSON byte size of one materialized managed value.
    ///
    /// This is deterministic logical byte accounting, not allocator capacity:
    /// string/key escaping and collection punctuation are included so native
    /// workers and zkVM guests make the same acceptance decision. The counter
    /// is fixed-width (`u64`), rather than pointer-sized.
    pub max_value_bytes: u64,
    /// Maximum number of direct elements in any materialized list or dict.
    pub max_collection_items: u64,
    /// Maximum nesting depth of any materialized managed value.
    pub max_value_depth: u64,
    /// Maximum cumulative deterministic logical bytes materialized by evaluator
    /// value copies and constructions. This is a safety limit only and never
    /// contributes to billed `usage_units`.
    pub max_value_materialization_bytes: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_ops: 1_000_000,
            max_usage_units: None,
            max_call_depth: 64,
            max_output_bytes: 1_048_576,
            max_loop_iterations: 100_000,
            max_value_bytes: 1_048_576,
            max_collection_items: 100_000,
            max_value_depth: 64,
            max_value_materialization_bytes: 16_777_216,
        }
    }
}

impl ExecutionLimits {
    #[must_use]
    pub fn unlimited() -> Self {
        Self {
            max_ops: u64::MAX,
            max_usage_units: None,
            max_call_depth: usize::MAX,
            max_output_bytes: u64::MAX,
            max_loop_iterations: u64::MAX,
            max_value_bytes: u64::MAX,
            max_collection_items: u64::MAX,
            max_value_depth: u64::MAX,
            max_value_materialization_bytes: u64::MAX,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionReceipt {
    pub executed_ops: u64,
    pub usage_units: u64,
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
    pub fn execute(
        &self,
        source: &str,
        limits: ExecutionLimits,
    ) -> Result<ExecutionResult, RuntimeError> {
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
        evaluator.validate_external_value(&input)?;
        evaluator.current_scope().insert("input".to_string(), input);
        evaluator.eval_program(&program)
    }

    pub fn execute_json_input_with_cancel(
        &self,
        source: &str,
        limits: ExecutionLimits,
        input_json: &str,
        cancelled: &AtomicBool,
    ) -> Result<ExecutionResult, RuntimeError> {
        let input = Value::from_json_str(input_json)?;
        let tokens = Lexer::new(source).tokenize()?;
        let program = Parser::new(tokens).parse_program()?;
        let mut evaluator = Evaluator::with_cancellation(limits, cancelled);
        evaluator.validate_external_value(&input)?;
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
            serde_json::Value::Number(value) => value.as_i64().map(Self::Int).ok_or_else(|| {
                RuntimeError::new(
                    "input_error",
                    "only signed 64-bit JSON integers are supported",
                )
            }),
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

#[derive(Clone, Copy)]
struct ValueMetrics {
    canonical_bytes: u64,
    depth: u64,
    max_collection_items: u64,
}

fn value_metrics(value: &Value) -> Result<ValueMetrics, RuntimeError> {
    match value {
        Value::Int(value) => Ok(ValueMetrics {
            canonical_bytes: logical_usize(value.to_string().len())?,
            depth: 1,
            max_collection_items: 0,
        }),
        Value::Bool(value) => Ok(ValueMetrics {
            canonical_bytes: if *value { 4 } else { 5 },
            depth: 1,
            max_collection_items: 0,
        }),
        Value::String(value) => Ok(ValueMetrics {
            canonical_bytes: json_string_len(value)?,
            depth: 1,
            max_collection_items: 0,
        }),
        Value::Null => Ok(ValueMetrics {
            canonical_bytes: 4,
            depth: 1,
            max_collection_items: 0,
        }),
        Value::List(values) => list_value_metrics(values),
        Value::Dict(values) => dict_value_metrics(values),
    }
}

fn list_value_metrics(values: &[Value]) -> Result<ValueMetrics, RuntimeError> {
    let mut canonical_bytes = 2;
    let mut depth = 1;
    let mut max_collection_items = logical_usize(values.len())?;
    let mut first = true;
    for value in values {
        if first {
            first = false;
        } else {
            canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        }
        let metrics = value_metrics(value)?;
        canonical_bytes = checked_value_add(canonical_bytes, metrics.canonical_bytes)?;
        depth = depth.max(metrics.depth.checked_add(1).ok_or_else(value_limit_error)?);
        max_collection_items = max_collection_items.max(metrics.max_collection_items);
    }
    Ok(ValueMetrics {
        canonical_bytes,
        depth,
        max_collection_items,
    })
}

fn dict_value_metrics(values: &BTreeMap<String, Value>) -> Result<ValueMetrics, RuntimeError> {
    let mut canonical_bytes = 2;
    let mut depth = 1;
    let mut max_collection_items = logical_usize(values.len())?;
    let mut first = true;
    for (key, value) in values {
        if first {
            first = false;
        } else {
            canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        }
        canonical_bytes = checked_value_add(canonical_bytes, json_string_len(key)?)?;
        canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        let metrics = value_metrics(value)?;
        canonical_bytes = checked_value_add(canonical_bytes, metrics.canonical_bytes)?;
        depth = depth.max(metrics.depth.checked_add(1).ok_or_else(value_limit_error)?);
        max_collection_items = max_collection_items.max(metrics.max_collection_items);
    }
    Ok(ValueMetrics {
        canonical_bytes,
        depth,
        max_collection_items,
    })
}

fn list_metrics_after_assignment(
    values: &[Value],
    replacement_index: usize,
    replacement: &Value,
) -> Result<ValueMetrics, RuntimeError> {
    let mut canonical_bytes = 2;
    let mut depth = 1;
    let mut max_collection_items = logical_usize(values.len())?;
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        }
        let metrics = if index == replacement_index {
            value_metrics(replacement)?
        } else {
            value_metrics(value)?
        };
        canonical_bytes = checked_value_add(canonical_bytes, metrics.canonical_bytes)?;
        depth = depth.max(metrics.depth.checked_add(1).ok_or_else(value_limit_error)?);
        max_collection_items = max_collection_items.max(metrics.max_collection_items);
    }
    Ok(ValueMetrics {
        canonical_bytes,
        depth,
        max_collection_items,
    })
}

fn dict_metrics_after_assignment(
    values: &BTreeMap<String, Value>,
    replacement_key: &str,
    replacement: &Value,
) -> Result<ValueMetrics, RuntimeError> {
    let contains_key = values.contains_key(replacement_key);
    let value_count = logical_usize(values.len())?;
    let item_count = if contains_key {
        value_count
    } else {
        checked_value_add(value_count, 1)?
    };
    let mut canonical_bytes = 2;
    let mut depth = 1;
    let mut max_collection_items = item_count;
    let mut first = true;
    for (key, value) in values {
        if first {
            first = false;
        } else {
            canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        }
        canonical_bytes = checked_value_add(canonical_bytes, json_string_len(key)?)?;
        canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        let metrics = if key == replacement_key {
            value_metrics(replacement)?
        } else {
            value_metrics(value)?
        };
        canonical_bytes = checked_value_add(canonical_bytes, metrics.canonical_bytes)?;
        depth = depth.max(metrics.depth.checked_add(1).ok_or_else(value_limit_error)?);
        max_collection_items = max_collection_items.max(metrics.max_collection_items);
    }
    if !contains_key {
        if !first {
            canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        }
        canonical_bytes = checked_value_add(canonical_bytes, json_string_len(replacement_key)?)?;
        canonical_bytes = checked_value_add(canonical_bytes, 1)?;
        let metrics = value_metrics(replacement)?;
        canonical_bytes = checked_value_add(canonical_bytes, metrics.canonical_bytes)?;
        depth = depth.max(metrics.depth.checked_add(1).ok_or_else(value_limit_error)?);
        max_collection_items = max_collection_items.max(metrics.max_collection_items);
    }
    Ok(ValueMetrics {
        canonical_bytes,
        depth,
        max_collection_items,
    })
}

fn json_string_len(value: &str) -> Result<u64, RuntimeError> {
    let mut length = 2;
    for byte in value.bytes() {
        let encoded_len = match byte {
            b'"' | b'\\' | b'\x08' | b'\x0c' | b'\n' | b'\r' | b'\t' => 2,
            b'\x00'..=b'\x1f' => 6,
            _ => 1,
        };
        length = checked_value_add(length, encoded_len)?;
    }
    Ok(length)
}

fn logical_usize(value: usize) -> Result<u64, RuntimeError> {
    u64::try_from(value).map_err(|_| value_limit_error())
}

fn checked_value_add(left: u64, right: u64) -> Result<u64, RuntimeError> {
    left.checked_add(right).ok_or_else(value_limit_error)
}

fn value_limit_error() -> RuntimeError {
    RuntimeError::new("value_limit_exceeded", "value limit exceeded")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Null,
    And,
    Or,
    Not,
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
        while matches!(
            self.peek(),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
        ) {
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
            "and" => Token::And,
            "or" => Token::Or,
            "not" => Token::Not,
            "null" => Token::Null,
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
                        return Err(RuntimeError::new(
                            "parse_error",
                            "unterminated string escape",
                        ));
                    };
                    self.bump();
                    let ch = match escaped {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        _ => {
                            return Err(RuntimeError::new(
                                "parse_error",
                                "unsupported string escape",
                            ));
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
    IndexAssign {
        target: Expr,
        index: Expr,
        value: Expr,
    },
    Expr(Expr),
}

#[derive(Debug, Clone)]
struct Function {
    name: String,
    params: Vec<String>,
    body: Vec<Stmt>,
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
    Unary {
        op: UnaryOp,
        expr: Box<Self>,
    },
    Logical {
        left: Box<Self>,
        op: LogicalOp,
        right: Box<Self>,
    },
    Index {
        target: Box<Self>,
        index: Box<Self>,
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

#[derive(Debug, Clone, Copy)]
enum UnaryOp {
    Neg,
    Pos,
    Not,
}

#[derive(Debug, Clone, Copy)]
enum LogicalOp {
    And,
    Or,
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
                if matches!(self.peek(), Token::Eq) {
                    self.advance();
                    let value = self.expression()?;
                    self.expect(&Token::Semi)?;
                    let Expr::Index { target, index } = expr else {
                        return Err(RuntimeError::new(
                            "parse_error",
                            "assignment target must be an index expression",
                        ));
                    };
                    return Ok(Stmt::IndexAssign {
                        target: *target,
                        index: *index,
                        value,
                    });
                }
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
        Ok(Stmt::For {
            item,
            iterable,
            body,
        })
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
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            body.push(self.statement()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(Function { name, params, body })
    }

    fn expression(&mut self) -> Result<Expr, RuntimeError> {
        self.logic_or()
    }

    fn logic_or(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.logic_and()?;
        while matches!(self.peek(), Token::Or) {
            self.advance();
            let right = self.logic_and()?;
            expr = Expr::Logical {
                left: Box::new(expr),
                op: LogicalOp::Or,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn logic_and(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.logic_not()?;
        while matches!(self.peek(), Token::And) {
            self.advance();
            let right = self.logic_not()?;
            expr = Expr::Logical {
                left: Box::new(expr),
                op: LogicalOp::And,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn logic_not(&mut self) -> Result<Expr, RuntimeError> {
        if matches!(self.peek(), Token::Not) {
            self.advance();
            let expr = self.logic_not()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            });
        }
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
        let mut expr = self.unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.unary()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn unary(&mut self) -> Result<Expr, RuntimeError> {
        let op = match self.peek() {
            Token::Minus => Some(UnaryOp::Neg),
            Token::Plus => Some(UnaryOp::Pos),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let expr = self.unary()?;
            return Ok(Expr::Unary {
                op,
                expr: Box::new(expr),
            });
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, RuntimeError> {
        let mut expr = self.primary()?;
        while matches!(self.peek(), Token::LBracket) {
            self.advance();
            let index = self.expression()?;
            self.expect(&Token::RBracket)?;
            expr = Expr::Index {
                target: Box::new(expr),
                index: Box::new(index),
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
            Token::Null => {
                self.advance();
                Ok(Expr::Value(Value::Null))
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
            token => Err(RuntimeError::new(
                "parse_error",
                format!("expected expression, found {token:?}"),
            )
            .at(self.span())),
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
            token => Err(RuntimeError::new(
                "parse_error",
                format!("expected identifier, found {token:?}"),
            )
            .at(self.span())),
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
            )
            .at(self.span()))
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

struct Evaluator<'a> {
    limits: ExecutionLimits,
    receipt: ExecutionReceipt,
    output: String,
    functions: HashMap<String, Function>,
    scopes: Vec<HashMap<String, Value>>,
    call_depth: usize,
    materialized_value_bytes: u64,
    cancelled: Option<&'a AtomicBool>,
}

impl<'a> Evaluator<'a> {
    fn new(limits: ExecutionLimits) -> Self {
        Self {
            limits,
            receipt: ExecutionReceipt::default(),
            output: String::new(),
            functions: HashMap::new(),
            scopes: vec![HashMap::new()],
            call_depth: 0,
            materialized_value_bytes: 0,
            cancelled: None,
        }
    }

    fn with_cancellation(limits: ExecutionLimits, cancelled: &'a AtomicBool) -> Self {
        Self {
            cancelled: Some(cancelled),
            ..Self::new(limits)
        }
    }

    fn eval_program(mut self, program: &Program) -> Result<ExecutionResult, RuntimeError> {
        for statement in &program.statements {
            if let Stmt::Fn(function) = statement {
                self.functions
                    .insert(function.name.clone(), function.clone());
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
            Stmt::For {
                item,
                iterable,
                body,
            } => {
                let iterable = self.eval_expr(iterable)?;
                let Value::List(values) = iterable else {
                    return Err(RuntimeError::new("type_error", "for expects a list"));
                };
                let mut last = Value::Null;
                for value in values {
                    let next_iterations =
                        self.receipt.loop_iterations.checked_add(1).ok_or_else(|| {
                            RuntimeError::new(
                                "loop_limit_exceeded",
                                "loop iteration limit exceeded",
                            )
                        })?;
                    if next_iterations > self.limits.max_loop_iterations {
                        return Err(RuntimeError::new(
                            "loop_limit_exceeded",
                            "loop iteration limit exceeded",
                        ));
                    }
                    self.receipt.loop_iterations = next_iterations;
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
                append_debug_output(&mut self.output, self.limits.max_output_bytes, &value)?;
                self.receipt.output_bytes = self.output.len();
                Ok(Control::Continue(Value::Null))
            }
            Stmt::IndexAssign {
                target,
                index,
                value,
            } => {
                let value = self.eval_expr(value)?;
                let index = self.eval_expr(index)?;
                let Expr::Variable(name) = target else {
                    return Err(RuntimeError::new(
                        "type_error",
                        "index assignment target must be a variable",
                    ));
                };
                let mut current = self.lookup(name)?;
                self.assign_index(&mut current, index, value)?;
                self.assign(name, current)?;
                Ok(Control::Continue(Value::Null))
            }
            Stmt::Expr(expr) => self.eval_expr(expr).map(Control::Continue),
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        self.charge(1)?;
        match expr {
            Expr::Value(value) => self.clone_value(value),
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
            Expr::List(values) => self.eval_list(values),
            Expr::Dict(values) => self.eval_dict(values),
            Expr::Binary { left, op, right } => {
                let left = self.eval_expr(left)?;
                let right = self.eval_expr(right)?;
                self.eval_binary(left, *op, right)
            }
            Expr::Unary { op, expr } => {
                let value = self.eval_expr(expr)?;
                eval_unary(*op, &value)
            }
            Expr::Logical { left, op, right } => {
                let left = self.eval_expr(left)?;
                let Value::Bool(left) = left else {
                    return Err(RuntimeError::new(
                        "type_error",
                        "logical operators expect bool operands",
                    ));
                };
                match op {
                    LogicalOp::And => {
                        if !left {
                            return Ok(Value::Bool(false));
                        }
                    }
                    LogicalOp::Or => {
                        if left {
                            return Ok(Value::Bool(true));
                        }
                    }
                }
                let right = self.eval_expr(right)?;
                match right {
                    Value::Bool(right) => Ok(Value::Bool(right)),
                    _ => Err(RuntimeError::new(
                        "type_error",
                        "logical operators expect bool operands",
                    )),
                }
            }
            Expr::Index { target, index } => {
                let target = self.eval_expr(target)?;
                let index = self.eval_expr(index)?;
                self.eval_index(target, index)
            }
        }
    }

    fn validate_external_value(&self, value: &Value) -> Result<(), RuntimeError> {
        self.validate_value_metrics(value_metrics(value)?)
    }

    fn validate_value_metrics(&self, metrics: ValueMetrics) -> Result<(), RuntimeError> {
        if metrics.canonical_bytes > self.limits.max_value_bytes
            || metrics.depth > self.limits.max_value_depth
            || metrics.max_collection_items > self.limits.max_collection_items
        {
            return Err(value_limit_error());
        }
        Ok(())
    }

    fn clone_value(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let metrics = value_metrics(value)?;
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;
        Ok(value.clone())
    }

    fn materialize_owned_value(&mut self, value: Value) -> Result<Value, RuntimeError> {
        let metrics = value_metrics(&value)?;
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;
        Ok(value)
    }

    fn charge_value_materialization(&mut self, bytes: u64) -> Result<(), RuntimeError> {
        if self.limits.max_value_materialization_bytes == u64::MAX {
            return Ok(());
        }
        let next = self
            .materialized_value_bytes
            .checked_add(bytes)
            .ok_or_else(value_limit_error)?;
        if next > self.limits.max_value_materialization_bytes {
            return Err(value_limit_error());
        }
        self.materialized_value_bytes = next;
        Ok(())
    }

    fn eval_list(&mut self, expressions: &[Expr]) -> Result<Value, RuntimeError> {
        let mut metrics = ValueMetrics {
            canonical_bytes: 2,
            depth: 1,
            max_collection_items: logical_usize(expressions.len())?,
        };
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;

        let mut values = Vec::new();
        for (index, expression) in expressions.iter().enumerate() {
            let value = self.eval_expr(expression)?;
            let value_metrics = value_metrics(&value)?;
            let separator_bytes = u64::from(index > 0);
            let additional_bytes =
                checked_value_add(separator_bytes, value_metrics.canonical_bytes)?;
            metrics = ValueMetrics {
                canonical_bytes: checked_value_add(metrics.canonical_bytes, additional_bytes)?,
                depth: metrics.depth.max(
                    value_metrics
                        .depth
                        .checked_add(1)
                        .ok_or_else(value_limit_error)?,
                ),
                max_collection_items: metrics
                    .max_collection_items
                    .max(value_metrics.max_collection_items),
            };
            self.validate_value_metrics(metrics)?;
            self.charge_value_materialization(additional_bytes)?;
            values.push(value);
        }
        Ok(Value::List(values))
    }

    fn eval_dict(&mut self, expressions: &[(String, Expr)]) -> Result<Value, RuntimeError> {
        let mut metrics = ValueMetrics {
            canonical_bytes: 2,
            depth: 1,
            max_collection_items: logical_usize(expressions.len())?,
        };
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;

        let mut values = BTreeMap::new();
        for (index, (key, expression)) in expressions.iter().enumerate() {
            let key_bytes = json_string_len(key)?;
            let value = self.eval_expr(expression)?;
            let value_metrics = value_metrics(&value)?;
            let separator_bytes = u64::from(index > 0);
            let additional_bytes = checked_value_add(
                checked_value_add(separator_bytes, key_bytes)?,
                checked_value_add(1, value_metrics.canonical_bytes)?,
            )?;
            metrics = ValueMetrics {
                canonical_bytes: checked_value_add(metrics.canonical_bytes, additional_bytes)?,
                depth: metrics.depth.max(
                    value_metrics
                        .depth
                        .checked_add(1)
                        .ok_or_else(value_limit_error)?,
                ),
                max_collection_items: metrics
                    .max_collection_items
                    .max(value_metrics.max_collection_items),
            };
            self.validate_value_metrics(metrics)?;
            self.charge_value_materialization(additional_bytes)?;
            values.insert(key.clone(), value);
        }
        Ok(Value::Dict(values))
    }

    fn eval_binary(
        &mut self,
        left: Value,
        op: BinaryOp,
        right: Value,
    ) -> Result<Value, RuntimeError> {
        match (op, left, right) {
            (BinaryOp::Add, Value::String(left), Value::String(right)) => {
                self.concat_strings(&left, &right)
            }
            (op, left, right) => eval_binary(left, op, right),
        }
    }

    fn concat_strings(&mut self, left: &str, right: &str) -> Result<Value, RuntimeError> {
        let raw_bytes = left
            .len()
            .checked_add(right.len())
            .ok_or_else(value_limit_error)?;
        let canonical_bytes = checked_value_add(json_string_len(left)?, json_string_len(right)?)?
            .checked_sub(2)
            .ok_or_else(value_limit_error)?;
        let metrics = ValueMetrics {
            canonical_bytes,
            depth: 1,
            max_collection_items: 0,
        };
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;

        let mut value = String::with_capacity(raw_bytes);
        value.push_str(left);
        value.push_str(right);
        Ok(Value::String(value))
    }

    fn eval_index(&mut self, target: Value, index: Value) -> Result<Value, RuntimeError> {
        match (target, index) {
            (Value::List(values), Value::Int(index)) => {
                let index = normalize_index(index, values.len())?;
                self.clone_value(&values[index])
            }
            (Value::String(value), Value::Int(index)) => {
                let index = normalize_index(index, value.chars().count())?;
                let character = value
                    .chars()
                    .nth(index)
                    .expect("a normalized string index must be present");
                self.materialize_owned_value(Value::String(character.to_string()))
            }
            (Value::Dict(values), Value::String(key)) => match values.get(&key) {
                Some(value) => self.clone_value(value),
                None => Err(RuntimeError::new(
                    "key_error",
                    format!("key '{key}' not found"),
                )),
            },
            _ => Err(RuntimeError::new(
                "type_error",
                "indexing expects list[int], string[int], or dict[string]",
            )),
        }
    }

    fn assign_index(
        &mut self,
        target: &mut Value,
        index: Value,
        value: Value,
    ) -> Result<(), RuntimeError> {
        match (target, index) {
            (Value::List(values), Value::Int(index)) => {
                let index = normalize_index(index, values.len())?;
                self.validate_value_metrics(list_metrics_after_assignment(values, index, &value)?)?;
                values[index] = value;
                Ok(())
            }
            (Value::Dict(values), Value::String(key)) => {
                let inserting = !values.contains_key(&key);
                self.validate_value_metrics(dict_metrics_after_assignment(values, &key, &value)?)?;
                if inserting {
                    let entry_bytes = checked_value_add(
                        checked_value_add(json_string_len(&key)?, 1)?,
                        value_metrics(&value)?.canonical_bytes,
                    )?;
                    self.charge_value_materialization(entry_bytes)?;
                }
                values.insert(key, value);
                Ok(())
            }
            _ => Err(RuntimeError::new(
                "type_error",
                "index assignment expects list[int] or dict[string]",
            )),
        }
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Result<Value, RuntimeError> {
        self.charge(5)?;
        if let Some(value) = self.builtin_call(name, args)? {
            return Ok(value);
        }
        let function =
            self.functions.get(name).cloned().ok_or_else(|| {
                RuntimeError::new("name_error", format!("unknown function '{name}'"))
            })?;
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
        let mut values = Vec::new();
        for arg in args {
            values.push(self.eval_expr(arg)?);
        }
        let next_call_depth = self
            .call_depth
            .checked_add(1)
            .ok_or_else(|| RuntimeError::new("call_depth_exceeded", "call depth exceeded"))?;
        if next_call_depth > self.limits.max_call_depth {
            return Err(RuntimeError::new(
                "call_depth_exceeded",
                "call depth exceeded",
            ));
        }
        self.receipt.function_calls += 1;
        self.call_depth = next_call_depth;
        self.receipt.max_call_depth = self.receipt.max_call_depth.max(self.call_depth);
        let mut scope = HashMap::new();
        for (param, value) in function.params.iter().zip(values) {
            scope.insert(param.clone(), value);
        }
        self.scopes.push(scope);
        let outcome = self.eval_function_body(&function.body);
        self.scopes.pop();
        self.call_depth -= 1;
        outcome
    }

    fn eval_function_body(&mut self, body: &[Stmt]) -> Result<Value, RuntimeError> {
        // Preserve legacy op metering for the common single-`return` body: the
        // expression is evaluated directly, without an extra statement-dispatch
        // charge, so existing budget calibrations remain valid.
        if let [Stmt::Return(expr)] = body {
            return self.eval_expr(expr);
        }
        let mut result = Value::Null;
        for statement in body {
            match self.eval_stmt(statement)? {
                Control::Continue(value) => result = value,
                Control::Return(value) => return Ok(value),
            }
        }
        Ok(result)
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
                    _ => {
                        return Err(RuntimeError::new(
                            "type_error",
                            "len expects string, list, or dict",
                        ));
                    }
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
                    (Value::Dict(values), Value::String(key)) => match values.get(&key) {
                        Some(value) => self.clone_value(value)?,
                        None => Value::Null,
                    },
                    (Value::List(values), Value::Int(index)) if index >= 0 => {
                        match values.get(usize::try_from(index).map_err(|_| {
                            RuntimeError::new("runtime_error", "list index is out of range")
                        })?) {
                            Some(value) => self.clone_value(value)?,
                            None => Value::Null,
                        }
                    }
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
                    return Err(RuntimeError::new(
                        "arity_error",
                        "contains expects 2 arguments",
                    ));
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

    fn lookup(&mut self, name: &str) -> Result<Value, RuntimeError> {
        let metrics = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).map(value_metrics))
            .transpose()?
            .ok_or_else(|| RuntimeError::new("name_error", format!("unknown variable '{name}'")))?;
        self.validate_value_metrics(metrics)?;
        self.charge_value_materialization(metrics.canonical_bytes)?;
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Ok(value.clone());
            }
        }
        unreachable!("a value found while measuring must still be present while cloning")
    }

    fn assign(&mut self, name: &str, value: Value) -> Result<(), RuntimeError> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(slot) = scope.get_mut(name) {
                *slot = value;
                return Ok(());
            }
        }
        Err(RuntimeError::new(
            "name_error",
            format!("unknown variable '{name}'"),
        ))
    }

    fn current_scope(&mut self) -> &mut HashMap<String, Value> {
        self.scopes
            .last_mut()
            .expect("evaluator always has a current scope")
    }

    fn charge(&mut self, cost: u64) -> Result<(), RuntimeError> {
        if self
            .cancelled
            .is_some_and(|cancelled| cancelled.load(Ordering::Acquire))
        {
            return Err(RuntimeError::new("cancelled", "task execution stopped"));
        }
        let next = self.receipt.executed_ops.saturating_add(cost);
        if next > self.limits.max_ops {
            return Err(RuntimeError::new(
                "op_limit_exceeded",
                "operation limit exceeded",
            ));
        }
        let next_usage = self.receipt.usage_units.saturating_add(cost);
        if self
            .limits
            .max_usage_units
            .is_some_and(|limit| next_usage > limit)
        {
            return Err(RuntimeError::new(
                "budget_exhausted",
                "execution budget exhausted",
            ));
        }
        self.receipt.executed_ops = next;
        self.receipt.usage_units = next_usage;
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
            _ => Err(RuntimeError::new(
                "type_error",
                "+ expects matching ints or strings",
            )),
        },
        BinaryOp::Sub => int_binary(left, right, "-", |left, right| left - right),
        BinaryOp::Mul => int_binary(left, right, "*", |left, right| left * right),
        BinaryOp::Div => match (left, right) {
            (Value::Int(_), Value::Int(0)) => {
                Err(RuntimeError::new("runtime_error", "division by zero"))
            }
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

fn eval_unary(op: UnaryOp, value: &Value) -> Result<Value, RuntimeError> {
    match op {
        UnaryOp::Neg => match value {
            Value::Int(value) => Ok(Value::Int(-value)),
            _ => Err(RuntimeError::new("type_error", "unary - expects an int")),
        },
        UnaryOp::Pos => match value {
            Value::Int(value) => Ok(Value::Int(*value)),
            _ => Err(RuntimeError::new("type_error", "unary + expects an int")),
        },
        UnaryOp::Not => match value {
            Value::Bool(value) => Ok(Value::Bool(!value)),
            _ => Err(RuntimeError::new("type_error", "not expects a bool")),
        },
    }
}

fn normalize_index(index: i64, len: usize) -> Result<usize, RuntimeError> {
    let len_i64 = i64::try_from(len)
        .map_err(|_| RuntimeError::new("runtime_error", "collection is too large to index"))?;
    let resolved = if index < 0 { index + len_i64 } else { index };
    if resolved < 0 || resolved >= len_i64 {
        return Err(RuntimeError::new("index_error", "index out of range"));
    }
    usize::try_from(resolved)
        .map_err(|_| RuntimeError::new("runtime_error", "index is out of range"))
}

fn int_binary(
    left: Value,
    right: Value,
    op: &'static str,
    apply: impl FnOnce(i64, i64) -> i64,
) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => Ok(Value::Int(apply(left, right))),
        _ => Err(RuntimeError::new(
            "type_error",
            format!("{op} expects ints"),
        )),
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
        _ => Err(RuntimeError::new(
            "type_error",
            format!("{op} expects ints"),
        )),
    }
}

fn append_debug_output(
    output: &mut String,
    max_bytes: u64,
    value: &Value,
) -> Result<(), RuntimeError> {
    let rendered_bytes = logical_usize(output.len())?;
    let mut writer = BoundedFmtWriter {
        output,
        rendered_bytes,
        max_bytes,
    };
    match value {
        Value::Int(value) => write!(&mut writer, "{value}"),
        Value::Bool(value) => write!(&mut writer, "{value}"),
        Value::String(value) => writer.write_str(value),
        Value::List(values) => write!(&mut writer, "{values:?}"),
        Value::Dict(values) => write!(&mut writer, "{values:?}"),
        Value::Null => writer.write_str("null"),
    }
    .map_err(|_| output_limit_error())?;
    writer.write_char('\n').map_err(|_| output_limit_error())
}

struct BoundedFmtWriter<'a> {
    output: &'a mut String,
    rendered_bytes: u64,
    max_bytes: u64,
}

impl Write for BoundedFmtWriter<'_> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        let value_bytes = u64::try_from(value.len()).map_err(|_| std::fmt::Error)?;
        let next_len = self
            .rendered_bytes
            .checked_add(value_bytes)
            .ok_or(std::fmt::Error)?;
        if next_len > self.max_bytes {
            return Err(std::fmt::Error);
        }
        self.output.push_str(value);
        self.rendered_bytes = next_len;
        Ok(())
    }

    fn write_char(&mut self, character: char) -> std::fmt::Result {
        let mut encoded = [0; 4];
        self.write_str(character.encode_utf8(&mut encoded))
    }
}

#[cfg(test)]
mod tests {
    use super::{Value, render_output_bounded};

    #[test]
    fn bounded_canonical_renderer_matches_serde_json_escaping() {
        for value in [
            "",
            "plain text",
            "quote: \"; slash: \\; controls: \u{0008}\u{000c}\n\r\t\u{0000}\u{001f}",
            "snowman: ☃; line separator: \u{2028}",
        ] {
            let managed = Value::List(vec![Value::String(value.to_string())]);
            let expected =
                serde_json::to_string(&serde_json::Value::Array(vec![serde_json::Value::String(
                    value.to_string(),
                )]))
                .unwrap();
            assert_eq!(render_output_bounded(&managed, u64::MAX).unwrap(), expected);
        }
    }
}
