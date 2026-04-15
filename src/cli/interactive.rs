use std::io;
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};
use serde::Deserialize;

use super::client::ApiClient;

const REFRESH_INTERVAL: Duration = Duration::from_secs(3);

// ── Command registry ────────────────────────────────────────────────────

#[derive(Clone)]
struct CommandEntry {
    path: String,
    description: String,
}

fn build_command_registry() -> Vec<CommandEntry> {
    use clap::CommandFactory;
    let mut entries = Vec::new();
    let app = super::Cli::command();
    for sub in app.get_subcommands() {
        let name = sub.get_name().to_string();
        // Skip commands that don't make sense inside interactive mode
        if name == "interactive" || name == "serve" || name == "version" {
            continue;
        }
        let about = sub
            .get_about()
            .map(|a| a.to_string())
            .unwrap_or_default();
        let has_subcommands = sub.get_subcommands().next().is_some();
        if has_subcommands {
            for subsub in sub.get_subcommands() {
                let sub_name = subsub.get_name();
                let sub_about = subsub
                    .get_about()
                    .map(|a| a.to_string())
                    .unwrap_or(about.clone());
                entries.push(CommandEntry {
                    path: format!("{} {}", name, sub_name),
                    description: sub_about,
                });
            }
        } else {
            entries.push(CommandEntry {
                path: name,
                description: about,
            });
        }
    }
    // Add interactive-only slash commands
    entries.push(CommandEntry {
        path: "/setup".to_string(),
        description: "Guided first-time setup wizard".to_string(),
    });
    entries.push(CommandEntry {
        path: "/clear".to_string(),
        description: "Clear transcript".to_string(),
    });
    entries.push(CommandEntry {
        path: "/help".to_string(),
        description: "Show all commands".to_string(),
    });
    entries.push(CommandEntry {
        path: "/exit".to_string(),
        description: "Exit interactive mode".to_string(),
    });
    entries
}

fn filter_suggestions(registry: &[CommandEntry], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let q = query.trim().to_lowercase();
    let mut scored: Vec<(usize, i32)> = registry
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let path = entry.path.to_lowercase();
            let score = if path == q {
                120
            } else if path.starts_with(&q) {
                100
            } else if path.contains(&q) {
                70
            } else if entry.description.to_lowercase().contains(&q) {
                40
            } else {
                0
            };
            (i, score)
        })
        .filter(|(_, s)| *s > 0)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().take(6).map(|(i, _)| i).collect()
}

// ── Overview (polled state) ─────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
struct Overview {
    endpoint: String,
    service_running: bool,
    tenants: Vec<TenantSummary>,
    agents: Vec<AgentSummary>,
    credentials: Vec<CredentialSummary>,
}

#[derive(Debug, Deserialize, Clone)]
struct TenantSummary {
    #[serde(default)]
    name: String,
    #[serde(default)]
    slug: String,
}
#[derive(Debug, Deserialize, Clone)]
struct AgentSummary {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    model_id: String,
    #[serde(default)]
    status: String,
}
#[derive(Debug, Deserialize, Clone)]
struct CredentialSummary {
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: String,
}

async fn fetch_overview(client: &ApiClient) -> Overview {
    let endpoint = crate::cli::local::default_endpoint();
    let service_running = client
        .get_raw("/healthz")
        .await
        .map(|s| s.is_success())
        .unwrap_or(false);
    let tenants: Vec<TenantSummary> = client.get("/api/tenants").await.unwrap_or_default();
    let agents: Vec<AgentSummary> = client
        .get("/api/tenants/default/agents")
        .await
        .unwrap_or_default();
    let credentials: Vec<CredentialSummary> = client
        .get("/api/tenants/default/credentials")
        .await
        .unwrap_or_default();
    Overview {
        endpoint,
        service_running,
        tenants,
        agents,
        credentials,
    }
}

// ── Chat response ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChatResponse {
    response: String,
    conversation_id: String,
    #[serde(default)]
    capabilities_used: Vec<String>,
}

// ── Transcript entry ────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Tone {
    System,
    Success,
    Warning,
    Info,
    User,
    Agent,
}

