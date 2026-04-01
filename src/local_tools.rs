use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::time::Duration;

use crate::{
    azure::AzureTool,
    conversation_store::ConversationStore,
    skills::{SkillRegistry, SkillTool},
    types::{AgentPermissions, FilesystemAccess},
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum SkillProgram {
    Direct(PathBuf),
    Python(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillLaunchSpec {
    program: SkillProgram,
    current_dir: PathBuf,
}

impl SkillLaunchSpec {
    fn new(skill_dir: &Path, script: &str) -> Result<Self> {
        let script_path = skill_dir.join(script);
        if !script_path.is_file() {
            bail!(
                "skill script not found: {} (resolved from '{}' in {})",
                script_path.display(),
                script,
                skill_dir.display()
            );
        }

        let program = if script_path.extension() == Some(OsStr::new("py")) {
            SkillProgram::Python(script_path)
        } else {
            SkillProgram::Direct(script_path)
        };

        Ok(Self {
            program,
            current_dir: skill_dir.to_path_buf(),
        })
    }

    fn display_program(&self) -> String {
        match &self.program {
            SkillProgram::Direct(path) | SkillProgram::Python(path) => path.display().to_string(),
        }
    }

    fn command_with_interpreter(&self, interpreter: Option<&str>) -> tokio::process::Command {
        let mut cmd = match (&self.program, interpreter) {
            (SkillProgram::Direct(path), _) => tokio::process::Command::new(path),
            (SkillProgram::Python(path), Some(interpreter)) => {
                let mut cmd = tokio::process::Command::new(interpreter);
                cmd.arg(path);
                cmd
            }
            (SkillProgram::Python(_), None) => unreachable!("python skills require an interpreter"),
        };
        cmd.current_dir(&self.current_dir);
        cmd
    }
}

fn apply_skill_arguments(cmd: &mut tokio::process::Command, params: &Value) {
    let Some(obj) = params.as_object() else {
        return;
    };
    for (key, value) in obj {
        let flag = format!("--{}", key.replace('_', "-"));
        match value {
            Value::Bool(true) => {
                cmd.arg(&flag);
            }
            Value::Bool(false) => {}
            other => {
                cmd.arg(&flag);
                cmd.arg(
                    other
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| other.to_string()),
                );
            }
        }
    }
}

pub struct LocalToolExecutor {
    store: ConversationStore,
    conversation_id: String,
    skills: Option<SkillRegistry>,
    permissions: AgentPermissions,
}

impl LocalToolExecutor {
    pub fn new(
        store: ConversationStore,
        conversation_id: impl Into<String>,
        skills: Option<SkillRegistry>,
        permissions: AgentPermissions,
    ) -> Self {
        Self {
            store,
            conversation_id: conversation_id.into(),
            skills,
            permissions,
        }
    }

    pub fn is_local_tool(&self, name: &str) -> bool {
        matches!(
            name,
            "local__sleep"
                | "local__remember_job"
                | "local__get_job"
                | "local__list_jobs"
                | "local__forget_job"
                | "local__run_skill"
        )
    }

    pub async fn execute(&self, name: &str, arguments: Value) -> Result<String> {
        match name {
            "local__sleep" => self.exec_sleep(arguments).await,
            "local__remember_job" => self.exec_remember_job(arguments),
            "local__get_job" => self.exec_get_job(arguments),
            "local__list_jobs" => self.exec_list_jobs(),
            "local__forget_job" => self.exec_forget_job(arguments),
            "local__run_skill" => self.exec_run_skill(arguments).await,
            _ => Err(anyhow!("unknown local tool: {name}")),
        }
    }

    async fn exec_sleep(&self, arguments: Value) -> Result<String> {
        let seconds = arguments
            .get("seconds")
            .and_then(Value::as_f64)
            .unwrap_or(1.0);
        let reason = arguments
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let capped = seconds.clamp(0.0, 300.0);
        if !reason.is_empty() {
            tracing::info!(seconds = capped, reason, "local tool: sleep");
        }
        tokio::time::sleep(Duration::from_secs_f64(capped)).await;
        Ok(format!("Slept for {capped:.1}s"))
    }

    fn jobs_path(&self) -> PathBuf {
        self.store
            .conversation_dir(&self.conversation_id)
            .expect("conversation_id should be validated by store callers")
            .join("jobs.json")
    }

    fn load_jobs(&self) -> Result<HashMap<String, Value>> {
        let path = self.jobs_path();
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let map: HashMap<String, Value> = serde_json::from_str(&raw)?;
        Ok(map)
    }

    fn save_jobs(&self, jobs: &HashMap<String, Value>) -> Result<()> {
        let path = self.jobs_path();
        let payload = serde_json::to_string_pretty(jobs)?;
        std::fs::write(&path, payload)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    fn exec_remember_job(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_write("local__remember_job")?;
        let alias = arguments
            .get("alias")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("remember_job: missing 'alias'"))?
            .to_string();
        let transaction_id = arguments
            .get("transaction_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("remember_job: missing 'transaction_id'"))?
            .to_string();
        let record = json!({
            "alias": alias,
            "transaction_id": transaction_id,
            "source_tool": arguments.get("source_tool"),
            "status": arguments.get("status"),
            "notes": arguments.get("notes"),
            "stored_at": chrono::Utc::now().to_rfc3339(),
        });
        let mut jobs = self.load_jobs()?;
        jobs.insert(alias.clone(), record);
        self.save_jobs(&jobs)?;
        Ok(format!("Job '{alias}' stored."))
    }

    fn exec_get_job(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_read("local__get_job")?;
        let alias = arguments
            .get("alias")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("get_job: missing 'alias'"))?;
        let jobs = self.load_jobs()?;
        let record = jobs
            .get(alias)
            .ok_or_else(|| anyhow!("Job '{alias}' not found."))?;
        Ok(serde_json::to_string_pretty(record)?)
    }

    fn exec_list_jobs(&self) -> Result<String> {
        self.require_filesystem_read("local__list_jobs")?;
        let jobs = self.load_jobs()?;
        if jobs.is_empty() {
            return Ok("No jobs stored.".to_string());
        }
        Ok(serde_json::to_string_pretty(&jobs)?)
    }

    fn exec_forget_job(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_write("local__forget_job")?;
        let alias = arguments
            .get("alias")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("forget_job: missing 'alias'"))?;
        let mut jobs = self.load_jobs()?;
        if jobs.remove(alias).is_none() {
            return Err(anyhow!("Job '{alias}' not found."));
        }
        self.save_jobs(&jobs)?;
        Ok(format!("Job '{alias}' removed."))
    }

    async fn exec_run_skill(&self, arguments: Value) -> Result<String> {
        let registry = self
            .skills
            .as_ref()
            .ok_or_else(|| anyhow!("skills registry not available"))?;

        let skill_name = arguments
            .get("skill_name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("run_skill: missing 'skill_name'"))?;
        let tool_slug = arguments.get("tool_slug").and_then(Value::as_str);
        let parameters_str = arguments
            .get("parameters")
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let params: Value = serde_json::from_str(parameters_str).unwrap_or_else(|_| json!({}));

        let (skill, tool) = registry
            .find_tool(skill_name, tool_slug)
            .ok_or_else(|| anyhow!("skill '{skill_name}' / tool '{tool_slug:?}' not found"))?;

        self.require_skill_permissions(skill_name, tool)?;

        if let Some(ref server) = tool.server {
            return Err(anyhow!(
                "MCP-backed skill tool '{server}' must be called via the MCP server directly"
            ));
        }

        let script = tool
            .script
            .as_deref()
            .ok_or_else(|| anyhow!("skill tool has no script defined"))?;

        let launch = SkillLaunchSpec::new(&skill.skill_dir, script)?;
        let mut cmd = launch.command_with_interpreter(match &launch.program {
            SkillProgram::Direct(_) => None,
            SkillProgram::Python(_) => Some("python3"),
        });
        apply_skill_arguments(&mut cmd, &params);

        let output = match &launch.program {
            SkillProgram::Direct(_) => cmd.output().await.with_context(|| {
                format!(
                    "failed to execute skill script {}",
                    launch.display_program()
                )
            })?,
            SkillProgram::Python(_) => match cmd.output().await {
                Ok(output) => output,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    let mut fallback = launch.command_with_interpreter(Some("python"));
                    apply_skill_arguments(&mut fallback, &params);
                    fallback.output().await.with_context(|| {
                        format!(
                            "failed to execute python skill script {} with python3 or python",
                            launch.display_program()
                        )
                    })?
                }
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!(
                            "failed to execute python skill script {} with python3",
                            launch.display_program()
                        )
                    });
                }
            },
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "skill script exited with {}: {}",
                output.status,
                stderr
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn require_filesystem_read(&self, capability: &str) -> Result<()> {
        if self.permissions.allows_filesystem_read() {
            Ok(())
        } else {
            bail!(
                "permission denied: {capability} requires filesystem read access. Enable it with /permissions fs read or /permissions fs write, or use /yolo on."
            )
        }
    }

    fn require_filesystem_write(&self, capability: &str) -> Result<()> {
        if self.permissions.allows_filesystem_write() {
            Ok(())
        } else {
            bail!(
                "permission denied: {capability} requires filesystem write access. Enable it with /permissions fs write, or use /yolo on."
            )
        }
    }

    fn require_network(&self, capability: &str) -> Result<()> {
        if self.permissions.allows_network() {
            Ok(())
        } else {
            bail!(
                "permission denied: {capability} requires network access. Enable it with /permissions network on, or use /yolo on."
            )
        }
    }

    fn require_skill_permissions(&self, skill_name: &str, tool: &SkillTool) -> Result<()> {
        if self.permissions.yolo {
            return Ok(());
        }

        if tool.requires_network {
            self.require_network(&format!("skill '{skill_name}' / tool '{}'", tool.slug))?;
        }

        match tool.filesystem {
            FilesystemAccess::None => {}
            FilesystemAccess::ReadOnly => {
                self.require_filesystem_read(&format!(
                    "skill '{skill_name}' / tool '{}'",
                    tool.slug
                ))?;
            }
            FilesystemAccess::ReadWrite => {
                self.require_filesystem_write(&format!(
                    "skill '{skill_name}' / tool '{}'",
                    tool.slug
                ))?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use crate::{
        conversation_store::ConversationStore,
        skills::SkillRegistry,
        types::{AgentPermissions, FilesystemAccess},
    };

    use super::{LocalToolExecutor, SkillLaunchSpec, SkillProgram};

    #[test]
    fn python_scripts_are_resolved_via_interpreter() {
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("webex-room-conversation");
        let scripts_dir = skill_dir.join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        let script_path = scripts_dir.join("webex_room_message_fetch.py");
        fs::write(&script_path, "#!/usr/bin/env python3\nprint('ok')\n").unwrap();

        let launch =
            SkillLaunchSpec::new(&skill_dir, "scripts/webex_room_message_fetch.py").unwrap();

        assert_eq!(launch.current_dir, skill_dir);
        assert_eq!(launch.program, SkillProgram::Python(script_path));
    }

    #[test]
    fn non_python_scripts_run_directly() {
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("demo-skill");
        let scripts_dir = skill_dir.join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        let script_path = scripts_dir.join("tool.sh");
        fs::write(&script_path, "#!/bin/sh\necho ok\n").unwrap();

        let launch = SkillLaunchSpec::new(&skill_dir, "scripts/tool.sh").unwrap();

        assert_eq!(launch.current_dir, skill_dir);
        assert_eq!(launch.program, SkillProgram::Direct(script_path));
    }

    #[test]
    fn missing_script_reports_resolved_path() {
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("demo-skill");
        fs::create_dir_all(&skill_dir).unwrap();

        let err = SkillLaunchSpec::new(&skill_dir, "scripts/missing.py").unwrap_err();

        let message = err.to_string();
        assert!(message.contains("skill script not found"));
        assert!(message.contains("scripts/missing.py"));
        assert!(message.contains(&skill_dir.display().to_string()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn job_storage_requires_filesystem_write_permissions() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            None,
            AgentPermissions {
                allow_network: false,
                filesystem: FilesystemAccess::ReadOnly,
                yolo: false,
            },
        );

        let err = executor
            .execute(
                "local__remember_job",
                json!({"alias": "demo", "transaction_id": "123"}),
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("filesystem write access"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn networked_skill_requires_network_permission() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let skills_dir = dir.path().join("skills");
        let skill_dir = skills_dir.join("webex-room-conversation");
        fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        fs::write(
            skill_dir.join("scripts/fetch.py"),
            "#!/usr/bin/env python3\nprint('ok')\n",
        )
        .unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: webex-room-conversation
description: Fetch Webex room messages
---

Tools:
  - name: Fetch
    slug: fetch
    script: scripts/fetch.py
    network: true
    filesystem: read_only
"#,
        )
        .unwrap();

        let skills = SkillRegistry::load(&skills_dir).unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            Some(skills),
            AgentPermissions::default(),
        );

        let err = executor
            .execute(
                "local__run_skill",
                json!({"skill_name": "webex-room-conversation", "tool_slug": "fetch"}),
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("network access"));
    }
}

pub fn local_tool_definitions() -> Vec<AzureTool> {
    vec![
        AzureTool {
            name: "local__sleep".to_string(),
            description: "Sleep for a specified number of seconds (max 300). Use to wait between polling operations.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "seconds": {"type": "number", "description": "Seconds to sleep (max 300)"},
                    "reason": {"type": "string", "description": "Optional reason for sleeping"}
                },
                "required": ["seconds"]
            }),
        },
        AzureTool {
            name: "local__remember_job".to_string(),
            description: "Store a job/transaction alias for later retrieval within this conversation. Requires filesystem write permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "alias": {"type": "string", "description": "Short name to refer to this job"},
                    "transaction_id": {"type": "string", "description": "The actual transaction or job ID"},
                    "source_tool": {"type": "string", "description": "Which tool created this job"},
                    "status": {"type": "string", "description": "Current job status"},
                    "notes": {"type": "string", "description": "Additional notes"}
                },
                "required": ["alias", "transaction_id"]
            }),
        },
        AzureTool {
            name: "local__get_job".to_string(),
            description: "Retrieve a stored job by alias. Requires filesystem read permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "alias": {"type": "string", "description": "The alias of the stored job"}
                },
                "required": ["alias"]
            }),
        },
        AzureTool {
            name: "local__list_jobs".to_string(),
            description: "List all stored jobs in this conversation. Requires filesystem read permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
        AzureTool {
            name: "local__forget_job".to_string(),
            description: "Remove a stored job by alias. Requires filesystem write permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "alias": {"type": "string", "description": "The alias of the stored job to remove"}
                },
                "required": ["alias"]
            }),
        },
        AzureTool {
            name: "local__run_skill".to_string(),
            description: "Execute a skill script with parameters. Skill-specific network/filesystem permissions are enforced unless yolo mode is enabled.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "skill_name": {"type": "string", "description": "Name of the skill directory"},
                    "tool_slug": {"type": "string", "description": "Slug of the specific tool within the skill"},
                    "parameters": {"type": "string", "description": "JSON string of parameters to pass to the script"}
                },
                "required": ["skill_name"]
            }),
        },
    ]
}
