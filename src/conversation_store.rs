use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;

use crate::types::{Conversation, ConversationSummary, Message, MessageMetadata};

#[derive(Debug, Clone)]
pub struct ConversationStore {
    root: PathBuf,
}

impl ConversationStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            root: data_dir.as_ref().join("conversations"),
        }
    }

    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        Ok(())
    }

    pub fn create_conversation(&self) -> Result<Conversation> {
        self.init()?;
        let now = Utc::now();
        let conversation = Conversation {
            conversation_id: generate_conversation_id(now),
            created_at: now,
            updated_at: now,
            pending_recipe: None,
            enabled_mcp_servers: None,
            active_compaction: None,
            messages: Vec::new(),
        };
        self.ensure_layout(&conversation.conversation_id)?;
        self.save(&conversation)?;
        self.append_log(&conversation.conversation_id, "conversation created")?;
        Ok(conversation)
    }

    pub fn list_conversations(&self) -> Result<Vec<ConversationSummary>> {
        self.init()?;
        let mut summaries = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("failed to read {}", self.root.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let convo_path = entry.path().join("conversation.json");
            if !convo_path.exists() {
                continue;
            }
            let conversation: Conversation =
                serde_json::from_str(&fs::read_to_string(&convo_path)?)?;
            summaries.push(ConversationSummary {
                conversation_id: conversation.conversation_id,
                updated_at: conversation.updated_at,
                message_count: conversation.messages.len(),
            });
        }
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(summaries)
    }

    pub fn load(&self, conversation_id: &str) -> Result<Conversation> {
        let path = self
            .conversation_dir(conversation_id)?
            .join("conversation.json");
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let conversation = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(conversation)
    }

    pub fn save(&self, conversation: &Conversation) -> Result<()> {
        self.ensure_layout(&conversation.conversation_id)?;
        let path = self
            .conversation_dir(&conversation.conversation_id)?
            .join("conversation.json");
        let payload = serde_json::to_string_pretty(conversation)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn append_message(
        &self,
        conversation_id: &str,
        role: &str,
        content: impl Into<String>,
    ) -> Result<Message> {
        self.append_message_with_metadata(conversation_id, role, content, None)
    }

    pub fn append_message_with_metadata(
        &self,
        conversation_id: &str,
        role: &str,
        content: impl Into<String>,
        metadata: Option<MessageMetadata>,
    ) -> Result<Message> {
        let mut conversation = self.load(conversation_id)?;
        let message = Message {
            role: role.to_string(),
            content: content.into(),
            timestamp: Utc::now(),
            metadata,
        };
        conversation.messages.push(message.clone());
        conversation.updated_at = Utc::now();
        self.save(&conversation)?;
        Ok(message)
    }

    pub fn delete(&self, conversation_id: &str) -> Result<()> {
        let dir = self.conversation_dir(conversation_id)?;
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .with_context(|| format!("failed to remove {}", dir.display()))?;
        }
        Ok(())
    }

    pub fn append_log(&self, conversation_id: &str, line: &str) -> Result<()> {
        self.ensure_layout(conversation_id)?;
        let path = self
            .conversation_dir(conversation_id)?
            .join("logs/conversation.log");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        writeln!(file, "[{}] {}", Utc::now().to_rfc3339(), line)?;
        Ok(())
    }

    pub fn conversation_dir(&self, conversation_id: &str) -> Result<PathBuf> {
        validate_conversation_id(conversation_id)?;
        Ok(self.root.join(conversation_id))
    }

    pub fn ensure_layout(&self, conversation_id: &str) -> Result<()> {
        let dir = self.conversation_dir(conversation_id)?;
        fs::create_dir_all(dir.join("tool_output"))?;
        fs::create_dir_all(dir.join("logs"))?;
        fs::create_dir_all(dir.join("compactions"))?;
        Ok(())
    }

    /// Save a compaction summary and update `active_compaction` in conversation.json.
    pub fn save_compaction(
        &self,
        conversation_id: &str,
        checkpoint_id: &str,
        summary: &str,
    ) -> Result<()> {
        self.ensure_layout(conversation_id)?;
        let path = self
            .conversation_dir(conversation_id)?
            .join("compactions")
            .join(format!("{checkpoint_id}.json"));
        let payload = serde_json::to_string_pretty(&serde_json::json!({
            "checkpoint_id": checkpoint_id,
            "created_at": Utc::now().to_rfc3339(),
            "summary": summary,
        }))?;
        fs::write(&path, payload)
            .with_context(|| format!("failed to write compaction {}", path.display()))?;
        // Update active_compaction pointer in conversation.json
        let mut conversation = self.load(conversation_id)?;
        conversation.active_compaction = Some(checkpoint_id.to_string());
        self.save(&conversation)?;
        Ok(())
    }

    /// Load the summary text for a compaction checkpoint.
    pub fn load_compaction(&self, conversation_id: &str, checkpoint_id: &str) -> Result<String> {
        let path = self
            .conversation_dir(conversation_id)?
            .join("compactions")
            .join(format!("{checkpoint_id}.json"));
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read compaction {}", path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&raw)?;
        Ok(value["summary"].as_str().unwrap_or_default().to_string())
    }
}

fn validate_conversation_id(conversation_id: &str) -> Result<()> {
    if conversation_id.is_empty() {
        bail!("conversation id must not be empty");
    }
    if conversation_id.len() > 128 {
        bail!("conversation id is too long");
    }
    if !conversation_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!(
            "invalid conversation id '{conversation_id}'; allowed characters are ASCII letters, digits, '-' and '_'"
        );
    }
    Ok(())
}

fn generate_conversation_id(now: chrono::DateTime<Utc>) -> String {
    let suffix = rand::random::<u32>();
    format!("convo-{}-{suffix:08x}", now.format("%Y%m%d%H%M%S"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::ConversationStore;

    #[test]
    fn creates_and_persists_conversation() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path());
        let conversation = store.create_conversation().unwrap();
        store
            .append_message(&conversation.conversation_id, "user", "hello")
            .unwrap();

        let loaded = store.load(&conversation.conversation_id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].content, "hello");
    }

    #[test]
    fn rejects_invalid_conversation_ids() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path());

        for invalid in ["", "..", "../escape", "nested/id", "/tmp/x", "with space"] {
            assert!(
                store.load(invalid).is_err(),
                "expected invalid id: {invalid}"
            );
            assert!(
                store.ensure_layout(invalid).is_err(),
                "expected invalid id: {invalid}"
            );
            assert!(
                store.delete(invalid).is_err(),
                "expected invalid id: {invalid}"
            );
        }
    }

    #[test]
    fn delete_cannot_remove_paths_outside_conversation_root() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path());
        let outside_file = dir.path().join("outside.txt");
        fs::write(&outside_file, "keep me").unwrap();

        let err = store.delete("../outside.txt").unwrap_err().to_string();

        assert!(err.contains("invalid conversation id"));
        assert_eq!(fs::read_to_string(&outside_file).unwrap(), "keep me");
    }
}