struct Entry {
    tone: Tone,
    label: String,
    lines: Vec<String>,
}

// ── App state ───────────────────────────────────────────────────────────

enum AppMode {
    Command,
    Chat {
        agent_name: String,
        agent_id: String,
        conversation_id: Option<String>,
    },
}

struct App {
    client: ApiClient,
    overview: Overview,
    registry: Vec<CommandEntry>,
    input: String,
    cursor: usize,
    transcript: Vec<Entry>,
    suggestions: Vec<usize>,
    selected_suggestion: usize,
    last_refresh: Instant,
    mode: AppMode,
    history: Vec<String>,
    history_idx: Option<usize>,
    scroll_offset: u16,
    should_auto_scroll: bool,
}

enum ExitAction {
    Quit,
    Dashboard,
    RunSetup,
}

impl App {
    async fn new(client: ApiClient) -> Self {
        let overview = fetch_overview(&client).await;
        let registry = build_command_registry();
        let is_fresh = overview.credentials.is_empty() && overview.agents.is_empty();

        let mut transcript = vec![Entry {
            tone: Tone::System,
            label: "hiveloom".to_string(),
            lines: if is_fresh {
                vec![
                    "Welcome! This looks like a fresh install.".to_string(),
                    "Type /setup to get started, or /help for all commands.".to_string(),
                ]
            } else {
                vec!["Ready. Type a command or /help for the full list.".to_string()]
            },
        }];

        if overview.service_running && !overview.agents.is_empty() {
            transcript.push(Entry {
                tone: Tone::Info,
                label: "tip".to_string(),
                lines: vec![format!(
                    "Try: chat {}",
                    overview.agents[0].name
                )],
            });
        }

        Self {
            client,
            overview,
            registry,
            input: String::new(),
            cursor: 0,
            transcript,
            suggestions: Vec::new(),
            selected_suggestion: 0,
            last_refresh: Instant::now(),
            mode: AppMode::Command,
            history: Vec::new(),
            history_idx: None,
            scroll_offset: 0,
            should_auto_scroll: true,
        }
    }

    async fn refresh(&mut self) {
        self.overview = fetch_overview(&self.client).await;
        self.last_refresh = Instant::now();
    }

    fn sync_suggestions(&mut self) {
        if matches!(self.mode, AppMode::Chat { .. }) {
            self.suggestions.clear();
            return;
        }
        self.suggestions = filter_suggestions(&self.registry, &self.input);
        if self.selected_suggestion >= self.suggestions.len() {
            self.selected_suggestion = 0;
        }
    }

    fn log(&mut self, tone: Tone, label: impl Into<String>, lines: Vec<String>) {
        self.transcript.push(Entry {
            tone,
            label: label.into(),
            lines,
        });
        if self.transcript.len() > 200 {
            self.transcript.drain(0..50);
        }
        self.should_auto_scroll = true;
    }
}

// ── Main entry ──────────────────────────────────────────────────────────

pub async fn run() -> anyhow::Result<()> {
    let client = ApiClient::new(None, None);
    let mut app = App::new(client).await;

    loop {
        let exit = run_tui(&mut app).await?;
        match exit {
            ExitAction::Quit => return Ok(()),
            ExitAction::Dashboard => {
                return super::top::run(super::top::TopArgs {
                    endpoint: None,
                    token: None,
                    interval: 2,
                })
                .await;
            }
            ExitAction::RunSetup => {
                run_setup(&mut app).await?;
                // Re-enter TUI
            }
        }
    }
}

// ── TUI loop ────────────────────────────────────────────────────────────

