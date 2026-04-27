use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::sync::{Mutex, OwnedMutexGuard, mpsc::UnboundedSender};
use tracing::{debug, error, info, warn};

use crate::{
    config::AppConfig,
    conversation_store::ConversationStore,
    llm::{
        LlmAssistantBlock, LlmClient, LlmMessage, LlmStopReason, LlmTool, LlmToolResult,
        llm_message_text_len,
    },
    local_tools::{LocalToolExecutor, local_tool_definitions},
    mcp_runtime::{McpManager, McpTool},
    paths::discover_project_root,
    recipes::RecipeRegistry,
    skills::SkillRegistry,
    tool_evidence::ToolEvidenceWriter,
    types::{
        AgentPermissions, MessageMetadata, MessageTiming, ProgressEvent, RememberedJob,
        RunTurnResult, UiEvent, permission_denied_user_prompt,
    },
};

const MAX_AGENT_ITERATIONS: usize = 10;
const MAX_AZURE_TOOL_COUNT: usize = 128;
const PINNED_LOCAL_TOOL_NAMES: &[&str] = &[
    "local__configure_mcp_servers",
    "local__activate_skill",
    "local__run_skill",
    "local__get_investigation_memory",
    "local__update_investigation_memory",
    "local__search_conversation_memories",
    "local__get_job",
    "local__list_jobs",
];

#[derive(Clone)]
pub struct Orchestrator {
    inner: Arc<OrchestratorInner>,
}

struct OrchestratorInner {
    config: AppConfig,
    store: ConversationStore,
    evidence: ToolEvidenceWriter,
    llm: LlmClient,
    mcp: Mutex<McpManager>,
    conversation_locks: ConversationTurnLocks,
    skills: SkillRegistry,
    recipes: RecipeRegistry,
}

struct FinishTurnContext<'a> {
    conversation_id: &'a str,
    final_reply: &'a str,
    turn_start: std::time::Instant,
    tool_seconds: f64,
    llm_seconds: f64,
    tool_call_count: usize,
    automation: bool,
    recipe: Option<&'a crate::recipes::Recipe>,
    ui_tx: &'a UnboundedSender<UiEvent>,
}

struct MessageBuildContext<'a> {
    prompt: Option<&'a str>,
    history: &'a [crate::types::Message],
    mcp_error: Option<&'a str>,
    recipe_instructions: Option<&'a str>,
    capability_summary: Option<&'a str>,
    mcp_capability_summary: Option<&'a str>,
    agent_permissions: &'a AgentPermissions,
    store: &'a ConversationStore,
    conversation_id: &'a str,
    user_message_override: Option<&'a str>,
}

#[derive(Default)]
struct ConversationTurnLocks {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl ConversationTurnLocks {
    async fn acquire(&self, conversation_id: &str) -> OwnedMutexGuard<()> {
        let conversation_lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(conversation_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        conversation_lock.lock_owned().await
    }
}

impl Orchestrator {
    pub fn new(config: AppConfig) -> Result<Self> {
        let data_dir = config.data_dir();
        let store = ConversationStore::new(&data_dir, config.agent_permissions.clone());
        store.init()?;
        let evidence = ToolEvidenceWriter::new(store.clone());
        let llm = LlmClient::new(&config)?;
        let mcp = McpManager::new(
            &data_dir,
            config.mcp_runtime.clone(),
            config.mcp_servers.clone(),
        )?;

        // Load Agent Skills from project/user locations, including .agents/skills.
        let project_root = discover_project_root().unwrap_or_else(|| std::path::PathBuf::from("."));
        let skills = match SkillRegistry::load_all(&project_root) {
            Ok(skills) => skills,
            Err(err) => {
                warn!(
                    project_root = %project_root.display(),
                    error = %err,
                    "failed to load skills registry; continuing with no skills"
                );
                eprintln!(
                    "Warning: failed to load skills from {}: {err}. Continuing with no skills.",
                    project_root.display()
                );
                SkillRegistry::default()
            }
        };

        // Load recipes from {project_root}/recipes/
        let recipes_dir = discover_project_root()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("recipes");
        let recipes = match RecipeRegistry::load(&recipes_dir) {
            Ok(recipes) => recipes,
            Err(err) => {
                warn!(
                    path = %recipes_dir.display(),
                    error = %err,
                    "failed to load recipe registry; continuing with no recipes"
                );
                eprintln!(
                    "Warning: failed to load recipes from {}: {err}. Continuing with no recipes.",
                    recipes_dir.display()
                );
                RecipeRegistry::default()
            }
        };

        info!(
            mcp_server_count = config.mcp_servers.len(),
            data_dir = %data_dir.display(),
            "initialized orchestrator"
        );
        Ok(Self {
            inner: Arc::new(OrchestratorInner {
                config,
                store,
                evidence,
                llm,
                mcp: Mutex::new(mcp),
                conversation_locks: ConversationTurnLocks::default(),
                skills,
                recipes,
            }),
        })
    }

    pub fn store(&self) -> ConversationStore {
        self.inner.store.clone()
    }

    pub fn recipes(&self) -> &RecipeRegistry {
        &self.inner.recipes
    }

    pub fn configured_mcp_server_names(&self) -> Vec<String> {
        self.inner
            .config
            .mcp_servers
            .iter()
            .map(|server| server.name.clone())
            .collect()
    }

    pub fn default_agent_permissions(&self) -> AgentPermissions {
        self.inner.config.agent_permissions.clone()
    }

    pub fn config(&self) -> AppConfig {
        self.inner.config.clone()
    }

    pub async fn mcp_tool_counts_by_server(
        &self,
        filter: Option<&[String]>,
    ) -> Result<HashMap<String, usize>> {
        let mut manager = self.inner.mcp.lock().await;
        let tools = manager.list_tools_filtered(filter).await?;
        let mut counts = HashMap::new();
        for tool in tools {
            *counts.entry(tool.server_name).or_insert(0) += 1;
        }
        Ok(counts)
    }

    pub async fn login_mcp_server(&self, server_name: &str) -> Result<()> {
        info!(server = server_name, "triggering MCP OAuth login");
        let mut manager = self.inner.mcp.lock().await;
        manager.login_server(server_name).await
    }

