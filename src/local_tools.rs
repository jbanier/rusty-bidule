use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::time::Duration;

use crate::{azure::AzureTool, conversation_store::ConversationStore, skills::SkillRegistry};

pub struct LocalToolExecutor {
    store: ConversationStore,
    conversation_id: String,
    skills: Option<SkillRegistry>,
}

impl LocalToolExecutor {
    pub fn new(
        store: ConversationStore,
        conversation_id: impl Into<String>,
        skills: Option<SkillRegistry>,
    ) -> Self {
        Self {
            store,
            conversation_id: conversation_id.into(),
            skills,
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
        let jobs = self.load_jobs()?;
        if jobs.is_empty() {
            return Ok("No jobs stored.".to_string());
        }
        Ok(serde_json::to_string_pretty(&jobs)?)
    }

    fn exec_forget_job(&self, arguments: Value) -> Result<String> {
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

        if let Some(ref server) = tool.server {
            return Err(anyhow!(
                "MCP-backed skill tool '{server}' must be called via the MCP server directly"
            ));
        }

        let script = tool
            .script
            .as_deref()
            .ok_or_else(|| anyhow!("skill tool has no script defined"))?;

        let script_path = skill.skill_dir.join(script);
        let mut cmd = tokio::process::Command::new(&script_path);

        if let Some(obj) = params.as_object() {
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

        let output = cmd
            .output()
            .await
            .with_context(|| format!("failed to execute skill script {}", script_path.display()))?;

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
            description: "Store a job/transaction alias for later retrieval within this conversation.".to_string(),
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
            description: "Retrieve a stored job by alias.".to_string(),
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
            description: "List all stored jobs in this conversation.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
        AzureTool {
            name: "local__forget_job".to_string(),
            description: "Remove a stored job by alias.".to_string(),
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
            description: "Execute a skill script with parameters.".to_string(),
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
