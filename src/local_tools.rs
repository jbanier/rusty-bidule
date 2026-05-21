use std::{
    ffi::OsStr,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Duration as ChronoDuration, Local, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use tokio::time::Duration;

use crate::{
    config::LocalToolsConfig,
    conversation_store::ConversationStore,
    llm::LlmTool,
    skills::{SkillRegistry, SkillTool},
    types::{AgentPermissions, FilesystemAccess, InvestigationMemory, RememberedJob},
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
        let skill_dir = std::fs::canonicalize(skill_dir)
            .with_context(|| format!("failed to canonicalize skill dir {}", skill_dir.display()))?;
        let script_path = skill_dir.join(script);
        if !script_path.is_file() {
            bail!(
                "skill script not found: {} (resolved from '{}' in {})",
                script_path.display(),
                script,
                skill_dir.display()
            );
        }
        let script_path = std::fs::canonicalize(&script_path).with_context(|| {
            format!(
                "failed to canonicalize skill script {}",
                script_path.display()
            )
        })?;
        if !script_path.starts_with(&skill_dir) {
            bail!(
                "skill script escapes skill directory: {} is outside {}",
                script_path.display(),
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
    enabled_local_tools: Option<Vec<String>>,
    execution_timeout: Duration,
    allowed_cli_tools: Vec<String>,
    workspace_root: PathBuf,
    max_file_read_bytes: u64,
    max_file_write_bytes: u64,
    max_directory_entries: usize,
}

impl LocalToolExecutor {
    pub fn new(
        store: ConversationStore,
        conversation_id: impl Into<String>,
        skills: Option<SkillRegistry>,
        permissions: AgentPermissions,
        enabled_local_tools: Option<Vec<String>>,
        execution_timeout: Duration,
        allowed_cli_tools: Vec<String>,
    ) -> Self {
        let default_local_tools = LocalToolsConfig::default();
        Self {
            store,
            conversation_id: conversation_id.into(),
            skills,
            permissions,
            enabled_local_tools,
            execution_timeout,
            allowed_cli_tools,
            workspace_root: default_workspace_root(),
            max_file_read_bytes: default_local_tools.max_file_read_bytes,
            max_file_write_bytes: default_local_tools.max_file_write_bytes,
            max_directory_entries: default_local_tools.max_directory_entries,
        }
    }

    pub fn with_local_tools_config(mut self, config: &LocalToolsConfig) -> Self {
        self.max_file_read_bytes = config.max_file_read_bytes;
        self.max_file_write_bytes = config.max_file_write_bytes;
        self.max_directory_entries = config.max_directory_entries;
        self
    }

    #[cfg(test)]
    fn with_workspace_root(mut self, root: impl AsRef<Path>) -> Self {
        self.workspace_root = canonicalize_existing_or_self(root.as_ref());
        self
    }

    pub fn is_local_tool(&self, name: &str) -> bool {
        is_advertised_local_tool_name(name) && self.is_tool_enabled(name)
    }

    pub fn is_known_local_tool(&self, name: &str) -> bool {
        is_advertised_local_tool_name(name)
    }

    pub async fn execute(&self, name: &str, arguments: Value) -> Result<String> {
        match name {
            "local__sleep" => self.exec_sleep(arguments).await,
            "local__remember_job" => self.exec_remember_job(arguments),
            "local__update_job" => self.exec_update_job(arguments),
            "local__get_job" => self.exec_get_job(arguments),
            "local__list_jobs" => self.exec_list_jobs(),
            "local__forget_job" => self.exec_forget_job(arguments),
            "local__get_investigation_memory" => self.exec_get_investigation_memory(),
            "local__update_investigation_memory" => {
                self.exec_update_investigation_memory(arguments)
            }
            "local__clear_investigation_memory" => self.exec_clear_investigation_memory(),
            "local__search_conversation_memories" => {
                self.exec_search_conversation_memories(arguments)
            }
            "local__time" => self.exec_time(arguments),
            "local__configure_mcp_servers" => self.exec_configure_mcp_servers(arguments),
            "local__exec_cli" => self.exec_cli(arguments).await,
            "local__list_directory" => self.exec_list_directory(arguments),
            "local__read_file" => self.exec_read_file(arguments),
            "local__write_file" => self.exec_write_file(arguments),
            "local__activate_skill" => self.exec_activate_skill(arguments),
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

    fn is_tool_enabled(&self, name: &str) -> bool {
        self.enabled_local_tools
            .as_ref()
            .map(|enabled| enabled.iter().any(|tool| tool == name))
            .unwrap_or(true)
    }

    fn load_jobs(&self) -> Result<Vec<RememberedJob>> {
        self.store.load_job_state(&self.conversation_id)
    }

    fn save_jobs(&self, jobs: &[RememberedJob]) -> Result<()> {
        self.store.save_job_state(&self.conversation_id, jobs)
    }

    fn upsert_job(&self, record: RememberedJob) -> Result<()> {
        let mut jobs = self.load_jobs()?;
        jobs.retain(|job| job.alias != record.alias);
        jobs.push(record);
        jobs.sort_by(|a, b| a.alias.cmp(&b.alias));
        self.save_jobs(&jobs)
    }

    fn exec_remember_job(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_write("local__remember_job")?;
        let alias = required_string_arg(&arguments, "alias", "remember_job")?;
        let transaction_id = required_string_arg(&arguments, "transaction_id", "remember_job")?;
        let mut record = RememberedJob::new(alias.clone(), transaction_id)?;
        record.source_tool = arguments
            .get("source_tool")
            .and_then(Value::as_str)
            .map(str::to_string);
        record.status = arguments
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_string);
        record.notes = arguments
            .get("notes")
            .and_then(Value::as_str)
            .map(str::to_string);
        record.set_mode(optional_string_arg(&arguments, "mode", "remember_job")?)?;
        record.set_poll_interval_seconds(optional_u64_arg(
            &arguments,
            "poll_interval_seconds",
            "remember_job",
        )?)?;
        record.next_poll_at = parse_optional_datetime(arguments.get("next_poll_at"))?;
        record.lease_expires_at = parse_optional_datetime(arguments.get("lease_expires_at"))?;
        record.result_expires_at = parse_optional_datetime(arguments.get("result_expires_at"))?;
        record.automation_prompt = arguments
            .get("automation_prompt")
            .and_then(Value::as_str)
            .map(str::to_string);
        record.retrieval_state = arguments
            .get("retrieval_state")
            .and_then(Value::as_str)
            .map(str::to_string);
        record.result_artifacts_json = arguments.get("result_artifacts_json").cloned();
        record.last_error = arguments
            .get("last_error")
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut jobs = self.load_jobs()?;
        jobs.retain(|job| job.alias != alias);
        jobs.push(record);
        jobs.sort_by(|a, b| a.alias.cmp(&b.alias));
        self.save_jobs(&jobs)?;
        Ok(format!("Job '{alias}' stored."))
    }

    fn exec_update_job(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_write("local__update_job")?;
        let alias = arguments
            .get("alias")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("update_job: missing 'alias'"))?;
        let mut jobs = self.load_jobs()?;
        let job = jobs
            .iter_mut()
            .find(|job| job.alias == alias)
            .ok_or_else(|| anyhow!("Job '{alias}' not found."))?;

        if arguments.get("transaction_id").is_some() {
            job.set_transaction_id(required_string_arg(
                &arguments,
                "transaction_id",
                "update_job",
            )?)?;
        }
        if let Some(value) = arguments.get("source_tool").and_then(Value::as_str) {
            job.source_tool = Some(value.to_string());
        }
        if arguments.get("status").is_some() {
            job.status = arguments
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if arguments.get("notes").is_some() {
            job.notes = arguments
                .get("notes")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if arguments.get("mode").is_some() {
            job.set_mode(optional_string_arg(&arguments, "mode", "update_job")?)?;
        }
        if arguments.get("poll_interval_seconds").is_some() {
            job.set_poll_interval_seconds(optional_u64_arg(
                &arguments,
                "poll_interval_seconds",
                "update_job",
            )?)?;
        }
        if arguments.get("next_poll_at").is_some() {
            job.next_poll_at = parse_optional_datetime(arguments.get("next_poll_at"))?;
        }
        if arguments.get("lease_expires_at").is_some() {
            job.lease_expires_at = parse_optional_datetime(arguments.get("lease_expires_at"))?;
        }
        if arguments.get("result_expires_at").is_some() {
            job.result_expires_at = parse_optional_datetime(arguments.get("result_expires_at"))?;
        }
        if arguments.get("automation_prompt").is_some() {
            job.automation_prompt = arguments
                .get("automation_prompt")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if arguments.get("retrieval_state").is_some() {
            job.retrieval_state = arguments
                .get("retrieval_state")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if arguments.get("result_artifacts_json").is_some() {
            job.result_artifacts_json = arguments.get("result_artifacts_json").cloned();
        }
        if arguments.get("last_error").is_some() {
            job.last_error = arguments
                .get("last_error")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        job.updated_at = chrono::Utc::now();
        self.save_jobs(&jobs)?;
        Ok(format!("Job '{alias}' updated."))
    }

    fn exec_get_job(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_read("local__get_job")?;
        let alias = arguments
            .get("alias")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("get_job: missing 'alias'"))?;
        let jobs = self.load_jobs()?;
        let record = jobs
            .iter()
            .find(|job| job.alias == alias)
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
        let original_len = jobs.len();
        jobs.retain(|job| job.alias != alias);
        if jobs.len() == original_len {
            return Err(anyhow!("Job '{alias}' not found."));
        }
        self.save_jobs(&jobs)?;
        Ok(format!("Job '{alias}' removed."))
    }

    fn exec_get_investigation_memory(&self) -> Result<String> {
        self.require_filesystem_read("local__get_investigation_memory")?;
        let memory = self
            .store
            .load_investigation_memory(&self.conversation_id)?;
        Ok(serde_json::to_string_pretty(&memory)?)
    }

    fn exec_update_investigation_memory(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_write("local__update_investigation_memory")?;
        let mode = arguments
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("merge");
        let replace = match mode {
            "merge" => false,
            "replace" => true,
            other => bail!("update_investigation_memory: unsupported mode '{other}'"),
        };

        let mut memory = if replace {
            InvestigationMemory::default()
        } else {
            self.store
                .load_investigation_memory(&self.conversation_id)?
        };
        let mut changed = false;

        if let Some(summary) = memory_patch_value(&arguments, "summary") {
            let summary = summary
                .as_str()
                .ok_or_else(|| anyhow!("update_investigation_memory: summary must be a string"))?;
            memory.summary = summary.trim().to_string();
            changed = true;
        }

        changed |= update_memory_array(
            &mut memory.entities,
            memory_patch_value(&arguments, "entities"),
            replace,
            "entities",
        )?;
        changed |= update_memory_array(
            &mut memory.timeline,
            memory_patch_value(&arguments, "timeline"),
            replace,
            "timeline",
        )?;
        changed |= update_memory_array(
            &mut memory.decisions,
            memory_patch_value(&arguments, "decisions"),
            replace,
            "decisions",
        )?;
        changed |= update_memory_array(
            &mut memory.hypotheses,
            memory_patch_value(&arguments, "hypotheses"),
            replace,
            "hypotheses",
        )?;
        changed |= update_memory_array(
            &mut memory.trusted_sources,
            memory_patch_value(&arguments, "trusted_sources"),
            replace,
            "trusted_sources",
        )?;
        changed |= update_memory_array(
            &mut memory.unresolved_questions,
            memory_patch_value(&arguments, "unresolved_questions"),
            replace,
            "unresolved_questions",
        )?;

        if !changed {
            bail!(
                "update_investigation_memory: provide summary, entities, timeline, decisions, hypotheses, trusted_sources, unresolved_questions, or a memory object. Use local__clear_investigation_memory to clear memory intentionally."
            );
        }

        memory.updated_at = Some(Utc::now());
        memory.updated_by = Some("local__update_investigation_memory".to_string());
        self.store
            .save_investigation_memory(&self.conversation_id, &memory)?;
        Ok(serde_json::to_string_pretty(&json!({
            "status": "updated",
            "conversation_id": self.conversation_id,
            "memory": memory
        }))?)
    }

    fn exec_clear_investigation_memory(&self) -> Result<String> {
        self.require_filesystem_write("local__clear_investigation_memory")?;
        let removed = self
            .store
            .clear_investigation_memory(&self.conversation_id)?;
        Ok(serde_json::to_string_pretty(&json!({
            "status": if removed { "cleared" } else { "already_empty" },
            "conversation_id": self.conversation_id
        }))?)
    }

    fn exec_search_conversation_memories(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_read("local__search_conversation_memories")?;
        let query = arguments
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("search_conversation_memories: missing 'query'"))?;
        let results = self.store.search_investigation_memories(query)?;
        Ok(serde_json::to_string_pretty(&results)?)
    }

    fn exec_time(&self, arguments: Value) -> Result<String> {
        let now_utc = Utc::now();
        let now_local = Local::now();
        let hours_ago = arguments.get("hours_ago").and_then(Value::as_i64);
        let days_ago = arguments.get("days_ago").and_then(Value::as_i64);
        let trailing_hours = arguments
            .get("trailing_hours")
            .and_then(Value::as_i64)
            .filter(|value| *value >= 0);
        let trailing_days = arguments
            .get("trailing_days")
            .and_then(Value::as_i64)
            .filter(|value| *value >= 0);

        let reference_utc = now_utc
            - ChronoDuration::hours(hours_ago.unwrap_or(0))
            - ChronoDuration::days(days_ago.unwrap_or(0));
        let reference_local = reference_utc.with_timezone(&Local);

        let mut payload = json!({
            "now_utc": now_utc.to_rfc3339(),
            "now_local": now_local.to_rfc3339(),
            "local_timezone_offset": now_local.format("%:z").to_string(),
            "reference_utc": reference_utc.to_rfc3339(),
            "reference_local": reference_local.to_rfc3339(),
            "input": {
                "hours_ago": hours_ago,
                "days_ago": days_ago,
                "trailing_hours": trailing_hours,
                "trailing_days": trailing_days,
            }
        });

        if let Some(hours) = trailing_hours {
            let start = now_utc - ChronoDuration::hours(hours);
            payload["window_start_utc"] = Value::String(start.to_rfc3339());
            payload["window_end_utc"] = Value::String(now_utc.to_rfc3339());
            payload["window_start_local"] = Value::String(start.with_timezone(&Local).to_rfc3339());
            payload["window_end_local"] = Value::String(now_local.to_rfc3339());
            payload["window_label"] = Value::String(format!("last {hours} hours"));
        } else if let Some(days) = trailing_days {
            let start = now_utc - ChronoDuration::days(days);
            payload["window_start_utc"] = Value::String(start.to_rfc3339());
            payload["window_end_utc"] = Value::String(now_utc.to_rfc3339());
            payload["window_start_local"] = Value::String(start.with_timezone(&Local).to_rfc3339());
            payload["window_end_local"] = Value::String(now_local.to_rfc3339());
            payload["window_label"] = Value::String(format!("last {days} days"));
        }

        Ok(serde_json::to_string_pretty(&payload)?)
    }

    fn exec_configure_mcp_servers(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_write("local__configure_mcp_servers")?;
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("configure_mcp_servers: missing 'action'"))?;
        let server_names = arguments
            .get("server_names")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let mut conversation = self.store.load(&self.conversation_id)?;
        let next = match action {
            "reset" => None,
            "only" => Some(server_names),
            "enable" => {
                let mut enabled = conversation.enabled_mcp_servers.clone().unwrap_or_default();
                for name in server_names {
                    if !enabled.contains(&name) {
                        enabled.push(name);
                    }
                }
                Some(enabled)
            }
            "disable" => {
                let mut enabled = conversation.enabled_mcp_servers.clone().unwrap_or_default();
                enabled.retain(|name| !server_names.contains(name));
                Some(enabled)
            }
            other => bail!("configure_mcp_servers: unsupported action '{other}'"),
        };
        conversation.enabled_mcp_servers = next.clone();
        self.store.save(&conversation)?;
        Ok(format!(
            "Conversation MCP selection updated to {}.",
            serde_json::to_string(&next)?
        ))
    }

    async fn exec_cli(&self, arguments: Value) -> Result<String> {
        self.require_network("local__exec_cli")?;
        let command = arguments
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("exec_cli: missing 'command'"))?
            .trim();
        if command.is_empty() {
            bail!("exec_cli: command must not be empty");
        }
        if command.contains('/') || command.contains('\\') {
            bail!("exec_cli: command must be a bare binary name, not a path");
        }
        if !self.allowed_cli_tools.iter().any(|tool| tool == command) {
            bail!(
                "exec_cli: command '{}' is not allowed. Allowed commands: {}",
                command,
                self.allowed_cli_tools.join(", ")
            );
        }

        let timeout_seconds = arguments
            .get("timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(self.execution_timeout.as_secs())
            .min(self.execution_timeout.as_secs());

        let args = arguments
            .get("args")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_string)
                            .ok_or_else(|| anyhow!("exec_cli: each arg must be a string"))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();

        let mut cmd = tokio::process::Command::new(command);
        cmd.args(&args);
        let output = self
            .run_child_command(cmd, timeout_seconds)
            .await
            .map_err(|err| {
                anyhow!(
                    "failed to execute allowed CLI command '{}' with direct argv: {err}",
                    command
                )
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !output.status.success() {
            bail!(
                "allowed CLI command '{}' exited with {}: {}",
                command,
                output.status,
                if stderr.is_empty() {
                    stdout.as_str()
                } else {
                    stderr.as_str()
                }
            );
        }

        let mut reply = format!("Command: `{command}`");
        if !args.is_empty() {
            reply.push_str(&format!("\nArgs: {}", serde_json::to_string(&args)?));
        }
        if !stdout.is_empty() {
            reply.push_str(&format!("\n\n{stdout}"));
        }
        if !stderr.is_empty() {
            reply.push_str(&format!("\n\n[stderr]\n{stderr}"));
        }
        Ok(reply)
    }

    fn exec_list_directory(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_read("local__list_directory")?;
        let raw_path = optional_string_arg(&arguments, "path", "list_directory")?
            .unwrap_or_else(|| ".".to_string());
        let path = self.resolve_existing_path(&raw_path, "local__list_directory")?;
        if !path.is_dir() {
            bail!(
                "list_directory: path is not a directory: {}",
                path.display()
            );
        }

        let offset = optional_u64_arg(&arguments, "offset", "list_directory")?.unwrap_or(0);
        let offset = usize::try_from(offset).unwrap_or(usize::MAX);
        let requested_limit =
            optional_u64_arg(&arguments, "limit", "list_directory")?.unwrap_or(200);
        let limit = usize::try_from(requested_limit)
            .unwrap_or(usize::MAX)
            .min(self.max_directory_entries);

        let mut entries = fs::read_dir(&path)
            .with_context(|| format!("list_directory: failed to read {}", path.display()))?
            .map(|entry| {
                let entry = entry?;
                let file_name = entry.file_name().to_string_lossy().to_string();
                let entry_path = entry.path();
                let metadata = fs::symlink_metadata(&entry_path)?;
                let file_type = metadata.file_type();
                let kind = if file_type.is_dir() {
                    "directory"
                } else if file_type.is_file() {
                    "file"
                } else if file_type.is_symlink() {
                    "symlink"
                } else {
                    "other"
                };
                Ok(json!({
                    "name": file_name,
                    "path": entry_path.display().to_string(),
                    "type": kind,
                    "size_bytes": if file_type.is_file() { Some(metadata.len()) } else { None },
                    "readonly": metadata.permissions().readonly(),
                }))
            })
            .collect::<Result<Vec<_>>>()?;
        entries.sort_by(|left, right| {
            left["name"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["name"].as_str().unwrap_or_default())
        });

        let total_entries = entries.len();
        let page = entries
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();
        let next_offset = offset.saturating_add(page.len());
        let eof = next_offset >= total_entries;
        Ok(serde_json::to_string_pretty(&json!({
            "path": path.display().to_string(),
            "workspace_root": self.workspace_root.display().to_string(),
            "scope": self.effective_filesystem_scope_label(),
            "offset": offset,
            "limit": limit,
            "returned_entries": page.len(),
            "total_entries": total_entries,
            "next_offset": if eof { Value::Null } else { json!(next_offset) },
            "eof": eof,
            "entries": page,
        }))?)
    }

    fn exec_read_file(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_read("local__read_file")?;
        let raw_path = required_string_arg(&arguments, "path", "read_file")?;
        let path = self.resolve_existing_path(&raw_path, "local__read_file")?;
        if !path.is_file() {
            bail!("read_file: path is not a file: {}", path.display());
        }

        let offset = optional_u64_arg(&arguments, "offset", "read_file")?.unwrap_or(0);
        let requested_length = optional_u64_arg(&arguments, "length", "read_file")?.unwrap_or(4096);
        let length = requested_length.min(self.max_file_read_bytes);
        let format = arguments
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("text");
        if format != "text" && format != "hex" {
            bail!("read_file: format must be 'text' or 'hex'");
        }

        let mut file = fs::File::open(&path)
            .with_context(|| format!("read_file: failed to open {}", path.display()))?;
        file.seek(SeekFrom::Start(offset))
            .with_context(|| format!("read_file: failed to seek {}", path.display()))?;
        let buffer_len = usize::try_from(length)
            .map_err(|_| anyhow!("read_file: length is too large for this platform"))?;
        let mut buffer = vec![0_u8; buffer_len];
        let read_size = file
            .read(&mut buffer)
            .with_context(|| format!("read_file: failed to read {}", path.display()))?;
        buffer.truncate(read_size);
        let file_size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        let next_offset = offset.saturating_add(read_size as u64);
        let eof = next_offset >= file_size || read_size == 0;

        let mut payload = json!({
            "path": path.display().to_string(),
            "workspace_root": self.workspace_root.display().to_string(),
            "scope": self.effective_filesystem_scope_label(),
            "format": format,
            "offset": offset,
            "requested_length": requested_length,
            "length": length,
            "read_size": read_size,
            "file_size": file_size,
            "next_offset": if eof { Value::Null } else { json!(next_offset) },
            "eof": eof,
            "truncated_by_cap": requested_length > length,
        });
        match format {
            "text" => {
                let text = String::from_utf8(buffer)
                    .map_err(|err| anyhow!("read_file: chunk is not valid UTF-8: {err}"))?;
                payload["text"] = Value::String(text);
            }
            "hex" => {
                payload["hex"] = Value::String(bytes_to_lower_hex(&buffer));
            }
            _ => unreachable!(),
        }
        Ok(serde_json::to_string_pretty(&payload)?)
    }

    fn exec_write_file(&self, arguments: Value) -> Result<String> {
        self.require_filesystem_write("local__write_file")?;
        let raw_path = required_string_arg(&arguments, "path", "write_file")?;
        let mode = arguments
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("create_new");
        if mode != "create_new" && mode != "overwrite" {
            bail!("write_file: mode must be 'create_new' or 'overwrite'");
        }

        let text = arguments.get("text").and_then(Value::as_str);
        let hex = arguments.get("hex").and_then(Value::as_str);
        if text.is_some() && hex.is_some() {
            bail!("write_file: provide either 'text' or 'hex', not both");
        }
        let (data, input_format) = if let Some(text) = text {
            (text.as_bytes().to_vec(), "text")
        } else if let Some(hex) = hex {
            (decode_hex_bytes(hex)?, "hex")
        } else {
            (Vec::new(), "empty")
        };
        if data.len() as u64 > self.max_file_write_bytes {
            bail!(
                "write_file: payload is {} bytes, above max_file_write_bytes {}",
                data.len(),
                self.max_file_write_bytes
            );
        }

        let path = self.resolve_write_path(&raw_path, "local__write_file")?;
        if path.is_dir() {
            bail!("write_file: path is a directory: {}", path.display());
        }
        let mut options = fs::OpenOptions::new();
        options.write(true);
        match mode {
            "create_new" => {
                options.create_new(true);
            }
            "overwrite" => {
                options.create(true).truncate(true);
            }
            _ => unreachable!(),
        }
        let mut file = options
            .open(&path)
            .with_context(|| format!("write_file: failed to open {}", path.display()))?;
        std::io::Write::write_all(&mut file, &data)
            .with_context(|| format!("write_file: failed to write {}", path.display()))?;
        Ok(serde_json::to_string_pretty(&json!({
            "status": "written",
            "path": path.display().to_string(),
            "workspace_root": self.workspace_root.display().to_string(),
            "scope": self.effective_filesystem_scope_label(),
            "mode": mode,
            "input_format": input_format,
            "bytes_written": data.len(),
        }))?)
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
        let (params, parameters_warning) = match serde_json::from_str(parameters_str) {
            Ok(params) => (params, None),
            Err(err) => {
                let warning = format!(
                    "run_skill: parameters was not valid JSON; using an empty object. error={err}"
                );
                tracing::warn!(
                    conversation_id = %self.conversation_id,
                    skill_name,
                    tool_slug = ?tool_slug,
                    error = %err,
                    "local__run_skill parameters parse failed; using empty object"
                );
                if let Err(log_err) = self.store.append_log(&self.conversation_id, &warning) {
                    tracing::warn!(
                        conversation_id = %self.conversation_id,
                        error = %log_err,
                        "failed to append local__run_skill parameters warning to conversation log"
                    );
                }
                (json!({}), Some(warning))
            }
        };
        let timeout_seconds = arguments
            .get("timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(self.execution_timeout.as_secs())
            .min(self.execution_timeout.as_secs());

        let (skill, tools) = registry
            .find_tools(skill_name, tool_slug)
            .ok_or_else(|| anyhow!("skill '{skill_name}' / tool '{tool_slug:?}' not found"))?;

        let mut outputs = Vec::new();
        if let Some(warning) = parameters_warning {
            outputs.push(format!("[warning]\n{warning}"));
        }
        for tool in tools {
            self.require_skill_permissions(&skill.name, tool)?;
            if tool.server.is_some() && tool.script.is_none() {
                outputs.push(format!(
                    "[{}] MCP-backed skill tool metadata cannot be executed through the local runner.",
                    tool.slug
                ));
                continue;
            }

            let script = tool
                .script
                .as_deref()
                .ok_or_else(|| anyhow!("skill tool has no script defined"))?;

            let launch = SkillLaunchSpec::new(&skill.skill_dir, script)?;
            let output = self
                .run_skill_process(&launch, &params, timeout_seconds)
                .await?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!(
                    "skill script exited with {}: {}",
                    output.status,
                    stderr
                ));
            }

            outputs.push(self.format_skill_output(
                &tool.slug,
                String::from_utf8_lossy(&output.stdout).as_ref(),
            )?);
        }

        Ok(outputs.join("\n\n"))
    }

    fn exec_activate_skill(&self, arguments: Value) -> Result<String> {
        let registry = self
            .skills
            .as_ref()
            .ok_or_else(|| anyhow!("skills registry not available"))?;
        let skill_name = arguments
            .get("name")
            .or_else(|| arguments.get("skill_name"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("activate_skill: missing 'name'"))?;

        let activation = registry.activate_skill_record(skill_name)?;
        self.store
            .upsert_activated_skill(&self.conversation_id, activation.clone())?;
        Ok(activation.content)
    }

    async fn run_skill_process(
        &self,
        launch: &SkillLaunchSpec,
        params: &Value,
        timeout_seconds: u64,
    ) -> Result<std::process::Output> {
        match &launch.program {
            SkillProgram::Direct(_) => {
                let mut cmd = launch.command_with_interpreter(None);
                self.apply_filesystem_env(&mut cmd);
                apply_skill_arguments(&mut cmd, params);
                self.run_child_command(cmd, timeout_seconds)
                    .await
                    .map_err(|err| {
                        anyhow!(
                            "failed to execute skill script {}: {err}",
                            launch.display_program()
                        )
                    })
            }
            SkillProgram::Python(_) => {
                let mut primary = launch.command_with_interpreter(Some("python3"));
                self.apply_filesystem_env(&mut primary);
                apply_skill_arguments(&mut primary, params);
                match self.run_child_command(primary, timeout_seconds).await {
                    Ok(output) => Ok(output),
                    Err(err)
                        if err.downcast_ref::<std::io::Error>().is_some_and(|io_err| {
                            io_err.kind() == std::io::ErrorKind::NotFound
                        }) =>
                    {
                        let mut fallback = launch.command_with_interpreter(Some("python"));
                        self.apply_filesystem_env(&mut fallback);
                        apply_skill_arguments(&mut fallback, params);
                        self.run_child_command(fallback, timeout_seconds)
                            .await
                            .map_err(|err| {
                                anyhow!(
                                    "failed to execute python skill script {} with python3 or python: {err}",
                                    launch.display_program()
                                )
                            })
                    }
                    Err(err) => Err(anyhow!(
                        "failed to execute python skill script {} with python3: {err}",
                        launch.display_program()
                    )),
                }
            }
        }
    }

    async fn run_child_command(
        &self,
        mut cmd: tokio::process::Command,
        timeout_seconds: u64,
    ) -> Result<std::process::Output> {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to capture child stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("failed to capture child stderr"))?;

        let stdout_task = tokio::spawn(async move { read_stream(stdout).await });
        let stderr_task = tokio::spawn(async move { read_stream(stderr).await });

        let wait_result =
            tokio::time::timeout(Duration::from_secs(timeout_seconds), child.wait()).await;
        let status = match wait_result {
            Ok(status) => status?,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(anyhow!(
                    "skill execution timed out after {}s. If this is a long-running remote job, prefer a skill that returns a pending job record for async follow-up.",
                    timeout_seconds
                ));
            }
        };

        let stdout = stdout_task.await??;
        let stderr = stderr_task.await??;
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    }

    fn format_skill_output(&self, tool_slug: &str, stdout: &str) -> Result<String> {
        if let Some(envelope) = parse_skill_envelope(stdout)? {
            match envelope.status.as_str() {
                "ok" => {
                    let body = envelope.output.unwrap_or_else(|| stdout.trim().to_string());
                    Ok(format!("[{tool_slug}]\n{body}"))
                }
                "pending" => {
                    self.require_filesystem_write("local__run_skill pending job persistence")?;
                    let pending = envelope.pending_job.ok_or_else(|| {
                        anyhow!("pending skill response is missing a job payload")
                    })?;
                    let record = self.build_pending_job_record(tool_slug, pending)?;
                    let alias = record.alias.clone();
                    let transaction_id = record.transaction_id.clone();
                    let next_poll_at = record.next_poll_at.map(|value| value.to_rfc3339());
                    self.upsert_job(record)?;
                    Ok(format!(
                        "[{tool_slug}]\nRemote job stored for follow-up.\nAlias: `{alias}`\nTransaction ID: `{transaction_id}`{}\nUse `local__get_job` or `local__list_jobs` to inspect it, or let auto-pull continue if configured.",
                        next_poll_at
                            .map(|value| format!("\nNext poll at: `{value}`"))
                            .unwrap_or_default()
                    ))
                }
                other => Err(anyhow!("unsupported skill response status '{other}'")),
            }
        } else {
            Ok(format!("[{tool_slug}]\n{}", stdout))
        }
    }

    fn build_pending_job_record(
        &self,
        tool_slug: &str,
        pending: PendingSkillJob,
    ) -> Result<RememberedJob> {
        let alias = pending
            .alias
            .unwrap_or_else(|| format!("{tool_slug}-{}", pending.transaction_id));
        let mut record = RememberedJob::new(alias, pending.transaction_id)?;
        record.source_tool = Some(tool_slug.to_string());
        record.status = Some(pending.status.unwrap_or_else(|| "pending".to_string()));
        record.notes = pending.notes;
        record.set_mode(Some(
            pending.mode.unwrap_or_else(|| "auto_pull".to_string()),
        ))?;
        record.set_poll_interval_seconds(Some(pending.poll_interval_seconds.unwrap_or(30)))?;
        record.next_poll_at = match pending.next_poll_at {
            Some(value) => Some(value),
            None => record
                .poll_interval_seconds
                .map(|seconds| Utc::now() + ChronoDuration::seconds(seconds as i64)),
        };
        record.result_expires_at = pending.result_expires_at;
        record.automation_prompt = pending.automation_prompt;
        record.retrieval_state = pending.retrieval_state;
        record.result_artifacts_json = pending.result_artifacts_json;
        record.last_error = pending.last_error;
        Ok(record)
    }

    fn resolve_existing_path(&self, raw_path: &str, capability: &str) -> Result<PathBuf> {
        let candidate = self.path_candidate(raw_path)?;
        let resolved = fs::canonicalize(&candidate)
            .with_context(|| format!("{capability}: failed to resolve {}", candidate.display()))?;
        self.require_path_scope(&resolved, capability)?;
        Ok(resolved)
    }

    fn resolve_write_path(&self, raw_path: &str, capability: &str) -> Result<PathBuf> {
        let candidate = self.path_candidate(raw_path)?;
        if candidate.file_name().is_none() {
            bail!("{capability}: path must include a file name");
        }

        if candidate.exists() {
            let resolved = fs::canonicalize(&candidate).with_context(|| {
                format!("{capability}: failed to resolve {}", candidate.display())
            })?;
            self.require_path_scope(&resolved, capability)?;
            return Ok(resolved);
        }

        let parent = candidate
            .parent()
            .ok_or_else(|| anyhow!("{capability}: path must include a parent directory"))?;
        let resolved_parent = fs::canonicalize(parent).with_context(|| {
            format!(
                "{capability}: failed to resolve parent {}",
                parent.display()
            )
        })?;
        if !resolved_parent.is_dir() {
            bail!(
                "{capability}: parent path is not a directory: {}",
                resolved_parent.display()
            );
        }
        self.require_path_scope(&resolved_parent, capability)?;
        Ok(resolved_parent.join(candidate.file_name().unwrap()))
    }

    fn path_candidate(&self, raw_path: &str) -> Result<PathBuf> {
        let raw_path = raw_path.trim();
        if raw_path.is_empty() {
            bail!("path must not be empty");
        }
        let path = Path::new(raw_path);
        Ok(if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        })
    }

    fn require_path_scope(&self, resolved_path: &Path, capability: &str) -> Result<()> {
        if self.permissions.allows_full_filesystem()
            || resolved_path.starts_with(&self.workspace_root)
        {
            return Ok(());
        }
        bail!(
            "permission denied: {capability} requires full filesystem access for path '{}' outside workspace root '{}'. Enable it with /permissions fs-scope full, or use /yolo on.",
            resolved_path.display(),
            self.workspace_root.display()
        )
    }

    fn effective_filesystem_scope_label(&self) -> &'static str {
        if self.permissions.allows_full_filesystem() {
            "full"
        } else {
            "workspace"
        }
    }

    fn apply_filesystem_env(&self, cmd: &mut tokio::process::Command) {
        cmd.env("RUSTY_BIDULE_FILESYSTEM_ROOT", &self.workspace_root)
            .env(
                "RUSTY_BIDULE_FILESYSTEM_SCOPE",
                self.effective_filesystem_scope_label(),
            )
            .env(
                "RUSTY_BIDULE_FILESYSTEM_ACCESS",
                if self.permissions.yolo {
                    "all"
                } else {
                    self.permissions.filesystem.label()
                },
            );
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

fn parse_optional_datetime(value: Option<&Value>) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let raw = value
        .as_str()
        .ok_or_else(|| anyhow!("expected RFC3339 timestamp string"))?;
    let parsed = chrono::DateTime::parse_from_rfc3339(raw)
        .with_context(|| format!("invalid RFC3339 timestamp '{raw}'"))?;
    Ok(Some(parsed.with_timezone(&chrono::Utc)))
}

fn required_string_arg(arguments: &Value, field: &str, tool_name: &str) -> Result<String> {
    let value = arguments
        .get(field)
        .ok_or_else(|| anyhow!("{tool_name}: missing '{field}'"))?;
    let value = value
        .as_str()
        .ok_or_else(|| anyhow!("{tool_name}: '{field}' must be a string"))?
        .trim();
    if value.is_empty() {
        bail!("{tool_name}: '{field}' must not be empty");
    }
    Ok(value.to_string())
}

fn optional_string_arg(arguments: &Value, field: &str, tool_name: &str) -> Result<Option<String>> {
    let Some(value) = arguments.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let value = value
        .as_str()
        .ok_or_else(|| anyhow!("{tool_name}: '{field}' must be a string or null"))?
        .trim();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

fn optional_u64_arg(arguments: &Value, field: &str, tool_name: &str) -> Result<Option<u64>> {
    let Some(value) = arguments.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_u64()
        .map(Some)
        .ok_or_else(|| anyhow!("{tool_name}: '{field}' must be an unsigned integer or null"))
}

fn memory_patch_value<'a>(arguments: &'a Value, field: &str) -> Option<&'a Value> {
    arguments
        .get(field)
        .or_else(|| arguments.get("memory").and_then(|memory| memory.get(field)))
}

fn update_memory_array(
    target: &mut Vec<Value>,
    value: Option<&Value>,
    replace: bool,
    field: &str,
) -> Result<bool> {
    let Some(value) = value else {
        return Ok(false);
    };
    let items = memory_array_items(value, field)?;
    if replace {
        *target = dedupe_memory_items(items);
    } else {
        for item in items {
            if !target.contains(&item) {
                target.push(item);
            }
        }
    }
    Ok(true)
}

fn dedupe_memory_items(items: Vec<Value>) -> Vec<Value> {
    let mut unique = Vec::new();
    for item in items {
        if !unique.contains(&item) {
            unique.push(item);
        }
    }
    unique
}

fn memory_array_items(value: &Value, field: &str) -> Result<Vec<Value>> {
    match value {
        Value::Array(items) => Ok(items.clone()),
        Value::Null => Ok(Vec::new()),
        other => bail!("update_investigation_memory: {field} must be an array, got {other}"),
    }
}

fn is_advertised_local_tool_name(name: &str) -> bool {
    matches!(
        name,
        "local__sleep"
            | "local__remember_job"
            | "local__update_job"
            | "local__get_job"
            | "local__list_jobs"
            | "local__forget_job"
            | "local__get_investigation_memory"
            | "local__update_investigation_memory"
            | "local__clear_investigation_memory"
            | "local__search_conversation_memories"
            | "local__time"
            | "local__configure_mcp_servers"
            | "local__exec_cli"
            | "local__list_directory"
            | "local__read_file"
            | "local__write_file"
            | "local__activate_skill"
            | "local__run_skill"
    )
}

#[derive(Debug, serde::Deserialize)]
struct SkillEnvelope {
    status: String,
    #[serde(default)]
    output: Option<String>,
    #[serde(default, rename = "job")]
    pending_job: Option<PendingSkillJob>,
}

#[derive(Debug, serde::Deserialize)]
struct PendingSkillJob {
    #[serde(default)]
    alias: Option<String>,
    transaction_id: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    poll_interval_seconds: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_datetime")]
    next_poll_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, deserialize_with = "deserialize_optional_datetime")]
    result_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    automation_prompt: Option<String>,
    #[serde(default)]
    retrieval_state: Option<String>,
    #[serde(default)]
    result_artifacts_json: Option<Value>,
    #[serde(default)]
    last_error: Option<String>,
}