    pub async fn ensure_default_conversation(&self) -> Result<String> {
        let conversations = self.inner.store.list_conversations()?;
        if let Some(conversation) = conversations.first() {
            Ok(conversation.conversation_id.clone())
        } else {
            Ok(self.inner.store.create_conversation()?.conversation_id)
        }
    }

    pub async fn compact_conversation(
        &self,
        conversation_id: &str,
        _ui_tx: UnboundedSender<UiEvent>,
    ) -> Result<String> {
        let _conversation_guard = self.acquire_conversation_lock(conversation_id).await;
        info!(%conversation_id, "compacting conversation");
        let conversation = self.inner.store.load(conversation_id)?;

        let mut messages = vec![LlmMessage::System(
            "Produce a compact but complete summary of the following conversation. Preserve key findings, tool results, and decisions. Be concise but thorough."
                .to_string(),
        )];
        messages.extend(
            conversation
                .messages
                .iter()
                .map(stored_message_to_llm_message),
        );

        let summary = match self.inner.llm.chat_completion(&messages, &[]).await {
            Ok(completion) => assistant_text_from_blocks(&completion.assistant_blocks)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Summary unavailable.".to_string()),
            Err(_) => fallback_compaction_summary(&conversation),
        };

        let timestamp = chrono::Utc::now().timestamp();
        let checkpoint_id = format!("compact-{timestamp}");

        self.inner
            .store
            .save_compaction(conversation_id, &checkpoint_id, &summary)?;
        self.inner.store.append_log(
            conversation_id,
            &format!(
                "compaction saved: {checkpoint_id} ({} chars)",
                summary.len()
            ),
        )?;
        info!(%conversation_id, %checkpoint_id, "conversation compacted");
        Ok(summary)
    }

    pub async fn run_turn(
        &self,
        conversation_id: &str,
        user_message: String,
        ui_tx: UnboundedSender<UiEvent>,
    ) -> Result<RunTurnResult> {
        self.run_turn_internal(conversation_id, Some(user_message), ui_tx, false)
            .await
    }

    pub async fn run_automation_turn(
        &self,
        conversation_id: &str,
        job: &RememberedJob,
        ui_tx: UnboundedSender<UiEvent>,
    ) -> Result<RunTurnResult> {
        let prompt = build_automation_prompt(job);
        self.run_turn_internal(conversation_id, Some(prompt), ui_tx, true)
            .await
    }

