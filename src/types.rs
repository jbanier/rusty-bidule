use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::LlmMessage;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemAccess {
    None,
    #[default]
    ReadOnly,
    ReadWrite,
}

impl FilesystemAccess {
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ReadOnly => "read_only",
            Self::ReadWrite => "read_write",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemScope {
    #[default]
    Workspace,
    Full,
}

impl FilesystemScope {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentPermissions {
    #[serde(default)]
    pub allow_network: bool,
    #[serde(default)]
    pub filesystem: FilesystemAccess,
    #[serde(default)]
    pub filesystem_scope: FilesystemScope,
    #[serde(default)]
    pub yolo: bool,
}

impl Default for AgentPermissions {
    fn default() -> Self {
        Self {
            allow_network: false,
            filesystem: FilesystemAccess::ReadOnly,
            filesystem_scope: FilesystemScope::Workspace,
            yolo: false,
        }
    }
}

impl AgentPermissions {
    pub fn allows_network(&self) -> bool {
        self.yolo || self.allow_network
    }

    pub fn allows_filesystem_read(&self) -> bool {
        self.yolo || !matches!(self.filesystem, FilesystemAccess::None)
    }

    pub fn allows_filesystem_write(&self) -> bool {
        self.yolo || matches!(self.filesystem, FilesystemAccess::ReadWrite)
    }

    pub fn allows_full_filesystem(&self) -> bool {
        self.yolo || matches!(self.filesystem_scope, FilesystemScope::Full)
    }

