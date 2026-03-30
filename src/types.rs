use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub conversation_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Name of the recipe currently loaded into this conversation (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_recipe: Option<String>,
    /// Per-conversation MCP server allowlist. `None` means all configured servers are active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_mcp_servers: Option<Vec<String>>,
    /// Checkpoint ID of the latest compaction summary stored in `compactions/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_compaction: Option<String>,
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
}