fn parse_skill_envelope(stdout: &str) -> Result<Option<SkillEnvelope>> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() || !trimmed.starts_with('{') {
        return Ok(None);
    }

    match serde_json::from_str::<SkillEnvelope>(trimmed) {
        Ok(value) => Ok(Some(value)),
        Err(_) => Ok(None),
    }
}

fn deserialize_optional_datetime<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<chrono::DateTime<chrono::Utc>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    value
        .map(|raw| {
            chrono::DateTime::parse_from_rfc3339(&raw)
                .map(|value| value.with_timezone(&chrono::Utc))
                .map_err(serde::de::Error::custom)
        })
        .transpose()
}

fn default_workspace_root() -> PathBuf {
    std::env::current_dir()
        .map(|path| canonicalize_existing_or_self(&path))
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn canonicalize_existing_or_self(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn bytes_to_lower_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn decode_hex_bytes(raw: &str) -> Result<Vec<u8>> {
    let stripped = raw
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let stripped = stripped
        .strip_prefix("0x")
        .or_else(|| stripped.strip_prefix("0X"))
        .unwrap_or(&stripped);
    if stripped.len() % 2 != 0 {
        bail!("write_file: hex input must contain an even number of hex digits");
    }
    let mut out = Vec::with_capacity(stripped.len() / 2);
    for index in (0..stripped.len()).step_by(2) {
        let byte = u8::from_str_radix(&stripped[index..index + 2], 16)
            .with_context(|| format!("write_file: invalid hex byte at offset {index}"))?;
        out.push(byte);
    }
    Ok(out)
}

async fn read_stream<R>(mut reader: R) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer).await?;
    Ok(buffer)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, os::unix::fs::symlink, path::Path};

    use serde_json::{Value, json};
    use tempfile::tempdir;
    use tokio::time::Duration;

    use crate::{
        config::LocalToolsConfig,
        conversation_store::ConversationStore,
        skills::SkillRegistry,
        types::{AgentPermissions, FilesystemAccess, FilesystemScope},
    };

    use super::{LocalToolExecutor, SkillLaunchSpec, SkillProgram};

    fn file_tool_executor(root: &Path, permissions: AgentPermissions) -> LocalToolExecutor {
        let store = ConversationStore::new(root.join(".agent-data"), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            None,
            permissions,
            None,
            Duration::from_secs(5),
            Vec::new(),
        )
        .with_workspace_root(root)
    }

    #[test]
    fn python_scripts_are_resolved_via_interpreter() {
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("webex-room-conversation");
        let scripts_dir = skill_dir.join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        let script_path = scripts_dir.join("webex_room_message_fetch.py");
        fs::write(&script_path, "#!/usr/bin/env python3\nprint('ok')\n").unwrap();
        let script_path = std::fs::canonicalize(script_path).unwrap();

        let launch =
            SkillLaunchSpec::new(&skill_dir, "scripts/webex_room_message_fetch.py").unwrap();

        assert_eq!(
            launch.current_dir,
            std::fs::canonicalize(skill_dir).unwrap()
        );
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
        let script_path = std::fs::canonicalize(script_path).unwrap();

        let launch = SkillLaunchSpec::new(&skill_dir, "scripts/tool.sh").unwrap();

        assert_eq!(
            launch.current_dir,
            std::fs::canonicalize(skill_dir).unwrap()
        );
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
                filesystem_scope: Default::default(),
                yolo: false,
            },
            None,
            Duration::from_secs(180),
            Vec::new(),
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
    async fn job_storage_rejects_invalid_identifiers_and_poll_intervals() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            None,
            AgentPermissions {
                allow_network: false,
                filesystem: FilesystemAccess::ReadWrite,
                filesystem_scope: Default::default(),
                yolo: false,
            },
            None,
            Duration::from_secs(180),
            Vec::new(),
        );

        let err = executor
            .execute(
                "local__remember_job",
                json!({"alias": " ", "transaction_id": "123"}),
            )
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("'alias' must not be empty"));

        let err = executor
            .execute(
                "local__remember_job",
                json!({"alias": "demo", "transaction_id": "123", "poll_interval_seconds": 0}),
            )
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("poll_interval_seconds"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn update_job_rejects_unknown_mode() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            None,
            AgentPermissions {
                allow_network: false,
                filesystem: FilesystemAccess::ReadWrite,
                filesystem_scope: Default::default(),
                yolo: false,
            },
            None,
            Duration::from_secs(180),
            Vec::new(),
        );
        executor
            .execute(
                "local__remember_job",
                json!({"alias": "demo", "transaction_id": "123"}),
            )
            .await
            .unwrap();

        let err = executor
            .execute(
                "local__update_job",
                json!({"alias": "demo", "mode": "manual"}),
            )
            .await
            .unwrap_err();

        assert!(format!("{err:#}").contains("job mode must be 'auto_pull'"));
    }

    #[test]
    fn known_local_tool_can_be_disabled_by_filter() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            None,
            AgentPermissions::default(),
            Some(vec!["local__time".to_string()]),
            Duration::from_secs(5),
            Vec::new(),
        );

        assert!(executor.is_known_local_tool("local__run_skill"));
        assert!(!executor.is_local_tool("local__run_skill"));
        assert!(executor.is_local_tool("local__time"));
        assert!(!executor.is_known_local_tool("mcp__demo_tool"));
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
            None,
            Duration::from_secs(180),
            Vec::new(),
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

    #[tokio::test(flavor = "current_thread")]
    async fn activate_skill_loads_skill_body_and_resources() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
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

