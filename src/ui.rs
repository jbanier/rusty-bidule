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
    widgets::{Block, Borders, Paragraph, Wrap},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::{debug, error, info, warn};

use crate::{
    orchestrator::Orchestrator,
    types::{AgentPermissions, Conversation, FilesystemAccess, Message, UiEvent},
};

const SPINNER: &[&str] = &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];
fn void_black() -> Color {
    Color::Rgb(8, 7, 22)
}

fn panel_ink() -> Color {
    Color::Rgb(18, 14, 40)
}

fn input_ink() -> Color {
    Color::Rgb(15, 10, 33)
}

fn neon_pink() -> Color {
    Color::Rgb(255, 88, 182)
}

fn neon_cyan() -> Color {
    Color::Rgb(77, 232, 255)
}

fn neon_gold() -> Color {
    Color::Rgb(255, 194, 92)
}

fn neon_orange() -> Color {
    Color::Rgb(255, 128, 89)
}

fn neon_lime() -> Color {
    Color::Rgb(148, 255, 125)
}

fn signal_red() -> Color {
    Color::Rgb(255, 94, 133)
}

fn synth_text() -> Color {
    Color::Rgb(223, 229, 255)
}

fn muted_synth() -> Color {
    Color::Rgb(125, 121, 179)
}

