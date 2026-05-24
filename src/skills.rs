use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

use crate::{
    config::SkillsConfig,
    doc_sections::ParsedMarkdownDoc,
    types::{ActivatedSkill, FilesystemAccess},
};

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
    pub safety_profile: Option<String>,
    pub requires_active_authorization: bool,
    pub requires_oob_authorization: bool,
    pub requires_destructive_authorization: bool,
    pub methodology: Vec<String>,
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
    #[cfg(test)]
    pub fn load(skills_dir: &Path) -> Result<Self> {
        Self::load_from_search_dirs(&[SkillSearchDir {
            path: skills_dir.to_path_buf(),
            label: "explicit",
        }])
    }

    pub fn load_all(project_root: &Path, config: &SkillsConfig) -> Result<Self> {
        Self::load_from_search_dirs(&default_skill_search_dirs(project_root, config))
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

    #[cfg(test)]
    pub fn activate_skill(&self, name: &str) -> Result<String> {
        Ok(self.activate_skill_record(name)?.content)
    }

    pub fn activate_skill_record(&self, name: &str) -> Result<ActivatedSkill> {
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
        let content_hash = sha256_hex(&out);
        Ok(ActivatedSkill {
            name: skill.name.clone(),
            skill_dir: skill.skill_dir.display().to_string(),
            skill_md: skill.skill_md.display().to_string(),
            content_hash,
            activated_at: Utc::now(),
            content: out,
        })
    }
}

