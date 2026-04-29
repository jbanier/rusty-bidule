use std::{
    ffi::OsStr,
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
        Self {
            store,
            conversation_id: conversation_id.into(),
            skills,
            permissions,
            enabled_local_tools,
            execution_timeout,
            allowed_cli_tools,
        }
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
        let mut record = RememberedJob::new(alias.clone(), transaction_id);
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
        record.mode = arguments
            .get("mode")
            .and_then(Value::as_str)
            .map(str::to_string);
        record.poll_interval_seconds = arguments
            .get("poll_interval_seconds")
            .and_then(Value::as_u64);
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

        if let Some(value) = arguments.get("transaction_id").and_then(Value::as_str) {
            job.transaction_id = value.to_string();
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
            job.mode = arguments
                .get("mode")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if arguments.get("poll_interval_seconds").is_some() {
            job.poll_interval_seconds = arguments
                .get("poll_interval_seconds")
                .and_then(Value::as_u64);
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
                apply_skill_arguments(&mut primary, params);
                match self.run_child_command(primary, timeout_seconds).await {
                    Ok(output) => Ok(output),
                    Err(err)
                        if err.downcast_ref::<std::io::Error>().is_some_and(|io_err| {
                            io_err.kind() == std::io::ErrorKind::NotFound
                        }) =>
                    {
                        let mut fallback = launch.command_with_interpreter(Some("python"));
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
        let mut record = RememberedJob::new(alias, pending.transaction_id);
        record.source_tool = Some(tool_slug.to_string());
        record.status = Some(pending.status.unwrap_or_else(|| "pending".to_string()));
        record.notes = pending.notes;
        record.mode = Some(pending.mode.unwrap_or_else(|| "auto_pull".to_string()));
        record.poll_interval_seconds = Some(pending.poll_interval_seconds.unwrap_or(30));
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
    use std::{fs, os::unix::fs::PermissionsExt};

    use serde_json::{Value, json};
    use tempfile::tempdir;
    use tokio::time::Duration;

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