async fn run_tui(app: &mut App) -> anyhow::Result<ExitAction> {
    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = tui_loop(&mut terminal, app).await;

    terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn tui_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<ExitAction> {
    loop {
        if app.last_refresh.elapsed() >= REFRESH_INTERVAL {
            app.refresh().await;
        }

        terminal.draw(|f| render(f, app))?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match key.code {
            KeyCode::Esc => {
                if matches!(app.mode, AppMode::Chat { .. }) {
                    app.log(Tone::System, "chat", vec!["Exited chat mode.".to_string()]);
                    app.mode = AppMode::Command;
                    app.input.clear();
                    app.cursor = 0;
                    app.sync_suggestions();
                } else if app.input.is_empty() {
                    return Ok(ExitAction::Quit);
                } else {
                    app.input.clear();
                    app.cursor = 0;
                    app.sync_suggestions();
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(ExitAction::Quit);
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.transcript.clear();
                app.scroll_offset = 0;
            }
            KeyCode::PageUp => {
                app.scroll_offset = app.scroll_offset.saturating_add(10);
                app.should_auto_scroll = false;
            }
            KeyCode::PageDown => {
                app.scroll_offset = app.scroll_offset.saturating_sub(10);
                if app.scroll_offset == 0 {
                    app.should_auto_scroll = true;
                }
            }
            KeyCode::Up => {
                if !app.suggestions.is_empty() {
                    app.selected_suggestion = app.selected_suggestion.saturating_sub(1);
                } else if !app.history.is_empty() {
                    let idx = match app.history_idx {
                        Some(i) => i.saturating_sub(1),
                        None => app.history.len() - 1,
                    };
                    app.history_idx = Some(idx);
                    app.input = app.history[idx].clone();
                    app.cursor = app.input.len();
                }
            }
            KeyCode::Down => {
                if !app.suggestions.is_empty() {
                    if app.selected_suggestion + 1 < app.suggestions.len() {
                        app.selected_suggestion += 1;
                    }
                } else if let Some(idx) = app.history_idx {
                    if idx + 1 < app.history.len() {
                        app.history_idx = Some(idx + 1);
                        app.input = app.history[idx + 1].clone();
                        app.cursor = app.input.len();
                    } else {
                        app.history_idx = None;
                        app.input.clear();
                        app.cursor = 0;
                    }
                }
            }
            KeyCode::Left => {
                app.cursor = app.cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                if app.cursor < app.input.len() {
                    app.cursor += 1;
                }
            }
            KeyCode::Backspace => {
                if app.cursor > 0 {
                    app.input.remove(app.cursor - 1);
                    app.cursor -= 1;
                    app.sync_suggestions();
                }
            }
            KeyCode::Tab => {
                if let Some(&idx) = app.suggestions.get(app.selected_suggestion) {
                    app.input = app.registry[idx].path.clone();
                    app.cursor = app.input.len();
                    app.sync_suggestions();
                }
            }
            KeyCode::Enter => {
                if let Some(exit) = handle_enter(app).await? {
                    return Ok(exit);
                }
            }
            KeyCode::Char(c) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.input.insert(app.cursor, c);
                    app.cursor += 1;
                    app.sync_suggestions();
                    app.history_idx = None;
                }
            }
            _ => {}
        }
    }
}

// ── Input handling ──────────────────────────────────────────────────────