# Demo Skill

Use `scripts/run.py`.
"#,
        )
        .unwrap();

        let skills = SkillRegistry::load(&skills_dir).unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            Some(skills),
            AgentPermissions::default(),
            None,
            Duration::from_secs(5),
            Vec::new(),
        );

        let output = executor
            .execute("local__activate_skill", json!({"name": "demo"}))
            .await
            .unwrap();

        assert!(output.contains("<skill_content name=\"demo\">"));
        assert!(output.contains("# Demo Skill"));
        assert!(output.contains("<file>scripts/run.py</file>"));
        assert!(!output.contains("description: Demo activation skill"));

        let activated = executor
            .store
            .load_activated_skills(&conversation.conversation_id)
            .unwrap();
        assert_eq!(activated.len(), 1);
        assert_eq!(activated[0].name, "demo");
        assert!(activated[0].content_hash.len() >= 64);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_skill_stores_pending_jobs_from_structured_output() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let skills_dir = dir.path().join("skills");
        let skill_dir = skills_dir.join("splunk-demo");
        fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        fs::write(
            skill_dir.join("scripts/submit.py"),
            r#"import json
print(json.dumps({
  "status": "pending",
  "job": {
    "alias": "splunk-search",
    "transaction_id": "sid-123",
    "status": "running",
    "poll_interval_seconds": 45,
    "automation_prompt": "Poll the Splunk job sid-123 and summarize the result when it finishes.",
    "retrieval_state": "submitted"
  }
}))
"#,
        )
        .unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: splunk-demo
