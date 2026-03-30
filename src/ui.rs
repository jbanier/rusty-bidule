use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use pulldown_cmark::{CodeBlockKind, Event as MdEvent, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::{debug, error, info, warn};

use crate::{
    orchestrator::Orchestrator,
    types::{Message, UiEvent},
};

const SPINNER: &[&str] = &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];

pub struct App {
    orchestrator: Orchestrator,
    current_conversation_id: String,
    messages: Vec<Message>,
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
        let messages = orchestrator
            .store()
            .load(&current_conversation_id)?
            .messages;
        let (ui_tx, ui_rx) = unbounded_channel();
        info!(
            %current_conversation_id,
            message_count = messages.len(),
            "initialized TUI application state"
        );

        Ok(Self {
            orchestrator,
            current_conversation_id,
            messages,
            message_scroll: 0,
            rendered_message_lines: 0,
            message_viewport_lines: 0,
            activities: vec!["Neon shell online. Type /help for commands.".to_string()],
            input: String::new(),
            multiline_buffer: None,
            inflight: false,
            spinner_index: 0,
            status: "Idle".to_string(),
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
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(5),
            ])
            .split(frame.area());

        let title = Paragraph::new(Line::from(vec![
            Span::styled(
                "RUSTY BIDULE",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  //  "),
            Span::styled(
                self.current_conversation_id.to_string(),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw("  //  "),
            Span::styled(self.status_line(), Style::default().fg(Color::Yellow)),
        ]))
        .block(Block::default().borders(Borders::ALL).title("CYBERDECK"));
        frame.render_widget(title, layout[0]);

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
            .split(layout[1]);

        let message_block = Block::default()
            .borders(Borders::ALL)
            .title("MESSAGE STACK");
        let message_inner = message_block.inner(body[0]);
        frame.render_widget(message_block, body[0]);
        let message_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(message_inner);

        let messages = self.render_messages(message_layout[0].height);
        frame.render_widget(messages, message_layout[0]);

        let indicator = render_scroll_indicator(
            self.rendered_message_lines,
            self.message_viewport_lines,
            self.message_scroll,
        );
        frame.render_widget(indicator, message_layout[1]);

        let activity_items: Vec<ListItem> = self
            .activities
            .iter()
            .rev()
            .take(18)
            .rev()
            .map(|entry| {
                ListItem::new(Line::from(Span::styled(
                    entry.clone(),
                    Style::default().fg(Color::Green),
                )))
            })
            .collect();
        let activity = List::new(activity_items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("ACTIVITY FEED"),
        );
        frame.render_widget(activity, body[1]);

        let input_title = if self.multiline_buffer.is_some() {
            "INPUT // MULTILINE"
        } else {
            "INPUT"
        };
        let input = Paragraph::new(self.input_preview())
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(input_title));
        frame.render_widget(input, layout[2]);
    }

