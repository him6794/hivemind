//! Conservative source-to-managed-function transpiler.

use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceLanguage {
    Python,
    Cpp,
}

#[derive(Debug, Eq, PartialEq)]
pub enum TranspileError {
    Unsupported {
        language: SourceLanguage,
        construct: String,
        line: usize,
    },
    Syntax {
        language: SourceLanguage,
        message: String,
        line: usize,
    },
}

impl TranspileError {
    #[must_use]
    pub fn language(&self) -> SourceLanguage {
        match self {
            Self::Unsupported { language, .. } | Self::Syntax { language, .. } => *language,
        }
    }
}

impl Display for TranspileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported {
                language,
                construct,
                line,
            } => write!(
                f,
                "{language:?} construct is outside the conservative transpiler subset at line {line}: {construct}"
            ),
            Self::Syntax {
                language,
                message,
                line,
            } => write!(f, "{language:?} syntax error at line {line}: {message}"),
        }
    }
}

impl Error for TranspileError {}

pub fn transpile_python(source: &str) -> Result<String, TranspileError> {
    let lines = python_lines(source);
    let header = lines.first().ok_or_else(|| py_syntax("missing function", 1))?;
    if !header.text.starts_with("def ") || !header.text.ends_with(':') {
        return Err(py_syntax("expected a single def function", header.number));
    }
    let signature = header.text.trim_start_matches("def ").trim_end_matches(':');
    let (name, args) = parse_signature(signature, SourceLanguage::Python, header.number)?;
    let mut index = 1;
    let body = parse_python_body(&lines, &mut index, 4)?;
    if index < lines.len() {
        return Err(py_syntax("unexpected top-level statement", lines[index].number));
    }
    Ok(format!("fn {name}({}) {{\n{body}\n}}", args.join(", ")))
}

pub fn transpile_cpp(source: &str) -> Result<String, TranspileError> {
    let source = strip_cpp_comments(source);
    reject_cpp_global_unsupported(&source)?;
    let open = source.find('{').ok_or_else(|| cpp_syntax("missing function body", 1))?;
    let close = source
        .rfind('}')
        .ok_or_else(|| cpp_syntax("missing closing brace", 1))?;
    if close <= open {
        return Err(cpp_syntax("malformed function body", 1));
    }
    let (name, args) = parse_cpp_header(source[..open].trim())?;
    let body = parse_cpp_body(source[open + 1..close].trim())?;
    Ok(format!("fn {name}({}) {{\n{body}\n}}", args.join(", ")))
}

#[derive(Debug)]
struct PythonLine {
    indent: usize,
    text: String,
    number: usize,
}

fn python_lines(source: &str) -> Vec<PythonLine> {
    source
        .lines()
        .enumerate()
        .filter_map(|(index, raw)| {
            let text = raw.trim();
            if text.is_empty() || text.starts_with('#') {
                return None;
            }
            Some(PythonLine {
                indent: raw.chars().take_while(|ch| *ch == ' ').count(),
                text: text.to_string(),
                number: index + 1,
            })
        })
        .collect()
}

fn parse_python_body(lines: &[PythonLine], index: &mut usize, indent: usize) -> Result<String, TranspileError> {
    let mut statements = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return Err(py_syntax("unexpected indentation", line.number));
        }
        reject_python_unsupported(&line.text, line.number)?;
        if let Some(expr) = line.text.strip_prefix("return ") {
            statements.push(format!("    return {};", convert_expr(expr)));
            *index += 1;
        } else if line.text.starts_with("if ") && line.text.ends_with(':') {
            statements.push(format!(
                "    return {};",
                parse_python_returning_if(lines, index, indent)?
            ));
        } else if let Some((name, expr)) = parse_assignment(&line.text) {
            statements.push(format!("    let {name} = {};", convert_expr(expr)));
            *index += 1;
        } else {
            return Err(py_syntax(
                "expected assignment, returning if/else, or return",
                line.number,
            ));
        }
    }
    Ok(statements.join("\n"))
}