async fn handle_enter(app: &mut App) -> anyhow::Result<Option<ExitAction>> {
    let input = app.input.trim().to_string();
    if input.is_empty() {
        return Ok(None);
    }

    // Save to history
    if app.history.last().map(|h| h.as_str()) != Some(&input) {
        app.history.push(input.clone());
        if app.history.len() > 50 {
            app.history.remove(0);
        }
    }
    app.history_idx = None;
    app.input.clear();
    app.cursor = 0;
    app.sync_suggestions();

    // Chat mode: send message to agent
    if matches!(app.mode, AppMode::Chat { .. }) {
        if input == "/exit" {
            app.log(Tone::System, "chat", vec!["Exited chat mode.".to_string()]);
            app.mode = AppMode::Command;
            return Ok(None);
        }

        // Extract what we need before borrowing app mutably
        let (agent_name, agent_id, conv_id) = match &app.mode {
            AppMode::Chat { agent_name, agent_id, conversation_id } => {
                (agent_name.clone(), agent_id.clone(), conversation_id.clone())
            }
            _ => unreachable!(),
        };

        app.log(Tone::User, "you", vec![input.clone()]);

        let mut body = serde_json::json!({ "message": input });
        if let Some(ref cid) = conv_id {
            body["conversation_id"] = serde_json::Value::String(cid.clone());
        }

        let tid = app
            .overview
            .tenants
            .first()
            .map(|t| t.slug.as_str())
            .unwrap_or("default");

        match app
            .client
            .post::<_, ChatResponse>(
                &format!("/api/tenants/{}/agents/{}/chat", tid, agent_id),
                &body,
            )
            .await
        {
            Ok(resp) => {
                let new_cid = resp.conversation_id.clone();
                let mut lines = vec![resp.response];
                if !resp.capabilities_used.is_empty() {
                    lines.push(format!(
                        "  [capabilities: {}]",
                        resp.capabilities_used.join(", ")
                    ));
                }
                app.log(Tone::Agent, &agent_name, lines);
                // Update conversation_id
                if let AppMode::Chat { ref mut conversation_id, .. } = app.mode {
                    *conversation_id = Some(new_cid);
                }
            }
            Err(e) => {
                app.log(
                    Tone::Warning,
                    "error",
                    vec![format!("Chat failed: {}", e)],
                );
            }
        }
        return Ok(None);
    }

    // Slash commands (interactive-only)
    if input.starts_with('/') {
        return handle_slash_command(app, &input).await;
    }

    // Check if this is a "chat <agent>" command
    if let Some(agent_name) = input.strip_prefix("chat ") {
        let agent_name = agent_name.trim().to_string();
        return enter_chat_mode(app, &agent_name).await;
    }
    if input == "chat" {
        if let Some(agent) = app.overview.agents.first() {
            let name = agent.name.clone();
            return enter_chat_mode(app, &name).await;
        } else {
            app.log(
                Tone::Warning,
                "chat",
                vec!["No agents found. Create one first.".to_string()],
            );
            return Ok(None);
        }
    }

    // CLI command dispatch
    app.log(Tone::User, ">", vec![input.clone()]);
    dispatch_cli_command(app, &input).await;
    Ok(None)
}

async fn enter_chat_mode(app: &mut App, agent_name: &str) -> anyhow::Result<Option<ExitAction>> {
    // Find agent
    app.refresh().await;
    let agent = app
        .overview
        .agents
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(agent_name));
    match agent {
        Some(a) => {
            let name = a.name.clone();
            let id = a.id.clone();
            app.log(
                Tone::System,
                "chat",
                vec![
                    format!("Chatting with {}. Type /exit or Esc to return.", name),
                ],
            );
            app.mode = AppMode::Chat {
                agent_name: name,
                agent_id: id,
                conversation_id: None,
            };
        }
        None => {
            app.log(
                Tone::Warning,
                "chat",
                vec![format!("Agent '{}' not found.", agent_name)],
            );
        }
    }
    Ok(None)
}

async fn handle_slash_command(
    app: &mut App,
    input: &str,
) -> anyhow::Result<Option<ExitAction>> {
    let cmd = input.trim_start_matches('/').trim().to_lowercase();

    match cmd.as_str() {
        "setup" => return Ok(Some(ExitAction::RunSetup)),
        "exit" | "quit" | "q" => return Ok(Some(ExitAction::Quit)),
        "clear" => {
            app.transcript.clear();
            app.scroll_offset = 0;
        }
        "top" | "dashboard" => return Ok(Some(ExitAction::Dashboard)),
        "help" | "?" => {
            let mut lines = vec!["Commands (type without hiveloom prefix):".to_string()];
            for entry in &app.registry {
                if !entry.path.starts_with('/') {
                    lines.push(format!("  {:<30} {}", entry.path, entry.description));
                }
            }
            lines.push(String::new());
            lines.push("Slash commands:".to_string());
            for entry in &app.registry {
                if entry.path.starts_with('/') {
                    lines.push(format!("  {:<30} {}", entry.path, entry.description));
                }
            }
            app.log(Tone::System, "help", lines);
        }
        _ => {
            app.log(
                Tone::Warning,
                "unknown",
                vec![format!("Unknown slash command: /{}", cmd)],
            );
        }
    }
    Ok(None)
}

