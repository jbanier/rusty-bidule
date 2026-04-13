use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;

use crate::types::{
    AgentPermissions, Conversation, ConversationSummary, FindingRecord, Message, MessageMetadata,
    RememberedJob, SearchResult,
};

#[derive(Debug, Clone)]
pub struct ConversationStore {
    data_root: PathBuf,
    root: PathBuf,
    default_agent_permissions: AgentPermissions,
}

impl ConversationStore {
    pub fn new(data_dir: impl AsRef<Path>, default_agent_permissions: AgentPermissions) -> Self {
        Self {
            data_root: data_dir.as_ref().to_path_buf(),
            root: data_dir.as_ref().join("conversations"),
            default_agent_permissions,
        }
    }

    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.data_root)
            .with_context(|| format!("failed to create {}", self.data_root.display()))?;
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
            title: None,
            archived_at: None,
            pending_recipe: None,
            enabled_mcp_servers: None,
            active_compaction: None,
            enabled_local_tools: None,
            agent_permissions: self.default_agent_permissions.clone(),
            messages: Vec::new(),
        };
        self.ensure_layout(&conversation.conversation_id)?;
        self.save(&conversation)?;
        self.append_log(&conversation.conversation_id, "conversation created")?;
        Ok(conversation)
    }

    pub fn list_conversations(&self) -> Result<Vec<ConversationSummary>> {
        self.list_conversations_with_archived(false)
    }

    pub fn list_conversations_with_archived(
        &self,
        include_archived: bool,
    ) -> Result<Vec<ConversationSummary>> {
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
            if !include_archived && conversation.archived_at.is_some() {
                continue;
            }
            summaries.push(ConversationSummary {
                conversation_id: conversation.conversation_id,
                updated_at: conversation.updated_at,
                message_count: conversation.messages.len(),
                title: conversation.title,
                archived_at: conversation.archived_at,
                preview: conversation
                    .messages
                    .last()
                    .map(|message| message.content.replace('\n', " "))
                    .map(|content| content.chars().take(160).collect()),
                pending_recipe: conversation.pending_recipe,
                active_compaction: conversation.active_compaction,
                enabled_mcp_servers: conversation.enabled_mcp_servers,
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

    fn job_state_path(&self, conversation_id: &str) -> Result<PathBuf> {
        Ok(self
            .conversation_dir(conversation_id)?
            .join("job_state.json"))
    }

    fn legacy_jobs_path(&self, conversation_id: &str) -> Result<PathBuf> {
        Ok(self.conversation_dir(conversation_id)?.join("jobs.json"))
    }

    fn scratchpad_path(&self, conversation_id: &str) -> Result<PathBuf> {
        Ok(self
            .conversation_dir(conversation_id)?
            .join("scratchpad.md"))
    }

    fn findings_path(&self) -> PathBuf {
        self.data_root.join("findings.json")
    }

    fn export_root(&self) -> PathBuf {
        self.data_root.join("exports")
    }

    pub fn load_scratchpad(&self, conversation_id: &str) -> Result<String> {
        self.ensure_layout(conversation_id)?;
        let path = self.scratchpad_path(conversation_id)?;
        if !path.exists() {
            return Ok(String::new());
        }
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
    }

    pub fn save_scratchpad(&self, conversation_id: &str, body: &str) -> Result<()> {
        self.ensure_layout(conversation_id)?;
        let path = self.scratchpad_path(conversation_id)?;
        fs::write(&path, body).with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn load_findings(&self) -> Result<Vec<FindingRecord>> {
        self.init()?;
        let path = self.findings_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn save_findings(&self, findings: &[FindingRecord]) -> Result<()> {
        self.init()?;
        let path = self.findings_path();
        let payload = serde_json::to_string_pretty(findings)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn add_finding(
        &self,
        conversation_id: &str,
        kind: &str,
        value: &str,
        note: Option<&str>,
        tags: &[String],
        confidence: Option<u8>,
        source_artifact: Option<&str>,
    ) -> Result<FindingRecord> {
        validate_conversation_id(conversation_id)?;
        let now = Utc::now();
        let finding = FindingRecord::new(
            format!(
                "finding-{}-{:08x}",
                now.format("%Y%m%d%H%M%S"),
                rand::random::<u32>()
            ),
            conversation_id.to_string(),
            kind.to_string(),
            value.to_string(),
            note.map(str::to_string),
            normalize_tags(tags),
            confidence,
            source_artifact.map(str::to_string),
        );
        let mut findings = self.load_findings()?;
        findings.push(finding.clone());
        findings.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        self.save_findings(&findings)?;
        Ok(finding)
    }

    pub fn update_finding(
        &self,
        finding_id: &str,
        kind: &str,
        value: &str,
        note: Option<&str>,
        tags: &[String],
        confidence: Option<u8>,
        source_artifact: Option<&str>,
    ) -> Result<Option<FindingRecord>> {
        let mut findings = self.load_findings()?;
        let mut updated = None;
        for finding in &mut findings {
            if finding.finding_id != finding_id {
                continue;
            }
            finding.kind = kind.trim().to_string();
            finding.value = value.trim().to_string();
            finding.note = normalize_optional_text(note);
            finding.tags = normalize_tags(tags);
            finding.confidence = confidence;
            finding.source_artifact = normalize_optional_text(source_artifact);
            finding.updated_at = Utc::now();
            updated = Some(finding.clone());
            break;
        }
        if updated.is_some() {
            findings.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            self.save_findings(&findings)?;
        }
        Ok(updated)
    }

    pub fn remove_finding(&self, finding_id: &str) -> Result<bool> {
        let mut findings = self.load_findings()?;
        let original_len = findings.len();
        findings.retain(|finding| finding.finding_id != finding_id);
        if findings.len() == original_len {
            return Ok(false);
        }
        self.save_findings(&findings)?;
        Ok(true)
    }

    pub fn archive_conversation(&self, conversation_id: &str) -> Result<Conversation> {
        let mut conversation = self.load(conversation_id)?;
        let now = Utc::now();
        conversation.archived_at = Some(now);
        conversation.updated_at = now;
        self.save(&conversation)?;
        Ok(conversation)
    }

    pub fn unarchive_conversation(&self, conversation_id: &str) -> Result<Conversation> {
        let mut conversation = self.load(conversation_id)?;
        conversation.archived_at = None;
        conversation.updated_at = Utc::now();
        self.save(&conversation)?;
        Ok(conversation)
    }

    pub fn set_conversation_title(
        &self,
        conversation_id: &str,
        title: Option<&str>,
    ) -> Result<Conversation> {
        let mut conversation = self.load(conversation_id)?;
        conversation.title = title
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        conversation.updated_at = Utc::now();
        self.save(&conversation)?;
        Ok(conversation)
    }

    pub fn export_conversation_summary(&self, conversation_id: &str) -> Result<PathBuf> {
        let conversation = self.load(conversation_id)?;
        let scratchpad = self.load_scratchpad(conversation_id)?;
        let jobs = self.load_job_state(conversation_id)?;
        let findings = self
            .load_findings()?
            .into_iter()
            .filter(|finding| finding.conversation_id == conversation_id)
            .collect::<Vec<_>>();

        let active_compaction_summary = conversation
            .active_compaction
            .as_deref()
            .map(|checkpoint_id| self.load_compaction(conversation_id, checkpoint_id))
            .transpose()?;

        let conversation_dir = self.conversation_dir(conversation_id)?;
        let log_path = conversation_dir.join("logs/conversation.log");
        let tool_output_dir = conversation_dir.join("tool_output");
        let tool_output_files = if tool_output_dir.exists() {
            fs::read_dir(&tool_output_dir)?
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let export_root = self.export_root();
        fs::create_dir_all(&export_root)
            .with_context(|| format!("failed to create {}", export_root.display()))?;
        let export_path = export_root.join(format!("{conversation_id}-summary.json"));
        let payload = serde_json::to_string_pretty(&serde_json::json!({
            "exported_at": Utc::now().to_rfc3339(),
            "conversation": conversation,
            "scratchpad": scratchpad,
            "jobs": jobs,
            "findings": findings,
            "active_compaction_summary": active_compaction_summary,
            "artifacts": {
                "conversation_dir": conversation_dir.display().to_string(),
                "log_path": log_path.display().to_string(),
                "tool_output_dir": tool_output_dir.display().to_string(),
                "tool_output_files": tool_output_files,
            }
        }))?;
        fs::write(&export_path, payload)
            .with_context(|| format!("failed to write {}", export_path.display()))?;
        Ok(export_path)
    }

    pub fn search_local(&self, query: &str) -> Result<Vec<SearchResult>> {
        let needle = query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();

        for summary in self.list_conversations_with_archived(true)? {
            let conversation = self.load(&summary.conversation_id)?;
            for message in &conversation.messages {
                if message.content.to_ascii_lowercase().contains(&needle) {
                    results.push(SearchResult {
                        scope: "conversation".to_string(),
                        title: format!("{} / {}", conversation.conversation_id, message.role),
                        snippet: summarize_match(&message.content, query),
                    });
                }
            }

            let scratchpad = self.load_scratchpad(&summary.conversation_id)?;
            if !scratchpad.is_empty() && scratchpad.to_ascii_lowercase().contains(&needle) {
                results.push(SearchResult {
                    scope: "scratchpad".to_string(),
                    title: summary.conversation_id.clone(),
                    snippet: summarize_match(&scratchpad, query),
                });
            }
        }

        for finding in self.load_findings()? {
            let haystack = format!(
                "{} {} {} {} {} {} {}",
                finding.conversation_id,
                finding.kind,
                finding.value,
                finding.note.as_deref().unwrap_or(""),
                finding.tags.join(" "),
                finding
                    .confidence
                    .map(|value| value.to_string())
                    .as_deref()
                    .unwrap_or(""),
                finding.source_artifact.as_deref().unwrap_or("")
            );
            if haystack.to_ascii_lowercase().contains(&needle) {
                results.push(SearchResult {
                    scope: "finding".to_string(),
                    title: format!("{} / {}", finding.conversation_id, finding.finding_id),
                    snippet: summarize_match(
                        &format!(
                            "{}: {}{}{}{}{}",
                            finding.kind,
                            finding.value,
                            finding
                                .note
                                .as_deref()
                                .map(|note| format!(" // {note}"))
                                .unwrap_or_default(),
                            if finding.tags.is_empty() {
                                String::new()
                            } else {
                                format!(" // tags: {}", finding.tags.join(", "))
                            },
                            finding
                                .confidence
                                .map(|value| format!(" // confidence: {value}"))
                                .unwrap_or_default(),
                            finding
                                .source_artifact
                                .as_deref()
                                .map(|path| format!(" // artifact: {path}"))
                                .unwrap_or_default()
                        ),
                        query,
                    ),
                });
            }
        }

        Ok(results)
    }

    pub fn load_job_state(&self, conversation_id: &str) -> Result<Vec<RememberedJob>> {
        self.ensure_layout(conversation_id)?;
        let path = self.job_state_path(conversation_id)?;
        if path.exists() {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let jobs = serde_json::from_str(&raw)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            return Ok(jobs);
        }

        let legacy_path = self.legacy_jobs_path(conversation_id)?;
        if !legacy_path.exists() {
            return Ok(Vec::new());
        }

        let raw = fs::read_to_string(&legacy_path)
            .with_context(|| format!("failed to read {}", legacy_path.display()))?;
        let legacy_jobs: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(&raw)
                .with_context(|| format!("failed to parse {}", legacy_path.display()))?;

        let mut jobs = Vec::new();
        for (alias, value) in legacy_jobs {
            let transaction_id = value
                .get("transaction_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if transaction_id.is_empty() {
                continue;
            }
            let mut job = RememberedJob::new(alias.clone(), transaction_id);
            job.source_tool = value
                .get("source_tool")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            job.status = value
                .get("status")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            job.notes = value
                .get("notes")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if let Some(stored_at) = value.get("stored_at").and_then(|v| v.as_str())
                && let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(stored_at)
            {
                let timestamp = timestamp.with_timezone(&Utc);
                job.stored_at = timestamp;
                job.updated_at = timestamp;
            }
            jobs.push(job);
        }

        jobs.sort_by(|a, b| a.alias.cmp(&b.alias));
        Ok(jobs)
    }

    pub fn save_job_state(&self, conversation_id: &str, jobs: &[RememberedJob]) -> Result<()> {
        self.ensure_layout(conversation_id)?;
        let path = self.job_state_path(conversation_id)?;
        let payload = serde_json::to_string_pretty(jobs)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn list_due_jobs(
        &self,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<(String, RememberedJob)>> {
        let mut due = Vec::new();
        for summary in self.list_conversations_with_archived(true)? {
            let jobs = self.load_job_state(&summary.conversation_id)?;
            for job in jobs {
                if job.is_due_for_poll(now) {
                    due.push((summary.conversation_id.clone(), job));
                }
            }
        }
        Ok(due)
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

fn summarize_match(text: &str, query: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let needle = query.to_ascii_lowercase();
    let start = lower.find(&needle).unwrap_or(0);
    let prefix = text[..start].chars().count();
    let match_len = text[start..]
        .chars()
        .take(query.chars().count().max(1))
        .count();
    let char_start = prefix.saturating_sub(40);
    let char_end = (prefix + match_len + 80).min(text.chars().count());
    let snippet = text
        .chars()
        .skip(char_start)
        .take(char_end.saturating_sub(char_start))
        .collect::<String>()
        .replace('\n', " ");
    if char_start > 0 || char_end < text.chars().count() {
        format!("...{}...", snippet.trim())
    } else {
        snippet.trim().to_string()
    }
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::types::AgentPermissions;

    use super::ConversationStore;

    #[test]
    fn creates_and_persists_conversation() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
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
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());

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
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let outside_file = dir.path().join("outside.txt");
        fs::write(&outside_file, "keep me").unwrap();

        let err = store.delete("../outside.txt").unwrap_err().to_string();

        assert!(err.contains("invalid conversation id"));
        assert_eq!(fs::read_to_string(&outside_file).unwrap(), "keep me");
    }

    #[test]
    fn scratchpad_round_trip_works() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();

        store
            .save_scratchpad(&conversation.conversation_id, "note one")
            .unwrap();

        assert_eq!(
            store
                .load_scratchpad(&conversation.conversation_id)
                .unwrap(),
            "note one"
        );
    }

    #[test]
    fn archived_conversations_are_hidden_from_default_list() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let active = store.create_conversation().unwrap();
        let archived = store.create_conversation().unwrap();

        store
            .archive_conversation(&archived.conversation_id)
            .unwrap();

        let active_only = store.list_conversations().unwrap();
        assert_eq!(active_only.len(), 1);
        assert_eq!(active_only[0].conversation_id, active.conversation_id);

        let with_archived = store.list_conversations_with_archived(true).unwrap();
        assert_eq!(with_archived.len(), 2);
        assert!(with_archived.iter().any(|summary| summary.conversation_id
            == archived.conversation_id
            && summary.archived_at.is_some()));
    }

    #[test]
    fn titles_are_persisted_and_surface_in_summaries() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();

        store
            .set_conversation_title(&conversation.conversation_id, Some("Malware triage"))
            .unwrap();

        let loaded = store.load(&conversation.conversation_id).unwrap();
        assert_eq!(loaded.title.as_deref(), Some("Malware triage"));
        let summary = store.list_conversations().unwrap().pop().unwrap();
        assert_eq!(summary.title.as_deref(), Some("Malware triage"));
    }

    #[test]
    fn export_summary_writes_session_snapshot() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        store
            .append_message(&conversation.conversation_id, "user", "Need export")
            .unwrap();
        store
            .save_scratchpad(&conversation.conversation_id, "working notes")
            .unwrap();
        store
            .add_finding(
                &conversation.conversation_id,
                "ip",
                "1.2.3.4",
                Some("pivot"),
                &["urgent".to_string()],
                Some(80),
                Some("sample.bin"),
            )
            .unwrap();

        let export_path = store
            .export_conversation_summary(&conversation.conversation_id)
            .unwrap();

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&export_path).unwrap()).unwrap();
        assert_eq!(
            value["conversation"]["conversation_id"],
            conversation.conversation_id
        );
        assert_eq!(value["scratchpad"], "working notes");
        assert_eq!(value["findings"].as_array().unwrap().len(), 1);
        assert_eq!(value["findings"][0]["confidence"], 80);
        assert_eq!(
            export_path.file_name().unwrap().to_string_lossy(),
            format!("{}-summary.json", conversation.conversation_id)
        );
    }

    #[test]
    fn local_search_finds_findings_and_messages() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        store
            .append_message(
                &conversation.conversation_id,
                "user",
                "Suspicious host 1.2.3.4",
            )
            .unwrap();
        store
            .save_scratchpad(&conversation.conversation_id, "pivot on malware family")
            .unwrap();
        store
            .add_finding(
                &conversation.conversation_id,
                "ip",
                "1.2.3.4",
                Some("confirmed beacon"),
                &["network".to_string()],
                Some(90),
                Some("ioc.txt"),
            )
            .unwrap();

        let results = store.search_local("1.2.3.4").unwrap();

        assert!(results.iter().any(|result| result.scope == "conversation"));
        assert!(results.iter().any(|result| result.scope == "finding"));
    }
}