    fn render_messages(&mut self, area_height: u16) -> Paragraph<'static> {
        let lines: Vec<Line<'static>> = ordered_messages(&self.messages)
            .into_iter()
            .flat_map(|message| {
                let role_style = match message.role.as_str() {
                    "user" => Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                    "assistant" => Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                    _ => Style::default().fg(Color::Yellow),
                };
                let mut lines = vec![Line::from(vec![
                    Span::styled(message.role.to_uppercase().to_string(), role_style),
                    Span::raw("  "),
                    Span::styled(
                        message.timestamp.format("%H:%M:%S").to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])];
                lines.extend(render_markdown(&message.content));
                lines.push(Line::raw(""));
                lines
            })
            .collect();

        self.rendered_message_lines = lines.len().min(u16::MAX as usize) as u16;
        self.message_viewport_lines = area_height.saturating_sub(2);
        self.message_scroll = self.message_scroll.min(max_message_scroll(
            self.rendered_message_lines,
            self.message_viewport_lines,
        ));

        Paragraph::new(Text::from(lines))
            .scroll((self.message_scroll, 0))
            .wrap(Wrap { trim: false })
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
                self.current_conversation_id = conversation.conversation_id.clone();
                self.messages = conversation.messages;
                self.message_scroll = 0;
                self.activities
                    .push(format!("Created {}", self.current_conversation_id));
            }
            "/list" => {
                let conversations = self.orchestrator.store().list_conversations()?;
                if conversations.is_empty() {
                    self.activities.push("No conversations found.".to_string());
                } else {
                    for summary in conversations.iter().take(8) {
                        self.activities.push(format!(
                            "{} ({} msgs)",
                            summary.conversation_id, summary.message_count
                        ));
                    }
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
                        self.current_conversation_id =
                            self.orchestrator.ensure_default_conversation().await?;
                        self.messages = self
                            .orchestrator
                            .store()
                            .load(&self.current_conversation_id)?
                            .messages;
                    }
                } else {
                    self.activities
                        .push("Usage: /delete <conversation-id>".to_string());
                }
            }
            "/help" => {
                self.activities.push("Commands: /new /list /use <id> /show [id] /delete <id> /login <server> /model /logging /compact /recipes /recipe use|show|clear /mcp reset|enable|disable|only /help /exit /quit".to_string());
                self.activities
                    .push("Multiline mode: enter <<< then finish with >>>".to_string());
                self.activities
                    .push("Scroll messages: Up/Down, PageUp/PageDown, Home, End".to_string());
                self.activities.push(
                    "/exit and /quit both leave the TUI and restore the terminal.".to_string(),
                );
                self.activities.push(
                    "/login <server> starts OAuth login for an MCP server configured with oauth_public.".to_string(),
                );
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
            "/mcp" => {
                let sub = parts.next().unwrap_or_default();
                match sub {
                    "reset" => {
                        let store = self.orchestrator.store();
                        let mut convo = store.load(&self.current_conversation_id)?;
                        convo.enabled_mcp_servers = None;
                        store.save(&convo)?;
                        self.activities.push("All MCP servers enabled".to_string());
                    }
                    "enable" => {
                        let names: Vec<String> = parts.map(str::to_string).collect();
                        if names.is_empty() {
                            self.activities.push("Usage: /mcp enable <name...>".to_string());
                        } else {
                            let store = self.orchestrator.store();
                            let mut convo = store.load(&self.current_conversation_id)?;
                            let mut current = convo.enabled_mcp_servers.unwrap_or_default();
                            for name in &names {
                                if !current.contains(name) {
                                    current.push(name.clone());
                                }
                            }
                            convo.enabled_mcp_servers = Some(current);
                            store.save(&convo)?;
                            self.activities.push(format!("Enabled MCP servers: {}", names.join(", ")));
                        }
                    }
                    "disable" => {
                        let names: Vec<String> = parts.map(str::to_string).collect();
                        if names.is_empty() {
                            self.activities.push("Usage: /mcp disable <name...>".to_string());
                        } else {
                            let store = self.orchestrator.store();
                            let mut convo = store.load(&self.current_conversation_id)?;
                            let mut current = convo.enabled_mcp_servers.unwrap_or_default();
                            current.retain(|n| !names.contains(n));
                            convo.enabled_mcp_servers = if current.is_empty() { None } else { Some(current) };
                            store.save(&convo)?;
                            self.activities.push(format!("Disabled MCP servers: {}", names.join(", ")));
                        }
                    }
                    "only" => {
                        let names: Vec<String> = parts.map(str::to_string).collect();
                        if names.is_empty() {
                            self.activities.push("Usage: /mcp only <name...>".to_string());
                        } else {
                            let store = self.orchestrator.store();
                            let mut convo = store.load(&self.current_conversation_id)?;
                            convo.enabled_mcp_servers = Some(names.clone());
                            store.save(&convo)?;
                            self.activities.push(format!("MCP servers restricted to: {}", names.join(", ")));
                        }
                    }
                    _ => {
                        self.activities.push("Usage: /mcp reset|enable|disable|only [names...]".to_string());
                    }
                }
            }
            "/compact" => {
                self.activities.push("Compacting conversation...".to_string());
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
                    self.activities.push("No recipes found.".to_string());
                } else {
                    for r in recipes {
                        let desc = r.description.as_deref().unwrap_or("");
                        self.activities.push(format!("{}: {}", r.name, desc));
                    }
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
                            self.activities.push("Usage: /recipe use <name>".to_string());
                        }
                    }
                    "show" => {
                        if let Some(name) = parts.next() {
                            if let Some(recipe) = self.orchestrator.recipes().find(name) {
                                self.activities.push(format!("Recipe: {}", recipe.name));
                                for line in recipe.instructions.lines().take(10) {
                                    self.activities.push(line.to_string());
                                }
                            } else {
                                self.activities.push(format!("Recipe '{name}' not found."));
                            }
                        } else {
                            self.activities.push("Usage: /recipe show <name>".to_string());
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
                        self.activities.push("Usage: /recipe use|show|clear [name]".to_string());
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
        self.current_conversation_id = conversation.conversation_id.clone();
        self.messages = conversation.messages;
        self.message_scroll = 0;
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
                        self.messages = self
                            .orchestrator
                            .store()
                            .load(&self.current_conversation_id)?
                            .messages;
                        self.scroll_messages_to_latest();
                        self.status = format!("Reply ready // {} tool calls", run.tool_calls);
                        self.activities
                            .push(format!("Assistant reply: {} chars · {} tool calls", run.reply.len(), run.tool_calls));
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
}

fn max_message_scroll(total_lines: u16, viewport_lines: u16) -> u16 {
    total_lines.saturating_sub(viewport_lines)
}

fn ordered_messages(messages: &[Message]) -> Vec<&Message> {
    messages.iter().rev().collect()
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
                ("█", Style::default().fg(Color::Magenta))
            } else {
                ("│", Style::default().fg(Color::DarkGray))
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
    list_stack: Vec<ListState>,
    link_href: Option<String>,
    pending_item_prefix: Option<String>,
}

#[derive(Debug, Clone)]
struct ListState {
    next_index: Option<u64>,
}

fn render_markdown(content: &str) -> Vec<Line<'static>> {
    let mut options = Options::empty();
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
                MdEvent::Text(text) => self.push_text(text.as_ref()),
                MdEvent::Code(text) => self.push_span(
                    text.to_string(),
                    self.inline_style()
                        .fg(Color::Yellow)
                        .bg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ),
                MdEvent::SoftBreak => {
                    if self.code_block {
                        self.flush_line();
                    } else {
                        self.push_text(" ");
                    }
                }
                MdEvent::HardBreak => self.flush_line(),
                MdEvent::Rule => {
                    self.flush_line();
                    self.lines.push(Line::from(Span::styled(
                        "────────────────────────",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                MdEvent::Html(text) | MdEvent::InlineHtml(text) => {
                    self.push_span(text.to_string(), self.inline_style().fg(Color::DarkGray));
                }
                MdEvent::FootnoteReference(text) => {
                    self.push_span(
                        format!("[{text}]"),
                        self.inline_style()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::ITALIC),
                    );
                }
                MdEvent::TaskListMarker(checked) => {
                    let marker = if checked { "[x] " } else { "[ ] " };
                    self.push_span(marker.to_string(), self.inline_style().fg(Color::Green));
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
            Tag::BlockQuote(_) => {
                self.flush_line();
                self.quote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.flush_line();
                self.code_block = true;
                self.code_block_language = match kind {
                    CodeBlockKind::Indented => None,
                    CodeBlockKind::Fenced(language) => Some(language.into_string()),
                };
                if let Some(language) = &self.code_block_language {
                    self.lines.push(Line::from(Span::styled(
                        format!("```{language}"),
                        Style::default().fg(Color::Cyan),
                    )));
                }
            }
            Tag::List(start) => {
                self.flush_line();
                self.list_stack.push(ListState { next_index: start });
            }
            Tag::Item => {
                self.flush_line();
                self.pending_item_prefix = Some(self.next_item_prefix());
            }
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
                self.push_blank_line();
            }
            TagEnd::CodeBlock => {
                self.flush_line();
                self.code_block = false;
                self.code_block_language = None;
                self.lines.push(Line::from(Span::styled(
                    "```",
                    Style::default().fg(Color::Cyan),
                )));
                self.push_blank_line();
            }
            TagEnd::List(..) => {
                self.flush_line();
                self.list_stack.pop();
                self.push_blank_line();
            }
            TagEnd::Item => self.flush_line(),
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
                            .fg(Color::Blue)
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
                if !line.is_empty() {
                    let style = Style::default().fg(Color::Yellow).bg(Color::Black);
                    self.push_span(line.to_string(), style);
                }
                self.flush_line();
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

    fn push_span(&mut self, text: String, style: Style) {
        if self.current_spans.is_empty() {
            self.push_prefixes();
        }
        self.current_spans.push(Span::styled(text, style));
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
                format!("{} ", "│".repeat(self.quote_depth)),
                Style::default().fg(Color::Green),
            ));
        }
        if let Some(prefix) = self.pending_item_prefix.take() {
            self.current_spans
                .push(Span::styled(prefix, Style::default().fg(Color::Cyan)));
        }
    }

    fn next_item_prefix(&mut self) -> String {
        match self
            .list_stack
            .last_mut()
            .and_then(|state| state.next_index.as_mut())
        {
            Some(index) => {
                let prefix = format!("{index}. ");
                *index += 1;
                prefix
            }
            None => "• ".to_string(),
        }
    }

    fn inline_style(&self) -> Style {
        let mut style = Style::default().fg(Color::White);
        if let Some(level) = self.heading_level {
            style = style
                .fg(match level {
                    HeadingLevel::H1 => Color::Magenta,
                    HeadingLevel::H2 => Color::Cyan,
                    _ => Color::Yellow,
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
            style = style.fg(Color::Blue).add_modifier(Modifier::UNDERLINED);
        }
        style
    }
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
    use chrono::{TimeZone, Utc};
    use ratatui::style::{Color, Modifier};

    use crate::types::Message;

    use super::{
        build_scroll_indicator_lines, max_message_scroll, ordered_messages, render_markdown,
    };

    #[test]
    fn renders_markdown_headings_and_emphasis() {
        let lines = render_markdown("# Title\n\nThis is **bold** and *italic*.");
        assert!(
            lines
                .iter()
                .any(|line| line.spans.iter().any(|span| span.content == "Title"
                    && span.style.fg == Some(Color::Magenta)
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
                .any(|span| span.content == "fn main() {}" && span.style.fg == Some(Color::Yellow))
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
}
