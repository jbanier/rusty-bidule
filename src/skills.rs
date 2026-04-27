use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use tracing::{debug, warn};

use crate::{doc_sections::ParsedMarkdownDoc, types::FilesystemAccess};

#[derive(Debug, Clone)]
pub struct SkillTool {
    pub name: Option<String>,
    pub slug: String,
    pub description: Option<String>,
    pub script: Option<String>,
    pub server: Option<String>,
    pub mcp_backed: bool,
    pub requires_network: bool,
    pub filesystem: FilesystemAccess,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub tools: Vec<SkillTool>,
    pub skill_dir: PathBuf,
    pub skill_md: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    skills: Vec<Skill>,
}

#[derive(Debug, Clone)]
struct SkillSearchDir {
    path: PathBuf,
    label: &'static str,
}

const MAX_LISTED_SKILL_RESOURCES: usize = 200;
const MAX_SKILL_RESOURCE_DEPTH: usize = 6;

impl SkillRegistry {
    pub fn load(skills_dir: &Path) -> Result<Self> {
        Self::load_from_search_dirs(&[SkillSearchDir {
            path: skills_dir.to_path_buf(),
            label: "explicit",
        }])
    }

    pub fn load_all(project_root: &Path) -> Result<Self> {
        Self::load_from_search_dirs(&default_skill_search_dirs(project_root))
    }