fn parse_python_returning_if(lines: &[PythonLine], index: &mut usize, indent: usize) -> Result<String, TranspileError> {
    let line = &lines[*index];
    let condition = line.text.trim_start_matches("if ").trim_end_matches(':').trim();
    *index += 1;
    let then_line = lines
        .get(*index)
        .ok_or_else(|| py_syntax("missing if body", line.number))?;
    if then_line.indent != indent + 4 {
        return Err(py_syntax("expected indented if body", then_line.number));
    }
    let then_expr = then_line
        .text
        .strip_prefix("return ")
        .ok_or_else(|| py_syntax("only returning if bodies are supported", then_line.number))?;
    *index += 1;
    let else_line = lines
        .get(*index)
        .ok_or_else(|| py_syntax("missing else branch", line.number))?;
    if else_line.indent != indent || else_line.text != "else:" {
        return Err(py_syntax("expected else branch", else_line.number));
    }
    *index += 1;
    let else_body = lines
        .get(*index)
        .ok_or_else(|| py_syntax("missing else body", else_line.number))?;
    if else_body.indent != indent + 4 {
        return Err(py_syntax("expected indented else body", else_body.number));
    }
    let else_expr = else_body
        .text
        .strip_prefix("return ")
        .ok_or_else(|| py_syntax("only returning else bodies are supported", else_body.number))?;
    *index += 1;
    Ok(format!(
        "if {} {{ {} }} else {{ {} }}",
        convert_expr(condition),
        convert_expr(then_expr),
        convert_expr(else_expr)
    ))
}

fn reject_python_unsupported(text: &str, line: usize) -> Result<(), TranspileError> {
    for keyword in ["for ", "while ", "class ", "import ", "from ", "try:", "with "] {
        if text.starts_with(keyword) {
            return Err(TranspileError::Unsupported {
                language: SourceLanguage::Python,
                construct: keyword.trim().trim_end_matches(':').to_string(),
                line,
            });
        }
    }
    Ok(())
}

fn parse_cpp_body(body: &str) -> Result<String, TranspileError> {
    let if_start = body
        .find("if")
        .ok_or_else(|| cpp_syntax("expected declaration plus if/else return", 1))?;
    let prefix = body[..if_start].trim();
    let mut statements = Vec::new();
    if !prefix.is_empty() {
        statements.push(parse_cpp_declaration(prefix)?);
    }
    statements.push(format!("    return {};", parse_cpp_returning_if(&body[if_start..])?));
    Ok(statements.join("\n"))
}

fn parse_cpp_declaration(statement: &str) -> Result<String, TranspileError> {
    let statement = statement.trim_end_matches(';').trim();
    for ty in ["int", "long", "float", "double", "bool"] {
        if let Some(rest) = statement.strip_prefix(ty).and_then(|rest| rest.strip_prefix(' ')) {
            let (name, expr) =
                parse_assignment(rest).ok_or_else(|| cpp_syntax("expected initialized declaration", 1))?;
            return Ok(format!("    let {name} = {};", convert_expr(expr)));
        }
    }
    Err(cpp_syntax("expected supported local declaration", 1))
}

fn parse_cpp_returning_if(statement: &str) -> Result<String, TranspileError> {
    let condition_start = statement
        .find('(')
        .ok_or_else(|| cpp_syntax("missing if condition", 1))?;
    let condition_end = matching_delimiter(statement, condition_start, '(', ')')
        .ok_or_else(|| cpp_syntax("unclosed if condition", 1))?;
    let then_open = statement[condition_end + 1..]
        .find('{')
        .map(|offset| condition_end + 1 + offset)
        .ok_or_else(|| cpp_syntax("missing if body", 1))?;
    let then_close =
        matching_delimiter(statement, then_open, '{', '}').ok_or_else(|| cpp_syntax("unclosed if body", 1))?;
    let condition = statement[condition_start + 1..condition_end].trim();
    let then_expr = extract_cpp_return_expr(&statement[then_open + 1..then_close])?;
    let rest = statement[then_close + 1..].trim();
    let else_rest = rest
        .strip_prefix("else")
        .ok_or_else(|| cpp_syntax("expected else branch", 1))?
        .trim();
    let else_open = else_rest.find('{').ok_or_else(|| cpp_syntax("missing else body", 1))?;
    let else_close =
        matching_delimiter(else_rest, else_open, '{', '}').ok_or_else(|| cpp_syntax("unclosed else body", 1))?;
    let else_expr = extract_cpp_return_expr(&else_rest[else_open + 1..else_close])?;
    Ok(format!(
        "if {} {{ {} }} else {{ {} }}",
        convert_expr(condition),
        convert_expr(&then_expr),
        convert_expr(&else_expr)
    ))
}