    async fn run_turn_internal(
        &self,
        conversation_id: &str,
        user_message: Option<String>,
        ui_tx: UnboundedSender<UiEvent>,
        automation: bool,
    ) -> Result<RunTurnResult> {
        let _conversation_guard = self.acquire_conversation_lock(conversation_id).await;
        let turn_start = std::time::Instant::now();
        let mut tool_seconds: f64 = 0.0;
        let mut llm_seconds: f64 = 0.0;

        info!(%conversation_id, "starting conversation turn");
        self.emit(
            &ui_tx,
            "turn_started",
            if automation {
                "Running automation turn"
            } else {
                "Recording user message"
            },
            None,
            None,
        );
        if let Some(user_message) = user_message.as_deref() {
            if !automation {
                self.inner
                    .store
                    .append_message(conversation_id, "user", user_message)?;
            }
            self.inner.store.append_log(
                conversation_id,
                &format!(
                    "{} message received: {}",
                    if automation { "automation" } else { "user" },
                    user_message.replace('\n', " ")
                ),
            )?;
        }

        // Load conversation to get per-conversation settings
        let conversation = self.inner.store.load(conversation_id)?;
        let enabled_mcp_servers = conversation.enabled_mcp_servers.clone();
        let enabled_local_tools = conversation.enabled_local_tools.clone();
        let agent_permissions = conversation.agent_permissions.clone();
        let pending_recipe_name = conversation.pending_recipe.clone();

        // Load recipe if pending
        let recipe = pending_recipe_name
            .as_deref()
            .and_then(|name| self.inner.recipes.find(name))
            .cloned();

        // Determine effective MCP server filter
        let mcp_filter: Option<Vec<String>> = recipe
            .as_ref()
            .and_then(|r| r.config_mcp_servers.clone())
            .or(enabled_mcp_servers);
        let local_tool_filter: Option<Vec<String>> = recipe
            .as_ref()
            .and_then(|r| r.config_local_tools.clone())
            .or(enabled_local_tools);

        let history = conversation.messages.clone();
        let (mcp_tools, mcp_error) = if agent_permissions.allows_network() {
            let mut manager = self.inner.mcp.lock().await;
            match manager.list_tools_filtered(mcp_filter.as_deref()).await {
                Ok(tools) => (tools, None),
                Err(err) => (Vec::new(), Some(err.to_string())),
            }
        } else {
            (
                Vec::new(),
                Some("Network access is disabled by the active agent permissions.".to_string()),
            )
        };

        if let Some(error) = mcp_error.as_deref() {
            warn!(%conversation_id, error, "MCP discovery degraded");
            self.inner
                .store
                .append_log(conversation_id, &format!("MCP discovery degraded: {error}"))?;
        }

        // Build local tool definitions and rank the advertised subset for the current turn.
        let local_defs = local_tool_definitions(
            local_tool_filter.as_deref(),
            &self.inner.config.local_tools,
            Some(&self.inner.skills),
        );
        let tool_selection_query =
            build_tool_selection_query(user_message.as_deref(), &history, recipe.as_ref());
        let (llm_tools, omitted_tool_count) =
            build_ranked_azure_tools_with_local(&local_defs, &mcp_tools, &tool_selection_query);
        let advertised_mcp_tools = advertised_mcp_tools(&llm_tools, &mcp_tools);
        let mcp_capability_summary =
            summarize_advertised_mcp_tools(&advertised_mcp_tools, mcp_tools.len());

        if omitted_tool_count > 0 {
            warn!(
                %conversation_id,
                total_tools = mcp_tools.len() + local_defs.len(),
                advertised_tools = llm_tools.len(),
                omitted_tool_count,
                "truncated tools to satisfy LLM tool limit"
            );
            self.emit(
                &ui_tx,
                "tool_limit",
                &format!(
                    "LLM tool limit reached: advertising {} of {} tools. Reduce enabled MCP servers with /mcp disable <name> or /mcp only <name...>, or narrow local tools in the active recipe/config.",
                    llm_tools.len(),
                    mcp_tools.len() + local_defs.len(),
                ),
                None,
                None,
            );
        }

        // Skills capability summary
        let capability_summary = self.inner.skills.capability_summary();
        let recipe_guidance = recipe.as_ref().map(|r| r.prompt_guidance());

        let mut messages = build_messages(MessageBuildContext {
            prompt: self.inner.config.prompt.as_deref(),
            history: &history,
            mcp_error: mcp_error.as_deref(),
            recipe_instructions: recipe_guidance.as_deref(),
            capability_summary: if capability_summary.is_empty() {
                None
            } else {
                Some(capability_summary.as_str())
            },
            mcp_capability_summary: if mcp_capability_summary.is_empty() {
                None
            } else {
                Some(mcp_capability_summary.as_str())
            },
            agent_permissions: &agent_permissions,
            store: &self.inner.store,
            conversation_id,
            user_message_override: if automation {
                user_message.as_deref()
            } else {
                None
            },
        });

        // Create local tool executor
        let local_executor = LocalToolExecutor::new(
            self.inner.store.clone(),
            conversation_id,
            Some(self.inner.skills.clone()),
            agent_permissions.clone(),
            local_tool_filter.clone(),
            std::time::Duration::from_secs(self.inner.config.local_tools.execution_timeout_seconds),
            self.inner.config.local_tools.allowed_cli_tools.clone(),
        );

        let mut tool_call_count = 0usize;

        for iteration in 0..MAX_AGENT_ITERATIONS {
            let context_chars = estimate_context_chars(&messages);
            debug!(
                %conversation_id,
                iteration,
                context_chars,
                message_count = messages.len(),
                "preparing LLM iteration"
            );
            self.emit(
                &ui_tx,
                "context_size",
                &format!(
                    "Context size: ~{} chars across {} messages",
                    context_chars,
                    messages.len()
                ),
                None,
                Some(tool_call_count),
            );
            let llm_status = if iteration == 0 {
                format!("Consulting {}", self.inner.llm.provider_label())
            } else {
                "Continuing tool-grounded reasoning".to_string()
            };
            self.emit(
                &ui_tx,
                "llm_start",
                &llm_status,
                None,
                Some(tool_call_count),
            );

            let llm_start = std::time::Instant::now();
            let completion = match self.inner.llm.chat_completion(&messages, &llm_tools).await {
                Ok(completion) => completion,
                Err(err) => {
                    if iteration == 0 {
                        let reply = if automation {
                            format!("Automation deferred: {err}")
                        } else {
                            format!(
                                "{} inference unavailable: {err}",
                                self.inner.llm.provider_label()
                            )
                        };
                        return self
                            .finish_turn(FinishTurnContext {
                                conversation_id,
                                final_reply: &reply,
                                turn_start,
                                tool_seconds,
                                llm_seconds,
                                tool_call_count,
                                automation,
                                recipe: recipe.as_ref(),
                                ui_tx: &ui_tx,
                            })
                            .await;
                    }
                    error!(%conversation_id, iteration, error = %err, "LLM completion failed");
                    return Err(err).context("LLM completion failed");
                }
            };
            llm_seconds += llm_start.elapsed().as_secs_f64();

            let tool_uses = completion
                .assistant_blocks
                .iter()
                .filter_map(|block| match block {
                    LlmAssistantBlock::ToolUse { id, name, input } => {
                        Some((id.clone(), name.clone(), input.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();

            if !tool_uses.is_empty() {
                messages.push(LlmMessage::Assistant {
                    blocks: completion.assistant_blocks.clone(),
                });

                let mut tool_results = Vec::with_capacity(tool_uses.len());
                for (tool_use_id, tool_name, tool_input) in tool_uses {
                    tool_call_count += 1;
                    info!(
                        %conversation_id,
                        tool = %tool_name,
                        tool_call_count,
                        "executing tool call"
                    );
                    self.emit(
                        &ui_tx,
                        "tool_start",
                        &format!("Calling {}", tool_name),
                        Some(tool_name.clone()),
                        Some(tool_call_count),
                    );

                    let tool_start = std::time::Instant::now();
                    let tool_result = self
                        .execute_tool_call(
                            conversation_id,
                            &tool_name,
                            tool_input,
                            &local_executor,
                            &agent_permissions,
                        )
                        .await;
                    tool_seconds += tool_start.elapsed().as_secs_f64();

                    let tool_output: Result<String, LlmToolResult> = match tool_result {
                        Ok(output) => Ok(output),
                        Err(err) => {
                            let err_text = format!("{err:#}");
                            if let Some(prompt) = permission_denied_user_prompt(&err_text) {
                                self.emit(
                                    &ui_tx,
                                    "tool_end",
                                    &format!("Finished {}", tool_name),
                                    Some(tool_name.clone()),
                                    Some(tool_call_count),
                                );
                                return self
                                    .finish_turn(FinishTurnContext {
                                        conversation_id,
                                        final_reply: &prompt,
                                        turn_start,
                                        tool_seconds,
                                        llm_seconds,
                                        tool_call_count,
                                        automation,
                                        recipe: recipe.as_ref(),
                                        ui_tx: &ui_tx,
                                    })
                                    .await;
                            }
                            Err(LlmToolResult {
                                tool_use_id: tool_use_id.clone(),
                                content: format!("Tool call failed: {err_text}"),
                                is_error: true,
                            })
                        }
                    };

                    self.emit(
                        &ui_tx,
                        "tool_end",
                        &format!("Finished {}", tool_name),
                        Some(tool_name.clone()),
                        Some(tool_call_count),
                    );

                    match tool_output {
                        Ok(output) => tool_results.push(LlmToolResult {
                            tool_use_id,
                            content: output,
                            is_error: false,
                        }),
                        Err(tool_result) => tool_results.push(tool_result),
                    }
                }
                messages.push(LlmMessage::UserToolResults {
                    results: tool_results,
                });
                continue;
            }

            let reply = assistant_text_from_blocks(&completion.assistant_blocks)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "The model returned an empty response.".to_string());

            match &completion.stop_reason {
                LlmStopReason::EndTurn => {}
                LlmStopReason::ToolUse => {
                    return Err(anyhow!(
                        "LLM returned tool_use stop reason without tool_use blocks"
                    ));
                }
                LlmStopReason::MaxTokens => {
                    let reply = format!(
                        "{} stopped at the output token limit before completing the response.",
                        self.inner.llm.provider_label()
                    );
                    return self
                        .finish_turn(FinishTurnContext {
                            conversation_id,
                            final_reply: &reply,
                            turn_start,
                            tool_seconds,
                            llm_seconds,
                            tool_call_count,
                            automation,
                            recipe: recipe.as_ref(),
                            ui_tx: &ui_tx,
                        })
                        .await;
                }
                LlmStopReason::PauseTurn => {
                    messages.push(LlmMessage::Assistant {
                        blocks: completion.assistant_blocks.clone(),
                    });
                    messages.push(LlmMessage::UserText(
                        "Continue from the prior pause and finish the turn.".to_string(),
                    ));
                    continue;
                }
                LlmStopReason::Unknown(reason) => {
                    return Err(anyhow!("unsupported provider stop reason: {reason}"));
                }
            }

            // Apply recipe response template if set
            let final_reply = if let Some(r) = &recipe {
                r.apply_template(&reply)
            } else {
                reply.clone()
            };
            return self
                .finish_turn(FinishTurnContext {
                    conversation_id,
                    final_reply: &final_reply,
                    turn_start,
                    tool_seconds,
                    llm_seconds,
                    tool_call_count,
                    automation,
                    recipe: recipe.as_ref(),
                    ui_tx: &ui_tx,
                })
                .await;
        }

        bail!(
            "agent exceeded the {}-iteration safety limit",
            MAX_AGENT_ITERATIONS
        )
    }

    async fn finish_turn(&self, ctx: FinishTurnContext<'_>) -> Result<RunTurnResult> {
        let mut convo = self.inner.store.load(ctx.conversation_id)?;
        let assistant_index = convo
            .messages
            .iter()
            .filter(|m| m.role == "assistant")
            .count()
            + 1;
        let total_seconds = ctx.turn_start.elapsed().as_secs_f64();
        let metadata = MessageMetadata {
            assistant_index,
            timing: MessageTiming {
                tool_seconds: ctx.tool_seconds,
                llm_seconds: ctx.llm_seconds,
                total_seconds,
            },
            tool_call_count: ctx.tool_call_count,
        };

        convo.messages.push(crate::types::Message {
            role: "assistant".to_string(),
            content: ctx.final_reply.to_string(),
            timestamp: chrono::Utc::now(),
            metadata: Some(metadata),
        });
        convo.updated_at = chrono::Utc::now();
        if ctx.recipe.is_some() && !ctx.automation {
            convo.pending_recipe = None;
        }
        self.inner.store.save(&convo)?;
        self.inner.store.append_log(
            ctx.conversation_id,
            &format!("assistant reply stored ({} chars)", ctx.final_reply.len()),
        )?;
        info!(
            conversation_id = %ctx.conversation_id,
            tool_call_count = ctx.tool_call_count,
            reply_len = ctx.final_reply.len(),
            total_seconds,
            automation = ctx.automation,
            "conversation turn finished"
        );
        self.emit(
            ctx.ui_tx,
            "turn_finished",
            &format!(
                "Assistant #{}  [tools: {:.1}s  llm: {:.1}s]",
                assistant_index, ctx.tool_seconds, ctx.llm_seconds
            ),
            None,
            Some(ctx.tool_call_count),
        );
        Ok(RunTurnResult {
            reply: ctx.final_reply.to_string(),
            tool_calls: ctx.tool_call_count,
        })
    }

    async fn execute_tool_call(
        &self,
        conversation_id: &str,
        tool_name: &str,
        arguments: Value,
        local_executor: &LocalToolExecutor,
        agent_permissions: &AgentPermissions,
    ) -> Result<String> {
        self.inner.store.append_log(
            conversation_id,
            &format!(
                "tool call started: {} args={} ",
                tool_name,
                serde_json::to_string(&arguments)?
            ),
        )?;
        debug!(
            %conversation_id,
            tool = tool_name,
            arguments = %serde_json::to_string(&arguments)?,
            "tool call started"
        );

        // Check if it's a local tool first
        if local_executor.is_local_tool(tool_name) {
            let result = local_executor.execute(tool_name, arguments.clone()).await;
            match result {
                Ok(output) => {
                    self.inner.evidence.write_artifact(
                        conversation_id,
                        tool_name,
                        &arguments,
                        "success",
                        &output,
                    )?;
                    self.inner.store.append_log(
                        conversation_id,
                        &format!("local tool call finished: {}", tool_name),
                    )?;
                    return Ok(output);
                }
                Err(err) => {
                    let failure = format!("{err:#}");
                    self.inner.evidence.write_artifact(
                        conversation_id,
                        tool_name,
                        &arguments,
                        "failure",
                        &failure,
                    )?;
                    self.inner.store.append_log(
                        conversation_id,
                        &format!("local tool call failed: {} error={}", tool_name, failure),
                    )?;
                    return Err(anyhow!(failure));
                }
            }
        }

        if local_executor.is_known_local_tool(tool_name) {
            let failure = format!(
                "local tool '{tool_name}' is disabled by the active recipe or conversation local tool filter. Enable it in Config.local_tools or reset the conversation local tool filter before retrying."
            );
            self.inner.evidence.write_artifact(
                conversation_id,
                tool_name,
                &arguments,
                "failure",
                &failure,
            )?;
            self.inner.store.append_log(
                conversation_id,
                &format!("local tool call disabled: {} error={}", tool_name, failure),
            )?;
            return Err(anyhow!(failure));
        }

        if !agent_permissions.allows_network() {
            let failure = format!(
                "permission denied: MCP tool '{tool_name}' requires network access. Enable it with /permissions network on, or use /yolo on."
            );
            self.inner.evidence.write_artifact(
                conversation_id,
                tool_name,
                &arguments,
                "failure",
                &failure,
            )?;
            self.inner.store.append_log(
                conversation_id,
                &format!("tool call denied: {} error={}", tool_name, failure),
            )?;
            return Err(anyhow!(failure));
        }

        let result = {
            let mut manager = self.inner.mcp.lock().await;
            manager.call_tool(tool_name, arguments.clone()).await
        };

        match result {
            Ok(output) => {
                self.inner.evidence.write_artifact(
                    conversation_id,
                    tool_name,
                    &arguments,
                    "success",
                    &output,
                )?;
                self.inner.store.append_log(
                    conversation_id,
                    &format!("tool call finished: {}", tool_name),
                )?;
                info!(%conversation_id, tool = tool_name, "tool call succeeded");
                Ok(output)
            }
            Err(err) => {
                let failure = format!("{err:#}");
                self.inner.evidence.write_artifact(
                    conversation_id,
                    tool_name,
                    &arguments,
                    "failure",
                    &failure,
                )?;
                self.inner.store.append_log(
                    conversation_id,
                    &format!("tool call failed: {} error={}", tool_name, failure),
                )?;
                warn!(%conversation_id, tool = tool_name, error = %failure, "tool call failed");
                Err(anyhow!(failure))
            }
        }
    }

    async fn acquire_conversation_lock(&self, conversation_id: &str) -> OwnedMutexGuard<()> {
        self.inner.conversation_locks.acquire(conversation_id).await
    }

    fn emit(
        &self,
        ui_tx: &UnboundedSender<UiEvent>,
        kind: &str,
        message: &str,
        tool_name: Option<String>,
        tool_call_count: Option<usize>,
    ) {
        let _ = ui_tx.send(UiEvent::Progress(ProgressEvent {
            kind: kind.to_string(),
            message: message.to_string(),
            tool_name,
            tool_call_count,
        }));
    }
}

fn build_messages(ctx: MessageBuildContext<'_>) -> Vec<LlmMessage> {
    let mut system_prompt = String::from(
        "You are a CSIRT investigation assistant operating inside an evidence-preserving tool runner with built-in local tools, local skill scripts, and optional MCP servers. Use tools when needed, stay grounded in tool output, and be explicit about uncertainty or failures.",
    );
    system_prompt.push_str(
        "\n\nTool execution rules:\n\
        - Prefer local skill execution when an appropriate listed skill exists.\n\
        - When a listed skill matches the task, use `local__activate_skill` when available to load its full `SKILL.md` instructions before acting.\n\
        - A listed skill with a local script must be executed via `local__run_skill`.\n\
        - Use `local__time` before making claims about relative windows like last 12 hours, last 2 days, today, or yesterday.\n\
        - Use investigation memory to preserve durable case context: read it when resuming a case, update it when durable conclusions, entities, decisions, or unresolved questions change.\n\
        - Use `local__exec_cli` only for explicitly allowed local CLI binaries such as `whois`, `dig`, `nslookup`, `vt`, or `nmap` when that tool is advertised.\n\
        - Never say a listed local skill is unavailable because of MCP; use the local runner first.\n\
        - Only describe MCP as unavailable when an advertised MCP tool is actually required or a skill explicitly says it is MCP-backed.\n\
        - Recipes provide prompt guidance and configuration; they are not executable scripts by themselves.",
    );
    system_prompt.push_str(&format!(
        "\n\nActive agent permissions:\n- {}\n",
        ctx.agent_permissions.summary()
    ));
    if let Some(prompt) = ctx.prompt {
        system_prompt.push_str("\n\nOperator prompt:\n");
        system_prompt.push_str(prompt);
    }
    if let Ok(memory) = ctx.store.load_investigation_memory(ctx.conversation_id)
        && !memory.is_empty()
        && let Ok(memory_json) = serde_json::to_string_pretty(&memory)
    {
        system_prompt.push_str("\n\nDurable investigation memory for this conversation:\n");
        system_prompt.push_str(&memory_json);
        system_prompt.push_str(
            "\nUse this as carry-over context. Update it with `local__update_investigation_memory` if the durable case state changes.",
        );
    }
    if let Some(summary) = ctx.capability_summary {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(summary);
    }
    if let Some(summary) = ctx.mcp_capability_summary {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(summary);
    }
    if let Some(instructions) = ctx.recipe_instructions {
        system_prompt.push_str("\n\nRecipe instructions:\n");
        system_prompt.push_str(instructions);
    }
    if let Some(mcp_error) = ctx.mcp_error {
        system_prompt.push_str("\n\nCurrent MCP warning:\n");
        system_prompt.push_str(mcp_error);
        system_prompt.push_str("\nProceed carefully and explain when tool access is degraded.");
    }

    let mut messages = vec![LlmMessage::System(system_prompt)];

    // Check for active compaction
    let conversation = ctx.store.load(ctx.conversation_id).ok();
    if let Some(ref convo) = conversation
        && let Some(checkpoint_id) = &convo.active_compaction
        && let Ok(summary) = ctx
            .store
            .load_compaction(ctx.conversation_id, checkpoint_id)
    {
        messages.push(LlmMessage::System(format!(
            "Previous conversation summary:\n{summary}"
        )));
        // Only include the last 10 messages after compaction
        let tail: Vec<_> = ctx.history.iter().rev().take(10).collect();
        for msg in tail.into_iter().rev() {
            messages.push(stored_message_to_llm_message(msg));
        }
        if let Some(user_message) = ctx.user_message_override {
            messages.push(LlmMessage::UserText(user_message.to_string()));
        }
        return messages;
    }

    messages.extend(ctx.history.iter().map(stored_message_to_llm_message));
    if let Some(user_message) = ctx.user_message_override {
        messages.push(LlmMessage::UserText(user_message.to_string()));
    }
    messages
}

fn stored_message_to_llm_message(message: &crate::types::Message) -> LlmMessage {
    if message.role == "assistant" {
        LlmMessage::Assistant {
            blocks: vec![LlmAssistantBlock::Text {
                text: message.content.clone(),
            }],
        }
    } else {
        LlmMessage::UserText(message.content.clone())
    }
}

fn estimate_context_chars(messages: &[LlmMessage]) -> usize {
    messages.iter().map(llm_message_text_len).sum()
}

fn assistant_text_from_blocks(blocks: &[LlmAssistantBlock]) -> Option<String> {
    let text = blocks
        .iter()
        .filter_map(|block| match block {
            LlmAssistantBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() { None } else { Some(text) }
}

fn build_automation_prompt(job: &RememberedJob) -> String {
    format!(
        "Continue tracking remembered job '{alias}' with transaction_id '{transaction_id}'. Current status: {status}. Retrieval state: {retrieval_state}. Notes: {notes}. Automation prompt: {automation_prompt}",
        alias = job.alias,
        transaction_id = job.transaction_id,
        status = job.status.as_deref().unwrap_or("unknown"),
        retrieval_state = job.retrieval_state.as_deref().unwrap_or("unknown"),
        notes = job.notes.as_deref().unwrap_or(""),
        automation_prompt = job
            .automation_prompt
            .as_deref()
            .unwrap_or("Follow up on the remote job and update the record if useful.")
    )
}

fn fallback_compaction_summary(conversation: &crate::types::Conversation) -> String {
    let mut summary = format!(
        "Compaction fallback generated without LLM inference. Messages: {}.",
        conversation.messages.len()
    );
    if let Some(last_user) = conversation
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
    {
        summary.push_str("\nLast operator message: ");
        summary.push_str(last_user.content.trim());
    }
    if let Some(last_assistant) = conversation
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
    {
        summary.push_str("\nLast assistant reply: ");
        summary.push_str(last_assistant.content.trim());
    }
    summary
}

fn build_ranked_azure_tools_with_local(
    local_tools: &[LlmTool],
    mcp_tools: &[McpTool],
    tool_selection_query: &str,
) -> (Vec<LlmTool>, usize) {
    let total_tool_count = local_tools.len() + mcp_tools.len();
    if total_tool_count <= MAX_AZURE_TOOL_COUNT {
        let mut llm_tools = local_tools.to_vec();
        llm_tools.extend(mcp_tools.iter().map(mcp_tool_to_azure_tool));
        return (llm_tools, 0);
    }

    let context_tokens = tokenize_tool_selection_text(tool_selection_query);
    let mut llm_tools = Vec::with_capacity(MAX_AZURE_TOOL_COUNT);
    let mut selected_names = HashSet::new();

    for pinned_name in PINNED_LOCAL_TOOL_NAMES {
        if llm_tools.len() >= MAX_AZURE_TOOL_COUNT {
            break;
        }
        if let Some(tool) = local_tools.iter().find(|tool| tool.name == *pinned_name)
            && selected_names.insert(tool.name.clone())
        {
            llm_tools.push(tool.clone());
        }
    }

    let remaining_capacity = MAX_AZURE_TOOL_COUNT.saturating_sub(llm_tools.len());
    let mut ranked_candidates = local_tools
        .iter()
        .filter(|tool| !selected_names.contains(&tool.name))
        .map(|tool| RankedAzureTool {
            score: score_local_tool(tool, &context_tokens),
            local_preferred: true,
            tool: tool.clone(),
        })
        .chain(mcp_tools.iter().map(|tool| RankedAzureTool {
            score: score_mcp_tool(tool, &context_tokens),
            local_preferred: false,
            tool: mcp_tool_to_azure_tool(tool),
        }))
        .collect::<Vec<_>>();

    ranked_candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.local_preferred.cmp(&left.local_preferred))
            .then_with(|| left.tool.name.cmp(&right.tool.name))
    });

    llm_tools.extend(
        ranked_candidates
            .into_iter()
            .take(remaining_capacity)
            .map(|candidate| candidate.tool),
    );
    let omitted_tool_count = total_tool_count.saturating_sub(llm_tools.len());
    (llm_tools, omitted_tool_count)
}

#[cfg(test)]
fn build_azure_tools(mcp_tools: &[McpTool]) -> (Vec<LlmTool>, usize) {
    let azure_tools = mcp_tools
        .iter()
        .take(MAX_AZURE_TOOL_COUNT)
        .map(|tool| LlmTool {
            name: tool.external_name.clone(),
            description: format!("{} (server: {})", tool.description, tool.server_name),
            parameters: sanitize_azure_tool_schema(tool.input_schema.clone()),
        })
        .collect::<Vec<_>>();
    let omitted_tool_count = mcp_tools.len().saturating_sub(azure_tools.len());
    (azure_tools, omitted_tool_count)
}

#[derive(Debug, Clone)]
struct RankedAzureTool {
    score: i64,
    local_preferred: bool,
    tool: LlmTool,
}

fn build_tool_selection_query(
    user_message: Option<&str>,
    history: &[crate::types::Message],
    recipe: Option<&crate::recipes::Recipe>,
) -> String {
    let mut parts = Vec::new();

    if let Some(message) = user_message {
        parts.push(message.to_string());
    }

    for message in history.iter().rev().take(6) {
        if message.role == "user" || message.role == "assistant" {
            parts.push(message.content.clone());
        }
    }

    if let Some(recipe) = recipe {
        parts.push(recipe.name.clone());
        parts.push(recipe.prompt_guidance());
    }

    parts.join("\n")
}

fn advertised_mcp_tools<'a>(azure_tools: &[LlmTool], mcp_tools: &'a [McpTool]) -> Vec<&'a McpTool> {
    let advertised_names = azure_tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<HashSet<_>>();
    mcp_tools
        .iter()
        .filter(|tool| advertised_names.contains(tool.external_name.as_str()))
        .collect()
}

fn summarize_advertised_mcp_tools(
    advertised_mcp_tools: &[&McpTool],
    discovered_count: usize,
) -> String {
    if discovered_count == 0 {
        return String::new();
    }

    let mut grouped = HashMap::<&str, Vec<&McpTool>>::new();
    for tool in advertised_mcp_tools {
        grouped
            .entry(tool.server_name.as_str())
            .or_default()
            .push(*tool);
    }

    let mut servers = grouped.into_iter().collect::<Vec<_>>();
    servers.sort_by(|left, right| left.0.cmp(right.0));

    let mut out = String::from("## Advertised MCP Tools\n\n");
    out.push_str(
        "These are the MCP tools currently advertised for this turn. Use these names when choosing MCP actions.\n",
    );
    for (server_name, mut tools) in servers {
        tools.sort_by(|left, right| left.external_name.cmp(&right.external_name));
        let shown = tools
            .iter()
            .take(12)
            .map(|tool| format!("`{}`", tool.external_name))
            .collect::<Vec<_>>()
            .join(", ");
        let remaining = tools.len().saturating_sub(12);
        if remaining > 0 {
            out.push_str(&format!(
                "- `{server_name}` ({} advertised): {} + {} more\n",
                tools.len(),
                shown,
                remaining
            ));
        } else {
            out.push_str(&format!(
                "- `{server_name}` ({} advertised): {}\n",
                tools.len(),
                shown
            ));
        }
    }

    let omitted = discovered_count.saturating_sub(advertised_mcp_tools.len());
    if omitted > 0 {
        out.push_str(&format!(
            "\n{} additional discovered MCP tools were omitted from this turn because of the tool budget.\n",
            omitted
        ));
    }

    out
}

fn tokenize_tool_selection_text(text: &str) -> HashSet<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(|token| {
            let token = token.trim().to_ascii_lowercase();
            if token.len() >= 3 { Some(token) } else { None }
        })
        .collect()
}

fn score_local_tool(tool: &LlmTool, context_tokens: &HashSet<String>) -> i64 {
    let mut score = 0;
    if PINNED_LOCAL_TOOL_NAMES.contains(&tool.name.as_str()) {
        score += 10_000;
    }
    score
        + score_text_match(&tool.name, context_tokens, 40)
        + score_text_match(&tool.description, context_tokens, 10)
}

fn score_mcp_tool(tool: &McpTool, context_tokens: &HashSet<String>) -> i64 {
    score_text_match(&tool.external_name, context_tokens, 60)
        + score_text_match(&tool.original_name, context_tokens, 50)
        + score_text_match(&tool.server_name, context_tokens, 30)
        + score_text_match(&tool.description, context_tokens, 12)
}

fn score_text_match(text: &str, context_tokens: &HashSet<String>, weight: i64) -> i64 {
    tokenize_tool_selection_text(text)
        .into_iter()
        .filter(|token| context_tokens.contains(token))
        .count() as i64
        * weight
}

fn mcp_tool_to_azure_tool(tool: &McpTool) -> LlmTool {
    LlmTool {
        name: tool.external_name.clone(),
        description: format!("{} (server: {})", tool.description, tool.server_name),
        parameters: sanitize_azure_tool_schema(tool.input_schema.clone()),
    }
}

fn sanitize_azure_tool_schema(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(sanitize_azure_tool_schema)
                .collect::<Vec<_>>(),
        ),
        Value::Object(mut map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                if let Some(entry) = map.remove(&key) {
                    map.insert(key, sanitize_azure_tool_schema(entry));
                }
            }

