use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentPermissions {
    #[serde(default)]
    pub allow_network: bool,
    #[serde(default)]
    pub filesystem: FilesystemAccess,
    #[serde(default)]
    pub yolo: bool,
}

impl Default for AgentPermissions {
    fn default() -> Self {
        Self {
            allow_network: false,
            filesystem: FilesystemAccess::ReadOnly,
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

    pub fn summary(&self) -> String {
        format!(
            "network={} filesystem={} yolo={}",
            if self.allows_network() { "on" } else { "off" },
            if self.yolo {
                "all"
            } else {
                self.filesystem.label()
            },
            if self.yolo { "on" } else { "off" }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub conversation_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
    #[serde(default)]
    pub agent_permissions: AgentPermissions,
    pub messages: Vec<Message>,
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

#[derive(Debug, Clone)]
pub struct ConversationSummary {
    pub conversation_id: String,
    pub updated_at: DateTime<Utc>,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FindingRecord {
    pub finding_id: String,
    pub conversation_id: String,
    pub kind: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl FindingRecord {
    pub fn new(
        finding_id: String,
        conversation_id: String,
        kind: String,
        value: String,
        note: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            finding_id,
            conversation_id,
            kind,
            value,
            note,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub scope: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub message: String,
    pub tool_name: Option<String>,
    pub tool_call_count: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunTurnResult {
    pub reply: String,
    pub tool_calls: usize,
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
    pub fn new(alias: String, transaction_id: String) -> Self {
        let now = Utc::now();
        Self {
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
        }
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