async fn dispatch_cli_command(app: &mut App, input: &str) {
    // Build synthetic argv and run through the CLI binary as a subprocess
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            app.log(Tone::Warning, "error", vec![format!("Cannot find executable: {}", e)]);
            return;
        }
    };

    let args: Vec<&str> = input.split_whitespace().collect();
    if args.is_empty() {
        return;
    }

    let result = ProcessCommand::new(&exe)
        .args(&args)
        .env(
            "HIVELOOM_ENDPOINT",
            &app.overview.endpoint,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let mut lines: Vec<String> = Vec::new();
            if !stdout.trim().is_empty() {
                for line in stdout.lines() {
                    lines.push(line.to_string());
                }
            }
            if !stderr.trim().is_empty() {
                for line in stderr.lines() {
                    lines.push(line.to_string());
                }
            }
            if lines.is_empty() {
                lines.push("(no output)".to_string());
            }
            // Truncate very long output
            if lines.len() > 100 {
                lines.truncate(100);
                lines.push("... (truncated)".to_string());
            }
            let tone = if output.status.success() {
                Tone::Info
            } else {
                Tone::Warning
            };
            app.log(tone, "result", lines);
        }
        Err(e) => {
            app.log(
                Tone::Warning,
                "error",
                vec![format!("Failed to execute: {}", e)],
            );
        }
    }
}

// ── TUI rendering ───────────────────────────────────────────────────────

fn render(f: &mut ratatui::Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // Title bar
            Constraint::Min(6),     // Transcript
            Constraint::Length(if app.suggestions.is_empty() { 0 } else { 2 }), // Suggestions
            Constraint::Length(3),  // Input
        ])
        .split(f.size());

    render_title(f, root[0], app);
    render_transcript(f, root[1], app);
    if !app.suggestions.is_empty() {
        render_suggestions(f, root[2], app);
    }
    render_input(f, root[3], app);
}

fn render_title(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let status_style = if app.overview.service_running {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Yellow)
    };
    let status_text = if app.overview.service_running {
        "online"
    } else {
        "offline"
    };

    let tenant = app
        .overview
        .tenants
        .first()
        .map(|t| t.slug.as_str())
        .unwrap_or("—");

    let mode_info = match &app.mode {
        AppMode::Command => String::new(),
        AppMode::Chat { agent_name, .. } => format!("  chatting with {}", agent_name),
    };

    let line1 = Line::from(vec![
        Span::styled(
            " ▸ hiveloom ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(status_text, status_style),
        Span::raw(format!(
            "  tenant:{}  agents:{}  creds:{}",
            tenant,
            app.overview.agents.len(),
            app.overview.credentials.len(),
        )),
        Span::styled(mode_info, Style::default().fg(Color::Magenta)),
    ]);

    let title = Paragraph::new(line1).block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(title, area);
}

