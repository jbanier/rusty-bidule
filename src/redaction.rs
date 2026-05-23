use serde_json::Value;

const REDACTED: &str = "[REDACTED]";

pub fn redact_value(value: &Value) -> Value {
    redact_value_at_key(None, value)
}

pub fn redact_tool_output(value: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<Value>(value)
        && let Ok(redacted) = serde_json::to_string_pretty(&redact_value(&parsed))
    {
        return redacted;
    }

    redact_http_text(value)
}

pub fn redact_text(value: &str) -> String {
    redact_tokens_preserving_whitespace(value)
}

fn redact_http_text(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            let Some((name, raw_value)) = line.split_once(':') else {
                return redact_text(line);
            };
            if is_sensitive_key(name.trim()) {
                format!("{}: {}", name, redact_sensitive_text(raw_value.trim()))
            } else {
                redact_text(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_tokens_preserving_whitespace(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut token = String::new();
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !token.is_empty() {
                out.push_str(&redact_token_preserving_punctuation(&token));
                token.clear();
            }
            out.push(ch);
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        out.push_str(&redact_token_preserving_punctuation(&token));
    }
    out
}

fn redact_token_preserving_punctuation(token: &str) -> String {
    let start = token
        .char_indices()
        .find_map(|(idx, ch)| (!is_token_boundary(ch)).then_some(idx))
        .unwrap_or(token.len());
    let end = token
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| (!is_token_boundary(ch)).then_some(idx + ch.len_utf8()))
        .unwrap_or(start);
    let prefix = &token[..start];
    let core = &token[start..end];
    let suffix = &token[end..];
    if looks_like_secret_token(core) {
        format!("{prefix}{}{suffix}", redacted_token_shape(core))
    } else {
        token.to_string()
    }
}

fn is_token_boundary(ch: char) -> bool {
    matches!(ch, '"' | '\'' | ',' | ';' | ')' | ']' | '}' | '(' | '[' | '{')
}

fn redact_value_at_key(key: Option<&str>, value: &Value) -> Value {
    if key.is_some_and(is_sensitive_key) {
        return match value {
            Value::String(text) => Value::String(redact_sensitive_text(text)),
            _ => Value::String(REDACTED.to_string()),
        };
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

fn redact_sensitive_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return REDACTED.to_string();
    }
    if let Some((prefix, token)) = trimmed.split_once(' ')
        && prefix.eq_ignore_ascii_case("bearer")
    {
        return format!("{prefix} {}", redacted_token_shape(token));
    }
    redacted_token_shape(trimmed)
}

fn redacted_token_shape(value: &str) -> String {
    let kind = if value.split('.').count() >= 3 {
        "JWT"
    } else if value.contains('=') && value.contains(';') {
        "COOKIE"
    } else {
        "SECRET"
    };
    format!("[REDACTED-{kind} len={}]", value.chars().count())
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

        assert_eq!(redacted["api_key"], "[REDACTED-SECRET len=6]");
        assert_eq!(
            redacted["nested"]["Authorization"],
            "Bearer [REDACTED-SECRET len=12]"
        );
        assert_eq!(redacted["safe"], "value");
    }

    #[test]
    fn redacts_long_secret_like_tokens_in_text() {
        let text = redact_text("Bearer abcdefghijklmnop1234567890");

        assert_eq!(text, "Bearer [REDACTED-SECRET len=26]");
    }

    #[test]
    fn redacts_raw_http_evidence_headers() {
        let text = super::redact_tool_output(
            "GET / HTTP/1.1\nAuthorization: Bearer header.payload.signature\nCookie: sid=abcdef1234567890abcdef1234567890; path=/\nX-Test: ok",
        );

        assert!(text.contains("Authorization: Bearer [REDACTED-JWT len=24]"));
        assert!(text.contains("Cookie: [REDACTED-COOKIE"));
        assert!(text.contains("X-Test: ok"));
    }
}