fn extract_cpp_return_expr(body: &str) -> Result<String, TranspileError> {
    body.trim()
        .strip_prefix("return ")
        .map(|expr| expr.trim_end_matches(';').trim().to_string())
        .ok_or_else(|| cpp_syntax("only returning if/else bodies are supported", 1))
}

fn reject_cpp_global_unsupported(source: &str) -> Result<(), TranspileError> {
    for keyword in [
        "for", "while", "switch", "do", "class", "template", "new", "delete", "#include",
    ] {
        if source.contains(keyword) {
            return Err(TranspileError::Unsupported {
                language: SourceLanguage::Cpp,
                construct: keyword.to_string(),
                line: 1,
            });
        }
    }
    Ok(())
}

fn parse_signature(
    signature: &str,
    language: SourceLanguage,
    line: usize,
) -> Result<(String, Vec<String>), TranspileError> {
    let open = signature
        .find('(')
        .ok_or_else(|| syntax(language, "missing parameter list", line))?;
    let close = signature
        .rfind(')')
        .ok_or_else(|| syntax(language, "missing parameter list close", line))?;
    let name = signature[..open].trim();
    if name.is_empty() {
        return Err(syntax(language, "missing function name", line));
    }
    Ok((
        name.to_string(),
        split_args(&signature[open + 1..close])
            .into_iter()
            .map(str::to_string)
            .collect(),
    ))
}

fn parse_cpp_header(header: &str) -> Result<(String, Vec<String>), TranspileError> {
    let open = header
        .find('(')
        .ok_or_else(|| cpp_syntax("missing parameter list", 1))?;
    let close = header
        .rfind(')')
        .ok_or_else(|| cpp_syntax("missing parameter list close", 1))?;
    let name = header[..open]
        .split_whitespace()
        .last()
        .ok_or_else(|| cpp_syntax("missing function name", 1))?;
    Ok((
        name.to_string(),
        split_args(&header[open + 1..close])
            .into_iter()
            .map(cpp_arg_name)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

fn split_args(args: &str) -> Vec<&str> {
    args.split(',')
        .map(str::trim)
        .filter(|arg| !arg.is_empty() && *arg != "void")
        .collect()
}

fn cpp_arg_name(arg: &str) -> Result<String, TranspileError> {
    arg.split_whitespace()
        .last()
        .map(|name| name.trim_start_matches('&').trim_start_matches('*').to_string())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| cpp_syntax("missing parameter name", 1))
}

fn parse_assignment(statement: &str) -> Option<(&str, &str)> {
    let (name, expr) = statement.split_once('=')?;
    if name.trim().is_empty() || expr.trim().is_empty() {
        return None;
    }
    Some((name.trim(), expr.trim().trim_end_matches(';')))
}

fn matching_delimiter(text: &str, open_index: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in text.char_indices().skip_while(|(index, _)| *index < open_index) {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn convert_expr(expr: &str) -> String {
    expr.replace("True", "true")
        .replace("False", "false")
        .replace(" and ", " && ")
        .replace(" or ", " || ")
}

fn strip_cpp_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(before, _)| before))
        .collect::<Vec<_>>()
        .join("\n")
}

fn py_syntax(message: &str, line: usize) -> TranspileError {
    syntax(SourceLanguage::Python, message, line)
}

fn cpp_syntax(message: &str, line: usize) -> TranspileError {
    syntax(SourceLanguage::Cpp, message, line)
}

fn syntax(language: SourceLanguage, message: &str, line: usize) -> TranspileError {
    TranspileError::Syntax {
        language,
        message: message.to_string(),
        line,
    }
}
