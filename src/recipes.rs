use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tracing::{debug, warn};

use crate::doc_sections::ParsedMarkdownDoc;

#[derive(Debug, Clone)]
pub struct Recipe {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub instructions: String,
    pub initial_prompt: Option<String>,
    pub config_mcp_servers: Option<Vec<String>>,
    pub config_local_tools: Option<Vec<String>>,
    pub config_max_agent_iterations: Option<usize>,
    pub config_continuation_increment: Option<usize>,
    pub workflow: Option<String>,
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

    pub fn prompt_guidance(&self) -> String {
        self.prompt_guidance_with_workflow(true)
    }

    pub fn prompt_guidance_without_workflow(&self) -> String {
        self.prompt_guidance_with_workflow(false)
    }

    fn prompt_guidance_with_workflow(&self, include_workflow: bool) -> String {
        let mut parts = Vec::new();
        if !self.instructions.trim().is_empty() {
            parts.push(format!("Instructions:\n{}", self.instructions.trim()));
        }
        if include_workflow
            && let Some(workflow) = self.workflow.as_deref().map(str::trim)
            && !workflow.is_empty()
        {
            parts.push(format!(
                "Workflow guidance:\n{workflow}\n\nTreat this workflow as model guidance. Do not claim it was executed unless you used the relevant tools and grounded the result in their output."
            ));
        }
        parts.join("\n\n")
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
    let doc = ParsedMarkdownDoc::parse(&raw, &path.display().to_string())?;

    let name;
    let mut title = None;
    let mut description = None;
    let mut keywords = Vec::new();

    if let Some(yaml) = doc.frontmatter.as_ref() {
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
        keywords = parse_keywords(yaml);
    } else {
        name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
    }

    let instructions = doc.section_string("Instructions");
    let initial_prompt = non_empty_section(&doc, "Initial Prompt");
    let response_template = non_empty_section(&doc, "Response Template");
    let workflow = non_empty_section(&doc, "Workflow");

    let config = parse_config_section(&doc);

    Ok(Recipe {
        name,
        title,
        description,
        keywords,
        instructions,
        initial_prompt,
        config_mcp_servers: config.mcp_servers,
        config_local_tools: config.local_tools,
        config_max_agent_iterations: config.max_agent_iterations,
        config_continuation_increment: config.continuation_increment,
        workflow,
        response_template,
    })
}

fn non_empty_section(doc: &ParsedMarkdownDoc, heading: &str) -> Option<String> {
    let value = doc.section_string(heading);
    if value.is_empty() { None } else { Some(value) }
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

#[derive(Debug, Default)]
struct RecipeConfig {
    mcp_servers: Option<Vec<String>>,
    local_tools: Option<Vec<String>>,
    max_agent_iterations: Option<usize>,
    continuation_increment: Option<usize>,
}

fn parse_config_section(doc: &ParsedMarkdownDoc) -> RecipeConfig {
    let Some(config_text) = doc.section("Config") else {
        return RecipeConfig::default();
    };

    let yaml: serde_yaml::Value = match serde_yaml::from_str(config_text) {
        Ok(v) => v,
        Err(_) => return RecipeConfig::default(),
    };

    RecipeConfig {
        mcp_servers: yaml_string_list(yaml.get("mcp_servers")),
        local_tools: yaml_string_list(yaml.get("local_tools")),
        max_agent_iterations: yaml_usize(yaml.get("max_agent_iterations")),
        continuation_increment: yaml_usize(yaml.get("continuation_increment")),
    }
}

fn yaml_string_list(value: Option<&serde_yaml::Value>) -> Option<Vec<String>> {
    match value {
        Some(serde_yaml::Value::Sequence(seq)) => Some(
            seq.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
        ),
        Some(serde_yaml::Value::String(s)) => Some(
            s.split([',', '\n'])
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect(),
        ),
        _ => None,
    }
}

fn yaml_usize(value: Option<&serde_yaml::Value>) -> Option<usize> {
    match value {
        Some(serde_yaml::Value::Number(number)) => number.as_u64().map(|value| value as usize),
        Some(serde_yaml::Value::String(value)) => value.trim().parse::<usize>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::workflows::parse_workflow_definition;

    use super::{RecipeRegistry, parse_recipe_md};

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

    #[test]
    fn keeps_markdown_heading_response_template() {
        let dir = tempdir().unwrap();
        let recipe_dir = dir.path().join("demo");
        std::fs::create_dir(&recipe_dir).unwrap();
        let recipe_path = recipe_dir.join("RECIPE.md");
        std::fs::write(
            &recipe_path,
            r#"---
name: demo
title: Demo Recipe
---

Instructions:
Follow the recipe.

Response Template:
## {{ recipe_title }}

{{ response }}
"#,
        )
        .unwrap();

        let recipe = parse_recipe_md(&recipe_path, recipe_dir).unwrap();

        assert_eq!(
            recipe.response_template.as_deref(),
            Some("## {{ recipe_title }}\n\n{{ response }}")
        );
        assert_eq!(recipe.apply_template("Done."), "## Demo Recipe\n\nDone.");
    }

    #[test]
    fn parses_config_and_workflow_sections() {
        let dir = tempdir().unwrap();
        let recipe_dir = dir.path().join("demo");
        std::fs::create_dir(&recipe_dir).unwrap();
        let recipe_path = recipe_dir.join("RECIPE.md");
        std::fs::write(
            &recipe_path,
            r#"---
name: demo
keywords: morning, handover
---

Instructions:
Summarize the shift.

Config:
  local_tools:
    - local__time
    - local__run_skill
  mcp_servers: csirt, splunk
  max_agent_iterations: 20
  continuation_increment: 7

Workflow:
  type: guided_collection
"#,
        )
        .unwrap();

        let recipe = parse_recipe_md(&recipe_path, recipe_dir).unwrap();

        assert_eq!(
            recipe.config_local_tools,
            Some(vec![
                "local__time".to_string(),
                "local__run_skill".to_string()
            ])
        );
        assert_eq!(
            recipe.config_mcp_servers,
            Some(vec!["csirt".to_string(), "splunk".to_string()])
        );
        assert_eq!(recipe.config_max_agent_iterations, Some(20));
        assert_eq!(recipe.config_continuation_increment, Some(7));
        assert!(recipe.prompt_guidance().contains("Workflow guidance"));
    }

    #[test]
    fn bundled_web_assessment_recipes_load_as_supervised_workflows() {
        let recipes_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes");
        let registry = RecipeRegistry::load(&recipes_dir).unwrap();
        let expected = [
            "web-app-scope-intake",
            "web-app-passive-recon",
            "web-app-active-baseline",
            "web-app-auth-session",
            "web-app-access-control",
            "web-app-input-validation",
            "web-app-api-graphql-websocket",
            "web-app-files-cache-host",
            "web-app-business-logic-race",
            "web-app-cms-wordpress",
            "web-app-final-report",
        ];

        for name in expected {
            let recipe = registry
                .find(name)
                .unwrap_or_else(|| panic!("missing {name}"));
            let local_tools = recipe
                .config_local_tools
                .as_ref()
                .unwrap_or_else(|| panic!("{name} missing local tool filter"));
            assert!(local_tools.contains(&"local__activate_skill".to_string()));
            assert!(local_tools.contains(&"local__run_skill".to_string()));

            assert!(
                recipe.config_max_agent_iterations.is_some(),
                "{name} missing recipe iteration budget"
            );
            assert!(
                recipe.config_continuation_increment.is_some(),
                "{name} missing continuation increment"
            );

            let workflow_raw = recipe
                .workflow
                .as_deref()
                .unwrap_or_else(|| panic!("{name} missing workflow"));
            let workflow = parse_workflow_definition(workflow_raw)
                .unwrap_or_else(|| panic!("{name} workflow should be parseable"));
            assert_eq!(workflow.workflow_type, "supervised_steps");
            assert!(!workflow.steps.is_empty(), "{name} workflow has no steps");
            for step in workflow.steps {
                assert!(
                    step.name.as_deref().is_some_and(|value| !value.is_empty()),
                    "{name} workflow step missing name"
                );
                assert!(
                    step.prompt
                        .as_deref()
                        .is_some_and(|value| !value.trim().is_empty()),
                    "{name} workflow step missing prompt"
                );
                assert!(
                    step.local_tools
                        .as_ref()
                        .is_some_and(|tools| !tools.is_empty()),
                    "{name} workflow step missing local tool filter"
                );
            }
        }
    }
}
