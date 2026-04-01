use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct Recipe {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub instructions: String,
    pub initial_prompt: Option<String>,
    pub config_mcp_servers: Option<Vec<String>>,
    pub response_template: Option<String>,
}

impl Recipe {
    pub fn apply_template(&self, response: &str) -> String {
        if let Some(template) = &self.response_template {
            let title = self.title.as_deref().unwrap_or("");
            template
                .replace("{{ recipe_title }}", title)
                .replace("{{ response }}", response)
        } else {
            response.to_string()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecipeRegistry {
    recipes: Vec<Recipe>,
}

impl RecipeRegistry {
    pub fn load(recipes_dir: &Path) -> Result<Self> {
        if !recipes_dir.exists() {
            debug!(path = %recipes_dir.display(), "recipes directory not found; using empty registry");
            return Ok(Self::default());
        }

        let mut recipes = Vec::new();
        for entry in fs::read_dir(recipes_dir)
            .with_context(|| format!("failed to read recipes dir {}", recipes_dir.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let recipe_md = entry.path().join("RECIPE.md");
            if !recipe_md.exists() {
                continue;
            }
            match parse_recipe_md(&recipe_md, entry.path()) {
                Ok(recipe) => {
                    debug!(name = %recipe.name, "loaded recipe");
                    recipes.push(recipe);
                }
                Err(err) => {
                    warn!(path = %recipe_md.display(), error = %err, "failed to parse RECIPE.md; skipping");
                }
            }
        }
        recipes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self { recipes })
    }

    pub fn list(&self) -> &[Recipe] {
        &self.recipes
    }

    pub fn find(&self, name: &str) -> Option<&Recipe> {
        self.recipes.iter().find(|r| r.name == name)
    }
}

fn parse_recipe_md(path: &Path, _recipe_dir: PathBuf) -> Result<Recipe> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;

    let (frontmatter, body) = split_frontmatter(&raw);

    let name;
    let mut title = None;
    let mut description = None;
    let mut keywords = Vec::new();

    if let Some(fm) = frontmatter {
        let yaml: serde_yaml::Value = serde_yaml::from_str(fm)
            .with_context(|| format!("failed to parse frontmatter in {}", path.display()))?;
        name = yaml
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        title = yaml
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        description = yaml
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        keywords = parse_keywords(&yaml);
    } else {
        name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
    }

    let instructions = extract_section(&body, "Instructions:");
    let initial_prompt = {
        let s = extract_section(&body, "Initial Prompt:");
        if s.is_empty() { None } else { Some(s) }
    };
    let response_template = {
        let s = extract_section(&body, "Response Template:");
        if s.is_empty() { None } else { Some(s) }
    };

    let config_mcp_servers = parse_config_section(&body);

    Ok(Recipe {
        name,
        title,
        description,
        keywords,
        instructions,
        initial_prompt,
        config_mcp_servers,
        response_template,
    })
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

fn parse_keywords(yaml: &serde_yaml::Value) -> Vec<String> {
    match yaml.get("keywords") {
        Some(serde_yaml::Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(serde_yaml::Value::String(s)) => s
            .split([',', ' ', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract the content under a section heading until the next section.
/// Section headings are lines like "Instructions:", "Initial Prompt:", etc.
fn extract_section(body: &str, heading: &str) -> String {
    let section_headings = [
        "Instructions:",
        "Initial Prompt:",
        "Config:",
        "Response Template:",
    ];

    let start = if let Some(pos) = body.find(heading) {
        pos + heading.len()
    } else {
        return String::new();
    };

    let remaining = &body[start..];
    // Find next section heading
    let end = section_headings
        .iter()
        .filter(|&&h| h != heading)
        .filter_map(|h| {
            // Find heading that occurs after start in remaining
            remaining.find(h)
        })
        .min()
        .unwrap_or(remaining.len());

    remaining[..end].trim().to_string()
}

fn parse_config_section(body: &str) -> Option<Vec<String>> {
    let config_text = extract_section(body, "Config:");
    if config_text.is_empty() {
        return None;
    }

    let yaml: serde_yaml::Value = match serde_yaml::from_str(&config_text) {
        Ok(v) => v,
        Err(_) => return None,
    };

    yaml.get("mcp_servers")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::parse_recipe_md;

    #[test]
    fn parses_initial_prompt_section() {
        let dir = tempdir().unwrap();
        let recipe_dir = dir.path().join("demo");
        std::fs::create_dir(&recipe_dir).unwrap();
        let recipe_path = recipe_dir.join("RECIPE.md");
        std::fs::write(
            &recipe_path,
            r#"---
name: demo
---

Instructions:
Follow the recipe.

Initial Prompt:
Draft this first.

Response Template:
{{ response }}
"#,
        )
        .unwrap();

        let recipe = parse_recipe_md(&recipe_path, recipe_dir).unwrap();

        assert_eq!(recipe.initial_prompt.as_deref(), Some("Draft this first."));
        assert_eq!(recipe.instructions, "Follow the recipe.");
    }
}
