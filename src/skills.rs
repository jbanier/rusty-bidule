use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tracing::{debug, warn};

use crate::types::FilesystemAccess;

#[derive(Debug, Clone)]
pub struct SkillTool {
    pub name: Option<String>,
    pub slug: String,
    pub description: Option<String>,
    pub script: Option<String>,
    pub server: Option<String>,
    pub requires_network: bool,
    pub filesystem: FilesystemAccess,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub tools: Vec<SkillTool>,
    pub skill_dir: PathBuf,
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
        out.push_str("Execution rules:\n");
        out.push_str(
            "- If a skill tool has a local `script`, execute it with `local__run_skill`.\n",
        );
        out.push_str("- Use the skill directory name for `skill_name` when shown below, plus the tool `slug` as `tool_slug`.\n");
        out.push_str("- Pass `parameters` as a JSON string of CLI-style arguments.\n");
        out.push_str(
            "- Do not claim a listed script-backed skill is unavailable because of MCP.\n",
        );
        out.push_str("- A tool with `server` but no `script` is MCP-backed metadata only.\n\n");
        for skill in &self.skills {
            let skill_name = skill_lookup_name(skill);
            if skill_name == skill.name {
                out.push_str(&format!("### {}\n", skill.name));
            } else {
                out.push_str(&format!("### {} (`{skill_name}`)\n", skill.name));
            }
            out.push_str(&format!("{}\n", skill.description));
            if !skill.tools.is_empty() {
                out.push_str("Tools:\n");
                for tool in &skill.tools {
                    let display_name = tool.name.as_deref().unwrap_or(&tool.slug);
                    let desc = tool
                        .description
                        .as_deref()
                        .unwrap_or("No description provided.");
                    match (tool.script.as_deref(), tool.server.as_deref()) {
                        (Some(_), _) => {
                            let mut requirements = Vec::new();
                            if tool.requires_network {
                                requirements.push("network");
                            }
                            if !matches!(tool.filesystem, FilesystemAccess::None) {
                                requirements.push(match tool.filesystem {
                                    FilesystemAccess::ReadOnly => "filesystem:read",
                                    FilesystemAccess::ReadWrite => "filesystem:write",
                                    FilesystemAccess::None => unreachable!(),
                                });
                            }
                            let requirement_note = if requirements.is_empty() {
                                String::new()
                            } else {
                                format!(" Requires {}.", requirements.join(" + "))
                            };
                            out.push_str(&format!(
                                "- `{}`: {} Use `local__run_skill` with `skill_name=\"{}\"` and `tool_slug=\"{}\"`.{}\n",
                                display_name, desc, skill_name, tool.slug, requirement_note
                            ));
                        }
                        (None, Some(server)) => {
                            out.push_str(&format!(
                                "- `{}`: {} MCP-backed via server `{server}`; not locally executable.\n",
                                display_name, desc
                            ));
                        }
                        (None, None) => {
                            out.push_str(&format!(
                                "- `{}`: {} Metadata only; no execution backend declared.\n",
                                display_name, desc
                            ));
                        }
                    }
                }
            }
            out.push('\n');
        }
        out
    }

    pub fn find_skill(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| {
            skill.skill_dir.file_name().and_then(|n| n.to_str()) == Some(name) || skill.name == name
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
}

fn skill_lookup_name(skill: &Skill) -> &str {
    skill
        .skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(skill.name.as_str())
}

fn parse_skill_md(path: &Path, skill_dir: PathBuf) -> Result<Skill> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;

    let (frontmatter, body) = split_frontmatter(&raw);

    let name;
    let mut description = String::new();

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
        tools,
        skill_dir,
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

fn parse_tools_block(body: &str) -> Vec<SkillTool> {
    // Find "Tools:" section
    let tools_start = if let Some(pos) = body.find("Tools:\n") {
        pos + "Tools:\n".len()
    } else {
        return Vec::new();
    };

    // Extract the tool block only. Stop at the next markdown heading or
    // obvious non-tool prose line, but still allow the documented shorthand
    // forms such as `slug: script.py` at column 0.
    let tools_text = &body[tools_start..];
    let tools_section: String = tools_text
        .lines()
        .take_while(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return true;
            }

            if line.starts_with('#') {
                return false;
            }

            if line.starts_with(' ') || line.starts_with('\t') || trimmed.starts_with("- ") {
                return true;
            }

            trimmed.contains(':') && !trimmed.ends_with(':')
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Try to parse as YAML list
    let yaml_attempt = format!("tools:\n{}", tools_section);
    if let Ok(yaml_val) = serde_yaml::from_str::<serde_yaml::Value>(&yaml_attempt)
        && let Some(serde_yaml::Value::Sequence(seq)) = yaml_val.get("tools")
    {
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
                        requires_network: map
                            .get("network")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        filesystem: map
                            .get("filesystem")
                            .and_then(|v| v.as_str())
                            .and_then(parse_filesystem_access)
                            .unwrap_or(FilesystemAccess::None),
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
                        requires_network: false,
                        filesystem: FilesystemAccess::None,
                    });
                }
                _ => {}
            }
        }
        if !tools.is_empty() {
            return tools;
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
                    requires_network: false,
                    filesystem: FilesystemAccess::None,
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
                requires_network: false,
                filesystem: FilesystemAccess::None,
            });
        }
    }
    tools
}