fn sha256_hex(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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

fn default_skill_search_dirs(project_root: &Path, config: &SkillsConfig) -> Vec<SkillSearchDir> {
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
    if config.allows_project_skill_dirs(project_root) {
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
    } else {
        debug!(
            project_root = %project_root.display(),
            policy = ?config.project_skills,
            "project skill directories are not trusted; skipping project-level .agents/.claude/.rusty-bidule skills"
        );
    }
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
                    let metadata_note = tool_metadata_note(tool);
                    match (tool.script.as_deref(), tool.server.as_deref()) {
                        (Some(_), _) => {
                            let mut requirements = Vec::new();
                            if tool.requires_network {
                                requirements.push("network");
                            }
                            if tool.requires_active_authorization {
                                requirements.push("active authorization");
                            }
                            if tool.requires_oob_authorization {
                                requirements.push("OOB authorization");
                            }
                            if tool.requires_destructive_authorization {
                                requirements.push("destructive authorization");
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
                                "- `{}`: {} Use `local__run_skill` with `skill_name=\"{}\"` and `tool_slug=\"{}\"`.{}{}\n",
                                display_name, desc, skill_name, tool.slug, requirement_note, metadata_note
                            ));
                        }
                        (None, Some(server)) => {
                            out.push_str(&format!(
                                "- `{}`: {} MCP-backed via server `{server}`; not locally executable.{}\n",
                                display_name, desc, metadata_note
                            ));
                        }
                        (None, None) if tool.mcp_backed => {
                            out.push_str(&format!(
                                "- `{}`: {} MCP-backed; not locally executable.{}\n",
                                display_name, desc, metadata_note
                            ));
                        }
                        (None, None) => {
                            out.push_str(&format!(
                                "- `{}`: {} Metadata only; no execution backend declared.{}\n",
                                display_name, desc, metadata_note
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

fn tool_metadata_note(tool: &SkillTool) -> String {
    let mut notes = Vec::new();
    if let Some(safety_profile) = tool.safety_profile.as_deref() {
        notes.push(format!("safety={safety_profile}"));
    }
    if !tool.methodology.is_empty() {
        notes.push(format!("methodology={}", tool.methodology.join(",")));
    }
    if notes.is_empty() {
        String::new()
    } else {
        format!(" Metadata: {}.", notes.join("; "))
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
                        safety_profile: map
                            .get("safety_profile")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        requires_active_authorization: yaml_bool(map.get("requires_active_authorization")),
                        requires_oob_authorization: yaml_bool(map.get("requires_oob_authorization")),
                        requires_destructive_authorization: yaml_bool(
                            map.get("requires_destructive_authorization"),
                        ),
                        methodology: yaml_string_list(map.get("methodology")),
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
                        safety_profile: None,
                        requires_active_authorization: false,
                        requires_oob_authorization: false,
                        requires_destructive_authorization: false,
                        methodology: Vec::new(),
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
                    safety_profile: None,
                    requires_active_authorization: false,
                    requires_oob_authorization: false,
                    requires_destructive_authorization: false,
                    methodology: Vec::new(),
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
                safety_profile: None,
                requires_active_authorization: false,
                requires_oob_authorization: false,
                requires_destructive_authorization: false,
                methodology: Vec::new(),
            });
        }
    }
    tools
}

fn yaml_bool(value: Option<&serde_yaml::Value>) -> bool {
    value.and_then(|v| v.as_bool()).unwrap_or(false)
}

fn yaml_string_list(value: Option<&serde_yaml::Value>) -> Vec<String> {
    match value {
        Some(serde_yaml::Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(str::trim))
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(serde_yaml::Value::String(s)) => s
            .split([',', '\n'])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
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
    use std::{fs, path::PathBuf, process::Command};

    use crate::{
        config::{ProjectSkillsPolicy, SkillsConfig},
        types::FilesystemAccess,
    };
    use serde_json::Value;
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
                    safety_profile: None,
                    requires_active_authorization: false,
                    requires_oob_authorization: false,
                    requires_destructive_authorization: false,
                    methodology: Vec::new(),
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
                    safety_profile: None,
                    requires_active_authorization: false,
                    requires_oob_authorization: false,
                    requires_destructive_authorization: false,
                    methodology: Vec::new(),
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
                safety_profile: None,
                requires_active_authorization: false,
                requires_oob_authorization: false,
                requires_destructive_authorization: false,
                methodology: Vec::new(),
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
    fn load_all_skips_project_skill_dirs_by_default() {
        let dir = tempdir().unwrap();
        let project_root = dir.path();
        fs::create_dir_all(project_root.join("skills/legacy-demo")).unwrap();
        fs::create_dir_all(project_root.join(".agents/skills/project-agent-demo")).unwrap();
        fs::write(
            project_root.join("skills/legacy-demo/SKILL.md"),
            r#"---
name: legacy-demo
description: Bundled legacy skill.
---

# Legacy
"#,
        )
        .unwrap();
        fs::write(
            project_root.join(".agents/skills/project-agent-demo/SKILL.md"),
            r#"---
name: project-agent-demo
description: Project Agent Skill.
---

# Project Agent
"#,
        )
        .unwrap();

        let registry = SkillRegistry::load_all(project_root, &SkillsConfig::default()).unwrap();

        assert!(registry.find_skill("legacy-demo").is_some());
        assert!(registry.find_skill("project-agent-demo").is_none());
    }

    #[test]
    fn load_all_includes_trusted_project_skill_dirs() {
        let dir = tempdir().unwrap();
        let project_root = dir.path();
        fs::create_dir_all(project_root.join(".agents/skills/project-agent-demo")).unwrap();
        fs::write(
            project_root.join(".agents/skills/project-agent-demo/SKILL.md"),
            r#"---
name: project-agent-demo
description: Project Agent Skill.
---

# Project Agent
"#,
        )
        .unwrap();
        let config = SkillsConfig {
            project_skills: ProjectSkillsPolicy::TrustedOnly,
            trusted_project_roots: vec![project_root.to_path_buf()],
        };

        let registry = SkillRegistry::load_all(project_root, &config).unwrap();

        assert!(registry.find_skill("project-agent-demo").is_some());
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

    #[test]
    fn bundled_web_assessment_skills_load_with_script_tools() {
        let skills_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills");
        let registry = SkillRegistry::load(&skills_dir).unwrap();
        let expected = [
            "web-scope-guard",
            "web-http-baseline",
            "web-crawler-inventory",
            "web-discovery-recon",
            "web-scanner-safe",
            "web-auth-session-auditor",
            "web-access-control-matrix",
            "web-input-probe",
            "web-api-graphql",
            "web-websocket",
            "web-upload-content",
            "web-evidence-report",
            "web-engagement-state",
            "web-coverage-status",
            "web-finding-validator",
            "web-burp-mcp-review",
            "web-browser-evidence",
            "web-scanner-result-normalizer",
            "web-js-route-extractor",
            "web-payload-catalog",
            "web-ai-feature-review",
            "web-client-side-audit",
            "web-crypto-posture",
            "web-dependency-sca",
            "web-error-handling-review",
        ];

        for name in expected {
            let skill = registry
                .find_skill(name)
                .unwrap_or_else(|| panic!("missing {name}"));
            assert!(
                skill.tools.iter().any(|tool| tool.script.is_some()),
                "{name} should expose at least one script-backed tool"
            );
        }
    }

    #[test]
    fn bundled_web_assessment_scripts_fail_closed_and_emit_json() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let scripts: Vec<(&str, Vec<&str>)> = vec![
            (
                "skills/web-scope-guard/scripts/scope_guard.py",
                vec![
                    "--target-urls",
                    "https://example.com",
                    "--allowed-hosts",
                    "example.com",
                ],
            ),
            (
                "skills/web-http-baseline/scripts/http_baseline.py",
                vec![
                    "--target-url",
                    "https://example.com",
                    "--allowed-hosts",
                    "example.com",
                ],
            ),
            (
                "skills/web-crawler-inventory/scripts/crawler_inventory.py",
                vec![
                    "--seed-url",
                    "https://example.com",
                    "--allowed-hosts",
                    "example.com",
                ],
            ),
            (
                "skills/web-discovery-recon/scripts/discovery_recon.py",
                vec![
                    "--target-url",
                    "https://example.com",
                    "--allowed-hosts",
                    "example.com",
                ],
            ),
            (
                "skills/web-scanner-safe/scripts/scanner_safe.py",
                vec![
                    "--target-url",
                    "https://example.com",
                    "--allowed-hosts",
                    "example.com",
                ],
            ),
            (
                "skills/web-auth-session-auditor/scripts/auth_session_auditor.py",
                vec!["--set-cookie", "sid=abc; Secure; HttpOnly; SameSite=Lax"],
            ),
            (
                "skills/web-access-control-matrix/scripts/access_control_matrix.py",
                vec![
                    "--observations-json",
                    r#"[{"role":"user","method":"GET","path":"/orders/1","object_id":"1","status":200,"expected":true}]"#,
                ],
            ),
            (
                "skills/web-input-probe/scripts/input_probe.py",
                vec!["--parameters", "q,id"],
            ),
            (
                "skills/web-api-graphql/scripts/api_graphql_review.py",
                vec![
                    "--target-url",
                    "https://example.com",
                    "--allowed-hosts",
                    "example.com",
                ],
            ),
            (
                "skills/web-websocket/scripts/websocket_review.py",
                vec![
                    "--websocket-url",
                    "wss://example.com/socket",
                    "--allowed-hosts",
                    "example.com",
                ],
            ),
            (
                "skills/web-upload-content/scripts/upload_content_review.py",
                vec!["--upload-endpoints", "/upload"],
            ),
            (
                "skills/web-evidence-report/scripts/evidence_report.py",
                vec![
                    "--findings-json",
                    r#"[{"title":"Missing HSTS","severity":"low","confirmed":true}]"#,
                ],
            ),
            (
                "skills/web-engagement-state/scripts/engagement_state.py",
                vec![
                    "--target-urls",
                    "https://example.com",
                    "--allowed-hosts",
                    "example.com",
                    "--endpoints-json",
                    r#"[{"method":"GET","path":"/api/users","parameters":["id"]}]"#,
                ],
            ),
            (
                "skills/web-coverage-status/scripts/coverage_status.py",
                vec![
                    "--coverage-json",
                    r#"[{"wstg_ids":["WSTG-INPV-05"],"api_top10_ids":["API8"]}]"#,
                ],
            ),
            (
                "skills/web-finding-validator/scripts/finding_validator.py",
                vec![
                    "--scope-json",
                    r#"{"target_urls":["https://example.com"],"allowed_hosts":["example.com"]}"#,
                    "--finding-json",
                    r#"{"affected_endpoint":"https://example.com/api/users/1","request":"GET /api/users/1 HTTP/1.1","response":"HTTP/1.1 200 OK","impact":"Read another user's profile","real_vulnerability":true,"client_reproducible":true}"#,
                ],
            ),
            (
                "skills/web-burp-mcp-review/scripts/burp_mcp_review.py",
                vec![
                    "--scope-json",
                    r#"{"target_urls":["https://example.com"],"allowed_hosts":["example.com"]}"#,
                    "--exchanges-json",
                    r#"[{"method":"GET","url":"https://example.com/api/users?id=1","status":200,"artifact":"artifact-1"}]"#,
                ],
            ),
            (
                "skills/web-browser-evidence/scripts/browser_evidence.py",
                vec![
                    "--page-url",
                    "https://example.com/app",
                    "--routes",
                    "/app,/api/users",
                    "--forms-json",
                    r#"[{"action":"/login","method":"POST"}]"#,
                ],
            ),
            (
                "skills/web-scanner-result-normalizer/scripts/scanner_result_normalizer.py",
                vec![
                    "--scope-json",
                    r#"{"target_urls":["https://example.com"],"allowed_hosts":["example.com"]}"#,
                    "--input-json",
                    r#"[{"template-id":"missing-hsts","matched-at":"https://example.com","info":{"name":"Missing HSTS","severity":"low","tags":["header"]}}]"#,
                ],
            ),
            (
                "skills/web-js-route-extractor/scripts/js_route_extractor.py",
                vec![
                    "--input-text",
                    r#"<script src="/app.js"></script><form action="/login"><input name="email"></form><script>fetch('/api/users?id=1'); const ws='wss://example.com/socket';</script>"#,
                ],
            ),
            (
                "skills/web-payload-catalog/scripts/payload_catalog.py",
                vec!["--categories", "sqli,xss"],
            ),
            (
                "skills/web-ai-feature-review/scripts/ai_feature_review.py",
                vec![
                    "--features",
                    "chat assistant",
                    "--tools",
                    "ticket_search",
                    "--data-sources",
                    "knowledge base",
                ],
            ),
            (
                "skills/web-client-side-audit/scripts/client_side_audit.py",
                vec![
                    "--base-url",
                    "https://example.com/app",
                    "--target-urls",
                    "https://example.com/app",
                    "--allowed-hosts",
                    "example.com",
                    "--html-text",
                    r#"<meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self' https://cdn.example"><script src="https://cdn.example/lib.js"></script><script>fetch('/api/users?id=1'); window.addEventListener('message', function(e) { el.innerHTML = location.hash; });</script>"#,
                ],
            ),
            (
                "skills/web-crypto-posture/scripts/crypto_posture.py",
                vec![
                    "--target-urls",
                    "https://example.com",
                    "--allowed-hosts",
                    "example.com",
                    "--headers-json",
                    r#"{"Strict-Transport-Security":"max-age=300","Content-Security-Policy":"default-src 'self' https://cdn.example"}"#,
                    "--ct-hosts",
                    "cdn.example,api.example.com",
                ],
            ),
            (
                "skills/web-dependency-sca/scripts/dependency_sca.py",
                vec![
                    "--asset-html-text",
                    r#"<script src="https://cdn.example/lib.js"></script><link rel="stylesheet" href="https://cdn.example/app.css">"#,
                    "--scanner-results-json",
                    r#"[{"name":"lodash","severity":"high","id":"CVE-0000-0000"}]"#,
                ],
            ),
            (
                "skills/web-error-handling-review/scripts/error_handling_review.py",
                vec![
                    "--observations-json",
                    r#"[{"body":"Traceback (most recent call last): File \"/app/views.py\", line 10, in handler"}]"#,
                    "--routes-json",
                    r#"["/api/users?id=1"]"#,
                    "--target-urls",
                    "https://example.com",
                    "--allowed-hosts",
                    "example.com",
                ],
            ),
        ];

        for (script, args) in scripts {
            let parsed = run_python_json(&root.join(script), &args);
            assert_eq!(parsed["status"], "ok", "script {script} did not return ok");
        }

        let failed = run_python_raw(
            &root.join("skills/web-http-baseline/scripts/http_baseline.py"),
            &[
                "--target-url",
                "https://evil.example",
                "--allowed-hosts",
                "example.com",
            ],
        );
        assert_ne!(failed.status.code(), Some(0));
        let parsed: Value = serde_json::from_slice(&failed.stdout).unwrap();
        assert_eq!(parsed["status"], "error");
        assert!(
            parsed["error"]
                .as_str()
                .unwrap()
                .contains("outside allowed_hosts")
        );

        let inactive_scope = run_python_raw(
            &root.join("skills/web-http-baseline/scripts/http_baseline.py"),
            &[
                "--target-url",
                "https://example.com",
                "--scope-json",
                r#"{"target_urls":["https://example.com"],"allowed_hosts":["example.com"],"active_authorized":"false"}"#,
                "--fetch",
                "true",
            ],
        );
        assert_ne!(inactive_scope.status.code(), Some(0));
        let parsed: Value = serde_json::from_slice(&inactive_scope.stdout).unwrap();
        assert_eq!(parsed["status"], "error");
        assert!(
            parsed["error"]
                .as_str()
                .unwrap()
                .contains("active network testing requires")
        );

        let redaction_failure = run_python_json(
            &root.join("skills/web-finding-validator/scripts/finding_validator.py"),
            &[
                "--scope-json",
                r#"{"target_urls":["https://example.com"],"allowed_hosts":["example.com"]}"#,
                "--finding-json",
                r#"{"affected_endpoint":"https://example.com/api/users/1","request":"GET /api/users/1 HTTP/1.1\nAuthorization: Bearer abcdefghijklmnopqrstuvwxyz1234567890","response":"HTTP/1.1 200 OK","impact":"Read another user's profile","real_vulnerability":true,"client_reproducible":true}"#,
            ],
        );
        assert_eq!(redaction_failure["recommended_status"], "rejected");
        assert!(
            redaction_failure["validation_gates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|gate| gate["gate"] == "credential-redaction" && gate["status"] == "fail")
        );

        let report = run_python_json(
            &root.join("skills/web-evidence-report/scripts/evidence_report.py"),
            &[
                "--findings-json",
                r#"[{"title":"Validated issue","status":"validated","evidence":"artifact-1"},{"title":"Scanner lead","status":"lead","evidence":"artifact-2"}]"#,
            ],
        );
        assert_eq!(report["finding_count"], 1);
        assert_eq!(report["lead_count"], 1);
        assert!(
            report["report_markdown"]
                .as_str()
                .unwrap()
                .contains("## Leads And Gaps")
        );

        let client_audit = run_python_json(
            &root.join("skills/web-client-side-audit/scripts/client_side_audit.py"),
            &[
                "--base-url",
                "https://example.com/app",
                "--target-urls",
                "https://example.com/app",
                "--allowed-hosts",
                "example.com",
                "--html-text",
                r#"<script src="https://cdn.example/lib.js"></script><script>fetch('/api/users?id=1'); window.addEventListener('message', function(e) { el.innerHTML = location.hash; });</script>"#,
                "--sitemap-text",
                "<urlset><url><loc>https://example.com/api/report.json</loc></url></urlset>",
                "--openapi-json",
                r#"{"openapi":"3.0.0","paths":{"/v1/orders":{"get":{}}}}"#,
                "--headers-json",
                r#"{"Content-Security-Policy":"default-src 'self'; script-src 'self' https://cdn.example"}"#,
            ],
        );
        assert!(
            client_audit["api_candidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item.as_str().unwrap().contains("/api/users"))
        );
        assert!(
            client_audit["dom_risk_indicators"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["indicator"] == "post-message-listener")
        );
        assert!(
            client_audit["shadow_api_candidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["candidate"].as_str().unwrap().contains("/v1/orders"))
        );

        let crypto = run_python_json(
            &root.join("skills/web-crypto-posture/scripts/crypto_posture.py"),
            &[
                "--target-urls",
                "https://example.com",
                "--allowed-hosts",
                "example.com",
                "--headers-json",
                r#"{"Strict-Transport-Security":"max-age=300","Content-Security-Policy":"default-src 'self' https://cdn.example"}"#,
                "--ct-hosts",
                "cdn.example",
            ],
        );
        assert!(
            crypto["related_host_candidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["host"] == "cdn.example" && item["in_scope"] == false)
        );
        assert!(
            crypto["crypto_findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["type"] == "short-hsts-max-age")
        );

        let dependency_sca = run_python_json(
            &root.join("skills/web-dependency-sca/scripts/dependency_sca.py"),
            &[
                "--asset-html-text",
                r#"<script src="https://cdn.example/lib.js"></script>"#,
                "--scanner-results-json",
                r#"[{"name":"lodash","severity":"high","id":"CVE-0000-0000"}]"#,
            ],
        );
        assert_eq!(dependency_sca["sri_coverage"]["missing_sri_count"], 1);
        assert!(
            dependency_sca["pinning_observations"]["floating_cdn_asset_count"]
                .as_i64()
                .unwrap()
                >= 1
        );
        assert_eq!(
            dependency_sca["normalized_sca_leads"][0]["package"],
            "lodash"
        );

        let error_review = run_python_json(
            &root.join("skills/web-error-handling-review/scripts/error_handling_review.py"),
            &[
                "--observations-json",
                r#"[{"body":"Traceback (most recent call last): File \"/app/views.py\", line 10, in handler"}]"#,
                "--routes-json",
                r#"["/api/users?id=1"]"#,
                "--target-urls",
                "https://example.com",
                "--allowed-hosts",
                "example.com",
            ],
        );
        assert!(
            error_review["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["type"] == "stack-trace-python")
        );
        assert!(
            error_review["prioritized_validation_targets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["target"] == "/api/users?id=1")
        );

        let route_scope_failure = run_python_raw(
            &root.join("skills/web-error-handling-review/scripts/error_handling_review.py"),
            &[
                "--urls",
                "https://evil.example/api/users?id=1",
                "--target-urls",
                "https://example.com",
                "--allowed-hosts",
                "example.com",
            ],
        );
        assert_ne!(route_scope_failure.status.code(), Some(0));
        let parsed: Value = serde_json::from_slice(&route_scope_failure.stdout).unwrap();
        assert_eq!(parsed["status"], "error");
        assert!(
            parsed["error"]
                .as_str()
                .unwrap()
                .contains("outside allowed_hosts")
        );
    }

    fn run_python_json(script: &std::path::Path, args: &[&str]) -> Value {
        let output = run_python_raw(script, args);
        assert!(
            output.status.success(),
            "{} failed: {}",
            script.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn run_python_raw(script: &std::path::Path, args: &[&str]) -> std::process::Output {
        for python in ["python3", "python"] {
            match Command::new(python).arg(script).args(args).output() {
                Ok(output) => return output,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => panic!("failed to execute {}: {err}", script.display()),
            }
        }
        panic!("python3 or python is required for web assessment script tests");
    }
}
