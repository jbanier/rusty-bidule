use std::time::Duration;

use anyhow::Result;
use chrono::{Local, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, Event as MdEvent, HeadingLevel, Options, Parser, Tag, TagEnd,
};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Wrap},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::{debug, error, info, warn};

use crate::{
    orchestrator::Orchestrator,
    paths::discover_project_root,
    prompt_expansion::expand_prompt_file_references,
    types::{AgentPermissions, Conversation, FilesystemAccess, Message, UiEvent},
};

const SPINNER: &[&str] = &["|", "/", "-", "\\"];
fn void_black() -> Color {
    Color::Rgb(7, 11, 10)
}

fn panel_ink() -> Color {
    Color::Rgb(15, 23, 21)
}

fn input_ink() -> Color {
    Color::Rgb(11, 17, 16)
}

fn neon_pink() -> Color {
    Color::Rgb(242, 184, 88)
}

fn neon_cyan() -> Color {
    Color::Rgb(123, 169, 157)
}

fn neon_gold() -> Color {
    Color::Rgb(246, 205, 110)
}

fn neon_orange() -> Color {
    Color::Rgb(208, 146, 99)
}

fn neon_lime() -> Color {
    Color::Rgb(146, 191, 122)
}

fn signal_red() -> Color {
    Color::Rgb(212, 121, 108)
}

fn synth_text() -> Color {
    Color::Rgb(231, 224, 202)
}

fn muted_synth() -> Color {
    Color::Rgb(126, 139, 121)
}

fn rule_line(label: Option<&str>) -> Line<'static> {
    let mut spans = vec![Span::styled(
        "─".repeat(18),
        Style::default().fg(neon_cyan()),
    )];
    if let Some(label) = label {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            label.to_string(),
            Style::default()
                .fg(neon_gold())
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            "─".repeat(18),
            Style::default().fg(neon_cyan()),
        ));
    }
    Line::from(spans)
}

fn activity_style(entry: &str) -> Style {
    let lower = entry.to_ascii_lowercase();
    if entry.contains("ERROR") || lower.contains("failed") {
        Style::default()
            .fg(signal_red())
            .add_modifier(Modifier::BOLD)
    } else if entry.starts_with("Usage:")
        || entry.starts_with("Commands:")
        || entry.starts_with("Conversations:")
        || entry.starts_with("Recipes:")
        || entry.starts_with("MCP:")
        || entry.starts_with("Session:")
        || entry.starts_with("Permissions:")
        || entry.starts_with("Agent permissions:")
        || entry.starts_with("Input:")
        || entry.starts_with("Navigation:")
    {
        Style::default().fg(neon_cyan())
    } else if lower.contains("disabled") {
        Style::default().fg(signal_red())
    } else if lower.contains("completed")
        || lower.contains("ready")
        || lower.contains("activated")
        || lower.contains("enabled")
        || lower.contains("created")
        || lower.contains("using ")
        || lower.contains("compacted")
    {
        Style::default().fg(neon_lime())
    } else if lower.contains("warning") || lower.contains("busy") || lower.contains("limit") {
        Style::default().fg(neon_gold())
    } else {
        Style::default().fg(synth_text())
    }
}

pub struct App {
    orchestrator: Orchestrator,
    current_conversation_id: String,
    configured_mcp_servers: Vec<String>,
    enabled_mcp_servers: Option<Vec<String>>,
    agent_permissions: AgentPermissions,
    messages: Vec<Message>,
    command_output: Vec<Message>,
    message_scroll: u16,
    rendered_message_lines: u16,
    message_viewport_lines: u16,
    activities: Vec<String>,
    input: String,
    input_history: Vec<String>,
    input_history_cursor: Option<usize>,
    input_history_draft: Option<String>,
    multiline_buffer: Option<Vec<String>>,
    inflight: bool,
    spinner_index: usize,
    status: String,
    persistent_warning: Option<String>,
    ui_tx: UnboundedSender<UiEvent>,
    ui_rx: UnboundedReceiver<UiEvent>,
    should_quit: bool,
}

struct TerminalRestoreGuard;

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

impl App {
    pub async fn new(orchestrator: Orchestrator) -> Result<Self> {
        let current_conversation_id = orchestrator.ensure_default_conversation().await?;
        let conversation = orchestrator.store().load(&current_conversation_id)?;
        let configured_mcp_servers = orchestrator.configured_mcp_server_names();
        let messages = conversation.messages;
        let enabled_mcp_servers = conversation.enabled_mcp_servers;
        let agent_permissions = conversation.agent_permissions;
        let (ui_tx, ui_rx) = unbounded_channel();
        info!(
            %current_conversation_id,
            message_count = messages.len(),
            "initialized TUI application state"
        );

        Ok(Self {
            orchestrator,
            current_conversation_id,
            configured_mcp_servers,
            enabled_mcp_servers,
            agent_permissions,
            messages,
            command_output: Vec::new(),
            message_scroll: 0,
            rendered_message_lines: 0,
            message_viewport_lines: 0,
            activities: Vec::new(),
            input: String::new(),
            input_history: Vec::new(),
            input_history_cursor: None,
            input_history_draft: None,
            multiline_buffer: None,
            inflight: false,
            spinner_index: 0,
            status: "Session idle".to_string(),
            persistent_warning: None,
            ui_tx,
            ui_rx,
            should_quit: false,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        info!("entering TUI alternate screen");
        let _terminal_guard = TerminalRestoreGuard;
        let terminal = ratatui::init();

        let result = self.run_loop(terminal).await;

        info!("restored terminal and exited TUI");
        result
    }

    async fn run_loop(&mut self, mut terminal: DefaultTerminal) -> Result<()> {
        while !self.should_quit {
            while let Ok(event) = self.ui_rx.try_recv() {
                self.handle_ui_event(event)?;
            }

            terminal.draw(|frame| self.render(frame))?;

            if event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.handle_key_event(key).await?;
            }

            if self.inflight {
                self.spinner_index = (self.spinner_index + 1) % SPINNER.len();
            }
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        frame.render_widget(
            Block::default().style(Style::default().bg(void_black())),
            frame.area(),
        );

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(frame.area());

        frame.render_widget(self.render_header(), layout[0]);
        frame.render_widget(self.render_separator(Some("TRANSCRIPT")), layout[1]);

        let transcript_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(layout[2]);

        let transcript =
            self.render_transcript(transcript_layout[0].width, transcript_layout[0].height);
        frame.render_widget(transcript, transcript_layout[0]);

        let indicator = render_scroll_indicator(
            self.rendered_message_lines,
            self.message_viewport_lines,
            self.message_scroll,
        );
        frame.render_widget(indicator, transcript_layout[1]);

        frame.render_widget(self.render_activity_line(), layout[3]);
        frame.render_widget(self.render_input_line(), layout[4]);
        frame.render_widget(self.render_footer(), layout[5]);
    }

    fn render_header(&self) -> Paragraph<'static> {
        let help_line = Line::from(vec![
            Span::styled(
                "Menu",
                Style::default()
                    .fg(neon_pink())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                "Enter send",
                Style::default()
                    .fg(neon_gold())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                "/help",
                Style::default()
                    .fg(neon_pink())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" commands  "),
            Span::styled(
                "PgUp/PgDn",
                Style::default()
                    .fg(neon_pink())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" scroll  "),
            Span::styled(
                "Ctrl+Up/Down",
                Style::default()
                    .fg(neon_pink())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" history  "),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(neon_pink())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" clear"),
        ]);

        let tip_line = Line::from(vec![
            Span::styled(
                "Tips",
                Style::default()
                    .fg(neon_pink())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                "<<< >>>",
                Style::default()
                    .fg(neon_orange())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" multiline  "),
            Span::styled(
                "/new",
                Style::default()
                    .fg(neon_pink())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" session  "),
            Span::styled(
                "/mcp",
                Style::default()
                    .fg(neon_pink())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" servers  "),
            Span::styled(
                "/permissions",
                Style::default()
                    .fg(neon_pink())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" access"),
        ]);

        Paragraph::new(Text::from(vec![help_line, tip_line]))
            .style(Style::default().fg(synth_text()).bg(void_black()))
            .wrap(Wrap { trim: false })
    }