description: Demo long-running Splunk search submission
---

Tools:
  - name: Submit
    slug: submit
    script: scripts/submit.py
    filesystem: read_write
"#,
        )
        .unwrap();

        let skills = SkillRegistry::load(&skills_dir).unwrap();
        let executor = LocalToolExecutor::new(
            store.clone(),
            &conversation.conversation_id,
            Some(skills),
            AgentPermissions {
                allow_network: false,
                filesystem: FilesystemAccess::ReadWrite,
                filesystem_scope: Default::default(),
                yolo: false,
            },
            None,
            Duration::from_secs(180),
            Vec::new(),
        );

        let output = executor
            .execute(
                "local__run_skill",
                json!({"skill_name": "splunk-demo", "tool_slug": "submit"}),
            )
            .await
            .unwrap();

        assert!(output.contains("Remote job stored for follow-up."));
        let jobs = store.load_job_state(&conversation.conversation_id).unwrap();
        let job = jobs
            .iter()
            .find(|job| job.alias == "splunk-search")
            .unwrap();
        assert_eq!(job.transaction_id, "sid-123");
        assert_eq!(job.mode.as_deref(), Some("auto_pull"));
        assert_eq!(job.poll_interval_seconds, Some(45));
        assert_eq!(job.retrieval_state.as_deref(), Some("submitted"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_skill_rejects_invalid_pending_job_metadata() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let skills_dir = dir.path().join("skills");
        let skill_dir = skills_dir.join("bad-job-demo");
        fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        fs::write(
            skill_dir.join("scripts/submit.py"),
            r#"import json
print(json.dumps({
  "status": "pending",
  "job": {
    "alias": "bad-job",
    "transaction_id": "sid-123",
    "poll_interval_seconds": 0
  }
}))
"#,
        )
        .unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: bad-job-demo
description: Demo invalid pending job metadata
---

Tools:
  - name: Submit
    slug: submit
    script: scripts/submit.py
    filesystem: read_write
"#,
        )
        .unwrap();

        let skills = SkillRegistry::load(&skills_dir).unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            Some(skills),
            AgentPermissions {
                allow_network: false,
                filesystem: FilesystemAccess::ReadWrite,
                filesystem_scope: Default::default(),
                yolo: false,
            },
            None,
            Duration::from_secs(180),
            Vec::new(),
        );

        let err = executor
            .execute(
                "local__run_skill",
                json!({"skill_name": "bad-job-demo", "tool_slug": "submit"}),
            )
            .await
            .unwrap_err();

        assert!(format!("{err:#}").contains("poll_interval_seconds"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_skill_warns_and_soft_fails_invalid_parameters_json() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let skills_dir = dir.path().join("skills");
        let skill_dir = skills_dir.join("echo-skill");
        fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        fs::write(
            skill_dir.join("scripts/echo.py"),
            "import json\nimport sys\nprint(json.dumps({'argv': sys.argv[1:]}))\n",
        )
        .unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: echo-skill
description: Echo script arguments
---

Tools:
  - name: Echo
    slug: echo
    script: scripts/echo.py
"#,
        )
        .unwrap();

        let skills = SkillRegistry::load(&skills_dir).unwrap();
        let executor = LocalToolExecutor::new(
            store.clone(),
            &conversation.conversation_id,
            Some(skills),
            AgentPermissions::default(),
            None,
            Duration::from_secs(5),
            Vec::new(),
        );

        let output = executor
            .execute(
                "local__run_skill",
                json!({
                    "skill_name": "echo-skill",
                    "tool_slug": "echo",
                    "parameters": "{not-json"
                }),
            )
            .await
            .unwrap();

        assert!(output.contains("[warning]"));
        assert!(output.contains("parameters was not valid JSON"));
        assert!(output.contains("\"argv\": []"));

        let log_path = store
            .conversation_dir(&conversation.conversation_id)
            .unwrap()
            .join("logs/conversation.log");
        let log = fs::read_to_string(log_path).unwrap();
        assert!(log.contains("parameters was not valid JSON"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_skill_enforces_timeout() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let skills_dir = dir.path().join("skills");
        let skill_dir = skills_dir.join("slow-skill");
        fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        let script_path = skill_dir.join("scripts/slow.sh");
        fs::write(&script_path, "#!/bin/sh\nsleep 2\necho done\n").unwrap();
        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: slow-skill
description: Slow script
---

Tools:
  - name: Slow
    slug: slow
    script: scripts/slow.sh
"#,
        )
        .unwrap();

        let skills = SkillRegistry::load(&skills_dir).unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            Some(skills),
            AgentPermissions::default(),
            None,
            Duration::from_secs(1),
            Vec::new(),
        );

        let err = executor
            .execute(
                "local__run_skill",
                json!({"skill_name": "slow-skill", "tool_slug": "slow", "timeout_seconds": 10}),
            )
            .await
            .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("timed out after"));
        assert!(message.contains("long-running remote job"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn exec_cli_rejects_disallowed_commands() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            None,
            AgentPermissions {
                allow_network: true,
                filesystem: FilesystemAccess::ReadOnly,
                filesystem_scope: Default::default(),
                yolo: false,
            },
            None,
            Duration::from_secs(5),
            vec!["echo".to_string()],
        );

        let err = executor
            .execute(
                "local__exec_cli",
                json!({"command": "whois", "args": ["example.com"]}),
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("is not allowed"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn exec_cli_runs_allowed_commands_with_direct_argv() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            None,
            AgentPermissions {
                allow_network: true,
                filesystem: FilesystemAccess::ReadOnly,
                filesystem_scope: Default::default(),
                yolo: false,
            },
            None,
            Duration::from_secs(5),
            vec!["echo".to_string()],
        );

        let output = executor
            .execute(
                "local__exec_cli",
                json!({"command": "echo", "args": ["hello", "world"]}),
            )
            .await
            .unwrap();

        assert!(output.contains("Command: `echo`"));
        assert!(output.contains("hello world"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn investigation_memory_tools_update_get_search_and_clear() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let executor = LocalToolExecutor::new(
            store.clone(),
            &conversation.conversation_id,
            None,
            AgentPermissions {
                allow_network: false,
                filesystem: FilesystemAccess::ReadWrite,
                filesystem_scope: Default::default(),
                yolo: false,
            },
            None,
            Duration::from_secs(5),
            Vec::new(),
        );

        executor
            .execute(
                "local__update_investigation_memory",
                json!({
                    "summary": "Investigating suspicious admin login",
                    "entities": [{"type": "user", "value": "alice@example.com"}],
                    "unresolved_questions": ["Confirm whether MFA challenge succeeded"]
                }),
            )
            .await
            .unwrap();

        let output = executor
            .execute("local__get_investigation_memory", json!({}))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["summary"], "Investigating suspicious admin login");
        assert_eq!(parsed["entities"][0]["value"], "alice@example.com");
        assert_eq!(parsed["updated_by"], "local__update_investigation_memory");
        assert!(parsed["updated_at"].as_str().is_some());

        executor
            .execute(
                "local__update_investigation_memory",
                json!({
                    "entities": [{"type": "user", "value": "alice@example.com"}]
                }),
            )
            .await
            .unwrap();
        let memory = store
            .load_investigation_memory(&conversation.conversation_id)
            .unwrap();
        assert_eq!(memory.entities.len(), 1);

        let search = executor
            .execute(
                "local__search_conversation_memories",
                json!({"query": "alice@example.com"}),
            )
            .await
            .unwrap();
        let results: Value = serde_json::from_str(&search).unwrap();
        assert_eq!(results.as_array().unwrap().len(), 1);

        executor
            .execute("local__clear_investigation_memory", json!({}))
            .await
            .unwrap();
        assert!(
            store
                .load_investigation_memory(&conversation.conversation_id)
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn replace_investigation_memory_requires_explicit_fields() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let executor = LocalToolExecutor::new(
            store.clone(),
            &conversation.conversation_id,
            None,
            AgentPermissions {
                allow_network: false,
                filesystem: FilesystemAccess::ReadWrite,
                filesystem_scope: Default::default(),
                yolo: false,
            },
            None,
            Duration::from_secs(5),
            Vec::new(),
        );

        executor
            .execute(
                "local__update_investigation_memory",
                json!({
                    "summary": "Existing case context",
                    "entities": [{"type": "host", "value": "server-1"}]
                }),
            )
            .await
            .unwrap();

        let err = executor
            .execute(
                "local__update_investigation_memory",
                json!({"mode": "replace"}),
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("provide summary"));
        assert!(
            err.to_string()
                .contains("local__clear_investigation_memory")
        );

        let memory = store
            .load_investigation_memory(&conversation.conversation_id)
            .unwrap();
        assert_eq!(memory.summary, "Existing case context");
        assert_eq!(memory.entities[0]["value"], "server-1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_directory_returns_sorted_paginated_entries() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        let executor = file_tool_executor(dir.path(), AgentPermissions::default());

        let output = executor
            .execute("local__list_directory", json!({"path": ".", "limit": 2}))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["returned_entries"], 2);
        assert_eq!(parsed["eof"], false);
        assert_eq!(parsed["entries"][0]["name"], ".agent-data");
        assert_eq!(parsed["entries"][1]["name"], "a.txt");
        assert_eq!(parsed["next_offset"], 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_file_supports_text_hex_offsets_and_caps() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("note.txt"), "hello world").unwrap();
        fs::write(dir.path().join("blob.bin"), [0_u8, 1, 254, 255]).unwrap();
        let executor = file_tool_executor(dir.path(), AgentPermissions::default())
            .with_local_tools_config(&LocalToolsConfig {
                max_file_read_bytes: 4,
                ..LocalToolsConfig::default()
            });

        let text = executor
            .execute(
                "local__read_file",
                json!({"path": "note.txt", "offset": 6, "length": 99, "format": "text"}),
            )
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["text"], "worl");
        assert_eq!(parsed["truncated_by_cap"], true);
        assert_eq!(parsed["next_offset"], 10);

        let hex = executor
            .execute(
                "local__read_file",
                json!({"path": "blob.bin", "format": "hex"}),
            )
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&hex).unwrap();
        assert_eq!(parsed["hex"], "0001feff");
        assert_eq!(parsed["eof"], true);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_file_rejects_invalid_utf8_text_chunks() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("blob.bin"), [0xff_u8]).unwrap();
        let executor = file_tool_executor(dir.path(), AgentPermissions::default());

        let err = executor
            .execute(
                "local__read_file",
                json!({"path": "blob.bin", "format": "text"}),
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("not valid UTF-8"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_file_creates_overwrites_hex_and_enforces_permissions() {
        let dir = tempdir().unwrap();
        let executor = file_tool_executor(
            dir.path(),
            AgentPermissions {
                filesystem: FilesystemAccess::ReadWrite,
                ..AgentPermissions::default()
            },
        );

        let output = executor
            .execute(
                "local__write_file",
                json!({"path": "created.txt", "text": "hello"}),
            )
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["bytes_written"], 5);
        assert_eq!(
            fs::read_to_string(dir.path().join("created.txt")).unwrap(),
            "hello"
        );

        let err = executor
            .execute(
                "local__write_file",
                json!({"path": "created.txt", "text": "again"}),
            )
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("failed to open"));

        executor
            .execute(
                "local__write_file",
                json!({"path": "created.txt", "mode": "overwrite", "hex": "00 ff"}),
            )
            .await
            .unwrap();
        assert_eq!(
            fs::read(dir.path().join("created.txt")).unwrap(),
            vec![0, 255]
        );

        let read_only = file_tool_executor(dir.path(), AgentPermissions::default());
        let err = read_only
            .execute(
                "local__write_file",
                json!({"path": "denied.txt", "text": "no"}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("filesystem write access"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn file_tools_restrict_paths_to_workspace_without_full_scope() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        let outside = dir.path().join("outside.txt");
        fs::create_dir(&workspace).unwrap();
        fs::write(workspace.join("inside.txt"), "inside").unwrap();
        fs::write(&outside, "outside").unwrap();
        symlink(&outside, workspace.join("outside-link")).unwrap();

        let executor = file_tool_executor(&workspace, AgentPermissions::default());
        executor
            .execute(
                "local__read_file",
                json!({"path": "inside.txt", "format": "text"}),
            )
            .await
            .unwrap();
        executor
            .execute(
                "local__read_file",
                json!({"path": workspace.join("inside.txt").display().to_string(), "format": "text"}),
            )
            .await
            .unwrap();

        let err = executor
            .execute(
                "local__read_file",
                json!({"path": "../outside.txt", "format": "text"}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("full filesystem access"));

        let err = executor
            .execute(
                "local__read_file",
                json!({"path": "outside-link", "format": "text"}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("full filesystem access"));

        let full_scope = file_tool_executor(
            &workspace,
            AgentPermissions {
                filesystem_scope: FilesystemScope::Full,
                ..AgentPermissions::default()
            },
        );
        let output = full_scope
            .execute(
                "local__read_file",
                json!({"path": "../outside.txt", "format": "text"}),
            )
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["text"], "outside");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_file_rejects_missing_parent_and_large_payloads() {
        let dir = tempdir().unwrap();
        let executor = file_tool_executor(
            dir.path(),
            AgentPermissions {
                filesystem: FilesystemAccess::ReadWrite,
                ..AgentPermissions::default()
            },
        )
        .with_local_tools_config(&LocalToolsConfig {
            max_file_write_bytes: 2,
            ..LocalToolsConfig::default()
        });

        let err = executor
            .execute(
                "local__write_file",
                json!({"path": "missing/created.txt", "text": "a"}),
            )
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("failed to resolve parent"));

        let err = executor
            .execute(
                "local__write_file",
                json!({"path": "too-large.txt", "text": "abc"}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("max_file_write_bytes"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn time_tool_returns_current_time_fields() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            None,
            AgentPermissions::default(),
            None,
            Duration::from_secs(5),
            Vec::new(),
        );

        let output = executor.execute("local__time", json!({})).await.unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();

        assert!(parsed.get("now_utc").is_some());
        assert!(parsed.get("now_local").is_some());
        assert!(parsed.get("reference_utc").is_some());
        assert!(parsed.get("reference_local").is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn time_tool_returns_trailing_window_fields() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let executor = LocalToolExecutor::new(
            store,
            &conversation.conversation_id,
            None,
            AgentPermissions::default(),
            None,
            Duration::from_secs(5),
            Vec::new(),
        );

        let output = executor
            .execute("local__time", json!({"trailing_hours": 12, "days_ago": 2}))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["window_label"], "last 12 hours");
        assert!(parsed.get("window_start_utc").is_some());
        assert!(parsed.get("window_end_utc").is_some());
        assert!(parsed.get("window_start_local").is_some());
        assert!(parsed.get("window_end_local").is_some());
    }
}

pub fn local_tool_definitions(
    enabled_local_tools: Option<&[String]>,
    local_tools_config: &LocalToolsConfig,
    skills: Option<&SkillRegistry>,
) -> Vec<LlmTool> {
    let mut defs = vec![
        LlmTool {
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
        LlmTool {
            name: "local__remember_job".to_string(),
            description: "Store a job/transaction alias for later retrieval within this conversation. Supports automation metadata. Requires filesystem write permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "alias": {"type": "string", "description": "Short name to refer to this job"},
                    "transaction_id": {"type": "string", "description": "The actual transaction or job ID"},
                    "source_tool": {"type": "string", "description": "Which tool created this job"},
                    "status": {"type": "string", "description": "Current job status"},
                    "notes": {"type": "string", "description": "Additional notes"},
                    "mode": {"type": "string", "description": "Tracking mode, for example auto_pull"},
                    "poll_interval_seconds": {"type": "integer"},
                    "next_poll_at": {"type": "string", "description": "RFC3339 timestamp"},
                    "lease_expires_at": {"type": "string", "description": "RFC3339 timestamp"},
                    "result_expires_at": {"type": "string", "description": "RFC3339 timestamp"},
                    "automation_prompt": {"type": "string"},
                    "retrieval_state": {"type": "string"},
                    "result_artifacts_json": {},
                    "last_error": {"type": "string"}
                },
                "required": ["alias", "transaction_id"]
            }),
        },
        LlmTool {
            name: "local__update_job".to_string(),
            description: "Update a stored job record within this conversation. Requires filesystem write permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "alias": {"type": "string"},
                    "transaction_id": {"type": "string"},
                    "source_tool": {"type": "string"},
                    "status": {"type": "string"},
                    "notes": {"type": "string"},
                    "mode": {"type": "string"},
                    "poll_interval_seconds": {"type": "integer"},
                    "next_poll_at": {"type": "string"},
                    "lease_expires_at": {"type": "string"},
                    "result_expires_at": {"type": "string"},
                    "automation_prompt": {"type": "string"},
                    "retrieval_state": {"type": "string"},
                    "result_artifacts_json": {},
                    "last_error": {"type": "string"}
                },
                "required": ["alias"]
            }),
        },
        LlmTool {
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
        LlmTool {
            name: "local__list_jobs".to_string(),
            description: "List all stored jobs in this conversation. Requires filesystem read permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
        LlmTool {
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
        LlmTool {
            name: "local__get_investigation_memory".to_string(),
            description: "Return the durable investigation memory for this conversation. Use before resuming an ongoing case or writing handover context. Requires filesystem read permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
        LlmTool {
            name: "local__update_investigation_memory".to_string(),
            description: "Merge or replace durable investigation memory for this conversation. Use to preserve case summary, entities, timeline, decisions, hypotheses, trusted sources, and unresolved questions. Requires filesystem write permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "mode": {"type": "string", "enum": ["merge", "replace"], "description": "merge appends array fields and replaces summary; replace rewrites the memory from provided fields"},
                    "summary": {"type": "string"},
                    "entities": {"type": "array", "items": {}},
                    "timeline": {"type": "array", "items": {}},
                    "decisions": {"type": "array", "items": {}},
                    "hypotheses": {"type": "array", "items": {}},
                    "trusted_sources": {"type": "array", "items": {}},
                    "unresolved_questions": {"type": "array", "items": {}},
                    "memory": {
                        "type": "object",
                        "description": "Optional object containing any of the stable memory fields"
                    }
                }
            }),
        },
        LlmTool {
            name: "local__clear_investigation_memory".to_string(),
            description: "Clear the durable investigation memory for this conversation. Requires filesystem write permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
        LlmTool {
            name: "local__search_conversation_memories".to_string(),
            description: "Search durable investigation memories across all conversations. Use to find prior case context, related entities, prior decisions, or unresolved questions. Requires filesystem read permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Case term, entity, source, or decision text to search for"}
                },
                "required": ["query"]
            }),
        },
        LlmTool {
            name: "local__time".to_string(),
            description: "Return the current UTC and local time, plus optional relative-time calculations. Use this before reasoning about windows like last 12 hours, last 2 days, today, or yesterday.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "hours_ago": {"type": "integer", "description": "Optional reference offset in hours before now"},
                    "days_ago": {"type": "integer", "description": "Optional reference offset in days before now"},
                    "trailing_hours": {"type": "integer", "description": "Optional trailing window size in hours ending now"},
                    "trailing_days": {"type": "integer", "description": "Optional trailing window size in days ending now"}
                }
            }),
        },
        LlmTool {
            name: "local__configure_mcp_servers".to_string(),
            description: "Update the conversation-scoped MCP server selection for subsequent turns. Use this to focus tool discovery when the MCP inventory exceeds the current tool budget. Requires filesystem write permission.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["enable", "disable", "only", "reset"]},
                    "server_names": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["action"]
            }),
        },
        LlmTool {
            name: "local__list_directory".to_string(),
            description: format!(
                "List immediate entries in a local directory. Paths are scoped to the workspace unless filesystem_scope is full. Requires filesystem read permission. Results are sorted by name and paginated; limit is capped at {}.",
                local_tools_config.max_directory_entries
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory path. Relative paths resolve under the workspace root. Defaults to ."},
                    "offset": {"type": "integer", "description": "Zero-based entry offset for pagination"},
                    "limit": {"type": "integer", "description": "Maximum entries to return, capped by local_tools.max_directory_entries"}
                }
            }),
        },
        LlmTool {
            name: "local__read_file".to_string(),
            description: format!(
                "Read a bounded chunk from a local file as strict UTF-8 text or lowercase hex. Paths are scoped to the workspace unless filesystem_scope is full. Requires filesystem read permission. Length is capped at {} bytes.",
                local_tools_config.max_file_read_bytes
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path. Relative paths resolve under the workspace root."},
                    "offset": {"type": "integer", "description": "Byte offset to start reading from"},
                    "length": {"type": "integer", "description": "Maximum bytes to read, capped by local_tools.max_file_read_bytes"},
                    "format": {"type": "string", "enum": ["text", "hex"], "description": "Output format. text requires valid UTF-8; hex is binary-safe."}
                },
                "required": ["path"]
            }),
        },
        LlmTool {
            name: "local__write_file".to_string(),
            description: format!(
                "Create or overwrite a local file from UTF-8 text or hex-encoded bytes. Parent directories must already exist. Paths are scoped to the workspace unless filesystem_scope is full. Requires filesystem write permission. Payloads are capped at {} bytes.",
                local_tools_config.max_file_write_bytes
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path. Relative paths resolve under the workspace root."},
                    "mode": {"type": "string", "enum": ["create_new", "overwrite"], "description": "create_new refuses existing files; overwrite truncates or creates the file"},
                    "text": {"type": "string", "description": "UTF-8 text payload"},
                    "hex": {"type": "string", "description": "Hex-encoded binary payload; whitespace and optional 0x prefix are accepted"}
                },
                "required": ["path"]
            }),
        },
    ];

    if !local_tools_config.allowed_cli_tools.is_empty() {
        defs.push(LlmTool {
            name: "local__exec_cli".to_string(),
            description: format!(
                "Execute an allowed local CLI binary with direct argv execution only; no shell parsing, pipes, redirects, or paths. Allowed commands: {}. Requires network permission when the command performs remote lookups.",
                local_tools_config.allowed_cli_tools.join(", ")
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Allowed binary name to execute"},
                    "args": {"type": "array", "items": {"type": "string"}, "description": "Argument vector passed directly to the binary"},
                    "timeout_seconds": {"type": "integer", "description": "Optional timeout override capped by local_tools.execution_timeout_seconds"}
                },
                "required": ["command"]
            }),
        });
    }

    if let Some(skills) = skills
        && !skills.is_empty()
    {
        let skill_names = skills.skill_names();
        let name_schema = if skill_names.is_empty() {
            json!({"type": "string", "description": "Agent Skills name to activate"})
        } else {
            json!({
                "type": "string",
                "enum": skill_names,
                "description": "Agent Skills name to activate"
            })
        };
        defs.push(LlmTool {
            name: "local__activate_skill".to_string(),
            description: "Load the full instructions for a discovered Agent Skills SKILL.md by name. Returns the Markdown body wrapped in skill_content tags plus the skill directory and bundled resource paths. Use before relying on a skill's detailed workflow.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": name_schema
                },
                "required": ["name"]
            }),
        });
    }

    defs.push(LlmTool {
        name: "local__run_skill".to_string(),
        description: "Execute one or more skill scripts with parameters. Omitting tool_slug runs every executable tool in the matched skill. Skill-specific network/filesystem permissions are enforced unless yolo mode is enabled. Local skill execution defaults to 180s and can be overridden with timeout_seconds. Scripts may return a JSON pending-job envelope so long-running remote work can be remembered and auto-polled.".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "skill_name": {"type": "string", "description": "Name of the skill directory"},
                "tool_slug": {"type": "string", "description": "Slug of the specific tool within the skill"},
                "parameters": {"type": "string", "description": "JSON string of parameters to pass to the script"},
                "timeout_seconds": {"type": "integer", "description": "Optional per-run timeout override for the local script execution"}
            },
            "required": ["skill_name"]
        }),
    });

    defs.into_iter()
        .filter(|tool| {
            enabled_local_tools
                .map(|enabled| enabled.iter().any(|name| name == &tool.name))
                .unwrap_or(true)
        })
        .collect()
}
