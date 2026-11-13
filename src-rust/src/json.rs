//! Stdin JSON reading and field extraction.

use serde_json::Value;
use std::io::Read;

/// Read and parse stdin once. Invalid JSON yields `Value::Null`, so every field reads as empty.
pub fn read_stdin_json() -> Value {
    let mut buf = Vec::new();
    let _ = std::io::stdin().lock().read_to_end(&mut buf);
    serde_json::from_str(&String::from_utf8_lossy(&buf)).unwrap_or_default()
}

/// A string field, or "" if missing or not a string.
pub fn extract_string(json: &Value, key: &str) -> String {
    json.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// The first question text from an AskUserQuestion tool's tool_input. Schema:
/// `{ "tool_input": { "questions": [ { "question": "...", "header": "..." }, ... ] } }`
pub fn extract_first_question(json: &Value) -> String {
    json.get("tool_input")
        .and_then(|ti| ti.get("questions"))
        .and_then(|qs| qs.get(0))
        .and_then(|q| q.get("question").or_else(|| q.get("header")))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Value {
        serde_json::from_str(s).unwrap_or_default()
    }

    #[test]
    fn extract_string_returns_empty_for_missing_non_string_or_bad_json() {
        let j = p(r#"{"session_id":"abc","n":1}"#);
        assert_eq!(extract_string(&j, "session_id"), "abc");
        assert_eq!(extract_string(&j, "n"), "");
        assert_eq!(extract_string(&j, "missing"), "");
        assert_eq!(extract_string(&p("not json"), "session_id"), "");
    }

    #[test]
    fn first_question_falls_back_to_header() {
        let q = p(r#"{"tool_input":{"questions":[{"question":"Q1?","header":"H"},{"question":"Q2"}]}}"#);
        assert_eq!(extract_first_question(&q), "Q1?");
        let h = p(r#"{"tool_input":{"questions":[{"header":"H"}]}}"#);
        assert_eq!(extract_first_question(&h), "H");
        assert_eq!(extract_first_question(&p(r#"{"tool_input":{}}"#)), "");
    }
}