    fn render_separator(&self, label: Option<&str>) -> Paragraph<'static> {
        Paragraph::new(Text::from(vec![rule_line(label)])).style(Style::default().bg(void_black()))
    }

    fn render_activity_line(&self) -> Paragraph<'static> {
        let line = if let Some(warning) = &self.persistent_warning {
            Line::from(vec![
                Span::styled("Warning ==> ", Style::default().fg(neon_gold())),
                Span::styled(
                    warning.clone(),
                    Style::default()
                        .fg(neon_gold())
                        .add_modifier(Modifier::BOLD),
                ),
            ])
        } else if let Some(entry) = self.activities.last() {
            Line::from(vec![
                Span::styled("Feedback ==> ", Style::default().fg(neon_cyan())),
                Span::styled(entry.clone(), activity_style(entry)),
            ])
        } else {
            Line::from(vec![
                Span::styled("Feedback ==> ", Style::default().fg(neon_cyan())),
                Span::styled("ready", Style::default().fg(muted_synth())),
            ])
        };

        Paragraph::new(Text::from(vec![line]))
            .style(Style::default().fg(synth_text()).bg(void_black()))
    }

    fn render_input_line(&self) -> Paragraph<'static> {
        let preview = self.input_preview();
        let input = if preview.is_empty() {
            "Type a prompt or run /help.".to_string()
        } else {
            preview.replace('\n', " ")
        };

        Paragraph::new(Text::from(vec![Line::from(vec![
            Span::styled(
                "Input ===> ",
                Style::default()
                    .fg(neon_orange())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                input,
                Style::default().fg(if preview.is_empty() {
                    muted_synth()
                } else {
                    synth_text()
                }),
            ),
        ])]))
        .style(Style::default().bg(input_ink()))
    }

    fn render_footer(&self) -> Paragraph<'static> {
        let mode = if self.inflight { "RUNNING" } else { "READY" };
        let input_mode = if self.multiline_buffer.is_some() {
            "MULTILINE"
        } else if self.input.trim().is_empty() {
            "CLEAR"
        } else {
            "DRAFT"
        };
        let tool_calls = self
            .messages
            .iter()
            .rev()
            .find_map(|message| {
                message
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.tool_call_count)
            })
            .unwrap_or(0);

        let network = if self.agent_permissions.allows_network() {
            "net:on"
        } else {
            "net:off"
        };
        let filesystem = if self.agent_permissions.yolo {
            "fs:all".to_string()
        } else {
            format!("fs:{}", self.agent_permissions.filesystem.label())
        };
        let yolo = if self.agent_permissions.yolo {
            "yolo:on"
        } else {
            "yolo:off"
        };
        let mcp = build_mcp_server_statuses(
            &self.configured_mcp_servers,
            self.enabled_mcp_servers.as_deref(),
            None,
        )
        .into_iter()
        .filter(|status| status.enabled)
        .count();
        let multiline = self
            .multiline_buffer
            .as_ref()
            .map(|buffer| format!("ml:{:>2}", buffer.len()))
            .unwrap_or_else(|| "ml:--".to_string());

        let line = Line::from(vec![
            Span::styled(
                "(:) ",
                Style::default()
                    .fg(neon_pink())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                self.status_line(),
                Style::default()
                    .fg(neon_gold())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(format!("state:{mode}"), Style::default().fg(neon_lime())),
            Span::raw("  "),
            Span::styled(
                format!("input:{input_mode}"),
                Style::default().fg(neon_orange()),
            ),
            Span::raw("  "),
            Span::styled(
                format!("msg:{}", self.messages.len()),
                Style::default().fg(synth_text()),
            ),
            Span::raw("  "),
            Span::styled(
                format!("tools:{tool_calls}"),
                Style::default().fg(synth_text()),
            ),
            Span::raw("  "),
            Span::styled(multiline, Style::default().fg(synth_text())),
            Span::raw("  "),
            Span::styled(format!("mcp:{mcp}"), Style::default().fg(neon_cyan())),
            Span::raw("  "),
            Span::styled(
                network,
                Style::default().fg(if self.agent_permissions.allows_network() {
                    neon_lime()
                } else {
                    signal_red()
                }),
            ),
            Span::raw("  "),
            Span::styled(filesystem, Style::default().fg(neon_cyan())),
            Span::raw("  "),
            Span::styled(
                yolo,
                Style::default().fg(if self.agent_permissions.yolo {
                    neon_pink()
                } else {
                    muted_synth()
                }),
            ),
            Span::raw("  "),
            Span::styled(
                format!("id:{}", self.current_conversation_id),
                Style::default().fg(muted_synth()),
            ),
        ]);

        Paragraph::new(Text::from(vec![line]))
            .style(Style::default().fg(synth_text()).bg(panel_ink()))
    }

    fn render_transcript(&mut self, area_width: u16, area_height: u16) -> Paragraph<'static> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        let transcript_messages = ordered_transcript_messages(&self.messages, &self.command_output);

        if transcript_messages.is_empty() {
            lines.push(Line::from(Span::styled(
                "No messages yet. Use the input pane below to start a session.",
                Style::default().fg(muted_synth()),
            )));
        } else {
            lines.extend(transcript_messages.into_iter().flat_map(|message| {
                let (role_label, role_style) = match message.role.as_str() {
                    "user" => (
                        "USER",
                        Style::default()
                            .fg(neon_cyan())
                            .add_modifier(Modifier::BOLD),
                    ),
                    "assistant" => (
                        "ASSISTANT",
                        Style::default()
                            .fg(neon_pink())
                            .add_modifier(Modifier::BOLD),
                    ),
                    "command" => (
                        "COMMAND",
                        Style::default()
                            .fg(neon_orange())
                            .add_modifier(Modifier::BOLD),
                    ),
                    _ => (
                        "SYSTEM",
                        Style::default()
                            .fg(neon_gold())
                            .add_modifier(Modifier::BOLD),
                    ),
                };
                let timestamp = message
                    .timestamp
                    .with_timezone(&Local)
                    .format("%H:%M:%S")
                    .to_string();
                let mut header = vec![
                    Span::styled(role_label.to_string(), role_style),
                    Span::raw("  "),
                    Span::styled(timestamp, Style::default().fg(muted_synth())),
                ];
                if let Some(metadata) = &message.metadata {
                    header.push(Span::raw("  "));
                    header.push(Span::styled(
                        format!(
                            "#{}  {}t  {:.1}s",
                            metadata.assistant_index,
                            metadata.tool_call_count,
                            metadata.timing.total_seconds
                        ),
                        Style::default().fg(neon_gold()),
                    ));
                }
                let mut message_lines = vec![Line::from(header)];
                message_lines.extend(render_markdown(&message.content));
                message_lines.push(Line::raw(""));
                message_lines
            }));
        }

        self.rendered_message_lines =
            count_wrapped_rows(&lines, area_width).min(u16::MAX as usize) as u16;
        self.message_viewport_lines = area_height;
        self.message_scroll = self.message_scroll.min(max_message_scroll(
            self.rendered_message_lines,
            self.message_viewport_lines,
        ));

        Paragraph::new(Text::from(lines))
            .scroll((self.message_scroll, 0))
            .style(Style::default().fg(synth_text()).bg(void_black()))
            .wrap(Wrap { trim: false })
    }

    async fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return Ok(());
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Up {
            self.navigate_input_history(true);
            return Ok(());
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Down {
            self.navigate_input_history(false);
            return Ok(());
        }

        match key.code {
            KeyCode::Up => self.scroll_messages_by(-1),
            KeyCode::Down => self.scroll_messages_by(1),
            KeyCode::PageUp => self.scroll_messages_by(-(self.page_scroll_amount() as i32)),
            KeyCode::PageDown => self.scroll_messages_by(self.page_scroll_amount() as i32),
            KeyCode::Home => self.message_scroll = 0,
            KeyCode::End => self.scroll_messages_to_latest(),
            KeyCode::Char(ch) => {
                self.detach_input_history_navigation();
                self.input.push(ch);
            }
            KeyCode::Backspace => {
                self.detach_input_history_navigation();
                self.input.pop();
            }
            KeyCode::Enter => {
                let submitted = std::mem::take(&mut self.input);
                self.input_history_cursor = None;
                self.input_history_draft = None;
                self.handle_submission(submitted).await?;
            }
            KeyCode::Esc => {
                self.input.clear();
                self.input_history_cursor = None;
                self.input_history_draft = None;
                self.multiline_buffer = None;
                self.status = "Input cleared".to_string();
            }
            _ => {}
        }

        Ok(())
    }

    async fn handle_submission(&mut self, submitted: String) -> Result<()> {
        let submitted = submitted.trim_end_matches(['\r', '\n']);

        if self.inflight {
            warn!("ignored submission while engine was busy");
            self.activities
                .push("Engine busy. Wait for the current run to finish.".to_string());
            return Ok(());
        }

        if let Some(buffer) = &mut self.multiline_buffer {
            if submitted == ">>>" {
                let payload = buffer.join("\n");
                self.multiline_buffer = None;
                self.dispatch_message(payload).await?;
            } else {
                buffer.push(submitted.to_string());
            }
            return Ok(());
        }

        let trimmed = submitted.trim_end().to_string();
        if trimmed.is_empty() {
            return Ok(());
        }

        self.remember_input_history(&trimmed);

        if trimmed == "<<<" {
            self.multiline_buffer = Some(Vec::new());
            self.status = "Multiline capture armed".to_string();
            return Ok(());
        }

        if trimmed.starts_with('/') {
            self.handle_command(&trimmed).await?;
        } else {
            self.dispatch_message(trimmed).await?;
        }
        Ok(())
    }

    fn remember_input_history(&mut self, entry: &str) {
        if self.input_history.last().is_some_and(|last| last == entry) {
            return;
        }
        self.input_history.push(entry.to_string());
        const MAX_INPUT_HISTORY: usize = 200;
        if self.input_history.len() > MAX_INPUT_HISTORY {
            let excess = self.input_history.len() - MAX_INPUT_HISTORY;
            self.input_history.drain(0..excess);
        }
    }

    fn detach_input_history_navigation(&mut self) {
        if self.input_history_cursor.is_some() {
            self.input_history_cursor = None;
            self.input_history_draft = None;
        }
    }

    fn navigate_input_history(&mut self, older: bool) {
        if self.input_history.is_empty() {
            return;
        }

        match (older, self.input_history_cursor) {
            (true, None) => {
                self.input_history_draft = Some(self.input.clone());
                self.input_history_cursor = Some(self.input_history.len().saturating_sub(1));
            }
            (true, Some(cursor)) => {
                self.input_history_cursor = Some(cursor.saturating_sub(1));
            }
            (false, None) => return,
            (false, Some(cursor)) if cursor + 1 < self.input_history.len() => {
                self.input_history_cursor = Some(cursor + 1);
            }
            (false, Some(_)) => {
                self.input = self.input_history_draft.take().unwrap_or_default();
                self.input_history_cursor = None;
                return;
            }
        }

        if let Some(cursor) = self.input_history_cursor {
            self.input = self.input_history[cursor].clone();
        }
    }

    async fn handle_command(&mut self, command: &str) -> Result<()> {
        debug!(command, "handling TUI command");
        let mut parts = command.split_whitespace();
        match parts.next().unwrap_or_default() {
            "/new" => {
                let conversation = self.orchestrator.store().create_conversation()?;
                let conversation_id = conversation.conversation_id.clone();
                self.apply_conversation(conversation);
                self.activities.push(format!("Created {conversation_id}"));
            }
            "/list" => {
                let list_mode = parts.next();
                let include_archived = matches!(list_mode, Some("all" | "archived"));
                let conversations = self
                    .orchestrator
                    .store()
                    .list_conversations_with_archived(include_archived)?;
                let archived_only = matches!(list_mode, Some("archived"));
                let conversations = conversations
                    .into_iter()
                    .filter(|summary| !archived_only || summary.archived_at.is_some())
                    .collect::<Vec<_>>();
                if conversations.is_empty() {
                    self.push_command_output(
                        "/list",
                        if archived_only {
                            "No archived conversations found."
                        } else {
                            "No conversations found."
                        },
                    );
                    self.activities
                        .push("Conversation list opened in transcript.".to_string());
                } else {
                    let body = conversations
                        .iter()
                        .take(12)
                        .map(|summary| {
                            format!(
                                "- `{}`: {} messages{} ({})",
                                summary.title.as_deref().unwrap_or(&summary.conversation_id),
                                summary.message_count,
                                if summary.archived_at.is_some() {
                                    " [archived]"
                                } else {
                                    ""
                                },
                                summary.conversation_id,
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.push_command_output("/list", body);
                    self.activities
                        .push("Conversation list opened in transcript.".to_string());
                }
            }
            "/use" => {
                if let Some(target) = parts.next() {
                    self.switch_conversation(target)?;
                } else {
                    self.activities
                        .push("Usage: /use <conversation-id>".to_string());
                }
            }
            "/show" => {
                let target = parts
                    .next()
                    .unwrap_or(&self.current_conversation_id)
                    .to_string();
                self.switch_conversation(&target)?;
            }
            "/title" => {
                let title = command.strip_prefix("/title").unwrap_or("").trim();
                let conversation = self.orchestrator.store().set_conversation_title(
                    &self.current_conversation_id,
                    if title.is_empty() { None } else { Some(title) },
                )?;
                let label = conversation
                    .title
                    .as_deref()
                    .unwrap_or("untitled conversation")
                    .to_string();
                self.apply_conversation(conversation);
                self.activities
                    .push(format!("Conversation title set: {label}"));
            }
            "/delete" => {
                if let Some(target) = parts.next() {
                    self.orchestrator.store().delete(target)?;
                    self.activities.push(format!("Deleted {target}"));
                    if target == self.current_conversation_id {
                        let conversation_id =
                            self.orchestrator.ensure_default_conversation().await?;
                        let conversation = self.orchestrator.store().load(&conversation_id)?;
                        self.apply_conversation(conversation);
                    }
                } else {
                    self.activities
                        .push("Usage: /delete <conversation-id>".to_string());
                }
            }
            "/archive" => {
                let target = parts
                    .next()
                    .unwrap_or(&self.current_conversation_id)
                    .to_string();
                self.orchestrator.store().archive_conversation(&target)?;
                self.activities.push(format!("Archived {target}"));
                if target == self.current_conversation_id {
                    let conversation_id = self.orchestrator.ensure_default_conversation().await?;
                    let conversation = self.orchestrator.store().load(&conversation_id)?;
                    self.apply_conversation(conversation);
                }
            }
            "/unarchive" => {
                if let Some(target) = parts.next() {
                    self.orchestrator.store().unarchive_conversation(target)?;
                    self.activities.push(format!("Unarchived {target}"));
                } else {
                    self.activities
                        .push("Usage: /unarchive <conversation-id>".to_string());
                }
            }
            "/export" => {
                let target = parts
                    .next()
                    .unwrap_or(&self.current_conversation_id)
                    .to_string();
                let export_path = self
                    .orchestrator
                    .store()
                    .export_conversation_summary(&target)?;
                self.push_command_output(
                    "/export",
                    format!("Session summary exported to `{}`", export_path.display()),
                );
                self.activities
                    .push(format!("Session summary exported for {target}"));
            }
            "/help" => {
                self.push_command_output("/help", build_help_markdown());
                self.activities
                    .push("Command help opened in transcript.".to_string());
            }
            "/history" => {
                let body = self
                    .messages
                    .iter()
                    .rev()
                    .take(20)
                    .rev()
                    .map(|message| format!("- `{}`: {}", message.role, message.content))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.push_command_output(
                    "/history",
                    if body.is_empty() {
                        "No conversation history yet.".to_string()
                    } else {
                        body
                    },
                );
                self.activities
                    .push("Conversation history opened in transcript.".to_string());
            }
            "/login" => {
                if let Some(server_name) = parts.next() {
                    self.status = format!("Logging into {server_name}");
                    self.activities
                        .push(format!("Starting OAuth login for {server_name}"));
                    info!(server = server_name, "starting MCP login from TUI");
                    match self.orchestrator.login_mcp_server(server_name).await {
                        Ok(()) => {
                            self.status = format!("Logged into {server_name}");
                            self.activities
                                .push(format!("OAuth login completed for {server_name}"));
                            info!(server = server_name, "completed MCP login from TUI");
                        }
                        Err(err) => {
                            self.status = "Login failed".to_string();
                            self.activities
                                .push(format!("OAuth login failed for {server_name}: {err}"));
                            warn!(server = server_name, error = %err, "MCP login failed from TUI");
                        }
                    }
                } else {
                    self.activities
                        .push("Usage: /login <mcp-server-name>".to_string());
                }
            }
            "/logging" => {
                let provider = self.orchestrator.config().tracing.map(|cfg| cfg.provider);
                self.push_command_output(
                    "/logging",
                    format!(
                        "Tracing provider: `{:?}`\nApplication log: `var/bidule.log`\nConversation audit log: `data/conversations/<id>/logs/conversation.log`",
                        provider.unwrap_or_default()
                    ),
                );
                self.activities
                    .push("Logging configuration opened in transcript.".to_string());
            }
            "/jobs" => {
                let jobs = self
                    .orchestrator
                    .store()
                    .load_job_state(&self.current_conversation_id)?;
                let body = if jobs.is_empty() {
                    "No remembered jobs for this conversation.".to_string()
                } else {
                    jobs.iter()
                        .map(|job| {
                            format!(
                                "- `{}`: tx=`{}` status=`{}` mode=`{}` next_poll_at=`{}`",
                                job.alias,
                                job.transaction_id,
                                job.status.as_deref().unwrap_or("unknown"),
                                job.mode.as_deref().unwrap_or("manual"),
                                job.next_poll_at
                                    .map(|value| value.to_rfc3339())
                                    .unwrap_or_else(|| "-".to_string())
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                self.push_command_output("/jobs", body);
                self.activities
                    .push("Remembered jobs opened in transcript.".to_string());
            }
            "/scratch" => {
                let sub = parts.next().unwrap_or_default();
                match sub {
                    "" | "show" => {
                        let body = self
                            .orchestrator
                            .store()
                            .load_scratchpad(&self.current_conversation_id)?;
                        self.push_command_output(
                            "/scratch",
                            if body.trim().is_empty() {
                                "Scratchpad is empty.".to_string()
                            } else {
                                body
                            },
                        );
                        self.activities
                            .push("Scratchpad opened in transcript.".to_string());
                    }
                    "set" => {
                        let body = command
                            .strip_prefix("/scratch set")
                            .unwrap_or("")
                            .trim_start();
                        self.orchestrator
                            .store()
                            .save_scratchpad(&self.current_conversation_id, body)?;
                        self.activities.push("Scratchpad updated.".to_string());
                    }
                    "append" => {
                        let suffix = command
                            .strip_prefix("/scratch append")
                            .unwrap_or("")
                            .trim_start();
                        let existing = self
                            .orchestrator
                            .store()
                            .load_scratchpad(&self.current_conversation_id)?;
                        let next = if existing.trim().is_empty() {
                            suffix.to_string()
                        } else if suffix.is_empty() {
                            existing
                        } else {
                            format!("{existing}\n{suffix}")
                        };
                        self.orchestrator
                            .store()
                            .save_scratchpad(&self.current_conversation_id, &next)?;
                        self.activities.push("Scratchpad appended.".to_string());
                    }
                    "clear" => {
                        self.orchestrator
                            .store()
                            .save_scratchpad(&self.current_conversation_id, "")?;
                        self.activities.push("Scratchpad cleared.".to_string());
                    }
                    _ => {
                        self.activities.push(
                            "Usage: /scratch [show] | /scratch set <text> | /scratch append <text> | /scratch clear"
                                .to_string(),
                        );
                    }
                }
            }
            "/findings" | "/finding" => {
                let sub = parts.next().unwrap_or_default();
                match sub {
                    "" | "list" => {
                        let findings = self.orchestrator.store().load_findings()?;
                        let body = findings
                            .iter()
                            .filter(|finding| {
                                finding.conversation_id == self.current_conversation_id
                            })
                            .map(|finding| {
                                let mut suffix = finding
                                    .note
                                    .as_deref()
                                    .map(|note| format!(" // {note}"))
                                    .unwrap_or_default();
                                if !finding.tags.is_empty() {
                                    suffix
                                        .push_str(&format!(" // tags: {}", finding.tags.join(",")));
                                }
                                if let Some(confidence) = finding.confidence {
                                    suffix.push_str(&format!(" // confidence: {confidence}"));
                                }
                                if let Some(path) = finding.source_artifact.as_deref() {
                                    suffix.push_str(&format!(" // artifact: {path}"));
                                }
                                format!(
                                    "- `{}` [{}] `{}`{}",
                                    finding.finding_id, finding.kind, finding.value, suffix
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        self.push_command_output(
                            "/findings",
                            if body.is_empty() {
                                "No findings stored for this conversation.".to_string()
                            } else {
                                body
                            },
                        );
                        self.activities
                            .push("Findings opened in transcript.".to_string());
                    }
                    "add" => {
                        let kind = parts.next();
                        let value = parts.next();
                        if let (Some(kind), Some(value)) = (kind, value) {
                            let note = command
                                .splitn(5, ' ')
                                .nth(4)
                                .map(str::trim)
                                .filter(|text| !text.is_empty());
                            let finding = self.orchestrator.store().add_finding(
                                &self.current_conversation_id,
                                kind,
                                value,
                                note,
                                &[],
                                None,
                                None,
                            )?;
                            self.activities.push(format!(
                                "Finding stored: {} [{}] {}",
                                finding.finding_id, finding.kind, finding.value
                            ));
                        } else {
                            self.activities
                                .push("Usage: /findings add <kind> <value> [note]".to_string());
                        }
                    }
                    "update" => {
                        let finding_id = parts.next();
                        let field = parts.next();
                        let value = parts.collect::<Vec<_>>().join(" ");
                        if let (Some(finding_id), Some(field)) = (finding_id, field) {
                            let findings = self.orchestrator.store().load_findings()?;
                            let Some(existing) = findings
                                .into_iter()
                                .find(|finding| finding.finding_id == finding_id)
                            else {
                                self.activities
                                    .push(format!("Finding not found: {finding_id}"));
                                return Ok(());
                            };

                            let mut kind = existing.kind;
                            let mut current_value = existing.value;
                            let mut note = existing.note;
                            let mut tags = existing.tags;
                            let mut confidence = existing.confidence;
                            let mut source_artifact = existing.source_artifact;

                            match field {
                                "kind" => kind = value.trim().to_string(),
                                "value" => current_value = value.trim().to_string(),
                                "note" => note = normalize_field_text(&value),
                                "tags" => tags = parse_tag_list(&value),
                                "confidence" => {
                                    confidence = if value.trim() == "-" || value.trim().is_empty() {
                                        None
                                    } else {
                                        match value.trim().parse::<u8>() {
                                            Ok(parsed) if parsed <= 100 => Some(parsed),
                                            _ => {
                                                self.activities.push(
                                                    "Usage: confidence must be 0-100 or `-` to clear"
                                                        .to_string(),
                                                );
                                                return Ok(());
                                            }
                                        }
                                    };
                                }
                                "artifact" => source_artifact = normalize_field_text(&value),
                                _ => {
                                    self.activities.push(
                                        "Usage: /findings update <finding-id> <kind|value|note|tags|confidence|artifact> <value>"
                                            .to_string(),
                                    );
                                    return Ok(());
                                }
                            }

                            let updated = self.orchestrator.store().update_finding(
                                finding_id,
                                &kind,
                                &current_value,
                                note.as_deref(),
                                &tags,
                                confidence,
                                source_artifact.as_deref(),
                            )?;
                            if let Some(finding) = updated {
                                self.activities.push(format!(
                                    "Finding updated: {} [{}] {}",
                                    finding.finding_id, finding.kind, finding.value
                                ));
                            } else {
                                self.activities
                                    .push(format!("Finding not found: {finding_id}"));
                            }
                        } else {
                            self.activities.push(
                                "Usage: /findings update <finding-id> <kind|value|note|tags|confidence|artifact> <value>"
                                    .to_string(),
                            );
                        }
                    }
                    "remove" | "delete" => {
                        if let Some(finding_id) = parts.next() {
                            if self.orchestrator.store().remove_finding(finding_id)? {
                                self.activities
                                    .push(format!("Finding removed: {finding_id}"));
                            } else {
                                self.activities
                                    .push(format!("Finding not found: {finding_id}"));
                            }
                        } else {
                            self.activities
                                .push("Usage: /findings remove <finding-id>".to_string());
                        }
                    }
                    _ => {
                        self.activities.push(
                            "Usage: /findings [list] | /findings add <kind> <value> [note] | /findings update <finding-id> <field> <value> | /findings remove <finding-id>"
                                .to_string(),
                        );
                    }
                }
            }
            "/search" => {
                let query = command.strip_prefix("/search").unwrap_or("").trim();
                if query.is_empty() {
                    self.activities.push("Usage: /search <query>".to_string());
                } else {
                    let results = self.orchestrator.store().search_local(query)?;
                    let body = results
                        .into_iter()
                        .take(20)
                        .map(|result| {
                            format!("- `{}` {}: {}", result.scope, result.title, result.snippet)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.push_command_output(
                        "/search",
                        if body.is_empty() {
                            format!("No local matches for `{query}`.")
                        } else {
                            body
                        },
                    );
                    self.activities
                        .push("Search results opened in transcript.".to_string());
                }
            }
            "/permissions" => {
                let sub = parts.next().unwrap_or_default();
                match sub {
                    "" | "show" | "status" => self.show_agent_permissions(),
                    "network" => match parts.next() {
                        Some("on") => {
                            self.agent_permissions.allow_network = true;
                            self.persist_agent_permissions(self.agent_permissions.clone())?;
                            self.activities.push("Network access enabled.".to_string());
                        }
                        Some("off") => {
                            self.agent_permissions.allow_network = false;
                            self.persist_agent_permissions(self.agent_permissions.clone())?;
                            self.activities.push("Network access disabled.".to_string());
                        }
                        _ => self
                            .activities
                            .push("Usage: /permissions network on|off".to_string()),
                    },
                    "fs" | "filesystem" => match parts.next() {
                        Some("none") => {
                            self.agent_permissions.filesystem = FilesystemAccess::None;
                            self.persist_agent_permissions(self.agent_permissions.clone())?;
                            self.activities.push("Filesystem access set to none.".to_string());
                        }
                        Some("read") => {
                            self.agent_permissions.filesystem = FilesystemAccess::ReadOnly;
                            self.persist_agent_permissions(self.agent_permissions.clone())?;
                            self.activities
                                .push("Filesystem access set to read-only.".to_string());
                        }
                        Some("write") => {
                            self.agent_permissions.filesystem = FilesystemAccess::ReadWrite;
                            self.persist_agent_permissions(self.agent_permissions.clone())?;
                            self.activities
                                .push("Filesystem access set to read-write.".to_string());
                        }
                        _ => self
                            .activities
                            .push("Usage: /permissions fs none|read|write".to_string()),
                    },
                    "yolo" => match parts.next() {
                        Some("on") => {
                            self.agent_permissions.yolo = true;
                            self.persist_agent_permissions(self.agent_permissions.clone())?;
                            self.activities.push(
                                "YOLO mode enabled. Internal permission safeguards are bypassed."
                                    .to_string(),
                            );
                        }
                        Some("off") => {
                            self.agent_permissions.yolo = false;
                            self.persist_agent_permissions(self.agent_permissions.clone())?;
                            self.activities
                                .push("YOLO mode disabled.".to_string());
                        }
                        _ => self
                            .activities
                            .push("Usage: /permissions yolo on|off".to_string()),
                    },
                    "reset" => {
                        let defaults = self.orchestrator.default_agent_permissions();
                        self.persist_agent_permissions(defaults)?;
                        self.activities
                            .push("Agent permissions reset to config defaults.".to_string());
                    }
                    _ => self.activities.push(
                        "Usage: /permissions [show] | /permissions network on|off | /permissions fs none|read|write | /permissions yolo on|off | /permissions reset"
                            .to_string(),
                    ),
                }
            }
            "/yolo" => match parts.next() {
                Some("on") => {
                    self.agent_permissions.yolo = true;
                    self.persist_agent_permissions(self.agent_permissions.clone())?;
                    self.activities.push(
                        "YOLO mode enabled. Internal permission safeguards are bypassed."
                            .to_string(),
                    );
                }
                Some("off") => {
                    self.agent_permissions.yolo = false;
                    self.persist_agent_permissions(self.agent_permissions.clone())?;
                    self.activities.push("YOLO mode disabled.".to_string());
                }
                _ => self.activities.push("Usage: /yolo on|off".to_string()),
            },
            "/mcp" => {
                let sub = parts.next().unwrap_or_default();
                match sub {
                    "" | "status" => self.show_mcp_server_status().await,
                    "reset" => {
                        self.persist_enabled_mcp_servers(None)?;
                        if self.configured_mcp_servers.is_empty() {
                            self.activities
                                .push("No MCP servers configured.".to_string());
                        } else {
                            self.activities
                                .push("All configured MCP servers enabled.".to_string());
                        }
                    }
                    "enable" => {
                        let names: Vec<String> = parts.map(str::to_string).collect();
                        if names.is_empty() {
                            self.activities
                                .push("Usage: /mcp enable <name...>".to_string());
                        } else if self.configured_mcp_servers.is_empty() {
                            self.activities
                                .push("No MCP servers configured.".to_string());
                        } else {
                            let (names, unknown) = self.split_known_mcp_server_names(names);
                            if !unknown.is_empty() {
                                self.activities
                                    .push(format!("Unknown MCP servers: {}", unknown.join(", ")));
                            }
                            if names.is_empty() {
                                return Ok(());
                            }
                            let next = match &self.enabled_mcp_servers {
                                None => None,
                                Some(current) => {
                                    let mut next = current.clone();
                                    for name in &names {
                                        if !next.contains(name) {
                                            next.push(name.clone());
                                        }
                                    }
                                    canonicalize_mcp_filter(&self.configured_mcp_servers, next)
                                }
                            };
                            self.persist_enabled_mcp_servers(next)?;
                            self.activities
                                .push(format!("Enabled MCP servers: {}", names.join(", ")));
                        }
                    }
                    "disable" => {
                        let names: Vec<String> = parts.map(str::to_string).collect();
                        if names.is_empty() {
                            self.activities
                                .push("Usage: /mcp disable <name...>".to_string());
                        } else if self.configured_mcp_servers.is_empty() {
                            self.activities
                                .push("No MCP servers configured.".to_string());
                        } else {
                            let (names, unknown) = self.split_known_mcp_server_names(names);
                            if !unknown.is_empty() {
                                self.activities
                                    .push(format!("Unknown MCP servers: {}", unknown.join(", ")));
                            }
                            if names.is_empty() {
                                return Ok(());
                            }
                            let mut next = match &self.enabled_mcp_servers {
                                None => self.configured_mcp_servers.clone(),
                                Some(current) => current.clone(),
                            };
                            next.retain(|name| !names.contains(name));
                            self.persist_enabled_mcp_servers(canonicalize_mcp_filter(
                                &self.configured_mcp_servers,
                                next,
                            ))?;
                            self.activities
                                .push(format!("Disabled MCP servers: {}", names.join(", ")));
                        }
                    }
                    "only" => {
                        let names: Vec<String> = parts.map(str::to_string).collect();
                        if names.is_empty() {
                            self.activities
                                .push("Usage: /mcp only <name...>".to_string());
                        } else if self.configured_mcp_servers.is_empty() {
                            self.activities
                                .push("No MCP servers configured.".to_string());
                        } else {
                            let (names, unknown) = self.split_known_mcp_server_names(names);
                            if !unknown.is_empty() {
                                self.activities
                                    .push(format!("Unknown MCP servers: {}", unknown.join(", ")));
                            }
                            if names.is_empty() {
                                return Ok(());
                            }
                            self.persist_enabled_mcp_servers(canonicalize_mcp_filter(
                                &self.configured_mcp_servers,
                                names.clone(),
                            ))?;
                            self.activities
                                .push(format!("MCP servers restricted to: {}", names.join(", ")));
                        }
                    }
                    _ => {
                        self.activities.push(
                            "Usage: /mcp [status] | /mcp reset|enable|disable|only <name...>"
                                .to_string(),
                        );
                    }
                }
            }
            "/compact" => {
                self.status = "Compacting conversation".to_string();
                self.activities
                    .push("Compacting conversation...".to_string());
                let orchestrator = self.orchestrator.clone();
                let conv_id = self.current_conversation_id.clone();
                let ui_tx = self.ui_tx.clone();
                tokio::spawn(async move {
                    let result = orchestrator
                        .compact_conversation(&conv_id, ui_tx.clone())
                        .await
                        .map_err(|err| {
                            tracing::error!(error = %err, "compaction failed");
                            format!("{err:#}")
                        });
                    let _ = ui_tx.send(UiEvent::CompactionFinished(result));
                });
            }
            "/recipes" => {
                let recipes = self.orchestrator.recipes().list();
                if recipes.is_empty() {
                    self.push_command_output("/recipes", "No recipes found.");
                    self.activities
                        .push("Recipe list opened in transcript.".to_string());
                } else {
                    let body = recipes
                        .iter()
                        .map(|recipe| {
                            let desc = recipe.description.as_deref().unwrap_or("No description.");
                            format!("- `{}`: {}", recipe.name, desc)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.push_command_output("/recipes", body);
                    self.activities
                        .push("Recipe list opened in transcript.".to_string());
                }
            }
            "/recipe" => {
                let sub = parts.next().unwrap_or_default();
                match sub {
                    "use" => {
                        if let Some(name) = parts.next() {
                            let name = name.to_string();
                            if let Some(recipe) = self.orchestrator.recipes().find(&name) {
                                let store = self.orchestrator.store();
                                let mut convo = store.load(&self.current_conversation_id)?;
                                convo.pending_recipe = Some(name.clone());
                                store.save(&convo)?;
                                self.activities.push(format!("Recipe '{name}' activated."));
                                if let Some(prompt) = recipe.initial_prompt.clone() {
                                    self.input = prompt;
                                    self.status = format!("Recipe '{name}' prompt loaded");
                                    self.activities
                                        .push(format!("Recipe '{name}' prompt loaded into input."));
                                }
                            } else {
                                self.activities.push(format!("Recipe '{name}' not found."));
                            }
                        } else {
                            self.activities
                                .push("Usage: /recipe use <name>".to_string());
                        }
                    }
                    "show" => {
                        if let Some(name) = parts.next() {
                            if let Some(recipe) = self.orchestrator.recipes().find(name) {
                                let recipe_name = recipe.name.clone();
                                let recipe_instructions = recipe.instructions.clone();
                                self.push_command_output(
                                    &format!("/recipe show {}", recipe_name),
                                    format!(
                                        "## Recipe: {}\n\n{}",
                                        recipe_name, recipe_instructions
                                    ),
                                );
                                self.activities.push(format!(
                                    "Recipe '{}' opened in transcript.",
                                    recipe_name
                                ));
                            } else {
                                self.activities.push(format!("Recipe '{name}' not found."));
                            }
                        } else {
                            self.activities
                                .push("Usage: /recipe show <name>".to_string());
                        }
                    }
                    "clear" => {
                        let store = self.orchestrator.store();
                        let mut convo = store.load(&self.current_conversation_id)?;
                        convo.pending_recipe = None;
                        store.save(&convo)?;
                        self.activities.push("Recipe cleared.".to_string());
                    }
                    _ => {
                        self.activities
                            .push("Usage: /recipe use|show|clear [name]".to_string());
                    }
                }
            }
            "/exit" | "/quit" => {
                self.status = "Restoring terminal".to_string();
                info!("received exit command from TUI");
                self.should_quit = true;
            }
            other => {
                self.activities.push(format!("Unknown command: {other}"));
            }
        }
        Ok(())
    }

    fn switch_conversation(&mut self, conversation_id: &str) -> Result<()> {
        let conversation = self.orchestrator.store().load(conversation_id)?;
        self.apply_conversation(conversation);
        self.activities
            .push(format!("Using {}", self.current_conversation_id));
        Ok(())
    }

    async fn dispatch_message(&mut self, message: String) -> Result<()> {
        let message = expand_prompt_file_references(
            &message,
            &self.agent_permissions,
            discover_project_root().as_deref(),
        )?;
        self.inflight = true;
        self.status = "Dispatching message".to_string();
        self.persistent_warning = None;
        let orchestrator = self.orchestrator.clone();
        let ui_tx = self.ui_tx.clone();
        let conversation_id = self.current_conversation_id.clone();
        tokio::spawn(async move {
            let result = orchestrator
                .run_turn(&conversation_id, message, ui_tx.clone())
                .await
                .map_err(|err| format!("{err:#}"));
            let _ = ui_tx.send(UiEvent::Finished(result));
        });
        Ok(())
    }

    fn handle_ui_event(&mut self, event: UiEvent) -> Result<()> {
        match event {
            UiEvent::Progress(progress) => {
                if progress.kind == "tool_limit" {
                    self.persistent_warning = Some(progress.message.clone());
                }
                let tool_prefix = progress
                    .tool_name
                    .as_deref()
                    .map(|name| format!("[{name}] "))
                    .unwrap_or_default();
                self.activities
                    .push(format!("{}{}", tool_prefix, progress.message));
                self.status = progress.kind;
            }
            UiEvent::Finished(result) => {
                self.inflight = false;
                match result {
                    Ok(run) => {
                        let conversation = self
                            .orchestrator
                            .store()
                            .load(&self.current_conversation_id)?;
                        self.messages = conversation.messages;
                        self.enabled_mcp_servers = conversation.enabled_mcp_servers;
                        self.agent_permissions = conversation.agent_permissions;
                        self.scroll_messages_to_latest();
                        self.status = format!("Reply ready // {} tool calls", run.tool_calls);
                        self.activities.push(format!(
                            "Assistant reply: {} chars · {} tool calls",
                            run.reply.len(),
                            run.tool_calls
                        ));
                    }
                    Err(err) => {
                        self.status = "Run failed".to_string();
                        self.activities.push(format!("ERROR // {err}"));
                        error!(
                            conversation_id = %self.current_conversation_id,
                            error = %err,
                            "conversation turn failed in TUI"
                        );
                    }
                }
            }
            UiEvent::CompactionFinished(result) => match result {
                Ok(summary) => {
                    self.status = "Conversation compacted".to_string();
                    self.activities.push(format!(
                        "Conversation compacted. Summary: {} chars",
                        summary.len()
                    ));
                }
                Err(err) => {
                    self.status = "Compaction failed".to_string();
                    self.activities.push(format!("ERROR // {err}"));
                    error!(
                        conversation_id = %self.current_conversation_id,
                        error = %err,
                        "conversation compaction failed in TUI"
                    );
                }
            },
        }
        Ok(())
    }

    fn input_preview(&self) -> String {
        if let Some(buffer) = &self.multiline_buffer {
            if buffer.is_empty() {
                format!("{}\n\n>>> to send", self.input)
            } else {
                format!("{}\n{}\n\n>>> to send", buffer.join("\n"), self.input)
            }
        } else {
            self.input.clone()
        }
    }

    fn status_line(&self) -> String {
        if self.inflight {
            format!("{} {}", SPINNER[self.spinner_index], self.status)
        } else if let Some(warning) = &self.persistent_warning {
            format!("WARNING // {warning}")
        } else {
            self.status.clone()
        }
    }

    fn scroll_messages_by(&mut self, delta: i32) {
        let max_scroll =
            max_message_scroll(self.rendered_message_lines, self.message_viewport_lines);
        let next = (i32::from(self.message_scroll) + delta).clamp(0, i32::from(max_scroll));
        self.message_scroll = next as u16;
    }

    fn scroll_messages_to_latest(&mut self) {
        self.message_scroll = 0;
    }

    fn page_scroll_amount(&self) -> u16 {
        self.message_viewport_lines.saturating_sub(1).max(1)
    }

    fn apply_conversation(&mut self, conversation: Conversation) {
        self.current_conversation_id = conversation.conversation_id;
        self.messages = conversation.messages;
        self.command_output.clear();
        self.enabled_mcp_servers = conversation.enabled_mcp_servers;
        self.agent_permissions = conversation.agent_permissions;
        self.message_scroll = 0;
        self.persistent_warning = None;
    }

    fn push_command_output(&mut self, command: &str, body: impl Into<String>) {
        let body = body.into();
        let content = if body.trim().is_empty() {
            format!("`{command}`")
        } else {
            format!("`{command}`\n\n{body}")
        };
        self.command_output.push(Message {
            role: "command".to_string(),
            content,
            timestamp: Utc::now(),
            metadata: None,
        });
        const MAX_COMMAND_OUTPUT_ENTRIES: usize = 24;
        if self.command_output.len() > MAX_COMMAND_OUTPUT_ENTRIES {
            let excess = self.command_output.len() - MAX_COMMAND_OUTPUT_ENTRIES;
            self.command_output.drain(0..excess);
        }
        self.scroll_messages_to_latest();
    }

    fn persist_agent_permissions(&mut self, agent_permissions: AgentPermissions) -> Result<()> {
        let store = self.orchestrator.store();
        let mut conversation = store.load(&self.current_conversation_id)?;
        conversation.agent_permissions = agent_permissions.clone();
        store.save(&conversation)?;
        self.agent_permissions = agent_permissions;
        Ok(())
    }

    fn persist_enabled_mcp_servers(
        &mut self,
        enabled_mcp_servers: Option<Vec<String>>,
    ) -> Result<()> {
        let store = self.orchestrator.store();
        let mut conversation = store.load(&self.current_conversation_id)?;
        conversation.enabled_mcp_servers = enabled_mcp_servers.clone();
        store.save(&conversation)?;
        self.enabled_mcp_servers = enabled_mcp_servers;
        Ok(())
    }

    fn split_known_mcp_server_names(&self, names: Vec<String>) -> (Vec<String>, Vec<String>) {
        let mut known = Vec::new();
        let mut unknown = Vec::new();

        for name in names {
            if self.configured_mcp_servers.contains(&name) {
                if !known.contains(&name) {
                    known.push(name);
                }
            } else if !unknown.contains(&name) {
                unknown.push(name);
            }
        }

        (known, unknown)
    }

    async fn show_mcp_server_status(&mut self) {
        let tool_counts = match self.orchestrator.mcp_tool_counts_by_server(None).await {
            Ok(counts) => Some(counts),
            Err(err) => {
                self.activities
                    .push(format!("MCP tool counts unavailable: {err}"));
                None
            }
        };

        let statuses = build_mcp_server_statuses(
            &self.configured_mcp_servers,
            self.enabled_mcp_servers.as_deref(),
            tool_counts.as_ref(),
        );

        if statuses.is_empty() {
            self.push_command_output("/mcp", "No MCP servers configured.");
            self.activities
                .push("MCP status opened in transcript.".to_string());
            return;
        }

        let body = statuses
            .into_iter()
            .map(|status| {
                let state = if status.enabled {
                    "enabled"
                } else {
                    "disabled"
                };
                let tool_count = status
                    .tool_count
                    .map(|count| format!("{count} tools"))
                    .unwrap_or_else(|| "tool count unavailable".to_string());
                format!("- `{}`: {} ({tool_count})", status.name, state)
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.push_command_output("/mcp", body);
        self.activities
            .push("MCP status opened in transcript.".to_string());
    }

    fn show_agent_permissions(&mut self) {
        let mut body = format!(
            "- Network: `{}`\n- Filesystem: `{}`\n- YOLO: `{}`",
            if self.agent_permissions.allows_network() {
                "on"
            } else {
                "off"
            },
            if self.agent_permissions.yolo {
                "all"
            } else {
                self.agent_permissions.filesystem.label()
            },
            if self.agent_permissions.yolo {
                "on"
            } else {
                "off"
            }
        );
        if self.agent_permissions.yolo {
            body.push_str("\n\nYOLO mode bypasses internal tool permission checks.");
        } else {
            body.push_str(
                "\n\nUse `/permissions network on|off`, `/permissions fs none|read|write`, or `/yolo on|off`.",
            );
        }
        self.push_command_output("/permissions", body);
        self.activities
            .push("Agent permissions opened in transcript.".to_string());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct McpServerStatus {
    name: String,
    enabled: bool,
    tool_count: Option<usize>,
}

fn build_mcp_server_statuses(
    configured_mcp_servers: &[String],
    enabled_mcp_servers: Option<&[String]>,
    tool_counts: Option<&std::collections::HashMap<String, usize>>,
) -> Vec<McpServerStatus> {
    configured_mcp_servers
        .iter()
        .map(|name| McpServerStatus {
            name: name.clone(),
            enabled: is_mcp_server_enabled(name, enabled_mcp_servers),
            tool_count: tool_counts.and_then(|counts| counts.get(name).copied()),
        })
        .collect()
}

fn is_mcp_server_enabled(server_name: &str, enabled_mcp_servers: Option<&[String]>) -> bool {
    match enabled_mcp_servers {
        None => true,
        Some(enabled_mcp_servers) => enabled_mcp_servers.iter().any(|name| name == server_name),
    }
}

fn canonicalize_mcp_filter(
    configured_mcp_servers: &[String],
    enabled_mcp_servers: Vec<String>,
) -> Option<Vec<String>> {
    let normalized: Vec<String> = configured_mcp_servers
        .iter()
        .filter(|server_name| enabled_mcp_servers.contains(server_name))
        .cloned()
        .collect();

    if normalized.len() == configured_mcp_servers.len() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_field_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "-" {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_tag_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty() && *tag != "-")
        .map(str::to_string)
        .collect()
}

fn max_message_scroll(total_lines: u16, viewport_lines: u16) -> u16 {
    total_lines.saturating_sub(viewport_lines)
}

#[cfg(test)]
fn ordered_messages(messages: &[Message]) -> Vec<&Message> {
    messages.iter().rev().collect()
}

fn ordered_transcript_messages(messages: &[Message], command_output: &[Message]) -> Vec<Message> {
    let mut combined = Vec::with_capacity(messages.len() + command_output.len());
    combined.extend(messages.iter().cloned());
    combined.extend(command_output.iter().cloned());
    combined.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    combined
}

fn build_help_markdown() -> String {
    [
        "### Conversations",
        "- `/new`: Create a conversation",
        "- `/list [all|archived]`: List active, all, or archived conversations",
        "- `/use <id>`: Switch conversation",
        "- `/show [id]`: Load and display a conversation",
        "- `/title [text]`: Set or clear the current conversation title",
        "- `/history`: Show recent transcript history for the current conversation",
        "- `/archive [id]`: Archive the current or specified conversation",
        "- `/unarchive <id>`: Restore an archived conversation",
        "- `/export [id]`: Export a local JSON session summary",
        "- `/delete <id>`: Delete a conversation",
        "- `/compact`: Compact the current conversation",
        "- `/jobs`: Show remembered jobs for the current conversation",
        "- `/scratch [show|set|append|clear]`: Manage the local scratchpad for this conversation",
        "- `/findings [list|add|update|remove]`: Manage structured findings for this conversation",
        "- `/search <query>`: Search conversations, scratchpads, and findings locally",
        "",
        "### Recipes",
        "- `/recipes`: List installed recipes",
        "- `/recipe use <name>`: Activate a recipe",
        "- `/recipe show <name>`: Show recipe instructions",
        "- `/recipe clear`: Clear the active recipe",
        "",
        "### MCP",
        "- `/mcp` or `/mcp status`: Show MCP server status",
        "- `/mcp reset`: Enable all configured MCP servers",
        "- `/mcp enable <name...>`: Enable one or more MCP servers",
        "- `/mcp disable <name...>`: Disable one or more MCP servers",
        "- `/mcp only <name...>`: Restrict the conversation to listed MCP servers",
        "- `/login <server>`: Start OAuth login for an MCP server using `oauth_public`",
        "",
        "### Permissions",
        "- `/permissions`: Show active agent permissions",
        "- `/permissions network on|off`: Toggle network access",
        "- `/permissions fs none|read|write`: Set filesystem access",
        "- `/permissions yolo on|off`: Toggle YOLO mode",
        "- `/permissions reset`: Restore config defaults",
        "- `/yolo on|off`: Shortcut for YOLO mode",
        "",
        "### Session",
        "- `/logging`: Show current tracing and audit log locations",
        "- `/help`: Open this command reference",
        "- `/exit` or `/quit`: Leave the TUI",
        "",
        "### Input",
        "- `Enter`: Send the current input",
        "- `<<<` then `>>>`: Capture and send multiline input",
        "- `Up` / `Down`: Scroll the transcript",
        "- `Ctrl+Up` / `Ctrl+Down`: Browse session-only input history",
        "- `PageUp` / `PageDown`: Page through the transcript",
        "- `Home` / `End`: Jump to the latest transcript position",
    ]
    .join("\n")
}

fn render_scroll_indicator(
    total_lines: u16,
    viewport_lines: u16,
    scroll: u16,
) -> Paragraph<'static> {
    let lines = build_scroll_indicator_lines(total_lines, viewport_lines, scroll);
    Paragraph::new(Text::from(lines))
}

fn build_scroll_indicator_lines(
    total_lines: u16,
    viewport_lines: u16,
    scroll: u16,
) -> Vec<Line<'static>> {
    let height = viewport_lines.max(1) as usize;
    let max_scroll = max_message_scroll(total_lines, viewport_lines);

    if height == 0 {
        return Vec::new();
    }

    let thumb_size = if total_lines <= viewport_lines || total_lines == 0 {
        height
    } else {
        ((usize::from(viewport_lines) * height) / usize::from(total_lines)).max(1)
    };

    let thumb_travel = height.saturating_sub(thumb_size);
    let thumb_offset = if max_scroll == 0 {
        0
    } else {
        (usize::from(scroll) * thumb_travel) / usize::from(max_scroll)
    };

    (0..height)
        .map(|index| {
            let (glyph, style) = if index >= thumb_offset && index < thumb_offset + thumb_size {
                (
                    "▐",
                    Style::default()
                        .fg(neon_pink())
                        .bg(panel_ink())
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("░", Style::default().fg(muted_synth()).bg(panel_ink()))
            };
            Line::from(Span::styled(glyph.to_string(), style))
        })
        .collect()
}

fn count_wrapped_rows(lines: &[Line<'_>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| {
            let line_width = line.width();
            if line_width == 0 {
                1
            } else {
                line_width.div_ceil(width)
            }
        })
        .sum()
}

#[derive(Debug, Clone, Default)]
struct MarkdownRenderer {
    lines: Vec<Line<'static>>,
    current_spans: Vec<Span<'static>>,
    strong_depth: usize,
    emphasis_depth: usize,
    strikethrough_depth: usize,
    code_block: bool,
    code_block_language: Option<String>,
    heading_level: Option<HeadingLevel>,
    quote_depth: usize,
    quote_kinds: Vec<Option<BlockQuoteKind>>,
    list_stack: Vec<ListState>,
    link_href: Option<String>,
    pending_item_prefix: Option<String>,
    table_active: bool,
    table_header: bool,
    table_row_open: bool,
    table_current_row: Vec<String>,
    table_current_cell: String,
}

#[derive(Debug, Clone)]
struct ListState {
    next_index: Option<u64>,
}

fn render_markdown(content: &str) -> Vec<Line<'static>> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_GFM);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(content, options);
    let mut renderer = MarkdownRenderer::default();
    renderer.render(parser);
    renderer.finish()
}

impl MarkdownRenderer {
    fn render<'a>(&mut self, parser: Parser<'a>) {
        for event in parser {
            match event {
                MdEvent::Start(tag) => self.start_tag(tag),
                MdEvent::End(tag) => self.end_tag(tag),
                MdEvent::Text(text) => {
                    if self.table_active {
                        self.push_table_text(text.as_ref());
                    } else {
                        self.push_text(text.as_ref());
                    }
                }
                MdEvent::Code(text) => {
                    if self.table_active {
                        self.push_table_text(&format!("`{text}`"));
                    } else {
                        self.push_span(
                            text.to_string(),
                            self.inline_style()
                                .fg(neon_gold())
                                .bg(input_ink())
                                .add_modifier(Modifier::BOLD),
                        );
                    }
                }
                MdEvent::SoftBreak => {
                    if self.code_block {
                        self.flush_line();
                    } else if self.table_active {
                        self.push_table_text(" ");
                    } else {
                        self.push_text(" ");
                    }
                }
                MdEvent::HardBreak => {
                    if self.table_active {
                        self.push_table_text(" ");
                    } else {
                        self.flush_line();
                    }
                }
                MdEvent::Rule => {
                    self.flush_line();
                    self.lines.push(Line::from(Span::styled(
                        "────────────────────",
                        Style::default().fg(muted_synth()),
                    )));
                }
                MdEvent::Html(text) | MdEvent::InlineHtml(text) => {
                    if self.table_active {
                        self.push_table_text(text.as_ref());
                    } else {
                        self.push_span(text.to_string(), self.inline_style().fg(muted_synth()));
                    }
                }
                MdEvent::FootnoteReference(text) => {
                    if self.table_active {
                        self.push_table_text(&format!("[{text}]"));
                    } else {
                        self.push_span(
                            format!("[{text}]"),
                            self.inline_style()
                                .fg(neon_gold())
                                .add_modifier(Modifier::ITALIC),
                        );
                    }
                }
                MdEvent::TaskListMarker(checked) => {
                    let marker = if checked { "[x] " } else { "[ ] " };
                    if self.table_active {
                        self.push_table_text(marker);
                    } else {
                        self.push_span(marker.to_string(), self.inline_style().fg(neon_lime()));
                    }
                }
                _ => {}
            }
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_line();
        if self.lines.is_empty() {
            self.lines.push(Line::raw(""));
        }
        self.lines
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush_line();
                self.heading_level = Some(level);
            }
            Tag::BlockQuote(kind) => {
                self.flush_line();
                self.quote_depth += 1;
                self.quote_kinds.push(kind);
                if let Some(kind) = kind {
                    self.lines.push(render_quote_label(kind));
                }
            }
            Tag::CodeBlock(kind) => {
                self.flush_line();
                self.code_block = true;
                self.code_block_language = match kind {
                    CodeBlockKind::Indented => None,
                    CodeBlockKind::Fenced(language) => Some(language.into_string()),
                };
                self.lines.push(render_code_block_chrome(
                    self.code_block_language.as_deref(),
                    true,
                ));
            }
            Tag::List(start) => {
                self.flush_line();
                self.list_stack.push(ListState { next_index: start });
            }
            Tag::Item => {
                self.flush_line();
                self.pending_item_prefix = Some(self.next_item_prefix());
            }
            Tag::Table(_) => {
                self.flush_line();
                self.table_active = true;
                self.table_header = false;
                self.table_row_open = false;
                self.table_current_row.clear();
                self.table_current_cell.clear();
            }
            Tag::TableHead => self.table_header = true,
            Tag::TableRow => {
                self.table_current_row.clear();
                self.table_row_open = true;
            }
            Tag::TableCell => self.table_current_cell.clear(),
            Tag::Emphasis => self.emphasis_depth += 1,
            Tag::Strong => self.strong_depth += 1,
            Tag::Strikethrough => self.strikethrough_depth += 1,
            Tag::Link { dest_url, .. } => self.link_href = Some(dest_url.into_string()),
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_line();
                self.push_blank_line();
            }
            TagEnd::Heading(..) => {
                self.flush_line();
                self.heading_level = None;
                self.push_blank_line();
            }
            TagEnd::BlockQuote(_) => {
                self.flush_line();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.quote_kinds.pop();
                self.push_blank_line();
            }
            TagEnd::CodeBlock => {
                self.code_block = false;
                self.code_block_language = None;
                self.lines.push(render_code_block_chrome(None, false));
                self.push_blank_line();
            }
            TagEnd::List(..) => {
                self.flush_line();
                self.list_stack.pop();
                self.push_blank_line();
            }
            TagEnd::Item => self.flush_line(),
            TagEnd::TableHead => {
                if !self.table_row_open && !self.table_current_row.is_empty() {
                    let columns = self.table_current_row.len();
                    self.render_table_row();
                    self.lines.push(render_table_separator(columns));
                }
                self.table_header = false;
            }
            TagEnd::TableRow => {
                let columns = self.table_current_row.len();
                self.render_table_row();
                if self.table_header {
                    self.lines.push(render_table_separator(columns));
                }
                self.table_row_open = false;
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.table_current_cell);
                self.table_current_row.push(cell.trim().to_string());
            }
            TagEnd::Table => {
                if !self.table_row_open && !self.table_current_row.is_empty() {
                    self.render_table_row();
                }
                self.table_active = false;
                self.table_header = false;
                self.table_row_open = false;
                self.table_current_row.clear();
                self.table_current_cell.clear();
                self.push_blank_line();
            }
            TagEnd::Emphasis => self.emphasis_depth = self.emphasis_depth.saturating_sub(1),
            TagEnd::Strong => self.strong_depth = self.strong_depth.saturating_sub(1),
            TagEnd::Strikethrough => {
                self.strikethrough_depth = self.strikethrough_depth.saturating_sub(1)
            }
            TagEnd::Link => {
                if let Some(href) = self.link_href.take() {
                    self.push_span(
                        format!(" ({href})"),
                        self.inline_style()
                            .fg(neon_cyan())
                            .add_modifier(Modifier::UNDERLINED),
                    );
                }
            }
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        if self.code_block {
            for line in text.split('\n') {
                self.push_code_block_line(line);
            }
            return;
        }

        let segments: Vec<&str> = text.split('\n').collect();
        for (index, segment) in segments.iter().enumerate() {
            if !segment.is_empty() {
                self.push_span(segment.to_string(), self.inline_style());
            }
            if index + 1 < segments.len() {
                self.flush_line();
            }
        }
    }

    fn push_table_text(&mut self, text: &str) {
        self.table_current_cell.push_str(&text.replace('\n', " "));
    }

    fn push_span(&mut self, text: String, style: Style) {
        if self.current_spans.is_empty() {
            self.push_prefixes();
        }
        self.current_spans.push(Span::styled(text, style));
    }

    fn push_code_block_line(&mut self, line: &str) {
        let mut spans = vec![Span::styled(
            "▏ ",
            Style::default().fg(neon_cyan()).bg(input_ink()),
        )];
        spans.push(Span::styled(
            if line.is_empty() {
                " ".to_string()
            } else {
                line.to_string()
            },
            Style::default().fg(neon_gold()).bg(input_ink()),
        ));
        self.lines.push(Line::from(spans));
    }

    fn render_table_row(&mut self) {
        if self.table_current_row.is_empty() {
            return;
        }

        let cells = std::mem::take(&mut self.table_current_row);
        let cell_style = if self.table_header {
            Style::default()
                .fg(neon_cyan())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(synth_text())
        };

        let mut spans = vec![Span::styled("│", Style::default().fg(neon_pink()))];
        for cell in cells {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(cell, cell_style));
            spans.push(Span::raw(" "));
            spans.push(Span::styled("│", Style::default().fg(neon_pink())));
        }
        self.lines.push(Line::from(spans));
    }

    fn flush_line(&mut self) {
        if self.current_spans.is_empty() {
            if self.lines.is_empty() || !self.lines.last().is_some_and(Line::spans_is_empty) {
                self.lines.push(Line::raw(""));
            }
            return;
        }
        let spans = std::mem::take(&mut self.current_spans);
        self.lines.push(Line::from(spans));
        self.pending_item_prefix = None;
    }

    fn push_blank_line(&mut self) {
        if !self.lines.last().is_some_and(Line::spans_is_empty) {
            self.lines.push(Line::raw(""));
        }
    }

    fn push_prefixes(&mut self) {
        if self.quote_depth > 0 {
            self.current_spans.push(Span::styled(
                format!("{} ", "▏".repeat(self.quote_depth)),
                self.current_quote_style(),
            ));
        }
        if let Some(level) = self.heading_level {
            let (marker, color) = match level {
                HeadingLevel::H1 => ("■ ", neon_pink()),
                HeadingLevel::H2 => ("▪ ", neon_cyan()),
                HeadingLevel::H3 => ("• ", neon_gold()),
                _ => ("· ", neon_gold()),
            };
            self.current_spans.push(Span::styled(
                marker,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }
        if let Some(prefix) = self.pending_item_prefix.take() {
            self.current_spans
                .push(Span::styled(prefix, Style::default().fg(neon_cyan())));
        }
    }

    fn current_quote_style(&self) -> Style {
        Style::default().fg(match self.quote_kinds.iter().rev().flatten().next() {
            Some(BlockQuoteKind::Note) => neon_cyan(),
            Some(BlockQuoteKind::Tip) => neon_lime(),
            Some(BlockQuoteKind::Important) => neon_pink(),
            Some(BlockQuoteKind::Warning) => neon_gold(),
            Some(BlockQuoteKind::Caution) => signal_red(),
            None => neon_lime(),
        })
    }

    fn next_item_prefix(&mut self) -> String {
        let indent = "  ".repeat(self.list_stack.len().saturating_sub(1));
        match self
            .list_stack
            .last_mut()
            .and_then(|state| state.next_index.as_mut())
        {
            Some(index) => {
                let prefix = format!("{indent}{index}. ");
                *index += 1;
                prefix
            }
            None => format!("{indent}- "),
        }
    }

    fn inline_style(&self) -> Style {
        let mut style = Style::default().fg(synth_text());
        if let Some(level) = self.heading_level {
            style = style
                .fg(match level {
                    HeadingLevel::H1 => neon_pink(),
                    HeadingLevel::H2 => neon_cyan(),
                    _ => neon_gold(),
                })
                .add_modifier(Modifier::BOLD);
        }
        if self.strong_depth > 0 {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.emphasis_depth > 0 {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.strikethrough_depth > 0 {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.link_href.is_some() {
            style = style.fg(neon_cyan()).add_modifier(Modifier::UNDERLINED);
        }
        style
    }
}

fn render_quote_label(kind: BlockQuoteKind) -> Line<'static> {
    let (label, color) = match kind {
        BlockQuoteKind::Note => ("NOTE", neon_cyan()),
        BlockQuoteKind::Tip => ("TIP", neon_lime()),
        BlockQuoteKind::Important => ("IMPORTANT", neon_pink()),
        BlockQuoteKind::Warning => ("WARNING", neon_gold()),
        BlockQuoteKind::Caution => ("CAUTION", signal_red()),
    };
    Line::from(vec![
        Span::styled(
            "▎ ",
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn render_code_block_chrome(language: Option<&str>, top: bool) -> Line<'static> {
    let label = match (top, language) {
        (true, Some(language)) if !language.is_empty() => format!(" code [{language}] "),
        (true, _) => " code ".to_string(),
        (false, _) => " end ".to_string(),
    };
    Line::from(Span::styled(
        label,
        Style::default()
            .fg(neon_cyan())
            .bg(input_ink())
            .add_modifier(Modifier::BOLD),
    ))
}

fn render_table_separator(columns: usize) -> Line<'static> {
    let mut text = String::from("├");
    for index in 0..columns.max(1) {
        text.push_str("────────");
        text.push(if index + 1 < columns.max(1) {
            '┼'
        } else {
            '┤'
        });
    }
    Line::from(Span::styled(text, Style::default().fg(neon_pink())))
}

trait LineExt {
    fn spans_is_empty(&self) -> bool;
}

impl LineExt for Line<'_> {
    fn spans_is_empty(&self) -> bool {
        self.spans.is_empty() || self.spans.iter().all(|span| span.content.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, net::SocketAddr, path::PathBuf};

    use axum::{Json, Router, routing::post};
    use chrono::{TimeZone, Utc};
    use ratatui::style::Modifier;
    use serde_json::json;
    use tempfile::{TempDir, tempdir};
    use tokio::net::TcpListener;

    use crate::{
        config::{
            AppConfig, AzureOpenAiConfig, LlmProvider, LocalToolsConfig, McpRuntimeConfig,
            McpServerConfig,
        },
        orchestrator::Orchestrator,
        prompt_expansion::expand_prompt_file_references,
        types::{AgentPermissions, FilesystemAccess, Message, UiEvent},
    };

    use super::{
        App, McpServerStatus, build_mcp_server_statuses, build_scroll_indicator_lines,
        canonicalize_mcp_filter, count_wrapped_rows, max_message_scroll, neon_gold, neon_pink,
        ordered_messages, render_markdown,
    };

    fn test_config(data_dir: PathBuf, mcp_servers: &[&str]) -> AppConfig {
        AppConfig {
            prompt: None,
            data_dir: Some(data_dir),
            llm_provider: Some(LlmProvider::AzureOpenAi),
            azure_openai: Some(AzureOpenAiConfig {
                api_key: "test-key".to_string(),
                api_version: "2024-10-21".to_string(),
                endpoint: "https://example.invalid".to_string(),
                deployment: "test-deployment".to_string(),
                temperature: 0.2,
                top_p: 1.0,
                max_output_tokens: 512,
            }),
            azure_anthropic: None,
            agent_permissions: AgentPermissions::default(),
            local_tools: LocalToolsConfig::default(),
            mcp_runtime: McpRuntimeConfig::default(),
            mcp_servers: mcp_servers
                .iter()
                .map(|name| McpServerConfig {
                    name: (*name).to_string(),
                    transport: "streamable_http".to_string(),
                    url: format!("http://127.0.0.1/{name}"),
                    command: None,
                    args: Vec::new(),
                    headers: HashMap::new(),
                    timeout: None,
                    sse_read_timeout: None,
                    client_session_timeout_seconds: None,
                    auth: None,
                })
                .collect(),
            tracing: None,
        }
    }

    async fn test_app(mcp_servers: &[&str]) -> (TempDir, App) {
        let dir = tempdir().unwrap();
        let orchestrator =
            Orchestrator::new(test_config(dir.path().to_path_buf(), mcp_servers)).unwrap();
        let app = App::new(orchestrator).await.unwrap();
        (dir, app)
    }

    async fn spawn_mock_mcp_server(tool_names: &[&str]) -> SocketAddr {
        let tools = tool_names
            .iter()
            .map(|name| {
                json!({
                    "name": name,
                    "description": format!("{name} description"),
                    "inputSchema": {"type": "object", "properties": {}}
                })
            })
            .collect::<Vec<_>>();

        let app = Router::new().route(
            "/mcp",
            post({
                let tools = tools.clone();
                move |Json(body): Json<serde_json::Value>| {
                    let tools = tools.clone();
                    async move {
                        let id = body.get("id").cloned().unwrap_or(serde_json::Value::Null);
                        let method = body
                            .get("method")
                            .and_then(|value| value.as_str())
                            .unwrap_or("");
                        let result = match method {
                            "initialize" => json!({
                                "protocolVersion": "2025-03-26",
                                "capabilities": {}
                            }),
                            "tools/list" => json!({ "tools": tools }),
                            _ => json!({}),
                        };
                        Json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": result
                        }))
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }

    fn load_live_config_for_test(data_dir: PathBuf) -> AppConfig {
        let root = crate::paths::discover_project_root().unwrap();
        let path = root.join("config").join("config.local.yaml");
        unsafe {
            std::env::set_var("AZURE_OPENAI_API_KEY", "test-key");
        }
        let mut config = AppConfig::load(path).unwrap();
        config.data_dir = Some(data_dir);
        config
    }

    #[test]
    fn renders_markdown_headings_and_emphasis() {
        let lines = render_markdown("# Title\n\nThis is **bold** and *italic*.");
        assert!(
            lines
                .iter()
                .any(|line| line.spans.iter().any(|span| span.content == "Title"
                    && span.style.fg == Some(neon_pink())
                    && span.style.add_modifier.contains(Modifier::BOLD)))
        );
        assert!(lines.iter().any(|line| line.spans.iter().any(
            |span| span.content == "bold" && span.style.add_modifier.contains(Modifier::BOLD)
        )));
        assert!(lines.iter().any(|line| {
            line.spans.iter().any(|span| {
                span.content == "italic" && span.style.add_modifier.contains(Modifier::ITALIC)
            })
        }));
    }

    #[test]
    fn renders_lists_and_code_blocks() {
        let lines = render_markdown("- alpha\n- beta\n\n```rust\nfn main() {}\n```");
        assert!(
            lines
                .iter()
                .any(|line| line.spans.iter().any(|span| span.content == "- "))
        );
        assert!(
            lines
                .iter()
                .any(|line| { line.spans.iter().any(|span| span.content.contains("code")) })
        );
        assert!(lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content == "fn main() {}" && span.style.fg == Some(neon_gold()))
        }));
    }

    #[test]
    fn renders_markdown_tables_as_rows() {
        let lines = render_markdown("| IOC | Value |\n| --- | --- |\n| IP | 1.2.3.4 |");

        assert!(lines.iter().any(|line| {
            line.spans.iter().any(|span| span.content == "IOC")
                && line.spans.iter().any(|span| span.content == "Value")
        }));
        assert!(lines.iter().any(|line| {
            line.spans.iter().any(|span| span.content == "IP")
                && line.spans.iter().any(|span| span.content == "1.2.3.4")
        }));
        assert!(lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("┼") || span.content.contains("┤"))
        }));
    }

    #[test]
    fn renders_gfm_blockquote_labels() {
        let lines = render_markdown("> [!WARNING]\n> Proceed carefully.");

        assert!(lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content == "WARNING" && span.style.fg == Some(neon_gold()))
        }));
    }

    #[test]
    fn computes_scroll_limits_from_viewport() {
        assert_eq!(max_message_scroll(10, 4), 6);
        assert_eq!(max_message_scroll(3, 8), 0);
    }

    #[test]
    fn counts_wrapped_rows_using_transcript_width() {
        let lines = render_markdown("12345\n\n123456");

        assert_eq!(count_wrapped_rows(&lines, 5), 5);
    }

    #[test]
    fn latest_messages_sort_first_in_stack() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: "older".to_string(),
                timestamp: Utc.with_ymd_and_hms(2026, 3, 15, 20, 0, 0).unwrap(),
                metadata: None,
            },
            Message {
                role: "assistant".to_string(),
                content: "newer".to_string(),
                timestamp: Utc.with_ymd_and_hms(2026, 3, 15, 20, 0, 1).unwrap(),
                metadata: None,
            },
        ];

        let ordered = ordered_messages(&messages);

        assert_eq!(ordered[0].content, "newer");
        assert_eq!(ordered[1].content, "older");
    }

    #[test]
    fn scroll_indicator_places_thumb_at_top_for_latest_messages() {
        let lines = build_scroll_indicator_lines(20, 5, 0);

        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0].spans[0].content, "▐");
    }

    #[test]
    fn scroll_indicator_moves_thumb_for_older_history() {
        let lines = build_scroll_indicator_lines(20, 5, 15);

        assert_eq!(lines.len(), 5);
        assert_eq!(lines[4].spans[0].content, "▐");
    }

    #[test]
    fn mcp_statuses_follow_filter_semantics() {
        let configured = vec!["alpha".to_string(), "beta".to_string()];

        assert_eq!(
            build_mcp_server_statuses(&configured, None, None),
            vec![
                McpServerStatus {
                    name: "alpha".to_string(),
                    enabled: true,
                    tool_count: None,
                },
                McpServerStatus {
                    name: "beta".to_string(),
                    enabled: true,
                    tool_count: None,
                },
            ]
        );
        assert_eq!(
            build_mcp_server_statuses(&configured, Some(&[]), None),
            vec![
                McpServerStatus {
                    name: "alpha".to_string(),
                    enabled: false,
                    tool_count: None,
                },
                McpServerStatus {
                    name: "beta".to_string(),
                    enabled: false,
                    tool_count: None,
                },
            ]
        );
        assert_eq!(
            canonicalize_mcp_filter(&configured, Vec::new()),
            Some(Vec::new())
        );
    }

    #[test]
    fn mcp_statuses_include_tool_counts_when_available() {
        let configured = vec!["alpha".to_string(), "beta".to_string()];
        let counts = HashMap::from([("alpha".to_string(), 4usize), ("beta".to_string(), 9usize)]);

        assert_eq!(
            build_mcp_server_statuses(&configured, None, Some(&counts)),
            vec![
                McpServerStatus {
                    name: "alpha".to_string(),
                    enabled: true,
                    tool_count: Some(4),
                },
                McpServerStatus {
                    name: "beta".to_string(),
                    enabled: true,
                    tool_count: Some(9),
                },
            ]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn help_mentions_mcp_status_listing() {
        let (_dir, mut app) = test_app(&["alpha", "beta"]).await;

        app.handle_command("/help").await.unwrap();

        assert_eq!(
            app.activities.last().unwrap(),
            "Command help opened in transcript."
        );
        let help = &app.command_output.last().unwrap().content;
        assert!(help.contains("`/mcp` or `/mcp status`: Show MCP server status"));
        assert!(help.contains("`/permissions reset`: Restore config defaults"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bare_mcp_command_lists_server_statuses() {
        let (_dir, mut app) = test_app(&["alpha", "beta"]).await;

        app.handle_command("/mcp").await.unwrap();

        assert_eq!(
            app.activities.last().unwrap(),
            "MCP status opened in transcript."
        );
        let status = &app.command_output.last().unwrap().content;
        assert!(status.contains("- `alpha`: enabled"));
        assert!(status.contains("- `beta`: enabled"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bare_mcp_command_uses_live_config_server_names_and_shows_counts() {
        let dir = tempdir().unwrap();
        let mut config = load_live_config_for_test(dir.path().to_path_buf());
        let addr = spawn_mock_mcp_server(&["search_events", "list_cases", "lookup_artifact"]).await;
        for server in &mut config.mcp_servers {
            server.url = format!("http://{addr}/mcp");
            server.auth = None;
            server.headers = HashMap::new();
        }

        let names = config
            .mcp_servers
            .iter()
            .map(|server| server.name.clone())
            .collect::<Vec<_>>();
        let orchestrator = Orchestrator::new(config).unwrap();
        let mut app = App::new(orchestrator).await.unwrap();

        app.handle_command("/mcp").await.unwrap();

        let status = &app.command_output.last().unwrap().content;
        for name in names {
            assert!(status.contains(&format!("- `{name}`: enabled (3 tools)")));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mcp_commands_update_conversation_filter_state() {
        let (_dir, mut app) = test_app(&["alpha", "beta"]).await;

        app.handle_command("/mcp disable alpha").await.unwrap();
        assert_eq!(app.enabled_mcp_servers, Some(vec!["beta".to_string()]));

        app.handle_command("/mcp disable beta").await.unwrap();
        assert_eq!(app.enabled_mcp_servers, Some(Vec::new()));

        app.handle_command("/mcp reset").await.unwrap();
        assert_eq!(app.enabled_mcp_servers, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bare_mcp_command_handles_no_configured_servers() {
        let (_dir, mut app) = test_app(&[]).await;

        app.handle_command("/mcp").await.unwrap();

        assert_eq!(
            app.activities.last().unwrap(),
            "MCP status opened in transcript."
        );
        assert!(
            app.command_output
                .last()
                .unwrap()
                .content
                .contains("No MCP servers configured.")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn permission_commands_update_conversation_state() {
        let (_dir, mut app) = test_app(&[]).await;

        app.handle_command("/permissions network on").await.unwrap();
        assert!(app.agent_permissions.allow_network);

        app.handle_command("/permissions fs write").await.unwrap();
        assert_eq!(
            app.agent_permissions.filesystem,
            FilesystemAccess::ReadWrite
        );

        app.handle_command("/yolo on").await.unwrap();
        assert!(app.agent_permissions.yolo);

        app.handle_command("/permissions reset").await.unwrap();
        assert_eq!(app.agent_permissions, AgentPermissions::default());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn recipe_use_prefills_input_without_dispatching() {
        let (_dir, mut app) = test_app(&[]).await;

        app.handle_command("/recipe use ip-reputation")
            .await
            .unwrap();

        assert_eq!(
            app.input.trim(),
            "I need a background check on the following IP addresses:"
        );
        assert!(!app.inflight);
        assert_eq!(app.status, "Recipe 'ip-reputation' prompt loaded");
        let convo = app
            .orchestrator
            .store()
            .load(&app.current_conversation_id)
            .unwrap();
        assert_eq!(convo.pending_recipe.as_deref(), Some("ip-reputation"));
    }

    #[test]
    fn prompt_expansion_is_available_to_tui_flow() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("note.md"), "hello from tui").unwrap();

        let expanded = expand_prompt_file_references(
            "Use @note.md",
            &AgentPermissions::default(),
            Some(dir.path()),
        )
        .unwrap();

        assert!(expanded.contains("[file: note.md]"));
        assert!(expanded.contains("hello from tui"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dispatch_message_rejects_invalid_file_reference_before_inflight() {
        let (_dir, mut app) = test_app(&[]).await;

        let err = app
            .dispatch_message("Use @missing.md".to_string())
            .await
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("failed to resolve referenced file")
        );
        assert!(!app.inflight);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn multiline_capture_preserves_blank_lines() {
        let (_dir, mut app) = test_app(&[]).await;
        app.multiline_buffer = Some(Vec::new());

        app.handle_submission("first".to_string()).await.unwrap();
        app.handle_submission(String::new()).await.unwrap();
        app.handle_submission("third".to_string()).await.unwrap();

        assert_eq!(
            app.multiline_buffer,
            Some(vec![
                "first".to_string(),
                "".to_string(),
                "third".to_string()
            ])
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compaction_completion_updates_status_when_event_arrives() {
        let (_dir, mut app) = test_app(&[]).await;

        app.handle_command("/compact").await.unwrap();
        assert_eq!(app.status, "Compacting conversation");
        assert_eq!(app.activities.last().unwrap(), "Compacting conversation...");

        app.handle_ui_event(UiEvent::CompactionFinished(Ok("summary".to_string())))
            .unwrap();

        assert_eq!(app.status, "Conversation compacted");
        assert!(
            app.activities
                .last()
                .unwrap()
                .contains("Conversation compacted. Summary: 7 chars")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scratchpad_commands_round_trip() {
        let (_dir, mut app) = test_app(&[]).await;

        app.handle_command("/scratch set first line").await.unwrap();
        app.handle_command("/scratch append second line")
            .await
            .unwrap();
        app.handle_command("/scratch").await.unwrap();

        let body = &app.command_output.last().unwrap().content;
        assert!(body.contains("first line"));
        assert!(body.contains("second line"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn findings_and_search_commands_surface_results() {
        let (_dir, mut app) = test_app(&[]).await;

        app.handle_command("/findings add ip 1.2.3.4 confirmed beacon")
            .await
            .unwrap();
        let finding_id = app
            .orchestrator
            .store()
            .load_findings()
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .finding_id;
        app.handle_command(&format!("/findings update {finding_id} confidence 88"))
            .await
            .unwrap();
        app.handle_command("/findings").await.unwrap();
        assert!(
            app.command_output
                .last()
                .unwrap()
                .content
                .contains("1.2.3.4")
        );
        assert!(
            app.command_output
                .last()
                .unwrap()
                .content
                .contains("confidence: 88")
        );

        app.handle_command("/search 1.2.3.4").await.unwrap();
        assert!(
            app.command_output
                .last()
                .unwrap()
                .content
                .contains("finding")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn archive_commands_hide_and_restore_conversations() {
        let (_dir, mut app) = test_app(&[]).await;
        let archived_id = app.current_conversation_id.clone();

        app.handle_command("/archive").await.unwrap();
        assert_eq!(
            app.activities.last().unwrap(),
            &format!("Archived {archived_id}")
        );
        assert_ne!(app.current_conversation_id, archived_id);

        app.handle_command("/list").await.unwrap();
        assert!(
            !app.command_output
                .last()
                .unwrap()
                .content
                .contains(&archived_id)
        );

        app.handle_command("/list archived").await.unwrap();
        assert!(
            app.command_output
                .last()
                .unwrap()
                .content
                .contains(&format!("{archived_id}`: 0 messages [archived]"))
        );

        app.handle_command(&format!("/unarchive {archived_id}"))
            .await
            .unwrap();
        assert_eq!(
            app.activities.last().unwrap(),
            &format!("Unarchived {archived_id}")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn title_command_updates_current_conversation_label() {
        let (_dir, mut app) = test_app(&[]).await;

        app.handle_command("/title Email Triage").await.unwrap();

        let conversation = app
            .orchestrator
            .store()
            .load(&app.current_conversation_id)
            .unwrap();
        assert_eq!(conversation.title.as_deref(), Some("Email Triage"));
        assert_eq!(
            app.activities.last().unwrap(),
            "Conversation title set: Email Triage"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn export_command_writes_summary_file() {
        let (_dir, mut app) = test_app(&[]).await;

        app.handle_command("/scratch set triage notes")
            .await
            .unwrap();
        app.handle_command("/export").await.unwrap();

        let body = &app.command_output.last().unwrap().content;
        assert!(body.contains("Session summary exported to `"));
        assert!(
            app.activities
                .last()
                .unwrap()
                .contains("Session summary exported for")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn input_history_recalls_previous_entries() {
        let (_dir, mut app) = test_app(&[]).await;

        app.handle_submission("/logging".to_string()).await.unwrap();
        app.handle_submission("/help".to_string()).await.unwrap();

        app.navigate_input_history(true);
        assert_eq!(app.input, "/help");

        app.navigate_input_history(true);
        assert_eq!(app.input, "/logging");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn input_history_restores_draft_when_exiting_navigation() {
        let (_dir, mut app) = test_app(&[]).await;

        app.handle_submission("/help".to_string()).await.unwrap();
        app.input = "draft query".to_string();

        app.navigate_input_history(true);
        assert_eq!(app.input, "/help");

        app.navigate_input_history(false);
        assert_eq!(app.input, "draft query");
        assert_eq!(app.input_history_cursor, None);
    }
}