            let is_object_schema = map.get("type").and_then(Value::as_str) == Some("object");
            let has_properties = map.get("properties").is_some();
            if is_object_schema && !has_properties {
                map.insert("properties".to_string(), json!({}));
            }

            Value::Object(map)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;
    use tokio::time::{Duration, timeout};

    use crate::{
        conversation_store::ConversationStore,
        llm::{LlmAssistantBlock, LlmMessage, LlmTool},
        mcp_runtime::McpTool,
        types::AgentPermissions,
    };

    use super::{
        ConversationTurnLocks, MAX_AZURE_TOOL_COUNT, MessageBuildContext, PINNED_LOCAL_TOOL_NAMES,
        build_azure_tools, build_messages, build_ranked_azure_tools_with_local,
        estimate_context_chars, sanitize_azure_tool_schema, summarize_advertised_mcp_tools,
    };

    #[test]
    fn estimates_context_chars_from_nested_messages() {
        let messages = vec![
            LlmMessage::System("hello".to_string()),
            LlmMessage::Assistant {
                blocks: vec![LlmAssistantBlock::ToolUse {
                    id: "1".to_string(),
                    name: "demo".to_string(),
                    input: json!({"x":1}),
                }],
            },
        ];

        let chars = estimate_context_chars(&messages);

        assert!(chars >= "hello1demox1".chars().count());
    }

