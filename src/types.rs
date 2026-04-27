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
    ) -> Self {
        let now = Utc::now();
        Self {
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
        }
    }
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
}
