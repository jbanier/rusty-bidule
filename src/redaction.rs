use serde_json::Value;

const REDACTED: &str = "[REDACTED]";

pub fn redact_value(value: &Value) -> Value {
    redact_value_at_key(None, value)
}

pub fn redact_text(value: &str) -> String {
    let mut out = Vec::new();
    for token in value.split_whitespace() {
        if token.eq_ignore_ascii_case("bearer") {
            out.push(token.to_string());
            continue;
        }
        if looks_like_secret_token(token) {
            out.push(REDACTED.to_string());
        } else {
            out.push(token.to_string());
        }
    }
    out.join(" ")
}

fn redact_value_at_key(key: Option<&str>, value: &Value) -> Value {
    if key.is_some_and(is_sensitive_key) {
        return Value::String(REDACTED.to_string());
    }

    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), redact_value_at_key(Some(key), value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|value| redact_value_at_key(None, value))
                .collect(),
        ),
        Value::String(text) => Value::String(redact_text(text)),
        other => other.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.contains("token")
        || normalized.contains("secret")
        || normalized.contains("password")
        || normalized.contains("passwd")
        || normalized.contains("api_key")
        || normalized.contains("apikey")
        || normalized.contains("authorization")
        || normalized.contains("cookie")
        || normalized.contains("credential")
}

fn looks_like_secret_token(value: &str) -> bool {
    let trimmed = value.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | ',' | ';' | ')' | ']' | '}' | '(' | '[' | '{'
        )
    });
    if trimmed.len() < 24 {
        return false;
    }
    let has_alpha = trimmed.chars().any(|ch| ch.is_ascii_alphabetic());
    let has_digit = trimmed.chars().any(|ch| ch.is_ascii_digit());
    let mostly_token_chars = trimmed
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '=' | ':'))
        .count()
        >= trimmed.chars().count().saturating_sub(2);
    has_alpha && has_digit && mostly_token_chars
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{redact_text, redact_value};

    #[test]
    fn redacts_sensitive_object_keys() {
        let redacted = redact_value(&json!({
            "api_key": "abc123",
            "nested": {"Authorization": "Bearer secret-token"},
            "safe": "value"
        }));

        assert_eq!(redacted["api_key"], "[REDACTED]");
        assert_eq!(redacted["nested"]["Authorization"], "[REDACTED]");
        assert_eq!(redacted["safe"], "value");
    }

    #[test]
    fn redacts_long_secret_like_tokens_in_text() {
        let text = redact_text("Bearer abcdefghijklmnop1234567890");

        assert_eq!(text, "Bearer [REDACTED]");
    }
}
