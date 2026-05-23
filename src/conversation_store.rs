use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::types::{
    ActivatedSkill, AgentPermissions, AuditEvent, Conversation, ConversationSummary, FindingRecord,
    FindingRecordDetails, InvestigationMemory, Message, MessageMetadata, RememberedJob,
    RetentionBlockedItem, RetentionItem, RetentionPolicy, RetentionPreview, ScheduleRecord,
    SearchResult, ToolArtifact, TurnContinuation, WorkflowRun,
};

#[derive(Debug, Deserialize, Serialize)]
struct CompactionRecord {
    checkpoint_id: String,
    created_at: String,
    summary: String,
}

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
            active_workflow: None,
            agent_budget: None,
            active_continuation: None,
            pinned: false,
            legal_hold: false,
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
                pinned: conversation.pinned,
                legal_hold: conversation.legal_hold,
            });
        }
        summaries.sort_by_key(|summary| std::cmp::Reverse(summary.updated_at));
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

    pub fn append_audit_event(
        &self,
        conversation_id: Option<&str>,
        kind: &str,
        message: &str,
        metadata: Value,
    ) -> Result<AuditEvent> {
        self.init()?;
        let event = AuditEvent {
            event_id: generate_prefixed_id("audit"),
            created_at: Utc::now(),
            conversation_id: conversation_id.map(str::to_string),
            kind: kind.to_string(),
            message: message.to_string(),
            metadata,
        };
        if let Some(conversation_id) = conversation_id {
            self.ensure_layout(conversation_id)?;
            let path = self.audit_log_path(conversation_id)?;
            append_json_line(&path, &event)?;
        }
        let global_path = self.data_root.join("audit.jsonl");
        append_json_line(&global_path, &event)?;
        Ok(event)
    }

    pub fn append_tool_artifact(&self, artifact: &ToolArtifact) -> Result<()> {
        self.ensure_layout(&artifact.conversation_id)?;
        let path = self.tool_artifact_index_path(&artifact.conversation_id)?;
        append_json_line(&path, artifact)
    }

    pub fn load_tool_artifacts(&self, conversation_id: &str) -> Result<Vec<ToolArtifact>> {
        self.ensure_layout(conversation_id)?;
        let path = self.tool_artifact_index_path(conversation_id)?;
        read_json_lines(&path)
    }

    pub fn get_tool_artifact(
        &self,
        conversation_id: &str,
        artifact_id: &str,
    ) -> Result<Option<ToolArtifact>> {
        Ok(self
            .load_tool_artifacts(conversation_id)?
            .into_iter()
            .find(|artifact| artifact.artifact_id == artifact_id))
    }

    pub fn resolve_artifact_path(&self, artifact: &ToolArtifact) -> Result<PathBuf> {
        let base = self.conversation_dir(&artifact.conversation_id)?;
        let path = base.join(&artifact.relative_path);
        let canonical_base = fs::canonicalize(&base)
            .with_context(|| format!("failed to canonicalize {}", base.display()))?;
        let canonical_path = fs::canonicalize(&path)
            .with_context(|| format!("failed to canonicalize {}", path.display()))?;
        if !canonical_path.starts_with(canonical_base) {
            bail!("artifact path escapes conversation directory");
        }
        Ok(canonical_path)
    }

    pub fn conversation_dir(&self, conversation_id: &str) -> Result<PathBuf> {
        validate_conversation_id(conversation_id)?;
        Ok(self.root.join(conversation_id))
    }

    pub fn ensure_layout(&self, conversation_id: &str) -> Result<()> {
        let dir = self.conversation_dir(conversation_id)?;
        fs::create_dir_all(dir.join("tool_output"))?;
        fs::create_dir_all(dir.join("managed_jobs"))?;
        fs::create_dir_all(dir.join("logs"))?;
        fs::create_dir_all(dir.join("compactions"))?;
        fs::create_dir_all(dir.join("workflow_runs"))?;
        fs::create_dir_all(dir.join("turn_continuations"))?;
        Ok(())
    }

    fn job_state_path(&self, conversation_id: &str) -> Result<PathBuf> {
        Ok(self
            .conversation_dir(conversation_id)?
            .join("job_state.json"))
    }

    fn tool_artifact_index_path(&self, conversation_id: &str) -> Result<PathBuf> {
        Ok(self
            .conversation_dir(conversation_id)?
            .join("tool_output/index.jsonl"))
    }

    fn audit_log_path(&self, conversation_id: &str) -> Result<PathBuf> {
        Ok(self
            .conversation_dir(conversation_id)?
            .join("logs/audit.jsonl"))
    }

    fn workflow_run_path(&self, conversation_id: &str, workflow_id: &str) -> Result<PathBuf> {
        validate_resource_id(workflow_id, "workflow id")?;
        Ok(self
            .conversation_dir(conversation_id)?
            .join("workflow_runs")
            .join(format!("{workflow_id}.json")))
    }

    fn turn_continuation_path(
        &self,
        conversation_id: &str,
        continuation_id: &str,
    ) -> Result<PathBuf> {
        validate_resource_id(continuation_id, "continuation id")?;
        Ok(self
            .conversation_dir(conversation_id)?
            .join("turn_continuations")
            .join(format!("{continuation_id}.json")))
    }

    fn legacy_jobs_path(&self, conversation_id: &str) -> Result<PathBuf> {
        Ok(self.conversation_dir(conversation_id)?.join("jobs.json"))
    }

    fn scratchpad_path(&self, conversation_id: &str) -> Result<PathBuf> {
        Ok(self
            .conversation_dir(conversation_id)?
            .join("scratchpad.md"))
    }

    fn investigation_memory_path(&self, conversation_id: &str) -> Result<PathBuf> {
        Ok(self
            .conversation_dir(conversation_id)?
            .join("investigation_memory.json"))
    }

    fn activated_skills_path(&self, conversation_id: &str) -> Result<PathBuf> {
        Ok(self
            .conversation_dir(conversation_id)?
            .join("activated_skills.json"))
    }

    fn findings_path(&self) -> PathBuf {
        self.data_root.join("findings.json")
    }

    fn export_root(&self) -> PathBuf {
        self.data_root.join("exports")
    }

    fn schedules_path(&self) -> PathBuf {
        self.data_root.join("schedules.json")
    }

    fn retention_preview_dir(&self) -> PathBuf {
        self.data_root.join("retention_previews")
    }

    fn retention_preview_path(&self, preview_id: &str) -> Result<PathBuf> {
        validate_resource_id(preview_id, "retention preview id")?;
        Ok(self
            .retention_preview_dir()
            .join(format!("{preview_id}.json")))
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

    pub fn load_investigation_memory(&self, conversation_id: &str) -> Result<InvestigationMemory> {
        self.ensure_layout(conversation_id)?;
        let path = self.investigation_memory_path(conversation_id)?;
        if !path.exists() {
            return Ok(InvestigationMemory::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn save_investigation_memory(
        &self,
        conversation_id: &str,
        memory: &InvestigationMemory,
    ) -> Result<()> {
        self.ensure_layout(conversation_id)?;
        let path = self.investigation_memory_path(conversation_id)?;
        let payload = serde_json::to_string_pretty(memory)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn clear_investigation_memory(&self, conversation_id: &str) -> Result<bool> {
        self.ensure_layout(conversation_id)?;
        let path = self.investigation_memory_path(conversation_id)?;
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
        Ok(true)
    }

    pub fn load_activated_skills(&self, conversation_id: &str) -> Result<Vec<ActivatedSkill>> {
        self.ensure_layout(conversation_id)?;
        let path = self.activated_skills_path(conversation_id)?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn save_activated_skills(
        &self,
        conversation_id: &str,
        skills: &[ActivatedSkill],
    ) -> Result<()> {
        self.ensure_layout(conversation_id)?;
        let path = self.activated_skills_path(conversation_id)?;
        let payload = serde_json::to_string_pretty(skills)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn upsert_activated_skill(
        &self,
        conversation_id: &str,
        skill: ActivatedSkill,
    ) -> Result<()> {
        let mut skills = self.load_activated_skills(conversation_id)?;
        skills.retain(|existing| existing.name != skill.name);
        skills.push(skill);
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        self.save_activated_skills(conversation_id, &skills)
    }

    pub fn load_findings(&self) -> Result<Vec<FindingRecord>> {
        self.init()?;
        let path = self.findings_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let findings: Vec<FindingRecord> = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        for finding in &findings {
            finding
                .validate_for_storage()
                .with_context(|| format!("invalid finding state in {}", path.display()))?;
        }
        Ok(findings)
    }

    pub fn save_findings(&self, findings: &[FindingRecord]) -> Result<()> {
        self.init()?;
        for finding in findings {
            finding
                .validate_for_storage()
                .with_context(|| format!("invalid finding state for '{}'", finding.finding_id))?;
        }
        let path = self.findings_path();
        let payload = serde_json::to_string_pretty(findings)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))
    }

    #[allow(clippy::too_many_arguments)]
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
        self.add_finding_detailed(
            conversation_id,
            kind,
            value,
            note,
            tags,
            confidence,
            source_artifact,
            FindingRecordDetails::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_finding_detailed(
        &self,
        conversation_id: &str,
        kind: &str,
        value: &str,
        note: Option<&str>,
        tags: &[String],
        confidence: Option<u8>,
        source_artifact: Option<&str>,
        details: FindingRecordDetails,
    ) -> Result<FindingRecord> {
        validate_conversation_id(conversation_id)?;
        let now = Utc::now();
        let mut finding = FindingRecord::new(
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
        )?;
        finding.apply_details(details)?;
        let mut findings = self.load_findings()?;
        findings.push(finding.clone());
        findings.sort_by_key(|finding| std::cmp::Reverse(finding.updated_at));
        self.save_findings(&findings)?;
        Ok(finding)
    }

    #[allow(clippy::too_many_arguments)]
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
        self.update_finding_detailed(
            finding_id,
            kind,
            value,
            note,
            tags,
            confidence,
            source_artifact,
            FindingRecordDetails::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_finding_detailed(
        &self,
        finding_id: &str,
        kind: &str,
        value: &str,
        note: Option<&str>,
        tags: &[String],
        confidence: Option<u8>,
        source_artifact: Option<&str>,
        details: FindingRecordDetails,
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
            finding.set_confidence(confidence)?;
            finding.source_artifact = normalize_optional_text(source_artifact);
            finding.apply_details(details.clone())?;
            finding.updated_at = Utc::now();
            updated = Some(finding.clone());
            break;
        }
        if updated.is_some() {
            findings.sort_by_key(|finding| std::cmp::Reverse(finding.updated_at));
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

    pub fn set_conversation_protection(
        &self,
        conversation_id: &str,
        pinned: Option<bool>,
        legal_hold: Option<bool>,
    ) -> Result<Conversation> {
        let mut conversation = self.load(conversation_id)?;
        if let Some(value) = pinned {
            conversation.pinned = value;
        }
        if let Some(value) = legal_hold {
            conversation.legal_hold = value;
        }
        conversation.updated_at = Utc::now();
        self.save(&conversation)?;
        Ok(conversation)
    }

    pub fn export_conversation_summary(&self, conversation_id: &str) -> Result<PathBuf> {
        let conversation = self.load(conversation_id)?;
        let scratchpad = self.load_scratchpad(conversation_id)?;
        let investigation_memory = self.load_investigation_memory(conversation_id)?;
        let jobs = self.load_job_state(conversation_id)?;
        let activated_skills = self.load_activated_skills(conversation_id)?;
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
            "investigation_memory": investigation_memory,
            "jobs": jobs,
            "activated_skills": activated_skills,
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

    pub fn create_retention_preview(&self, policy: RetentionPolicy) -> Result<RetentionPreview> {
        self.init()?;
        let now = Utc::now();
        let cutoff = policy
            .older_than_days
            .map(|days| now - chrono::Duration::days(days.max(0)));
        let schedule_conversations = self
            .load_schedules()?
            .into_iter()
            .map(|schedule| schedule.conversation_id)
            .collect::<std::collections::HashSet<_>>();

        let mut preview = RetentionPreview {
            preview_id: generate_prefixed_id("retention"),
            created_at: now,
            policy: policy.clone(),
            items: Vec::new(),
            blocked: Vec::new(),
        };

        for summary in self.list_conversations_with_archived(true)? {
            let conversation = self.load(&summary.conversation_id)?;
            let conversation_dir = self.conversation_dir(&summary.conversation_id)?;
            let age_ok = cutoff
                .map(|cutoff| conversation.updated_at <= cutoff)
                .unwrap_or(true);
            let is_archived = conversation.archived_at.is_some();
            let protected_reason = if conversation.pinned {
                Some("conversation is pinned")
            } else if conversation.legal_hold {
                Some("conversation has a legal hold")
            } else if conversation.active_workflow.is_some() {
                Some("conversation has an active workflow")
            } else if conversation.active_continuation.is_some() {
                Some("conversation has an active turn continuation")
            } else if schedule_conversations.contains(&conversation.conversation_id) {
                Some("conversation is owned by a schedule")
            } else {
                None
            };

            if let Some(reason) = protected_reason {
                preview.blocked.push(RetentionBlockedItem {
                    kind: "conversation".to_string(),
                    path: conversation_dir.display().to_string(),
                    conversation_id: Some(conversation.conversation_id),
                    reason: reason.to_string(),
                });
                continue;
            }

            let selected = age_ok
                && ((is_archived && policy.include_archived)
                    || (!is_archived && policy.include_active && policy.force));
            if selected {
                preview.items.push(RetentionItem {
                    kind: "conversation".to_string(),
                    path: conversation_dir.display().to_string(),
                    conversation_id: Some(conversation.conversation_id),
                    byte_count: dir_size(&conversation_dir).unwrap_or(0),
                    reason: if is_archived {
                        "archived conversation matched retention policy".to_string()
                    } else {
                        "active conversation matched forced retention policy".to_string()
                    },
                });
            }
        }

        if policy.include_exports {
            let export_root = self.export_root();
            if export_root.exists() {
                for entry in fs::read_dir(&export_root)
                    .with_context(|| format!("failed to read {}", export_root.display()))?
                {
                    let entry = entry?;
                    if entry.file_type()?.is_file() {
                        preview.items.push(RetentionItem {
                            kind: "export".to_string(),
                            path: entry.path().display().to_string(),
                            conversation_id: None,
                            byte_count: entry.metadata().map(|m| m.len()).unwrap_or(0),
                            reason: "export matched retention policy".to_string(),
                        });
                    }
                }
            }
        }

        fs::create_dir_all(self.retention_preview_dir())?;
        let path = self.retention_preview_path(&preview.preview_id)?;
        fs::write(&path, serde_json::to_string_pretty(&preview)?)
            .with_context(|| format!("failed to write {}", path.display()))?;
        self.append_audit_event(
            None,
            "retention_preview",
            "retention preview created",
            json!({"preview_id": preview.preview_id, "item_count": preview.items.len(), "blocked_count": preview.blocked.len()}),
        )?;
        Ok(preview)
    }

    pub fn load_retention_preview(&self, preview_id: &str) -> Result<RetentionPreview> {
        let path = self.retention_preview_path(preview_id)?;
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn apply_retention_preview(&self, preview_id: &str, force: bool) -> Result<Vec<String>> {
        let preview = self.load_retention_preview(preview_id)?;
        if !force && !preview.policy.force {
            bail!("retention apply requires force=true");
        }
        let mut deleted = Vec::new();
        for item in preview.items {
            let path = PathBuf::from(&item.path);
            match item.kind.as_str() {
                "conversation" => {
                    if let Some(conversation_id) = item.conversation_id.as_deref() {
                        self.delete(conversation_id)?;
                        deleted.push(path.display().to_string());
                    }
                }
                "export" => {
                    let export_root = self.export_root();
                    let canonical_root = fs::canonicalize(&export_root).with_context(|| {
                        format!("failed to canonicalize {}", export_root.display())
                    })?;
                    let canonical_path = fs::canonicalize(&path)
                        .with_context(|| format!("failed to canonicalize {}", path.display()))?;
                    if canonical_path.starts_with(canonical_root) {
                        fs::remove_file(&canonical_path).with_context(|| {
                            format!("failed to remove {}", canonical_path.display())
                        })?;
                        deleted.push(canonical_path.display().to_string());
                    }
                }
                _ => {}
            }
        }
        self.append_audit_event(
            None,
            "retention_apply",
            "retention preview applied",
            json!({"preview_id": preview_id, "deleted": deleted}),
        )?;
        Ok(deleted)
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

            let memory = self.load_investigation_memory(&summary.conversation_id)?;
            if !memory.is_empty() {
                let memory_text = investigation_memory_search_text(&memory);
                if memory_text.to_ascii_lowercase().contains(&needle) {
                    results.push(SearchResult {
                        scope: "investigation_memory".to_string(),
                        title: summary.conversation_id.clone(),
                        snippet: summarize_match(&memory_text, query),
                    });
                }
            }
        }

        for finding in self.load_findings()? {
            let haystack = format!(
                "{} {} {} {} {} {} {} {} {} {} {} {} {} {}",
                finding.conversation_id,
                finding.kind,
                finding.value,
                finding.status,
                finding.severity.as_deref().unwrap_or(""),
                finding.affected_endpoint.as_deref().unwrap_or(""),
                finding.vuln_class.as_deref().unwrap_or(""),
                finding.wstg_ids.join(" "),
                finding.api_top10_ids.join(" "),
                finding.evidence_artifacts.join(" "),
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
                            "{}: {} // status: {}{}{}{}{}{}{}",
                            finding.kind,
                            finding.value,
                            finding.status,
                            finding
                                .severity
                                .as_deref()
                                .map(|severity| format!(" // severity: {severity}"))
                                .unwrap_or_default(),
                            finding
                                .affected_endpoint
                                .as_deref()
                                .map(|endpoint| format!(" // endpoint: {endpoint}"))
                                .unwrap_or_default(),
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

    pub fn search_investigation_memories(&self, query: &str) -> Result<Vec<SearchResult>> {
        let needle = query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        for summary in self.list_conversations_with_archived(true)? {
            let memory = self.load_investigation_memory(&summary.conversation_id)?;
            if memory.is_empty() {
                continue;
            }
            let memory_text = investigation_memory_search_text(&memory);
            if memory_text.to_ascii_lowercase().contains(&needle) {
                results.push(SearchResult {
                    scope: "investigation_memory".to_string(),
                    title: summary.conversation_id,
                    snippet: summarize_match(&memory_text, query),
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
            let jobs: Vec<RememberedJob> = serde_json::from_str(&raw)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            for job in &jobs {
                job.validate_for_storage()
                    .with_context(|| format!("invalid job state in {}", path.display()))?;
            }
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
            let mut job = RememberedJob::new(alias.clone(), transaction_id)?;
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
        for job in jobs {
            job.validate_for_storage()
                .with_context(|| format!("invalid job state for '{}'", job.alias))?;
        }
        let path = self.job_state_path(conversation_id)?;
        let payload = serde_json::to_string_pretty(jobs)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn save_workflow_run(&self, run: &WorkflowRun) -> Result<()> {
        self.ensure_layout(&run.conversation_id)?;
        let path = self.workflow_run_path(&run.conversation_id, &run.workflow_id)?;
        let payload = serde_json::to_string_pretty(run)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))?;
        let mut conversation = self.load(&run.conversation_id)?;
        conversation.active_workflow = match run.status.as_str() {
            "completed" | "failed" | "cancelled" => None,
            _ => Some(run.workflow_id.clone()),
        };
        self.save(&conversation)?;
        Ok(())
    }

    pub fn save_turn_continuation(&self, continuation: &TurnContinuation) -> Result<()> {
        self.ensure_layout(&continuation.conversation_id)?;
        let path = self
            .turn_continuation_path(&continuation.conversation_id, &continuation.continuation_id)?;
        let payload = serde_json::to_string_pretty(continuation)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))?;
        let mut conversation = self.load(&continuation.conversation_id)?;
        conversation.active_continuation = match continuation.status.as_str() {
            "needs_continuation" => Some(continuation.continuation_id.clone()),
            _ => None,
        };
        conversation.updated_at = Utc::now();
        self.save(&conversation)?;
        Ok(())
    }

    pub fn load_turn_continuation(
        &self,
        conversation_id: &str,
        continuation_id: &str,
    ) -> Result<TurnContinuation> {
        let path = self.turn_continuation_path(conversation_id, continuation_id)?;
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn list_turn_continuations(&self, conversation_id: &str) -> Result<Vec<TurnContinuation>> {
        self.ensure_layout(conversation_id)?;
        let dir = self
            .conversation_dir(conversation_id)?
            .join("turn_continuations");
        let mut continuations = Vec::new();
        if !dir.exists() {
            return Ok(continuations);
        }
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read_to_string(entry.path())?;
            if let Ok(continuation) = serde_json::from_str::<TurnContinuation>(&raw) {
                continuations.push(continuation);
            }
        }
        continuations.sort_by_key(|continuation| std::cmp::Reverse(continuation.updated_at));
        Ok(continuations)
    }

    pub fn load_workflow_run(
        &self,
        conversation_id: &str,
        workflow_id: &str,
    ) -> Result<WorkflowRun> {
        let path = self.workflow_run_path(conversation_id, workflow_id)?;
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn list_workflow_runs(&self, conversation_id: Option<&str>) -> Result<Vec<WorkflowRun>> {
        self.init()?;
        let mut runs = Vec::new();
        let summaries = self.list_conversations_with_archived(true)?;
        for summary in summaries {
            if conversation_id.is_some_and(|id| id != summary.conversation_id) {
                continue;
            }
            let dir = self
                .conversation_dir(&summary.conversation_id)?
                .join("workflow_runs");
            if !dir.exists() {
                continue;
            }
            for entry in
                fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
            {
                let entry = entry?;
                if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let raw = fs::read_to_string(entry.path())?;
                if let Ok(run) = serde_json::from_str::<WorkflowRun>(&raw) {
                    runs.push(run);
                }
            }
        }
        runs.sort_by_key(|run| std::cmp::Reverse(run.updated_at));
        Ok(runs)
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

    pub fn load_schedules(&self) -> Result<Vec<ScheduleRecord>> {
        self.init()?;
        let path = self.schedules_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn save_schedules(&self, schedules: &[ScheduleRecord]) -> Result<()> {
        self.init()?;
        let path = self.schedules_path();
        let payload = serde_json::to_string_pretty(schedules)?;
        fs::write(&path, payload).with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn upsert_schedule(&self, schedule: ScheduleRecord) -> Result<ScheduleRecord> {
        let mut schedules = self.load_schedules()?;
        schedules.retain(|existing| existing.id != schedule.id);
        schedules.push(schedule.clone());
        schedules.sort_by_key(|schedule| schedule.name.clone());
        self.save_schedules(&schedules)?;
        Ok(schedule)
    }

    pub fn get_schedule(&self, schedule_id: &str) -> Result<Option<ScheduleRecord>> {
        validate_resource_id(schedule_id, "schedule id")?;
        Ok(self
            .load_schedules()?
            .into_iter()
            .find(|schedule| schedule.id == schedule_id))
    }

    pub fn delete_schedule(&self, schedule_id: &str) -> Result<bool> {
        validate_resource_id(schedule_id, "schedule id")?;
        let mut schedules = self.load_schedules()?;
        let original_len = schedules.len();
        schedules.retain(|schedule| schedule.id != schedule_id);
        if schedules.len() == original_len {
            return Ok(false);
        }
        self.save_schedules(&schedules)?;
        Ok(true)
    }

    pub fn list_due_schedules(&self, now: chrono::DateTime<Utc>) -> Result<Vec<ScheduleRecord>> {
        Ok(self
            .load_schedules()?
            .into_iter()
            .filter(|schedule| {
                schedule.enabled
                    && schedule.next_run_at <= now
                    && schedule
                        .lease_expires_at
                        .map(|lease| lease <= now)
                        .unwrap_or(true)
            })
            .collect())
    }

    pub fn claim_schedule(
        &self,
        schedule_id: &str,
        now: chrono::DateTime<Utc>,
        lease_seconds: i64,
        force: bool,
    ) -> Result<Option<ScheduleRecord>> {
        validate_resource_id(schedule_id, "schedule id")?;
        let mut schedules = self.load_schedules()?;
        let Some(schedule) = schedules
            .iter_mut()
            .find(|schedule| schedule.id == schedule_id)
        else {
            return Ok(None);
        };
        if !force
            && (!schedule.enabled
                || schedule.next_run_at > now
                || schedule
                    .lease_expires_at
                    .map(|lease| lease > now)
                    .unwrap_or(false))
        {
            return Ok(None);
        }
        schedule.lease_expires_at = Some(now + chrono::Duration::seconds(lease_seconds));
        schedule.last_status = Some("running".to_string());
        schedule.last_error = None;
        schedule.updated_at = now;
        let claimed = schedule.clone();
        self.save_schedules(&schedules)?;
        Ok(Some(claimed))
    }

    pub fn release_schedule(
        &self,
        schedule_id: &str,
        status: &str,
        error: Option<String>,
        next_run_at: chrono::DateTime<Utc>,
    ) -> Result<Option<ScheduleRecord>> {
        validate_resource_id(schedule_id, "schedule id")?;
        let mut schedules = self.load_schedules()?;
        let Some(schedule) = schedules
            .iter_mut()
            .find(|schedule| schedule.id == schedule_id)
        else {
            return Ok(None);
        };
        let now = Utc::now();
        schedule.lease_expires_at = None;
        schedule.last_status = Some(status.to_string());
        schedule.last_error = error;
        schedule.last_run_at = Some(now);
        schedule.next_run_at = next_run_at;
        schedule.updated_at = now;
        let updated = schedule.clone();
        self.save_schedules(&schedules)?;
        Ok(Some(updated))
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
        let payload = serde_json::to_string_pretty(&CompactionRecord {
            checkpoint_id: checkpoint_id.to_string(),
            created_at: Utc::now().to_rfc3339(),
            summary: summary.to_string(),
        })?;
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
        let record: CompactionRecord = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse compaction {}", path.display()))?;
        if record.checkpoint_id != checkpoint_id {
            bail!(
                "compaction {} contains checkpoint id '{}'",
                path.display(),
                record.checkpoint_id
            );
        }
        if record.summary.trim().is_empty() {
            bail!("compaction {} has an empty summary", path.display());
        }
        Ok(record.summary)
    }
}

fn validate_conversation_id(conversation_id: &str) -> Result<()> {
    validate_resource_id(conversation_id, "conversation id")
}

fn validate_resource_id(value: &str, label: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{label} must not be empty");
    }
    if value.len() > 128 {
        bail!("{label} is too long");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!(
            "invalid {label} '{value}'; allowed characters are ASCII letters, digits, '-' and '_'"
        );
    }
    Ok(())
}

fn generate_conversation_id(now: chrono::DateTime<Utc>) -> String {
    let suffix = rand::random::<u32>();
    format!("convo-{}-{suffix:08x}", now.format("%Y%m%d%H%M%S"))
}

fn generate_prefixed_id(prefix: &str) -> String {
    let suffix = rand::random::<u32>();
    format!(
        "{}-{}-{suffix:08x}",
        prefix,
        Utc::now().format("%Y%m%d%H%M%S")
    )
}

fn append_json_line<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(value)?)?;
    Ok(())
}

fn read_json_lines<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut values = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        values.push(
            serde_json::from_str(&line)
                .with_context(|| format!("failed to parse JSON line in {}", path.display()))?,
        );
    }
    Ok(values)
}

fn dir_size(path: &Path) -> Result<u64> {
    if path.is_file() {
        return Ok(path.metadata()?.len());
    }
    let mut total = 0u64;
    if !path.exists() {
        return Ok(total);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            total = total.saturating_add(dir_size(&path)?);
        } else if path.is_file() {
            total = total.saturating_add(entry.metadata()?.len());
        }
    }
    Ok(total)
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

fn investigation_memory_search_text(memory: &InvestigationMemory) -> String {
    let mut parts = Vec::new();
    if let Some(updated_at) = memory.updated_at.as_ref() {
        parts.push(format!("updated_at: {}", updated_at.to_rfc3339()));
    }
    if let Some(updated_by) = memory.updated_by.as_deref()
        && !updated_by.trim().is_empty()
    {
        parts.push(format!("updated_by: {}", updated_by.trim()));
    }
    if !memory.summary.trim().is_empty() {
        parts.push(format!("summary: {}", memory.summary.trim()));
    }
    append_memory_values(&mut parts, "entities", &memory.entities);
    append_memory_values(&mut parts, "timeline", &memory.timeline);
    append_memory_values(&mut parts, "decisions", &memory.decisions);
    append_memory_values(&mut parts, "hypotheses", &memory.hypotheses);
    append_memory_values(&mut parts, "trusted_sources", &memory.trusted_sources);
    append_memory_values(
        &mut parts,
        "unresolved_questions",
        &memory.unresolved_questions,
    );
    parts.join("\n")
}

fn append_memory_values(parts: &mut Vec<String>, label: &str, values: &[serde_json::Value]) {
    for value in values {
        parts.push(format!("{label}: {}", memory_value_text(value)));
    }
}

fn memory_value_text(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default())
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

    use chrono::Utc;
    use serde_json::json;
    use tempfile::tempdir;

    use crate::{
        llm::LlmMessage,
        types::{
            AgentPermissions, FindingRecordDetails, InvestigationMemory, RetentionPolicy,
            ScheduleCadence, ScheduleIntervalUnit, ScheduleRecord, ToolArtifact,
            TurnContinuation,
        },
    };

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
    fn turn_continuation_round_trips_and_marks_active_conversation() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let now = Utc::now();
        let continuation = TurnContinuation {
            continuation_id: "turn-test".to_string(),
            conversation_id: conversation.conversation_id.clone(),
            status: "needs_continuation".to_string(),
            recipe_name: Some("demo".to_string()),
            workflow: None,
            messages: vec![LlmMessage::UserText("continue me".to_string())],
            iterations_used: 10,
            max_total_iterations: 50,
            continuation_increment: 10,
            tool_seconds: 1.0,
            llm_seconds: 2.0,
            tool_call_count: 3,
            llm_usage: None,
            evidence: Vec::new(),
            automation: false,
            suppress_persistence: false,
            created_at: now,
            updated_at: now,
        };

        store.save_turn_continuation(&continuation).unwrap();

        let loaded = store
            .load_turn_continuation(&conversation.conversation_id, "turn-test")
            .unwrap();
        assert_eq!(loaded.iterations_used, 10);
        assert_eq!(
            store
                .load(&conversation.conversation_id)
                .unwrap()
                .active_continuation
                .as_deref(),
            Some("turn-test")
        );
        assert_eq!(
            store
                .list_turn_continuations(&conversation.conversation_id)
                .unwrap()
                .len(),
            1
        );
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
    fn tool_artifact_index_round_trips() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let artifact = ToolArtifact {
            artifact_id: "artifact-demo".to_string(),
            conversation_id: conversation.conversation_id.clone(),
            tool_name: "local__time".to_string(),
            status: "success".to_string(),
            created_at: chrono::Utc::now(),
            relative_path: "tool_output/demo.txt".to_string(),
            byte_count: 4,
            arguments_redacted: json!({"token": "[REDACTED]"}),
            preview: "demo".to_string(),
        };

        store.append_tool_artifact(&artifact).unwrap();

        let artifacts = store
            .load_tool_artifacts(&conversation.conversation_id)
            .unwrap();
        assert_eq!(artifacts, vec![artifact.clone()]);
        assert_eq!(
            store
                .get_tool_artifact(&conversation.conversation_id, "artifact-demo")
                .unwrap(),
            Some(artifact)
        );
    }

    #[test]
    fn schedule_claim_and_release_round_trip() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let now = chrono::Utc::now();
        let schedule = ScheduleRecord {
            id: "schedule-demo".to_string(),
            name: "Demo".to_string(),
            title: None,
            run_type: "prompt".to_string(),
            recipe_name: None,
            prompt: Some("hello".to_string()),
            conversation_id: conversation.conversation_id,
            cadence: ScheduleCadence::Interval {
                every: 15,
                unit: ScheduleIntervalUnit::Minutes,
            },
            enabled: true,
            next_run_at: now - chrono::Duration::minutes(1),
            last_run_at: None,
            last_status: None,
            last_error: None,
            lease_expires_at: None,
            created_at: now,
            updated_at: now,
        };

        store.upsert_schedule(schedule).unwrap();
        let claimed = store
            .claim_schedule("schedule-demo", now, 60, false)
            .unwrap()
            .unwrap();
        assert_eq!(claimed.last_status.as_deref(), Some("running"));
        assert!(claimed.lease_expires_at.is_some());

        let next = now + chrono::Duration::minutes(15);
        let released = store
            .release_schedule("schedule-demo", "done", None, next)
            .unwrap()
            .unwrap();
        assert_eq!(released.last_status.as_deref(), Some("done"));
        assert_eq!(released.next_run_at, next);
        assert!(released.lease_expires_at.is_none());
    }

    #[test]
    fn retention_preview_blocks_protected_conversations() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let pinned = store.create_conversation().unwrap();
        store
            .set_conversation_protection(&pinned.conversation_id, Some(true), None)
            .unwrap();
        store.archive_conversation(&pinned.conversation_id).unwrap();

        let preview = store
            .create_retention_preview(RetentionPolicy {
                older_than_days: Some(0),
                include_archived: true,
                include_active: false,
                include_exports: false,
                force: true,
            })
            .unwrap();

        assert!(preview.items.is_empty());
        assert!(preview.blocked.iter().any(|item| {
            item.conversation_id.as_deref() == Some(pinned.conversation_id.as_str())
                && item.reason.contains("pinned")
        }));
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
        assert!(value.get("investigation_memory").is_some());
        assert_eq!(value["findings"].as_array().unwrap().len(), 1);
        assert_eq!(value["findings"][0]["confidence"], 80);
        assert_eq!(
            export_path.file_name().unwrap().to_string_lossy(),
            format!("{}-summary.json", conversation.conversation_id)
        );
    }

    #[test]
    fn load_compaction_rejects_missing_summary() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        fs::write(
            store
                .conversation_dir(&conversation.conversation_id)
                .unwrap()
                .join("compactions/checkpoint-1.json"),
            r#"{"checkpoint_id":"checkpoint-1","created_at":"2026-05-09T00:00:00Z"}"#,
        )
        .unwrap();

        let err = store
            .load_compaction(&conversation.conversation_id, "checkpoint-1")
            .unwrap_err();

        assert!(format!("{err:#}").contains("missing field `summary`"));
    }

    #[test]
    fn findings_reject_invalid_confidence() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();

        let err = store
            .add_finding(
                &conversation.conversation_id,
                "ip",
                "1.2.3.4",
                None,
                &[],
                Some(101),
                None,
            )
            .unwrap_err();

        assert!(format!("{err:#}").contains("invalid finding confidence"));
    }

    #[test]
    fn findings_store_validation_metadata() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();

        let finding = store
            .add_finding_detailed(
                &conversation.conversation_id,
                "web",
                "IDOR on profile endpoint",
                Some("validated manually"),
                &["web".to_string()],
                Some(95),
                Some("tool_output/request.txt"),
                FindingRecordDetails {
                    status: Some("validated".to_string()),
                    severity: Some("high".to_string()),
                    affected_endpoint: Some("https://example.com/api/users/2".to_string()),
                    vuln_class: Some("access-control".to_string()),
                    wstg_ids: Some(vec!["WSTG-ATHZ".to_string()]),
                    api_top10_ids: Some(vec!["API1".to_string()]),
                    evidence_artifacts: Some(vec!["artifact-1".to_string()]),
                    validation_gates: Some(vec![crate::types::FindingValidationGate {
                        gate: "in-scope".to_string(),
                        status: "pass".to_string(),
                        reason: Some("allowed host".to_string()),
                    }]),
                },
            )
            .unwrap();

        assert_eq!(finding.status, "validated");
        assert_eq!(finding.severity.as_deref(), Some("high"));
        assert_eq!(finding.wstg_ids, vec!["WSTG-ATHZ"]);
        assert_eq!(finding.validation_gates[0].status, "pass");
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

    #[test]
    fn investigation_memory_round_trip_and_search_works() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let conversation = store.create_conversation().unwrap();
        let memory = InvestigationMemory {
            summary: "Tracking suspicious admin login".to_string(),
            entities: vec![json!({"type": "user", "value": "alice@example.com"})],
            timeline: vec![json!({"time": "2026-04-23T02:00:00Z", "event": "login"})],
            ..InvestigationMemory::default()
        };

        store
            .save_investigation_memory(&conversation.conversation_id, &memory)
            .unwrap();

        let loaded = store
            .load_investigation_memory(&conversation.conversation_id)
            .unwrap();
        assert_eq!(loaded.summary, "Tracking suspicious admin login");

        let results = store
            .search_investigation_memories("alice@example.com")
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].scope, "investigation_memory");

        assert!(
            store
                .clear_investigation_memory(&conversation.conversation_id)
                .unwrap()
        );
        assert!(
            store
                .load_investigation_memory(&conversation.conversation_id)
                .unwrap()
                .is_empty()
        );
    }
}