    #[test]
    fn caps_advertised_tools_to_azure_limit() {
        let mcp_tools = (0..(MAX_AZURE_TOOL_COUNT + 5))
            .map(|index| McpTool {
                server_name: "demo".to_string(),
                original_name: format!("tool_{index}"),
                external_name: format!("demo__tool_{index}"),
                description: format!("tool {index}"),
                input_schema: json!({"type": "object"}),
            })
            .collect::<Vec<_>>();

        let (azure_tools, omitted_tool_count) = build_azure_tools(&mcp_tools);

        assert_eq!(azure_tools.len(), MAX_AZURE_TOOL_COUNT);
        assert_eq!(omitted_tool_count, 5);
        assert_eq!(azure_tools[0].name, "demo__tool_0");
        assert_eq!(
            azure_tools.last().map(|tool| tool.name.as_str()),
            Some("demo__tool_127")
        );
    }

    #[test]
    fn caps_combined_local_and_mcp_tools_to_azure_limit() {
        let local_tools = (0..MAX_AZURE_TOOL_COUNT)
            .map(|index| LlmTool {
                name: format!("local_{index}"),
                description: format!("local tool {index}"),
                parameters: json!({"type": "object"}),
            })
            .collect::<Vec<_>>();
        let mcp_tools = (0..5)
            .map(|index| McpTool {
                server_name: "demo".to_string(),
                original_name: format!("tool_{index}"),
                external_name: format!("demo__tool_{index}"),
                description: format!("tool {index}"),
                input_schema: json!({"type": "object"}),
            })
            .collect::<Vec<_>>();

        let (azure_tools, omitted_tool_count) =
            build_ranked_azure_tools_with_local(&local_tools, &mcp_tools, "");

        assert_eq!(azure_tools.len(), MAX_AZURE_TOOL_COUNT);
        assert_eq!(omitted_tool_count, 5);
        assert!(
            azure_tools
                .iter()
                .all(|tool| tool.name.starts_with("local_"))
        );
    }

