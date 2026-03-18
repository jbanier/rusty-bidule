use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::Value;

use crate::conversation_store::ConversationStore;

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
    ) -> Result<PathBuf> {
        self.store.ensure_layout(conversation_id)?;
        let timestamp = Utc::now().format("%Y%m%d%H%M%S%3f");
        let filename = format!("{}_{}.txt", sanitize(tool_name), timestamp);
        let path = self
            .store
            .conversation_dir(conversation_id)
            .join("tool_output")
            .join(filename);
        let payload = format!(
            "tool: {tool_name}\nstatus: {status}\ntimestamp: {}\narguments: {}\n\n{}",
            Utc::now().to_rfc3339(),
            serde_json::to_string_pretty(arguments)?,
            raw_output
        );
        fs::write(&path, payload)
            .with_context(|| format!("failed to write tool artifact {}", path.display()))?;
        Ok(path)
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