fn render_transcript(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();

    for entry in &app.transcript {
        let label_style = match entry.tone {
            Tone::System => Style::default().fg(Color::Cyan),
            Tone::Success => Style::default().fg(Color::Green),
            Tone::Warning => Style::default().fg(Color::Yellow),
            Tone::Info => Style::default().fg(Color::Blue),
            Tone::User => Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            Tone::Agent => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        };

        if let Some(first) = entry.lines.first() {
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", entry.label), label_style),
                Span::raw(first.clone()),
            ]));
        }
        for line in entry.lines.iter().skip(1) {
            let indent = " ".repeat(entry.label.len() + 1);
            lines.push(Line::from(vec![
                Span::raw(indent),
                Span::raw(line.clone()),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Handle scrolling
    let visible_height = area.height as usize;
    let total = lines.len();
    let scroll = if app.should_auto_scroll {
        total.saturating_sub(visible_height) as u16
    } else {
        let max_scroll = total.saturating_sub(visible_height) as u16;
        max_scroll.saturating_sub(app.scroll_offset)
    };

    let transcript = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(transcript, area);
}

fn render_suggestions(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let mut spans = Vec::new();
    for (pos, &idx) in app.suggestions.iter().take(5).enumerate() {
        let entry = &app.registry[idx];
        let style = if pos == app.selected_suggestion {
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        if pos > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(&entry.path, style));
        spans.push(Span::styled(
            format!(" {}", truncate_str(&entry.description, 20)),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let suggestions = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(suggestions, area);
}

fn render_input(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let prompt = match &app.mode {
        AppMode::Command => "> ",
        AppMode::Chat { .. } => "you: ",
    };
    let prompt_style = match &app.mode {
        AppMode::Command => Style::default().fg(Color::Cyan),
        AppMode::Chat { .. } => Style::default().fg(Color::Green),
    };

    let content = if app.input.is_empty() {
        let placeholder = match &app.mode {
            AppMode::Command => "type a command, /help, or /setup...",
            AppMode::Chat { .. } => "type a message, /exit to return...",
        };
        Line::from(vec![
            Span::styled(prompt, prompt_style),
            Span::styled(placeholder, Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![
            Span::styled(prompt, prompt_style),
            Span::raw(app.input.clone()),
        ])
    };

    let input = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::TOP)
            .title(" Tab:complete  Enter:run  Esc:back  Ctrl-C:quit ")
            .title_alignment(Alignment::Right)
            .title_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(input, area);

    let cursor_x = area.x + prompt.len() as u16 + app.cursor as u16;
    let cursor_y = area.y + 1;
    f.set_cursor(cursor_x, cursor_y);
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

// ── Setup wizard ────────────────────────────────────────────────────────

async fn run_setup(app: &mut App) -> anyhow::Result<()> {
    use std::io::Write;

    println!();
    println!("  === Hiveloom Setup ===");
    println!();

    // Step 1: Start service
    app.refresh().await;
    if !app.overview.service_running {
        println!("  [1/5] Starting service...");
        let exe = std::env::current_exe()?;
        let data_dir = crate::cli::local::default_data_dir();
        let logs_dir = std::path::PathBuf::from(&data_dir).join("logs");
        std::fs::create_dir_all(&logs_dir)?;
        let log_path = logs_dir.join("service.log");
        let stdout = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let stderr = stdout.try_clone()?;
        let child = ProcessCommand::new(exe)
            .arg("serve")
            .env("HIVELOOM_DATA_DIR", &data_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()?;
        println!("  Started service (pid {})", child.id());
        tokio::time::sleep(Duration::from_millis(1500)).await;
        app.refresh().await;
        if !app.overview.service_running {
            println!("  Service may still be starting. Waiting...");
            tokio::time::sleep(Duration::from_millis(2000)).await;
            app.refresh().await;
        }
        if app.overview.service_running {
            println!("  ✓ Service is running at {}", app.overview.endpoint);
        } else {
            println!("  ✗ Service failed to start. Check logs at {}", log_path.display());
            return Ok(());
        }
    } else {
        println!("  [1/5] ✓ Service already running at {}", app.overview.endpoint);
    }

    // Step 2: API key
    app.refresh().await;
    if app.overview.credentials.is_empty() {
        println!();
        println!("  [2/5] Enter your LLM API key:");
        println!("        Anthropic keys start with sk-ant-...");
        println!("        OpenAI keys start with sk-...");
        println!();
        print!("  API Key: ");
        io::stdout().flush()?;
        let mut key = String::new();
        io::stdin().read_line(&mut key)?;
        let key = key.trim();
        if key.is_empty() {
            println!("  Skipped. You can set it later with: credential set anthropic");
        } else {
            let cred_name = if key.starts_with("sk-ant-") { "anthropic" } else { "openai" };
            let body = serde_json::json!({ "name": cred_name, "kind": "static", "value": key });
            match app
                .client
                .post::<_, serde_json::Value>("/api/tenants/default/credentials", &body)
                .await
            {
                Ok(_) => println!("  ✓ Stored credential '{}'", cred_name),
                Err(e) => println!("  ✗ Failed: {}", e),
            }
        }
    } else {
        println!("  [2/5] ✓ Credential already stored: {}", app.overview.credentials[0].name);
    }

    // Step 3: Create agent
    app.refresh().await;
    if app.overview.agents.is_empty() {
        println!();
        print!("  [3/5] Agent name [support-bot]: ");
        io::stdout().flush()?;
        let mut name = String::new();
        io::stdin().read_line(&mut name)?;
        let name = name.trim();
        let name = if name.is_empty() { "support-bot" } else { name };

        let model = if app.overview.credentials.iter().any(|c| c.name == "anthropic") {
            "claude-sonnet-4-20250514"
        } else {
            "gpt-4o"
        };

        print!("  System prompt [You are a helpful assistant.]: ");
        io::stdout().flush()?;
        let mut prompt = String::new();
        io::stdin().read_line(&mut prompt)?;
        let prompt = prompt.trim();
        let prompt = if prompt.is_empty() { "You are a helpful assistant." } else { prompt };

        let body = serde_json::json!({
            "name": name, "model_id": model,
            "system_prompt": prompt, "scope_mode": "dual",
        });
        match app
            .client
            .post::<_, serde_json::Value>("/api/tenants/default/agents", &body)
            .await
        {
            Ok(_) => println!("  ✓ Agent '{}' created with model {}", name, model),
            Err(e) => println!("  ✗ Failed: {}", e),
        }
    } else {
        println!("  [3/5] ✓ Agent already exists: {}", app.overview.agents[0].name);
    }

    // Step 4: Create MCP identity
    app.refresh().await;
    if !app.overview.agents.is_empty() {
        let agent = &app.overview.agents[0];
        println!();
        print!("  [4/5] Create MCP identity? (name) [desktop-user]: ");
        io::stdout().flush()?;
        let mut id_name = String::new();
        io::stdin().read_line(&mut id_name)?;
        let id_name = id_name.trim();
        let id_name = if id_name.is_empty() { "desktop-user" } else { id_name };

        let body = serde_json::json!({
            "name": id_name, "agent_slug": agent.name,
        });
        match app
            .client
            .post::<_, serde_json::Value>(
                "/api/tenants/default/mcp-identities",
                &body,
            )
            .await
        {
            Ok(resp) => {
                if let Some(setup_code) = resp.get("setup_code").and_then(|v| v.as_str()) {
                    println!("  ✓ MCP identity created");
                    println!();
                    println!("  MCP URL:     {}/mcp/default/{}", app.overview.endpoint, agent.name);
                    println!("  Setup code:  {}", setup_code);
                    println!();
                    println!("  Add the URL to Claude Desktop or any MCP client.");
                    println!("  Enter the setup code when prompted.");
                } else {
                    println!("  ✓ MCP identity '{}' created", id_name);
                }
            }
            Err(e) => println!("  ✗ Failed: {}", e),
        }
    }

    // Step 5: Test chat
    app.refresh().await;
    if !app.overview.agents.is_empty() {
        let agent = &app.overview.agents[0];
        println!();
        println!("  [5/5] Test chat with {}:", agent.name);
        print!("  you: ");
        io::stdout().flush()?;
        let mut msg = String::new();
        io::stdin().read_line(&mut msg)?;
        let msg = msg.trim();
        if !msg.is_empty() {
            let body = serde_json::json!({ "message": msg });
            match app
                .client
                .post::<_, ChatResponse>(
                    &format!("/api/tenants/default/agents/{}/chat", agent.id),
                    &body,
                )
                .await
            {
                Ok(resp) => {
                    println!("  {}: {}", agent.name, resp.response);
                    if !resp.capabilities_used.is_empty() {
                        println!("  [capabilities: {}]", resp.capabilities_used.join(", "));
                    }
                }
                Err(e) => println!("  ✗ Chat failed: {}", e),
            }
        }
    }

    println!();
    println!("  Setup complete! Returning to interactive shell...");
    println!();
    tokio::time::sleep(Duration::from_secs(1)).await;
    app.refresh().await;
    app.log(
        Tone::Success,
        "setup",
        vec!["Setup complete. Try 'chat' to talk to your agent.".to_string()],
    );

    Ok(())
}