    fn load_from_search_dirs(search_dirs: &[SkillSearchDir]) -> Result<Self> {
        let mut by_name: BTreeMap<String, Skill> = BTreeMap::new();

        for search_dir in search_dirs {
            let dir_skills = match load_skills_from_dir(&search_dir.path) {
                Ok(skills) => skills,
                Err(err) if search_dirs.len() == 1 => return Err(err),
                Err(err) => {
                    warn!(
                        path = %search_dir.path.display(),
                        source = search_dir.label,
                        error = %err,
                        "failed to read skills directory; skipping"
                    );
                    continue;
                }
            };

            for skill in dir_skills {
                if let Some(shadowed) = by_name.insert(skill.name.clone(), skill.clone()) {
                    warn!(
                        name = %skill.name,
                        source = search_dir.label,
                        selected = %skill.skill_md.display(),
                        shadowed = %shadowed.skill_md.display(),
                        "skill name collision; later search path takes precedence"
                    );
                }
            }
        }

        let skills = by_name.into_values().collect();
        Ok(Self { skills })
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn skill_names(&self) -> Vec<String> {
        self.skills.iter().map(|skill| skill.name.clone()).collect()
    }

    pub fn activate_skill(&self, name: &str) -> Result<String> {
        let skill = self
            .find_skill(name)
            .or_else(|| self.find_skill_fuzzy(name))
            .ok_or_else(|| anyhow!("skill '{name}' not found"))?;
        let raw = fs::read_to_string(&skill.skill_md)
            .with_context(|| format!("failed to read {}", skill.skill_md.display()))?;
        let doc = ParsedMarkdownDoc::parse(&raw, &skill.skill_md.display().to_string())?;
        let (resources, truncated) = list_skill_resources(&skill.skill_dir)?;

        let mut out = format!(
            "<skill_content name=\"{}\">\n",
            escape_xml_attr(&skill.name)
        );
        out.push_str(doc.body.trim());
        out.push_str("\n\nSkill directory: ");
        out.push_str(&skill.skill_dir.display().to_string());
        out.push_str("\nSKILL.md location: ");
        out.push_str(&skill.skill_md.display().to_string());
        out.push_str("\nRelative paths in this skill are relative to the skill directory.\n");
        if resources.is_empty() {
            out.push_str("<skill_resources />\n");
        } else {
            out.push_str("<skill_resources>\n");
            for resource in resources {
                out.push_str("  <file>");
                out.push_str(&escape_xml_text(&resource));
                out.push_str("</file>\n");
            }
            if truncated {
                out.push_str("  <truncated>true</truncated>\n");
            }
            out.push_str("</skill_resources>\n");
        }
        out.push_str("</skill_content>");
        Ok(out)
    }
}

fn load_skills_from_dir(skills_dir: &Path) -> Result<Vec<Skill>> {
    if !skills_dir.exists() {
        debug!(path = %skills_dir.display(), "skills directory not found; using empty registry");
        return Ok(Vec::new());
    }
    if !skills_dir.is_dir() {
        warn!(path = %skills_dir.display(), "skills path is not a directory; skipping");
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    let mut entries = fs::read_dir(skills_dir)
        .with_context(|| format!("failed to read skills dir {}", skills_dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| {
            format!(
                "failed to read entry in skills dir {}",
                skills_dir.display()
            )
        })?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if !entry.path().is_dir() {
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
    Ok(skills)
}

fn default_skill_search_dirs(project_root: &Path) -> Vec<SkillSearchDir> {
    let mut dirs = Vec::new();
    if let Some(home) = home_dir() {
        dirs.push(SkillSearchDir {
            path: home.join(".claude").join("skills"),
            label: "user-claude",
        });
        dirs.push(SkillSearchDir {
            path: home.join(".rusty-bidule").join("skills"),
            label: "user-native",
        });
        dirs.push(SkillSearchDir {
            path: home.join(".agents").join("skills"),
            label: "user-agents",
        });
    }
    dirs.push(SkillSearchDir {
        path: project_root.join("skills"),
        label: "project-legacy",
    });
    dirs.push(SkillSearchDir {
        path: project_root.join(".claude").join("skills"),
        label: "project-claude",
    });
    dirs.push(SkillSearchDir {
        path: project_root.join(".rusty-bidule").join("skills"),
        label: "project-native",
    });
    dirs.push(SkillSearchDir {
        path: project_root.join(".agents").join("skills"),
        label: "project-agents",
    });
    dirs
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

impl SkillRegistry {
    pub fn capability_summary(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let mut out = String::from("## Available Skills\n\n");
        out.push_str("The following entries follow the Agent Skills `SKILL.md` format. When a task matches a skill description, call `local__activate_skill` with the skill `name` to load the full instructions before acting. The activation result lists bundled resources; load only the referenced files you need.\n\n");
        out.push_str("Execution rules:\n");
        out.push_str(
            "- Use `local__activate_skill` for progressive disclosure of a skill's `SKILL.md` body.\n",
        );
        out.push_str(
            "- If a skill tool has a local `script`, execute it with `local__run_skill`.\n",
        );
        out.push_str("- Use the skill `name` or directory name for `skill_name`, plus the tool `slug` as `tool_slug`.\n");
        out.push_str("- Pass `parameters` as a JSON string of CLI-style arguments.\n");
        out.push_str("- Local skill execution defaults to a 180s timeout unless overridden by config or `timeout_seconds`.\n");
        out.push_str("- A script may return JSON like `{\"status\":\"pending\",\"job\":{...}}` to store a long-running remote job for follow-up.\n");
        out.push_str(
            "- Do not claim a listed script-backed skill is unavailable because of MCP.\n",
        );
        out.push_str("- A tool with `mcp: true` but no `script` is MCP-backed metadata only.\n\n");
        for skill in &self.skills {
            let skill_name = skill_lookup_name(skill);
            if skill_name == skill.name {
                out.push_str(&format!("### {}\n", skill.name));
            } else {
                out.push_str(&format!("### {} (`{skill_name}`)\n", skill.name));
            }
            out.push_str(&format!("{}\n", skill.description));
            out.push_str(&format!("Location: {}\n", skill.skill_md.display()));
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
                        (None, None) if tool.mcp_backed => {
                            out.push_str(&format!(
                                "- `{}`: {} MCP-backed; not locally executable.\n",
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

    pub fn find_skill_fuzzy(&self, name: &str) -> Option<&Skill> {
        let needle = normalize_lookup(name);
        self.skills.iter().find(|skill| {
            normalize_lookup(&skill.name).contains(&needle)
                || normalize_lookup(skill_lookup_name(skill)).contains(&needle)
        })
    }

    pub fn find_tools<'a>(
        &'a self,
        skill_name: &str,
        tool_slug: Option<&str>,
    ) -> Option<(&'a Skill, Vec<&'a SkillTool>)> {
        let skill = self
            .find_skill(skill_name)
            .or_else(|| self.find_skill_fuzzy(skill_name))?;
        let tools = if let Some(slug) = tool_slug {
            vec![find_tool_fuzzy(skill, slug)?]
        } else {
            skill.tools.iter().collect::<Vec<_>>()
        };
        Some((skill, tools))
    }
}

fn skill_lookup_name(skill: &Skill) -> &str {
    skill
        .skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(skill.name.as_str())
}

fn normalize_lookup(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn looks_like_script_reference(value: &str) -> bool {
    let value = value.trim();
    value.contains('/')
        || value.ends_with(".py")
        || value.ends_with(".sh")
        || value.ends_with(".js")
        || value.ends_with(".rb")
}

fn find_tool_fuzzy<'a>(skill: &'a Skill, tool_slug: &str) -> Option<&'a SkillTool> {
    if let Some(tool) = skill.tools.iter().find(|tool| tool.slug == tool_slug) {
        return Some(tool);
    }

    let needle = normalize_lookup(tool_slug);
    if needle.is_empty() {
        return None;
    }

    if let Some(tool) = skill.tools.iter().find(|tool| {
        normalize_lookup(&tool.slug) == needle
            || tool
                .name
                .as_deref()
                .is_some_and(|name| normalize_lookup(name) == needle)
    }) {
        return Some(tool);
    }

    let partial_matches = skill
        .tools
        .iter()
        .filter(|tool| {
            normalize_lookup(&tool.slug).contains(&needle)
                || needle.contains(&normalize_lookup(&tool.slug))
                || tool.name.as_deref().is_some_and(|name| {
                    let normalized = normalize_lookup(name);
                    normalized.contains(&needle) || needle.contains(&normalized)
                })
        })
        .collect::<Vec<_>>();

    match partial_matches.as_slice() {
        [tool] => Some(*tool),
        _ => None,
    }
}

fn parse_skill_md(path: &Path, skill_dir: PathBuf) -> Result<Skill> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let doc = ParsedMarkdownDoc::parse(&raw, &path.display().to_string())?;

    let skill_dir = fs::canonicalize(&skill_dir).unwrap_or(skill_dir);
    let skill_md = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let parent_name = skill_dir.file_name().and_then(|n| n.to_str());
    let yaml = doc
        .frontmatter
        .as_ref()
        .ok_or_else(|| anyhow!("SKILL.md missing required YAML frontmatter"))?;

    let name = frontmatter_string(yaml, "name")
        .map(str::to_string)
        .unwrap_or_else(|| {
            warn!(
                path = %skill_md.display(),
                "SKILL.md missing required name; falling back to parent directory name"
            );
            parent_name.unwrap_or("unknown").to_string()
        });
    let description = frontmatter_string(yaml, "description")
        .map(str::to_string)
        .ok_or_else(|| anyhow!("SKILL.md missing required non-empty description"))?;

    validate_skill_metadata(&name, &description, yaml, parent_name, &skill_md);

    let tools = doc
        .section("Tools")
        .map(parse_tools_section)
        .unwrap_or_default();

    Ok(Skill {
        name,
        description,
        tools,
        skill_dir,
        skill_md,
    })
}

fn frontmatter_string<'a>(yaml: &'a serde_yaml::Value, key: &str) -> Option<&'a str> {
    yaml.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn validate_skill_metadata(
    name: &str,
    description: &str,
    yaml: &serde_yaml::Value,
    parent_name: Option<&str>,
    skill_md: &Path,
) {
    if name.chars().count() > 64 {
        warn!(
            path = %skill_md.display(),
            name,
            "skill name exceeds Agent Skills 64 character limit; loading anyway"
        );
    }
    if !is_agent_skill_name(name) {
        warn!(
            path = %skill_md.display(),
            name,
            "skill name does not match Agent Skills lowercase-hyphen naming rules; loading anyway"
        );
    }
    if parent_name.is_some_and(|parent| parent != name) {
        warn!(
            path = %skill_md.display(),
            name,
            parent_name,
            "skill name does not match parent directory; loading anyway"
        );
    }
    if description.chars().count() > 1024 {
        warn!(
            path = %skill_md.display(),
            name,
            "skill description exceeds Agent Skills 1024 character limit; loading anyway"
        );
    }
    if let Some(compatibility) = frontmatter_string(yaml, "compatibility")
        && compatibility.chars().count() > 500
    {
        warn!(
            path = %skill_md.display(),
            name,
            "skill compatibility exceeds Agent Skills 500 character limit; loading anyway"
        );
    }
}

fn is_agent_skill_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn list_skill_resources(skill_dir: &Path) -> Result<(Vec<String>, bool)> {
    let mut resources = Vec::new();
    let mut truncated = false;
    collect_skill_resources(skill_dir, skill_dir, 0, &mut resources, &mut truncated)?;
    resources.sort();
    Ok((resources, truncated))
}

fn collect_skill_resources(
    root: &Path,
    current: &Path,
    depth: usize,
    resources: &mut Vec<String>,
    truncated: &mut bool,
) -> Result<()> {
    if resources.len() >= MAX_LISTED_SKILL_RESOURCES || depth > MAX_SKILL_RESOURCE_DEPTH {
        *truncated = true;
        return Ok(());
    }

    let mut entries = match fs::read_dir(current) {
        Ok(entries) => entries.collect::<std::io::Result<Vec<_>>>()?,
        Err(err) if current == root => return Err(err.into()),
        Err(err) => {
            warn!(path = %current.display(), error = %err, "failed to list skill resource directory");
            return Ok(());
        }
    };
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if resources.len() >= MAX_LISTED_SKILL_RESOURCES {
            *truncated = true;
            break;
        }
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if path.is_dir() {
            if should_skip_resource_dir(&file_name) {
                continue;
            }
            collect_skill_resources(root, &path, depth + 1, resources, truncated)?;
            continue;
        }
        if !path.is_file() || file_name == "SKILL.md" {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .with_context(|| format!("failed to relativize {}", path.display()))?;
        resources.push(path_to_slash_string(rel));
    }
    Ok(())
}

fn should_skip_resource_dir(name: &str) -> bool {
    matches!(name, ".git" | "node_modules" | "target")
}

fn path_to_slash_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn escape_xml_attr(value: &str) -> String {
    escape_xml_text(value).replace('"', "&quot;")
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
fn parse_tools_block(body: &str) -> Vec<SkillTool> {
    ParsedMarkdownDoc::parse(body, "skill tools block")
        .ok()
        .and_then(|doc| doc.section("Tools").map(parse_tools_section))
        .unwrap_or_default()
}

fn parse_tools_section(tools_section: &str) -> Vec<SkillTool> {
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
                            .or_else(|| map.get("mcp_server"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        mcp_backed: map
                            .get("mcp")
                            .or_else(|| map.get("mcp_backed"))
                            .and_then(|v| v.as_bool())
                            .unwrap_or_else(|| {
                                map.get("server").is_some() || map.get("mcp_server").is_some()
                            }),
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
                        mcp_backed: false,
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
            if !slug.is_empty()
                && !script.is_empty()
                && !slug.starts_with('-')
                && looks_like_script_reference(&script)
            {
                tools.push(SkillTool {
                    name: None,
                    slug,
                    description: None,
                    script: Some(script),
                    server: None,
                    mcp_backed: false,
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
                mcp_backed: false,
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
    use std::{fs, path::PathBuf};

    use crate::types::FilesystemAccess;
    use tempfile::tempdir;

    use super::{
        Skill, SkillRegistry, SkillSearchDir, SkillTool, find_tool_fuzzy,
        looks_like_script_reference, parse_tools_block,
    };

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
    fn parses_generic_mcp_backed_tool_metadata() {
        let body = "\
Tools:
  - name: Submit Splunk Search
    slug: submit-search
    mcp: true
    description: Submit a Splunk query through an advertised MCP tool.
";

        let tools = parse_tools_block(body);

        assert_eq!(tools.len(), 1);
        assert!(tools[0].mcp_backed);
        assert_eq!(tools[0].server, None);
    }

    #[test]
    fn fallback_parser_does_not_treat_yaml_fields_as_scripts() {
        let body = "\
Tools:
  - name: Fetch Webex Room Messages
    slug: webex_room_message_fetch
    description: Fetch all messages from a named Webex room.
    script: scripts/webex_room_message_fetch.py
";

        let tools = parse_tools_block(body);

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].slug, "webex_room_message_fetch");
        assert_eq!(
            tools[0].script.as_deref(),
            Some("scripts/webex_room_message_fetch.py")
        );
    }

    #[test]
    fn identifies_script_like_shorthand_targets() {
        assert!(looks_like_script_reference("scripts/tool.py"));
        assert!(looks_like_script_reference("tool.sh"));
        assert!(!looks_like_script_reference("Fetch Webex Room Messages"));
        assert!(!looks_like_script_reference("webex_room_message_fetch"));
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
                    mcp_backed: false,
                    requires_network: true,
                    filesystem: FilesystemAccess::ReadOnly,
                }],
                skill_dir: PathBuf::from("skills/webex-room-conversation"),
                skill_md: PathBuf::from("skills/webex-room-conversation/SKILL.md"),
            }],
        };

        let summary = registry.capability_summary();

        assert!(summary.contains("local__activate_skill"));
        assert!(summary.contains("local__run_skill"));
        assert!(summary.contains("skill_name=\"webex-room-conversation\""));
        assert!(summary.contains("Location: skills/webex-room-conversation/SKILL.md"));
        assert!(summary.contains("tool_slug=\"webex_room_message_fetch\""));
        assert!(summary.contains("Requires network + filesystem:read"));
        assert!(summary.contains("Do not claim a listed script-backed skill"));
    }

    #[test]
    fn capability_summary_describes_generic_mcp_backed_tools_without_server_binding() {
        let registry = SkillRegistry {
            skills: vec![Skill {
                name: "splunk".to_string(),
                description: "Investigate Splunk through MCP.".to_string(),
                tools: vec![SkillTool {
                    name: Some("Submit Splunk Search".to_string()),
                    slug: "submit-search".to_string(),
                    description: Some("Submit a Splunk query.".to_string()),
                    script: None,
                    server: None,
                    mcp_backed: true,
                    requires_network: false,
                    filesystem: FilesystemAccess::None,
                }],
                skill_dir: PathBuf::from("skills/splunk"),
                skill_md: PathBuf::from("skills/splunk/SKILL.md"),
            }],
        };

        let summary = registry.capability_summary();

        assert!(summary.contains("MCP-backed; not locally executable."));
        assert!(!summary.contains("via server"));
    }

    #[test]
    fn fuzzy_tool_lookup_matches_webex_fetch_shorthand() {
        let skill = Skill {
            name: "webex-room-conversation".to_string(),
            description: "Fetch Webex room messages.".to_string(),
            tools: vec![SkillTool {
                name: Some("Fetch Webex Room Messages".to_string()),
                slug: "webex_room_message_fetch".to_string(),
                description: Some("Fetch all room messages for a date interval.".to_string()),
                script: Some("scripts/webex_room_message_fetch.py".to_string()),
                server: None,
                mcp_backed: false,
                requires_network: true,
                filesystem: FilesystemAccess::ReadOnly,
            }],
            skill_dir: PathBuf::from("skills/webex-room-conversation"),
            skill_md: PathBuf::from("skills/webex-room-conversation/SKILL.md"),
        };

        let tool = find_tool_fuzzy(&skill, "fetch").unwrap();
        assert_eq!(tool.slug, "webex_room_message_fetch");
    }

    #[test]
    fn load_from_search_dirs_uses_later_paths_for_name_collisions() {
        let dir = tempdir().unwrap();
        let user_skills = dir.path().join("user/.agents/skills");
        let project_skills = dir.path().join("project/.agents/skills");
        fs::create_dir_all(user_skills.join("demo")).unwrap();
        fs::create_dir_all(project_skills.join("demo")).unwrap();
        fs::write(
            user_skills.join("demo/SKILL.md"),
            r#"---
name: demo
description: User-level demo skill.
---

# User Demo
"#,
        )
        .unwrap();
        fs::write(
            project_skills.join("demo/SKILL.md"),
            r#"---
name: demo
description: Project-level demo skill.
---

# Project Demo
"#,
        )
        .unwrap();

        let registry = SkillRegistry::load_from_search_dirs(&[
            SkillSearchDir {
                path: user_skills,
                label: "user",
            },
            SkillSearchDir {
                path: project_skills,
                label: "project",
            },
        ])
        .unwrap();

        let skill = registry.find_skill("demo").unwrap();
        assert_eq!(skill.description, "Project-level demo skill.");
        assert!(skill.skill_md.is_absolute());
    }

    #[test]
    fn load_skips_skills_without_required_description() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        fs::create_dir_all(skills_dir.join("missing-description")).unwrap();
        fs::write(
            skills_dir.join("missing-description/SKILL.md"),
            r#"---
name: missing-description
---

# Missing Description
"#,
        )
        .unwrap();

        let registry = SkillRegistry::load(&skills_dir).unwrap();

        assert!(registry.is_empty());
    }

    #[test]
    fn activate_skill_returns_body_and_resource_listing() {
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        let skill_dir = skills_dir.join("demo");
        fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        fs::write(skill_dir.join("scripts/run.py"), "print('ok')\n").unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: demo
description: Demo activation skill.
---

# Demo

Run `scripts/run.py`.
"#,
        )
        .unwrap();

        let registry = SkillRegistry::load(&skills_dir).unwrap();
        let activated = registry.activate_skill("demo").unwrap();

        assert!(activated.contains("<skill_content name=\"demo\">"));
        assert!(activated.contains("# Demo"));
        assert!(activated.contains("Skill directory:"));
        assert!(activated.contains("<file>scripts/run.py</file>"));
        assert!(!activated.contains("description: Demo activation skill"));
    }
}