    pub fn summary(&self) -> String {
        format!(
            "network={} filesystem={} filesystem_scope={} yolo={}",
            if self.allows_network() { "on" } else { "off" },
            if self.yolo {
                "all"
            } else {
                self.filesystem.label()
            },
            if self.yolo {
                "full"
            } else {
                self.filesystem_scope.label()
            },
            if self.yolo { "on" } else { "off" }
        )
    }
}

pub fn permission_denied_user_prompt(error: &str) -> Option<String> {
    let requirement =
        if error.contains("requires network access") || error.contains("require network access") {
            Some((
                "network access",
                vec!["`/permissions network on`", "`/yolo on`"],
            ))
        } else if error.contains("requires filesystem write access")
            || error.contains("require filesystem write access")
        {
            Some((
                "filesystem write access",
                vec!["`/permissions fs write`", "`/yolo on`"],
            ))
        } else if error.contains("requires filesystem read access")
            || error.contains("require filesystem read access")
        {
            Some((
                "filesystem read access",
                vec![
                    "`/permissions fs read`",
                    "`/permissions fs write`",
                    "`/yolo on`",
                ],
            ))
        } else if error.contains("requires full filesystem access")
            || error.contains("require full filesystem access")
        {
            Some((
                "full filesystem access",
                vec!["`/permissions fs-scope full`", "`/yolo on`"],
            ))
        } else {
            None
        }?;

    let capability = error
        .strip_prefix("permission denied: ")
        .and_then(|rest| {
            rest.split_once(" requires ")
                .map(|(prefix, _)| prefix)
                .or_else(|| rest.split_once(" require ").map(|(prefix, _)| prefix))
        })
        .unwrap_or("This action");
    let commands = requirement.1.join(" or ");
    Some(format!(
        "{capability} was blocked because this conversation does not currently allow {}.\n\nEnable it and retry? Use {}.\nIf not, reply without changing permissions and I will continue without that action.",
        requirement.0, commands
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub conversation_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
    /// Name of the recipe currently loaded into this conversation (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_recipe: Option<String>,
    /// Per-conversation MCP server allowlist. `None` means all configured servers are active.
    /// `Some([])` means all configured servers are filtered out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_mcp_servers: Option<Vec<String>>,
    /// Checkpoint ID of the latest compaction summary stored in `compactions/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_compaction: Option<String>,
    /// Per-conversation allowlist for local tools. `None` means all built-in local tools are active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_local_tools: Option<Vec<String>>,
    /// Workflow run currently associated with this conversation, if one is active or paused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_workflow: Option<String>,
    /// Agent iteration budget override for this conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_budget: Option<AgentBudgetOverride>,
    /// Turn continuation currently waiting for operator approval, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_continuation: Option<String>,
    /// Protect this conversation from retention cleanup.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    /// Stronger cleanup protection for cases under legal or operational hold.
    #[serde(default, skip_serializing_if = "is_false")]
    pub legal_hold: bool,
    #[serde(default)]
    pub agent_permissions: AgentPermissions,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AgentBudgetOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations_per_turn: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_increment: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    /// Structured metadata present on assistant messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MessageMetadata>,
}

/// Metadata attached to every assistant reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// 1-based counter of assistant replies within this conversation.
    pub assistant_index: usize,
    pub timing: MessageTiming,
    pub tool_call_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_usage: Option<LlmUsage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<ToolArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTiming {
    /// Cumulative seconds spent executing tool calls.
    pub tool_seconds: f64,
    /// Seconds spent in LLM inference.
    pub llm_seconds: f64,
    /// Total wall-clock seconds for the turn.
    pub total_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivatedSkill {
    pub name: String,
    pub skill_dir: String,
    pub skill_md: String,
    pub content_hash: String,
    pub activated_at: DateTime<Utc>,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ConversationSummary {
    pub conversation_id: String,
    pub updated_at: DateTime<Utc>,
    pub message_count: usize,
    pub title: Option<String>,
    pub archived_at: Option<DateTime<Utc>>,
    pub preview: Option<String>,
    pub pending_recipe: Option<String>,
    pub active_compaction: Option<String>,
    pub enabled_mcp_servers: Option<Vec<String>>,
    pub pinned: bool,
    pub legal_hold: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LlmUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_micros: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_currency: Option<String>,
}

impl LlmUsage {
    pub fn add_assign(&mut self, other: &Self) {
        self.input_tokens = add_optional_u64(self.input_tokens, other.input_tokens);
        self.output_tokens = add_optional_u64(self.output_tokens, other.output_tokens);
        self.total_tokens = add_optional_u64(self.total_tokens, other.total_tokens);
        self.estimated_cost_micros =
            add_optional_u64(self.estimated_cost_micros, other.estimated_cost_micros);
        if self.estimated_cost_currency.is_none() {
            self.estimated_cost_currency = other.estimated_cost_currency.clone();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.total_tokens.is_none()
            && self.estimated_cost_micros.is_none()
            && self.estimated_cost_currency.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolArtifact {
    pub artifact_id: String,
    pub conversation_id: String,
    pub tool_name: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub relative_path: String,
    pub byte_count: u64,
    #[serde(default)]
    pub arguments_redacted: Value,
    #[serde(default)]
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditEvent {
    pub event_id: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleCadence {
    Interval {
        every: u64,
        unit: ScheduleIntervalUnit,
    },
    Daily {
        time: String,
    },
    Weekdays {
        time: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleIntervalUnit {
    Minutes,
    Hours,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleRecord {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub run_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    pub conversation_id: String,
    pub cadence: ScheduleCadence,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub next_run_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRun {
    pub workflow_id: String,
    pub conversation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_name: Option<String>,
    pub workflow_type: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
    #[serde(default)]
    pub steps: Vec<WorkflowStep>,
    #[serde(default)]
    pub approvals: Vec<ApprovalRequest>,
    #[serde(default)]
    pub artifacts: Vec<ToolArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_answer: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowStep {
    pub index: usize,
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub attempt: usize,
    #[serde(default)]
    pub max_attempts: usize,
    #[serde(default)]
    pub approval_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowContinuationRef {
    pub workflow_id: String,
    pub step_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TurnContinuation {
    pub continuation_id: String,
    pub conversation_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowContinuationRef>,
    pub messages: Vec<LlmMessage>,
    pub iterations_used: usize,
    pub max_total_iterations: usize,
    pub continuation_increment: usize,
    pub tool_seconds: f64,
    pub llm_seconds: f64,
    pub tool_call_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_usage: Option<LlmUsage>,
    #[serde(default)]
    pub evidence: Vec<ToolArtifact>,
    pub automation: bool,
    pub suppress_persistence: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApprovalRequest {
    pub approval_id: String,
    pub workflow_id: String,
    pub step_index: usize,
    pub status: String,
    pub prompt: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetentionPolicy {
    #[serde(default)]
    pub older_than_days: Option<i64>,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub include_active: bool,
    #[serde(default)]
    pub include_exports: bool,
    #[serde(default)]
    pub force: bool,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            older_than_days: Some(30),
            include_archived: true,
            include_active: false,
            include_exports: false,
            force: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetentionPreview {
    pub preview_id: String,
    pub created_at: DateTime<Utc>,
    pub policy: RetentionPolicy,
    #[serde(default)]
    pub items: Vec<RetentionItem>,
    #[serde(default)]
    pub blocked: Vec<RetentionBlockedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetentionItem {
    pub kind: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub byte_count: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetentionBlockedItem {
    pub kind: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FindingRecord {
    pub finding_id: String,
    pub conversation_id: String,
    pub kind: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_artifact: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl FindingRecord {
    pub const MIN_CONFIDENCE: u8 = 0;
    pub const MAX_CONFIDENCE: u8 = 100;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        finding_id: String,
        conversation_id: String,
        kind: String,
        value: String,
        note: Option<String>,
        tags: Vec<String>,
        confidence: Option<u8>,
        source_artifact: Option<String>,
    ) -> Result<Self> {
        validate_finding_confidence(confidence)?;
        let now = Utc::now();
        Ok(Self {
            finding_id,
            conversation_id,
            kind,
            value,
            note,
            tags,
            confidence,
            source_artifact,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn set_confidence(&mut self, confidence: Option<u8>) -> Result<()> {
        validate_finding_confidence(confidence)?;
        self.confidence = confidence;
        Ok(())
    }

    pub fn validate_for_storage(&self) -> Result<()> {
        validate_finding_confidence(self.confidence)
    }
}

fn validate_finding_confidence(confidence: Option<u8>) -> Result<()> {
    if let Some(confidence) = confidence
        && !(FindingRecord::MIN_CONFIDENCE..=FindingRecord::MAX_CONFIDENCE).contains(&confidence)
    {
        bail!(
            "invalid finding confidence: must be between {} and {}",
            FindingRecord::MIN_CONFIDENCE,
            FindingRecord::MAX_CONFIDENCE
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchResult {
    pub scope: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct InvestigationMemory {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timeline: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hypotheses: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trusted_sources: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_questions: Vec<Value>,
}

impl InvestigationMemory {
    pub fn is_empty(&self) -> bool {
        self.summary.trim().is_empty()
            && self.entities.is_empty()
            && self.timeline.is_empty()
            && self.decisions.is_empty()
            && self.hypotheses.is_empty()
            && self.trusted_sources.is_empty()
            && self.unresolved_questions.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub message: String,
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_display_name: Option<String>,
    pub tool_call_count: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunTurnResult {
    pub reply: String,
    pub tool_calls: usize,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_increment: Option<usize>,
}

impl RunTurnResult {
    pub fn completed(reply: impl Into<String>, tool_calls: usize) -> Self {
        Self {
            reply: reply.into(),
            tool_calls,
            status: "completed".to_string(),
            continuation_id: None,
            continuation_increment: None,
        }
    }

    pub fn needs_continuation(
        reply: impl Into<String>,
        tool_calls: usize,
        continuation_id: String,
        continuation_increment: usize,
    ) -> Self {
        Self {
            reply: reply.into(),
            tool_calls,
            status: "needs_continuation".to_string(),
            continuation_id: Some(continuation_id),
            continuation_increment: Some(continuation_increment),
        }
    }
}

#[derive(Debug, Clone)]
pub enum UiEvent {
    Progress(ProgressEvent),
    Finished(Result<RunTurnResult, String>),
    CompactionFinished(Result<String, String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RememberedJob {
    pub alias: String,
    pub transaction_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_interval_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_poll_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_artifacts_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub stored_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RememberedJob {
    pub const MIN_POLL_INTERVAL_SECONDS: u64 = 1;
    pub const MAX_POLL_INTERVAL_SECONDS: u64 = 86_400;

    pub fn new(alias: String, transaction_id: String) -> Result<Self> {
        let now = Utc::now();
        let alias = validate_non_empty_job_field("job alias", alias)?;
        let transaction_id = validate_non_empty_job_field("job transaction_id", transaction_id)?;
        Ok(Self {
            alias,
            transaction_id,
            source_tool: None,
            status: None,
            notes: None,
            mode: None,
            poll_interval_seconds: None,
            next_poll_at: None,
            lease_expires_at: None,
            result_expires_at: None,
            automation_prompt: None,
            retrieval_state: None,
            result_artifacts_json: None,
            last_error: None,
            stored_at: now,
            updated_at: now,
        })
    }

    pub fn set_transaction_id(&mut self, transaction_id: String) -> Result<()> {
        self.transaction_id = validate_non_empty_job_field("job transaction_id", transaction_id)?;
        Ok(())
    }

    pub fn set_mode(&mut self, mode: Option<String>) -> Result<()> {
        self.mode = match mode {
            Some(mode) => {
                let mode = mode.trim();
                if mode.is_empty() {
                    None
                } else if mode == "auto_pull" {
                    Some(mode.to_string())
                } else {
                    bail!("job mode must be 'auto_pull' when set");
                }
            }
            None => None,
        };
        Ok(())
    }

    pub fn set_poll_interval_seconds(&mut self, interval: Option<u64>) -> Result<()> {
        if let Some(interval) = interval {
            validate_poll_interval_seconds(interval)?;
        }
        self.poll_interval_seconds = interval;
        Ok(())
    }

    pub fn validate_for_storage(&self) -> Result<()> {
        validate_non_empty_job_field("job alias", self.alias.clone())?;
        validate_non_empty_job_field("job transaction_id", self.transaction_id.clone())?;
        if let Some(mode) = &self.mode
            && mode != "auto_pull"
        {
            bail!("job mode must be 'auto_pull' when set");
        }
        if let Some(interval) = self.poll_interval_seconds {
            validate_poll_interval_seconds(interval)?;
        }
        Ok(())
    }

    pub fn is_due_for_poll(&self, now: DateTime<Utc>) -> bool {
        matches!(self.mode.as_deref(), Some("auto_pull"))
            && self.next_poll_at.map(|due| due <= now).unwrap_or(false)
            && self
                .lease_expires_at
                .map(|lease| lease <= now)
                .unwrap_or(true)
    }
}

fn validate_non_empty_job_field(label: &str, value: String) -> Result<String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(value)
}

fn validate_poll_interval_seconds(value: u64) -> Result<()> {
    if !(RememberedJob::MIN_POLL_INTERVAL_SECONDS..=RememberedJob::MAX_POLL_INTERVAL_SECONDS)
        .contains(&value)
    {
        bail!(
            "job poll_interval_seconds must be between {} and {}",
            RememberedJob::MIN_POLL_INTERVAL_SECONDS,
            RememberedJob::MAX_POLL_INTERVAL_SECONDS
        );
    }
    Ok(())
}

fn add_optional_u64(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::permission_denied_user_prompt;

    #[test]
    fn formats_network_permission_prompt_for_user() {
        let prompt = permission_denied_user_prompt(
            "permission denied: MCP tool 'wiz__issues_query' requires network access. Enable it with /permissions network on, or use /yolo on.",
        )
        .unwrap();

        assert!(prompt.contains("MCP tool 'wiz__issues_query' was blocked"));
        assert!(prompt.contains("Enable it and retry?"));
        assert!(prompt.contains("`/permissions network on`"));
    }

    #[test]
    fn formats_filesystem_read_permission_prompt_for_user() {
        let prompt = permission_denied_user_prompt(
            "permission denied: inline file references require filesystem read access. Enable it with /permissions fs read or /permissions fs write, or use /yolo on.",
        )
        .unwrap();

        assert!(prompt.contains("filesystem read access"));
        assert!(prompt.contains("`/permissions fs read`"));
        assert!(prompt.contains("reply without changing permissions"));
    }

    #[test]
    fn formats_filesystem_scope_permission_prompt_for_user() {
        let prompt = permission_denied_user_prompt(
            "permission denied: local__read_file requires full filesystem access for path '/tmp/outside' outside workspace root '/workspace'. Enable it with /permissions fs-scope full, or use /yolo on.",
        )
        .unwrap();

        assert!(prompt.contains("full filesystem access"));
        assert!(prompt.contains("`/permissions fs-scope full`"));
    }
}
