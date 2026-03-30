use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct SkillTool {
    pub name: Option<String>,
    pub slug: String,
    pub description: Option<String>,
    pub script: Option<String>,
    pub server: Option<String>,
    pub tool: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub compatibility: Option<String>,
    pub tools: Vec<SkillTool>,
    pub skill_dir: PathBuf,
    pub body: String,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    skills: Vec<Skill>,
}

impl SkillRegistry {
    pub fn load(skills_dir: &Path) -> Result<Self> {
        if !skills_dir.exists() {
            debug!(path = %skills_dir.display(), "skills directory not found; using empty registry");
            return Ok(Self::default());
        }

        let mut skills = Vec::new();
        for entry in fs::read_dir(skills_dir)
            .with_context(|| format!("failed to read skills dir {}", skills_dir.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let skill_md = entry.path().join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }
            match parse_skill_md(&skill_md, entry.path()) {
                Ok(skill) => {
                    debug!(name = %skill.name, "loaded skill");
                    skills.push(skill);
                }
                Err(err) => {
                    warn!(path = %skill_md.display(), error = %err, "failed to parse SKILL.md; skipping");
                }
            }
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self { skills })
    }

    pub fn capability_summary(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let mut out = String::from("## Available Skills\n\n");
        for skill in &self.skills {
            out.push_str(&format!("### {}\n", skill.name));
            out.push_str(&format!("{}\n", skill.description));
            if !skill.tools.is_empty() {
                out.push_str("Tools:\n");
                for tool in &skill.tools {
                    let display_name = tool.name.as_deref().unwrap_or(&tool.slug);
                    let desc = tool.description.as_deref().unwrap_or("");
                    out.push_str(&format!("- `{}`: {}\n", display_name, desc));
                }
            }
            out.push('\n');
        }
        out
    }

    pub fn find_skill(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| {
            skill.skill_dir.file_name().and_then(|n| n.to_str()) == Some(name)
                || skill.name == name
        })
    }

    pub fn find_tool<'a>(
        &'a self,
        skill_name: &str,
        tool_slug: Option<&str>,
    ) -> Option<(&'a Skill, &'a SkillTool)> {
        let skill = self.find_skill(skill_name)?;
        let tool = if let Some(slug) = tool_slug {
            skill.tools.iter().find(|t| t.slug == slug)?
        } else {
            skill.tools.first()?
        };
        Some((skill, tool))
    }

    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }
}

fn parse_skill_md(path: &Path, skill_dir: PathBuf) -> Result<Skill> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let (frontmatter, body) = split_frontmatter(&raw);

    let name;
    let mut description = String::new();
    let mut keywords = Vec::new();
    let mut compatibility = None;

    if let Some(fm) = frontmatter {
        let yaml: serde_yaml::Value = serde_yaml::from_str(fm)
            .with_context(|| format!("failed to parse frontmatter in {}", path.display()))?;
        name = yaml
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                skill_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
            })
            .to_string();
        description = yaml
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        keywords = parse_keywords(&yaml);
        compatibility = yaml
            .get("compatibility")
            .and_then(|v| v.as_str())
            .map(str::to_string);
    } else {
        name = skill_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
    }

    let tools = parse_tools_block(&body);

    Ok(Skill {
        name,
        description,
        keywords,
        compatibility,
        tools,
        skill_dir,
        body,
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

fn parse_tools_block(body: &str) -> Vec<SkillTool> {
    // Find "Tools:" section
    let tools_start = if let Some(pos) = body.find("Tools:\n") {
        pos + "Tools:\n".len()
    } else {
        return Vec::new();
    };

    // Extract content until next section heading (line starting with non-whitespace followed by ':')
    let tools_text = &body[tools_start..];
    let tools_section: String = tools_text
        .lines()
        .take_while(|line| {
            // Stop at a line that looks like a new section heading (word followed by colon, no leading whitespace)
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return true;
            }
            !(!line.starts_with(' ') && !line.starts_with('\t') && !line.starts_with('-')
                && line.contains(':')
                && !line.starts_with("  ")
                && line.len() < 50
                && &body[..tools_start] != tools_text)
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Try to parse as YAML list
    let yaml_attempt = format!("tools:\n{}", tools_section);
    if let Ok(yaml_val) = serde_yaml::from_str::<serde_yaml::Value>(&yaml_attempt) {
        if let Some(serde_yaml::Value::Sequence(seq)) = yaml_val.get("tools") {
            let mut tools = Vec::new();
            for item in seq {
                match item {
                    serde_yaml::Value::Mapping(map) => {
                        let slug = map
                            .get("slug")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let tool = SkillTool {
                            name: map.get("name").and_then(|v| v.as_str()).map(str::to_string),
                            slug,
                            description: map
                                .get("description")
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                            script: map
                                .get("script")
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                            server: map
                                .get("server")
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                            tool: map
                                .get("tool")
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                        };
                        tools.push(tool);
                    }
                    serde_yaml::Value::String(s) => {
                        // dash shorthand: - path/to/script.py
                        let slug = std::path::Path::new(s)
                            .file_stem()
                            .and_then(|n| n.to_str())
                            .unwrap_or(s)
                            .to_string();
                        tools.push(SkillTool {
                            name: None,
                            slug,
                            description: None,
                            script: Some(s.clone()),
                            server: None,
                            tool: None,
                        });
                    }
                    _ => {}
                }
            }
            if !tools.is_empty() {
                return tools;
            }
        }
    }

    // Parse line-by-line as shorthand
    let mut tools = Vec::new();
    for line in tools_section.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // colon shorthand: slug: path/to/script.py
        if let Some((slug, script)) = trimmed.split_once(':') {
            let slug = slug.trim().to_string();
            let script = script.trim().to_string();
            if !slug.is_empty() && !script.is_empty() {
                tools.push(SkillTool {
                    name: None,
                    slug,
                    description: None,
                    script: Some(script),
                    server: None,
                    tool: None,
                });
            }
        }
        // dash shorthand: - path/to/script.py
        else if let Some(script) = trimmed.strip_prefix("- ") {
            let slug = std::path::Path::new(script)
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or(script)
                .to_string();
            tools.push(SkillTool {
                name: None,
                slug,
                description: None,
                script: Some(script.to_string()),
                server: None,
                tool: None,
            });
        }
    }
    tools
}