fn parse_filesystem_access(value: &str) -> Option<FilesystemAccess> {
    match value {
        "none" => Some(FilesystemAccess::None),
        "read" | "read_only" | "readonly" => Some(FilesystemAccess::ReadOnly),
        "write" | "read_write" | "readwrite" => Some(FilesystemAccess::ReadWrite),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::types::FilesystemAccess;

    use super::{Skill, SkillRegistry, SkillTool, parse_tools_block};

    #[test]
    fn parses_yaml_tool_list_before_markdown_heading() {
        let body = "\
Tools:
  - name: Fetch Webex Room Messages
    slug: webex_room_message_fetch
    description: Fetch all messages from a named Webex room.
    script: scripts/webex_room_message_fetch.py

## When to use

- Build incident timelines
";

        let tools = parse_tools_block(body);

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name.as_deref(), Some("Fetch Webex Room Messages"));
        assert_eq!(tools[0].slug, "webex_room_message_fetch");
        assert_eq!(
            tools[0].script.as_deref(),
            Some("scripts/webex_room_message_fetch.py")
        );
        assert!(!tools[0].requires_network);
        assert_eq!(tools[0].filesystem, FilesystemAccess::None);
    }

    #[test]
    fn keeps_column_zero_colon_shorthand() {
        let body = "\
Tools:
retrieve_emails: scripts/retrieve_emails.py

## Output
";

        let tools = parse_tools_block(body);

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].slug, "retrieve_emails");
        assert_eq!(
            tools[0].script.as_deref(),
            Some("scripts/retrieve_emails.py")
        );
    }

    #[test]
    fn parses_tool_permissions_from_yaml() {
        let body = "\
Tools:
  - name: Fetch Webex Room Messages
    slug: webex_room_message_fetch
    script: scripts/webex_room_message_fetch.py
    network: true
    filesystem: read_only
";

        let tools = parse_tools_block(body);

        assert_eq!(tools.len(), 1);
        assert!(tools[0].requires_network);
        assert_eq!(tools[0].filesystem, FilesystemAccess::ReadOnly);
    }

    #[test]
    fn capability_summary_includes_local_skill_invocation_details() {
        let registry = SkillRegistry {
            skills: vec![Skill {
                name: "Webex Room Conversation".to_string(),
                description: "Fetch Webex room messages.".to_string(),
                tools: vec![SkillTool {
                    name: Some("Fetch Webex Room Messages".to_string()),
                    slug: "webex_room_message_fetch".to_string(),
                    description: Some("Fetch all room messages for a date interval.".to_string()),
                    script: Some("scripts/webex_room_message_fetch.py".to_string()),
                    server: None,
                    requires_network: true,
                    filesystem: FilesystemAccess::ReadOnly,
                }],
                skill_dir: PathBuf::from("skills/webex-room-conversation"),
            }],
        };

        let summary = registry.capability_summary();

        assert!(summary.contains("local__run_skill"));
        assert!(summary.contains("skill_name=\"webex-room-conversation\""));
        assert!(summary.contains("tool_slug=\"webex_room_message_fetch\""));
        assert!(summary.contains("Requires network + filesystem:read"));
        assert!(summary.contains("Do not claim a listed script-backed skill"));
    }
}
