//! Strict parsing of the model's output into a tool call.

use serde_json::{Map, Value};

/// A tool call as proposed by the model, not yet validated.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Map<String, Value>,
}

impl ToolCall {
    /// The call as one canonical string (used to spot a repeated call).
    pub fn key(&self) -> String {
        format!("{}@{}", self.name, Value::Object(self.arguments.clone()))
    }
}

/// Why the model output is not a usable tool call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// No `<tool_call>` block and not a JSON object.
    NoCall,
    /// More than one `<tool_call>` block.
    MultipleCalls,
    /// The block is not valid JSON.
    BadJson(String),
    /// The JSON is not `{"name": string, "arguments": object}` (extra keys are refused).
    BadShape(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::NoCall => write!(f, "the model output contains no tool call"),
            ParseError::MultipleCalls => write!(f, "more than one tool call in one reply"),
            ParseError::BadJson(e) => write!(f, "the tool call is not valid JSON: {e}"),
            ParseError::BadShape(e) => write!(f, "the tool call has the wrong shape: {e}"),
        }
    }
}

const OPEN: &str = "<tool_call>";
const CLOSE: &str = "</tool_call>";
/// The longest model reply that is looked at.
const MAX_OUTPUT: usize = 16 * 1024;

const PY_OPEN: &str = "<function_calls>";
const PY_CLOSE: &str = "</function_calls>";

/// One call in Olmo's own form, `name(key=json, key=json)`, inside `<function_calls>`. Values are JSON (Python's True, False and None are read too);
/// more than one call is refused like more than one `<tool_call>`.
fn parse_function_calls(output: &str) -> Result<ToolCall, ParseError> {
    let start = output.find(PY_OPEN).map(|i| i + PY_OPEN.len()).ok_or(ParseError::NoCall)?;
    let rest = output.get(start..).ok_or(ParseError::NoCall)?;
    let body = rest.find(PY_CLOSE).and_then(|e| rest.get(..e)).unwrap_or(rest).trim();
    if output.matches(PY_OPEN).count() > 1 {
        return Err(ParseError::MultipleCalls);
    }
    let open = body.find('(').ok_or_else(|| ParseError::BadShape("no `(` in the call".into()))?;
    let name = body[..open].trim();
    if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(ParseError::BadShape("`name` must be a short identifier".into()));
    }
    let inner = body[open + 1..].strip_suffix(')').ok_or_else(|| ParseError::BadShape("the call is not closed with `)`".into()))?;
    let mut arguments = Map::new();
    for part in split_top_level(inner) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (key, value) = part.split_once('=').ok_or_else(|| ParseError::BadShape(format!("`{part}` is not key=value")))?;
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(ParseError::BadShape(format!("bad argument name `{key}`")));
        }
        let v = value.trim();
        let json = match v {
            "True" => "true".to_string(),
            "False" => "false".to_string(),
            "None" => "null".to_string(),
            _ if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') => Value::String(v[1..v.len() - 1].replace("\\'", "'")).to_string(),
            _ => v.to_string(),
        };
        let parsed: Value = serde_json::from_str(&json).map_err(|e| ParseError::BadJson(format!("{key}: {e}")))?;
        arguments.insert(key.to_string(), parsed);
    }
    Ok(ToolCall { name: name.to_string(), arguments })
}

/// Split at commas that are outside strings and brackets.
fn split_top_level(s: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut quote, mut esc, mut from) = (Vec::new(), 0i32, None::<char>, false, 0usize);
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[from..]);
    parts
}

/// Extract the one tool call of a model reply: either a single `<tool_call>{…}</tool_call>` block
/// (Qwen's format) or a reply that is nothing but a JSON object. Text that merely resembles a call
/// is not executed.
pub fn parse_tool_call(output: &str) -> Result<ToolCall, ParseError> {
    if output.len() > MAX_OUTPUT {
        return Err(ParseError::BadShape("reply too long".into()));
    }
    // Olmo 3 is trained on `<function_calls>name(arg=json, ...)</function_calls>`; it usually follows the prompt's JSON form, but not always.
    if output.contains(PY_OPEN) && !output.contains(OPEN) {
        return parse_function_calls(output);
    }
    let json = match output.matches(OPEN).count() {
        0 => {
            let t = output.trim();
            if t.starts_with('{') && t.ends_with('}') { t } else { return Err(ParseError::NoCall) }
        }
        1 => {
            let start = output.find(OPEN).map(|i| i + OPEN.len()).ok_or(ParseError::NoCall)?;
            let rest = output.get(start..).ok_or(ParseError::NoCall)?;
            let end = rest.find(CLOSE).ok_or_else(|| ParseError::BadShape("unterminated <tool_call>".into()))?;
            rest.get(..end).ok_or(ParseError::NoCall)?.trim()
        }
        _ => return Err(ParseError::MultipleCalls),
    };
    let v: Value = serde_json::from_str(json).map_err(|e| ParseError::BadJson(e.to_string()))?;
    let Value::Object(mut o) = v else { return Err(ParseError::BadShape("not an object".into())) };
    if let Some(k) = o.keys().find(|k| *k != "name" && *k != "arguments") {
        return Err(ParseError::BadShape(format!("unexpected key `{k}`")));
    }
    let name = match o.remove("name") {
        Some(Value::String(n)) if !n.is_empty() && n.len() <= 64 => n,
        _ => return Err(ParseError::BadShape("`name` must be a short string".into())),
    };
    let arguments = match o.remove("arguments") {
        None => Map::new(),
        Some(Value::Object(a)) => a,
        Some(_) => return Err(ParseError::BadShape("`arguments` must be an object".into())),
    };
    Ok(ToolCall { name, arguments })
}

#[cfg(test)]
mod olmo_tests {
    use super::*;

    #[test]
    fn olmos_own_form_is_read() {
        let c = parse_tool_call(
            "<function_calls>find_objects_in_window(x1=0, y1=0, x2=100.5, y2=50, layers=[\"A,B\", \"C\"], deep=True)</function_calls>",
        )
        .unwrap();
        assert_eq!(c.name, "find_objects_in_window");
        assert_eq!(c.arguments["x2"], serde_json::json!(100.5));
        assert_eq!(c.arguments["layers"], serde_json::json!(["A,B", "C"]));
        assert_eq!(c.arguments["deep"], serde_json::json!(true));
    }

    #[test]
    fn a_call_without_arguments_and_a_missing_close_tag_are_read() {
        assert_eq!(parse_tool_call("<function_calls>audit_drawing()</function_calls>").unwrap().name, "audit_drawing");
        assert_eq!(parse_tool_call("<function_calls>audit_drawing()").unwrap().name, "audit_drawing");
    }

    #[test]
    fn malformed_or_repeated_calls_are_refused() {
        assert!(parse_tool_call("<function_calls>audit_drawing(</function_calls>").is_err());
        assert!(parse_tool_call("<function_calls>a b()</function_calls>").is_err());
        assert!(parse_tool_call("<function_calls>x(k)</function_calls>").is_err());
        assert!(matches!(
            parse_tool_call("<function_calls>a()</function_calls><function_calls>b()</function_calls>"),
            Err(ParseError::MultipleCalls)
        ));
    }
}