fn pane_block(title: &'static str, accent: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .style(Style::default().bg(panel_ink()))
        .title(Line::from(vec![
            Span::styled("▣ ", Style::default().fg(neon_gold())),
            Span::styled(
                title,
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
        ]))
}

fn section_heading(label: &'static str, accent: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "◆ ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            label,
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
    ])
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
    multiline_buffer: Option<Vec<String>>,
    inflight: bool,
    spinner_index: usize,
    status: String,
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
            multiline_buffer: None,
            inflight: false,
            spinner_index: 0,
            status: "Idle in the neon rain".to_string(),
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
                Constraint::Length(10),
                Constraint::Min(10),
                Constraint::Length(8),
            ])
            .split(frame.area());

        frame.render_widget(self.render_agent_monitor(), layout[0]);

        let transcript_block = pane_block("TRANSCRIPT // OUTPUT", neon_cyan());
        let transcript_inner = transcript_block.inner(layout[1]);
        frame.render_widget(transcript_block, layout[1]);
        let transcript_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(transcript_inner);

        let transcript = self.render_transcript(transcript_layout[0].height);
        frame.render_widget(transcript, transcript_layout[0]);

        let indicator = render_scroll_indicator(
            self.rendered_message_lines,
            self.message_viewport_lines,
            self.message_scroll,
        );
        frame.render_widget(indicator, transcript_layout[1]);

        frame.render_widget(self.render_input(), layout[2]);
    }

    fn render_agent_monitor(&self) -> Paragraph<'static> {
        let mode = if self.inflight { "RUNNING" } else { "STANDBY" };
        let input_mode = if self.multiline_buffer.is_some() {
            "MULTILINE"
        } else if self.input.trim().is_empty() {
            "READY"
        } else {
            "DRAFT LOADED"
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

        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    "RUSTY BIDULE",
                    Style::default()
                        .fg(neon_pink())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  //  "),
                Span::styled(
                    self.current_conversation_id.to_string(),
                    Style::default().fg(neon_cyan()),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    self.status.clone(),
                    Style::default()
                        .fg(neon_gold())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  //  "),
                Span::styled(mode, Style::default().fg(neon_lime())),
                Span::raw("  //  "),
                Span::styled(input_mode, Style::default().fg(neon_orange())),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("messages {}", self.messages.len()),
                    Style::default().fg(synth_text()),
                ),
                Span::raw("   "),
                Span::styled(
                    format!("agent events {}", self.activities.len()),
                    Style::default().fg(synth_text()),
                ),
                Span::raw("   "),
                Span::styled(
                    format!("latest tools {}", tool_calls),
                    Style::default().fg(synth_text()),
                ),
            ]),
            render_mcp_status_line(
                &self.configured_mcp_servers,
                self.enabled_mcp_servers.as_deref(),
            ),
            render_agent_permissions_line(&self.agent_permissions),
            section_heading("LIVE SIGNAL", neon_pink()),
        ];

        if self.activities.is_empty() {
            lines.push(Line::from(Span::styled(
                "Awaiting operator input.",
                Style::default().fg(muted_synth()),
            )));
        } else {
            for entry in self.activities.iter().rev().take(2) {
                lines.push(Line::from(vec![
                    Span::styled(">", Style::default().fg(neon_pink())),
                    Span::raw(" "),
                    Span::styled(entry.clone(), activity_style(entry)),
                ]));
            }
        }

        Paragraph::new(Text::from(lines))
            .style(Style::default().bg(panel_ink()))
            .wrap(Wrap { trim: false })
            .block(pane_block("AGENT // LIVE SIGNAL", neon_pink()))
    }

    fn render_transcript(&mut self, area_height: u16) -> Paragraph<'static> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        let transcript_messages = ordered_transcript_messages(&self.messages, &self.command_output);

        if transcript_messages.is_empty() {
            lines.push(Line::from(Span::styled(
                "No messages yet. Type into the deck below to start a run.",
                Style::default().fg(muted_synth()),
            )));
        } else {
            lines.extend(transcript_messages.into_iter().flat_map(|message| {
                let (role_label, role_style) = match message.role.as_str() {
                    "user" => (
                        "OPERATOR",
                        Style::default()
                            .fg(neon_cyan())
                            .add_modifier(Modifier::BOLD),
                    ),
                    "assistant" => (
                        "AGENT",
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
                            "#{}  {} tools  {:.1}s",
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

        self.rendered_message_lines = lines.len().min(u16::MAX as usize) as u16;
        self.message_viewport_lines = area_height.saturating_sub(2);
        self.message_scroll = self.message_scroll.min(max_message_scroll(
            self.rendered_message_lines,
            self.message_viewport_lines,
        ));

        Paragraph::new(Text::from(lines))
            .scroll((self.message_scroll, 0))
            .style(Style::default().fg(synth_text()).bg(panel_ink()))
            .wrap(Wrap { trim: false })
    }

    fn render_input(&self) -> Paragraph<'static> {
        let preview = self.input_preview();
        let mut lines = Vec::new();

        if self.inflight {
            lines.push(Line::from(vec![
                Span::styled(
                    self.status_line(),
                    Style::default()
                        .fg(neon_gold())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled("RUNNING", Style::default().fg(neon_lime())),
            ]));
            lines.push(Line::raw(""));
        } else if self.multiline_buffer.is_some() {
            lines.push(Line::from(Span::styled(
                "MULTILINE CAPTURE ACTIVE",
                Style::default()
                    .fg(neon_cyan())
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::raw(""));
        }

        if preview.is_empty() {
            lines.push(Line::from(Span::styled(
                "Type a prompt, or use /help to browse commands.",
                Style::default().fg(muted_synth()),
            )));
        } else {
            for line in preview.lines() {
                lines.push(Line::from(vec![
                    Span::styled(">", Style::default().fg(neon_pink())),
                    Span::raw(" "),
                    Span::styled(line.to_string(), Style::default().fg(synth_text())),
                ]));
            }
        }

        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(
                "Enter",
                Style::default()
                    .fg(neon_gold())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" send", Style::default().fg(muted_synth())),
            Span::raw("   "),
            Span::styled(
                "/help",
                Style::default()
                    .fg(neon_pink())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" command list", Style::default().fg(muted_synth())),
            Span::raw("   "),
            Span::styled(
                "<<< ... >>>",
                Style::default()
                    .fg(neon_orange())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" multiline", Style::default().fg(muted_synth())),
        ]));

        Paragraph::new(Text::from(lines))
            .style(Style::default().fg(synth_text()).bg(input_ink()))
            .wrap(Wrap { trim: false })
            .block(pane_block("INPUT // COMMAND DECK", neon_orange()))
    }

    async fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
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
                self.input.push(ch);
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Enter => {
                let submitted = std::mem::take(&mut self.input);
                self.handle_submission(submitted).await?;
            }
            KeyCode::Esc => {
                self.input.clear();
                self.multiline_buffer = None;
                self.status = "Input cleared".to_string();
            }
            _ => {}
        }

        Ok(())
    }

    async fn handle_submission(&mut self, submitted: String) -> Result<()> {
        let trimmed = submitted.trim_end().to_string();
        if trimmed.is_empty() {
            return Ok(());
        }

        if self.inflight {
            warn!("ignored submission while engine was busy");
            self.activities
                .push("Engine busy. Wait for the current run to finish.".to_string());
            return Ok(());
        }

        if let Some(buffer) = &mut self.multiline_buffer {
            if trimmed == ">>>" {
                let payload = buffer.join("\n");
                self.multiline_buffer = None;
                self.dispatch_message(payload).await?;
            } else {
                buffer.push(trimmed);
            }
            return Ok(());
        }

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
                let conversations = self.orchestrator.store().list_conversations()?;
                if conversations.is_empty() {
                    self.push_command_output("/list", "No conversations found.");
                    self.activities
                        .push("Conversation list opened in transcript.".to_string());
                } else {
                    let body = conversations
                        .iter()
                        .take(12)
                        .map(|summary| {
                            format!(
                                "- `{}`: {} messages",
                                summary.conversation_id, summary.message_count
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
            "/help" => {
                self.push_command_output("/help", build_help_markdown());
                self.activities
                    .push("Command help opened in transcript.".to_string());
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
            "/model" => {
                self.activities.push("Model selection is fixed to the configured Azure deployment in this prototype.".to_string());
            }
            "/logging" => {
                self.activities.push("Logging verbosity toggles are not implemented yet; audit logs are always written to disk.".to_string());
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
                    "" | "status" => self.show_mcp_server_status(),
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
                self.activities
                    .push("Compacting conversation...".to_string());
                let orchestrator = self.orchestrator.clone();
                let conv_id = self.current_conversation_id.clone();
                let ui_tx = self.ui_tx.clone();
                tokio::spawn(async move {
                    match orchestrator.compact_conversation(&conv_id, ui_tx).await {
                        Ok(_) => {}
                        Err(err) => {
                            tracing::error!(error = %err, "compaction failed");
                        }
                    }
                });
                self.activities.push("Conversation compacted.".to_string());
            }
            "/recipes" => {
                let recipes = self.orchestrator.recipes().list();
                if recipes.is_empty() {
                    self.push_command_output("/recipes", "No recipes found.");
                    self.activities
                        .push("Recipe list opened in transcript.".to_string());
                } else {
                    let body = recipes
                        .into_iter()
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
                                // Auto-dispatch initial prompt if set
                                if let Some(prompt) = recipe.initial_prompt.clone() {
                                    self.dispatch_message(prompt).await?;
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
        self.inflight = true;
        self.status = "Dispatching message".to_string();
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

    fn show_mcp_server_status(&mut self) {
        let statuses = build_mcp_server_statuses(
            &self.configured_mcp_servers,
            self.enabled_mcp_servers.as_deref(),
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
                format!("- `{}`: {}", status.name, state)
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
}

fn build_mcp_server_statuses(
    configured_mcp_servers: &[String],
    enabled_mcp_servers: Option<&[String]>,
) -> Vec<McpServerStatus> {
    configured_mcp_servers
        .iter()
        .map(|name| McpServerStatus {
            name: name.clone(),
            enabled: is_mcp_server_enabled(name, enabled_mcp_servers),
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

fn render_mcp_status_line(
    configured_mcp_servers: &[String],
    enabled_mcp_servers: Option<&[String]>,
) -> Line<'static> {
    if configured_mcp_servers.is_empty() {
        return Line::from(vec![
            Span::styled(
                "MCP",
                Style::default()
                    .fg(neon_gold())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("none configured", Style::default().fg(muted_synth())),
        ]);
    }

    let mut spans = vec![Span::styled(
        "MCP",
        Style::default()
            .fg(neon_gold())
            .add_modifier(Modifier::BOLD),
    )];

    for status in build_mcp_server_statuses(configured_mcp_servers, enabled_mcp_servers) {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            "●",
            Style::default().fg(if status.enabled {
                neon_lime()
            } else {
                signal_red()
            }),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            status.name,
            Style::default().fg(if status.enabled {
                synth_text()
            } else {
                muted_synth()
            }),
        ));
    }

    Line::from(spans)
}

fn render_agent_permissions_line(agent_permissions: &AgentPermissions) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "PERMS",
            Style::default()
                .fg(neon_gold())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "net:{}",
                if agent_permissions.allows_network() {
                    "on"
                } else {
                    "off"
                }
            ),
            Style::default().fg(if agent_permissions.allows_network() {
                neon_lime()
            } else {
                signal_red()
            }),
        ),
        Span::raw("   "),
        Span::styled(
            format!(
                "fs:{}",
                if agent_permissions.yolo {
                    "all"
                } else {
                    agent_permissions.filesystem.label()
                }
            ),
            Style::default().fg(match agent_permissions.filesystem {
                FilesystemAccess::None if !agent_permissions.yolo => signal_red(),
                FilesystemAccess::ReadOnly if !agent_permissions.yolo => neon_cyan(),
                _ => neon_lime(),
            }),
        ),
        Span::raw("   "),
        Span::styled(
            format!("yolo:{}", if agent_permissions.yolo { "on" } else { "off" }),
            Style::default().fg(if agent_permissions.yolo {
                neon_pink()
            } else {
                muted_synth()
            }),
        ),
    ])
}

fn max_message_scroll(total_lines: u16, viewport_lines: u16) -> u16 {
    total_lines.saturating_sub(viewport_lines)
}

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
        "- `/list`: List recent conversations",
        "- `/use <id>`: Switch conversation",
        "- `/show [id]`: Load and display a conversation",
        "- `/delete <id>`: Delete a conversation",
        "- `/compact`: Compact the current conversation",
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
        "- `/model`: Show the current model note",
        "- `/logging`: Show the logging note",
        "- `/help`: Open this command reference",
        "- `/exit` or `/quit`: Leave the TUI",
        "",
        "### Input",
        "- `Enter`: Send the current input",
        "- `<<<` then `>>>`: Capture and send multiline input",
        "- `Up` / `Down`: Scroll the transcript",
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
                    "█",
                    Style::default()
                        .fg(neon_pink())
                        .bg(panel_ink())
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("│", Style::default().fg(muted_synth()).bg(panel_ink()))
            };
            Line::from(Span::styled(glyph.to_string(), style))
        })
        .collect()
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
                        "╶──────────────────────╴",
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
            "│ ",
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
                format!("{} ", "▍".repeat(self.quote_depth)),
                self.current_quote_style(),
            ));
        }
        if let Some(level) = self.heading_level {
            let (marker, color) = match level {
                HeadingLevel::H1 => ("▓ ", neon_pink()),
                HeadingLevel::H2 => ("◆ ", neon_cyan()),
                HeadingLevel::H3 => ("◇ ", neon_gold()),
                _ => ("• ", neon_gold()),
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
            None => format!("{indent}• "),
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
            "▌ ",
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
        (true, Some(language)) if !language.is_empty() => format!("┌─ code: {language}"),
        (true, _) => "┌─ code".to_string(),
        (false, _) => "└─".to_string(),
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
    use std::{collections::HashMap, path::PathBuf};

    use chrono::{TimeZone, Utc};
    use ratatui::style::Modifier;
    use tempfile::{TempDir, tempdir};

    use crate::{
        config::{AppConfig, AzureOpenAiConfig, McpRuntimeConfig, McpServerConfig},
        orchestrator::Orchestrator,
        types::{AgentPermissions, FilesystemAccess, Message},
    };

    use super::{
        App, McpServerStatus, build_mcp_server_statuses, build_scroll_indicator_lines,
        canonicalize_mcp_filter, max_message_scroll, neon_gold, neon_pink, ordered_messages,
        render_markdown,
    };

    fn test_config(data_dir: PathBuf, mcp_servers: &[&str]) -> AppConfig {
        AppConfig {
            prompt: None,
            data_dir: Some(data_dir),
            azure_openai: AzureOpenAiConfig {
                api_key: "test-key".to_string(),
                api_version: "2024-10-21".to_string(),
                endpoint: "https://example.invalid".to_string(),
                deployment: "test-deployment".to_string(),
                temperature: 0.2,
                top_p: 1.0,
                max_output_tokens: 512,
            },
            agent_permissions: AgentPermissions::default(),
            mcp_runtime: McpRuntimeConfig::default(),
            mcp_servers: mcp_servers
                .iter()
                .map(|name| McpServerConfig {
                    name: (*name).to_string(),
                    transport: "streamable_http".to_string(),
                    url: format!("http://127.0.0.1/{name}"),
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
                .any(|line| line.spans.iter().any(|span| span.content == "• "))
        );
        assert!(lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("┌") && span.content.contains("code"))
        }));
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
        assert_eq!(lines[0].spans[0].content, "█");
    }

    #[test]
    fn scroll_indicator_moves_thumb_for_older_history() {
        let lines = build_scroll_indicator_lines(20, 5, 15);

        assert_eq!(lines.len(), 5);
        assert_eq!(lines[4].spans[0].content, "█");
    }

    #[test]
    fn mcp_statuses_follow_filter_semantics() {
        let configured = vec!["alpha".to_string(), "beta".to_string()];

        assert_eq!(
            build_mcp_server_statuses(&configured, None),
            vec![
                McpServerStatus {
                    name: "alpha".to_string(),
                    enabled: true,
                },
                McpServerStatus {
                    name: "beta".to_string(),
                    enabled: true,
                },
            ]
        );
        assert_eq!(
            build_mcp_server_statuses(&configured, Some(&[])),
            vec![
                McpServerStatus {
                    name: "alpha".to_string(),
                    enabled: false,
                },
                McpServerStatus {
                    name: "beta".to_string(),
                    enabled: false,
                },
            ]
        );
        assert_eq!(
            canonicalize_mcp_filter(&configured, Vec::new()),
            Some(Vec::new())
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
}
