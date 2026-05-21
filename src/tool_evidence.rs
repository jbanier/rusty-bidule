use std::fs;

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::Value;

use crate::{conversation_store::ConversationStore, redaction::redact_value, types::ToolArtifact};

#[derive(Debug, Clone)]
pub struct ToolEvidenceWriter {
    store: ConversationStore,
}

impl ToolEvidenceWriter {
    pub fn new(store: ConversationStore) -> Self {
        Self { store }
    }

    pub fn write_artifact(
        &self,
        conversation_id: &str,
        tool_name: &str,
        arguments: &Value,
        status: &str,
        raw_output: &str,
    ) -> Result<ToolArtifact> {
        self.store.ensure_layout(conversation_id)?;
        let timestamp = Utc::now().format("%Y%m%d%H%M%S%3f");
        let filename = format!("{}_{}.txt", sanitize(tool_name), timestamp);
        let relative_path = format!("tool_output/{filename}");
        let path = self
            .store
            .conversation_dir(conversation_id)?
            .join(&relative_path);
        let created_at = Utc::now();
        let arguments_redacted = redact_value(arguments);
        let payload = format!(
            "tool: {tool_name}\nstatus: {status}\ntimestamp: {}\narguments: {}\n\n{}",
            created_at.to_rfc3339(),
            serde_json::to_string_pretty(&arguments_redacted)?,
            raw_output
        );
        fs::write(&path, payload)
            .with_context(|| format!("failed to write tool artifact {}", path.display()))?;
        let byte_count = fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let artifact = ToolArtifact {
            artifact_id: format!(
                "artifact-{}-{:08x}",
                created_at.format("%Y%m%d%H%M%S%3f"),
                rand::random::<u32>()
            ),
            conversation_id: conversation_id.to_string(),
            tool_name: tool_name.to_string(),
            status: status.to_string(),
            created_at,
            relative_path,
            byte_count,
            arguments_redacted,
            preview: preview(raw_output),
        };
        self.store.append_tool_artifact(&artifact)?;
        self.store.append_audit_event(
            Some(conversation_id),
            "tool_artifact",
            "tool output saved as evidence",
            serde_json::json!({
                "artifact_id": artifact.artifact_id,
                "tool_name": artifact.tool_name,
                "status": artifact.status,
                "relative_path": artifact.relative_path,
                "byte_count": artifact.byte_count,
            }),
        )?;
        Ok(artifact)
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch,
            _ => '_',
        })
        .collect()
}

fn preview(value: &str) -> String {
    const LIMIT: usize = 2_000;
    let mut out = value.chars().take(LIMIT).collect::<String>();
    if value.chars().count() > LIMIT {
        out.push_str("\n[truncated]");
    }
    out
}
