use std::collections::HashMap;

use anyhow::{Context, Result};

const BARE_SECTION_HEADINGS: &[&str] = &[
    "Instructions",
    "Initial Prompt",
    "Config",
    "Response Template",
    "Tools",
    "Workflow",
    "When to use",
    "Constraints",
    "Authentication setup",
    "Output",
    "Edge cases",
];

#[derive(Debug, Clone)]
pub struct ParsedMarkdownDoc {
    pub frontmatter: Option<serde_yaml::Value>,
    sections: HashMap<String, String>,
}

impl ParsedMarkdownDoc {
    pub fn parse(raw: &str, path_label: &str) -> Result<Self> {
        let (frontmatter_raw, body) = split_frontmatter(raw);
        let frontmatter = frontmatter_raw
            .map(|fm| {
                serde_yaml::from_str(fm)
                    .with_context(|| format!("failed to parse frontmatter in {path_label}"))
            })
            .transpose()?;

        Ok(Self {
            frontmatter,
            sections: parse_sections(&body),
        })
    }

    pub fn section(&self, heading: &str) -> Option<&str> {
        self.sections
            .get(&normalize_heading(heading))
            .map(String::as_str)
    }

    pub fn section_string(&self, heading: &str) -> String {
        self.section(heading).unwrap_or_default().trim().to_string()
    }
}

fn split_frontmatter(raw: &str) -> (Option<&str>, String) {
    if !raw.starts_with("---\n") {
        return (None, raw.to_string());
    }
    if let Some(end_pos) = raw[4..].find("\n---\n") {
        let fm = &raw[4..4 + end_pos];
        let body = raw[4 + end_pos + 5..].to_string();
        (Some(fm), body)
    } else {
        (None, raw.to_string())
    }
}

fn parse_sections(body: &str) -> HashMap<String, String> {
    let mut sections = HashMap::new();
    let mut current: Option<String> = None;
    let mut buffer = Vec::new();
    let mut in_fenced_block = false;

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fenced_block = !in_fenced_block;
        }

        if !in_fenced_block && let Some(heading) = detect_section_heading(line) {
            flush_section(&mut sections, current.take(), &buffer);
            buffer.clear();
            current = Some(heading);
            continue;
        }

        if current.is_some() {
            buffer.push(line.to_string());
        }
    }

    flush_section(&mut sections, current, &buffer);
    sections
}

fn flush_section(
    sections: &mut HashMap<String, String>,
    current: Option<String>,
    buffer: &[String],
) {
    let Some(heading) = current else {
        return;
    };
    sections
        .entry(normalize_heading(&heading))
        .or_insert_with(|| normalize_section_content(buffer));
}

fn detect_section_heading(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(markdown_heading) = trimmed.strip_prefix('#') {
        let heading = markdown_heading.trim_start_matches('#').trim();
        if heading.is_empty() {
            return None;
        }
        return recognized_section_heading(heading);
    }

    let bare = trimmed.strip_suffix(':')?.trim();
    recognized_section_heading(bare)
}

fn recognized_section_heading(value: &str) -> Option<String> {
    let heading = value.trim_end_matches(':').trim();
    if BARE_SECTION_HEADINGS
        .iter()
        .any(|candidate| normalize_heading(candidate) == normalize_heading(heading))
    {
        Some(heading.to_string())
    } else {
        None
    }
}

fn normalize_section_content(buffer: &[String]) -> String {
    let start = buffer
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(buffer.len());
    let end = buffer
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(start);
    let lines = &buffer[start..end];
    let common_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.chars()
                .take_while(|ch| *ch == ' ' || *ch == '\t')
                .count()
        })
        .min()
        .unwrap_or(0);

    lines
        .iter()
        .map(|line| strip_indent(line, common_indent))
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_indent(line: &str, indent: usize) -> String {
    let mut chars = line.chars();
    for _ in 0..indent {
        match chars.next() {
            Some(' ' | '\t') => {}
            Some(other) => {
                let rest = chars.collect::<String>();
                return format!("{other}{rest}");
            }
            None => return String::new(),
        }
    }
    chars.collect()
}

fn normalize_heading(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(':')
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::ParsedMarkdownDoc;

    #[test]
    fn parses_frontmatter_and_bare_sections() {
        let doc = ParsedMarkdownDoc::parse(
            r#"---
name: demo
---

Instructions:
Do the work.

Workflow:
type: guided

Response Template:
{{ response }}
"#,
            "demo",
        )
        .unwrap();

        assert_eq!(
            doc.frontmatter
                .as_ref()
                .and_then(|value| value.get("name"))
                .and_then(|value| value.as_str()),
            Some("demo")
        );
        assert_eq!(doc.section("Instructions"), Some("Do the work."));
        assert_eq!(doc.section("workflow"), Some("type: guided"));
        assert_eq!(doc.section("Response Template"), Some("{{ response }}"));
    }

    #[test]
    fn parses_markdown_headings_without_breaking_code_blocks() {
        let doc = ParsedMarkdownDoc::parse(
            r#"Tools:
  - slug: fetch
    script: scripts/fetch.py

## When to use

- Review recent messages.

```text
Instructions:
not a real section
```

## Constraints

- Stay grounded.
"#,
            "skill",
        )
        .unwrap();

        assert!(doc.section("Tools").unwrap().contains("slug: fetch"));
        assert!(
            doc.section("When to use")
                .unwrap()
                .contains("Review recent messages")
        );
        assert_eq!(doc.section("Instructions"), None);
        assert!(
            doc.section("Constraints")
                .unwrap()
                .contains("Stay grounded")
        );
    }

    #[test]
    fn keeps_markdown_headings_inside_section_content() {
        let doc = ParsedMarkdownDoc::parse(
            r#"Instructions:
Follow the recipe.

Response Template:
## {{ recipe_title }}

{{ response }}
"#,
            "recipe",
        )
        .unwrap();

        assert_eq!(
            doc.section("Response Template"),
            Some("## {{ recipe_title }}\n\n{{ response }}")
        );
    }
}