    #[test]
    fn ranks_relevant_mcp_tools_ahead_of_irrelevant_filler() {
        let local_tools = vec![LlmTool {
            name: "local__run_skill".to_string(),
            description: "Execute a skill script".to_string(),
            parameters: json!({"type": "object"}),
        }];
        let mcp_tools = (0..200)
            .map(|index| {
                let (server_name, original_name, description) = if index == 199 {
                    (
                        "telemetry".to_string(),
                        "query_alerts".to_string(),
                        "Query telemetry alerts and detections".to_string(),
                    )
                } else {
                    (
                        "demo".to_string(),
                        format!("tool_{index}"),
                        format!("generic tool {index}"),
                    )
                };
                McpTool {
                    server_name: server_name.clone(),
                    original_name: original_name.clone(),
                    external_name: format!("{server_name}__{original_name}"),
                    description,
                    input_schema: json!({"type": "object"}),
                }
            })
            .collect::<Vec<_>>();

        let (azure_tools, omitted_tool_count) = build_ranked_azure_tools_with_local(
            &local_tools,
            &mcp_tools,
            "query telemetry alerts for failed authentication",
        );

        assert_eq!(azure_tools.len(), MAX_AZURE_TOOL_COUNT);
        assert_eq!(omitted_tool_count, 73);
        assert!(
            azure_tools
                .iter()
                .any(|tool| tool.name == "telemetry__query_alerts")
        );
    }

