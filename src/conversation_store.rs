use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::Utc;

use crate::types::{Conversation, ConversationSummary, Message};

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
            .conversation_dir(conversation_id)
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
            .conversation_dir(&conversation.conversation_id)
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
        let mut conversation = self.load(conversation_id)?;
        let message = Message {
            role: role.to_string(),
            content: content.into(),
            timestamp: Utc::now(),
        };
        conversation.messages.push(message.clone());
        conversation.updated_at = Utc::now();
        self.save(&conversation)?;
        Ok(message)
    }

    pub fn delete(&self, conversation_id: &str) -> Result<()> {
        let dir = self.conversation_dir(conversation_id);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .with_context(|| format!("failed to remove {}", dir.display()))?;
        }
        Ok(())
    }

    pub fn append_log(&self, conversation_id: &str, line: &str) -> Result<()> {
        self.ensure_layout(conversation_id)?;
        let path = self
            .conversation_dir(conversation_id)
            .join("logs/conversation.log");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        writeln!(file, "[{}] {}", Utc::now().to_rfc3339(), line)?;
        Ok(())
    }

    pub fn conversation_dir(&self, conversation_id: &str) -> PathBuf {
        self.root.join(conversation_id)
    }

    pub fn ensure_layout(&self, conversation_id: &str) -> Result<()> {
        let dir = self.conversation_dir(conversation_id);
        fs::create_dir_all(dir.join("tool_output"))?;
        fs::create_dir_all(dir.join("logs"))?;
        Ok(())
    }
}

fn generate_conversation_id(now: chrono::DateTime<Utc>) -> String {
    let suffix = rand::random::<u32>();
    format!("convo-{}-{suffix:08x}", now.format("%Y%m%d%H%M%S"))
}

#[cfg(test)]
mod tests {
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
}