    #[test]
    fn preserves_pinned_local_tools_when_tool_budget_is_tight() {
        let local_tools = PINNED_LOCAL_TOOL_NAMES
            .iter()
            .map(|name| LlmTool {
                name: (*name).to_string(),
                description: format!("{name} description"),
                parameters: json!({"type": "object"}),
            })
            .chain((0..200).map(|index| LlmTool {
                name: format!("local__extra_{index}"),
                description: format!("extra local tool {index}"),
                parameters: json!({"type": "object"}),
            }))
            .collect::<Vec<_>>();

        let (azure_tools, omitted_tool_count) =
            build_ranked_azure_tools_with_local(&local_tools, &[], "unrelated prompt");

        assert_eq!(azure_tools.len(), MAX_AZURE_TOOL_COUNT);
        assert_eq!(omitted_tool_count, local_tools.len() - MAX_AZURE_TOOL_COUNT);
        for name in PINNED_LOCAL_TOOL_NAMES {
            assert!(azure_tools.iter().any(|tool| tool.name == *name));
        }
    }

    #[test]
    fn fills_missing_object_properties_for_azure_tools() {
        let schema = json!({
            "type": "object",
            "properties": {
                "nested": {
                    "type": "object"
                }
            }
        });

        let sanitized = sanitize_azure_tool_schema(schema);

        assert_eq!(sanitized["properties"]["nested"]["properties"], json!({}));
    }

    #[test]
    fn build_messages_explains_local_skill_execution_rules() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path(), AgentPermissions::default());
        let permissions = AgentPermissions::default();

        let messages = build_messages(MessageBuildContext {
            prompt: None,
            history: &[],
            mcp_error: None,
            recipe_instructions: None,
            capability_summary: Some("## Available Skills\n"),
            mcp_capability_summary: Some(
                "## Advertised MCP Tools\n\n- `csirt-mcp` (2 advertised): `csirt_mcp__foo`, `csirt_mcp__bar`\n",
            ),
            agent_permissions: &permissions,
            store: &store,
            conversation_id: "convo_test",
            user_message_override: None,
        });

        let LlmMessage::System(system_prompt) = &messages[0] else {
            panic!("first message should be system");
        };

        assert!(system_prompt.contains("local__run_skill"));
        assert!(
            system_prompt.contains("Never say a listed local skill is unavailable because of MCP")
        );
        assert!(system_prompt.contains("## Advertised MCP Tools"));
        assert!(system_prompt.contains("`csirt-mcp` (2 advertised)"));
        assert!(system_prompt.contains("Active agent permissions"));
        assert!(system_prompt.contains("Recipes provide prompt guidance and configuration"));
    }

    #[test]
    fn summarizes_advertised_mcp_tools_by_server() {
        let tools = vec![
            McpTool {
                server_name: "csirt-mcp".to_string(),
                original_name: "search_events".to_string(),
                external_name: "csirt_mcp__search_events".to_string(),
                description: "Search events".to_string(),
                input_schema: json!({"type": "object"}),
            },
            McpTool {
                server_name: "csirt-mcp".to_string(),
                original_name: "list_cases".to_string(),
                external_name: "csirt_mcp__list_cases".to_string(),
                description: "List cases".to_string(),
                input_schema: json!({"type": "object"}),
            },
            McpTool {
                server_name: "wiz".to_string(),
                original_name: "issues_query".to_string(),
                external_name: "wiz__issues_query".to_string(),
                description: "Query issues".to_string(),
                input_schema: json!({"type": "object"}),
            },
        ];

        let summary =
            summarize_advertised_mcp_tools(&[&tools[0], &tools[1], &tools[2]], tools.len());

        assert!(summary.contains("## Advertised MCP Tools"));
        assert!(summary.contains(
            "`csirt-mcp` (2 advertised): `csirt_mcp__list_cases`, `csirt_mcp__search_events`"
        ));
        assert!(summary.contains("`wiz` (1 advertised): `wiz__issues_query`"));
    }

    #[tokio::test]
    async fn serializes_access_for_same_conversation_id() {
        let locks = ConversationTurnLocks::default();
        let guard = locks.acquire("same-convo").await;

        assert!(
            timeout(Duration::from_millis(20), locks.acquire("same-convo"))
                .await
                .is_err()
        );

        drop(guard);

        assert!(
            timeout(Duration::from_millis(20), locks.acquire("same-convo"))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn allows_parallel_access_for_different_conversation_ids() {
        let locks = ConversationTurnLocks::default();
        let _guard = locks.acquire("convo-a").await;

        assert!(
            timeout(Duration::from_millis(20), locks.acquire("convo-b"))
                .await
                .is_ok()
        );
    }
}
