use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeferredAction {
    Dashboard,
}

#[derive(Clone, Copy)]
struct SlashCommand {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
}

const COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "start",
        title: "Start service",
        description: "Launch the local Hiveloom service in the background.",
        aliases: &["serve", "boot", "run service", "start service"],
    },
    SlashCommand {
        name: "health",
        title: "Health",
        description: "Check whether the local instance responds on /healthz.",
        aliases: &["ping", "ready", "check health"],
    },
    SlashCommand {
        name: "status",
        title: "Status",
        description: "Summarize tenants, agents, credentials, backups, and endpoint state.",
        aliases: &["summary", "overview", "what next", "show status"],
    },
    SlashCommand {
        name: "agents",
        title: "Agents",
        description: "List the current agents in the default tenant.",
        aliases: &["list agents", "agent list", "show agents"],
    },
    SlashCommand {
        name: "credentials",
        title: "Credentials",
        description: "List stored provider credentials without exposing secret values.",
        aliases: &["credentials list", "creds", "keys", "show credentials"],
    },
    SlashCommand {
        name: "backups",
        title: "Backups",
        description: "List backup archives recorded for the local instance.",
        aliases: &["backup list", "archives", "show backups"],
    },
    SlashCommand {
        name: "doctor",
        title: "Doctor",
        description: "Run local filesystem and store checks against the active data dir.",
        aliases: &["diag", "diagnostics", "run doctor"],
    },
    SlashCommand {
        name: "create-agent",
        title: "Create agent",
        description: "Show the next recommended agent command based on current setup.",
        aliases: &["new agent", "agent create", "create an agent"],
    },
    SlashCommand {
        name: "top",
        title: "Live dashboard",
        description: "Open `hiveloom top` for the live terminal dashboard.",
        aliases: &["dashboard", "monitor", "open dashboard"],
    },
    SlashCommand {
        name: "help",
        title: "Help",
        description: "Show command examples and launcher shortcuts.",
        aliases: &["commands", "menu", "show help"],
    },
    SlashCommand {
        name: "quit",
        title: "Quit",
        description: "Leave the interactive CLI.",
        aliases: &["exit", "close", "quit interactive"],
    },
];

#[derive(Debug, Default, Clone)]
struct Overview {
    endpoint: String,
    service_running: bool,
    tenants: Vec<TenantSummary>,
    agents: Vec<AgentSummary>,
    credentials: Vec<CredentialSummary>,
    backups: Vec<BackupSummary>,
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

#[derive(Debug, Deserialize, Clone)]
struct BackupSummary {
    #[serde(default)]
    filename: String,
    #[serde(default)]
    size_bytes: u64,
}

#[derive(Clone, Copy)]
enum EntryTone {
    System,
    Success,
    Warning,
    Info,
}

struct TranscriptEntry {
    tone: EntryTone,
    title: String,
    body: Vec<String>,
}

struct InteractiveApp {
    client: ApiClient,
    overview: Overview,
    input: String,
    cursor: usize,
    transcript: Vec<TranscriptEntry>,
    suggestions: Vec<usize>,
    selected_suggestion: usize,
    last_refresh: Instant,
}

enum InteractiveExit {
    Quit,
    Run(DeferredAction),
}

pub async fn run() -> anyhow::Result<()> {
    let client = ApiClient::new(None, None);
    let mut app = InteractiveApp::new(client).await;
    let exit = run_shell(&mut app).await?;

    match exit {
        InteractiveExit::Quit => Ok(()),
        InteractiveExit::Run(DeferredAction::Dashboard) => {
            super::top::run(super::top::TopArgs {
                endpoint: None,
                token: None,
                interval: 2,
            })
            .await
        }
    }
}

impl InteractiveApp {
    async fn new(client: ApiClient) -> Self {
        let overview = fetch_overview(&client).await;
        let transcript = vec![TranscriptEntry {
            tone: EntryTone::System,
            title: "ready".to_string(),
            body: vec![
                "Type a full command or plain request. Tab completes. Use `/help` for the command list."
                    .to_string(),
            ],
        }];

        Self {
            client,
            overview,
            input: String::new(),
            cursor: 0,
            transcript,
            suggestions: Vec::new(),
            selected_suggestion: 0,
            last_refresh: Instant::now(),
        }
    }

    async fn refresh(&mut self) {
        self.overview = fetch_overview(&self.client).await;
        self.last_refresh = Instant::now();
        self.sync_suggestions();
    }

    fn sync_suggestions(&mut self) {
        if self.input.trim().is_empty() {
            self.suggestions.clear();
            self.selected_suggestion = 0;
            return;
        }
        self.suggestions = ranked_commands(&self.input);
        if self.selected_suggestion >= self.suggestions.len() {
            self.selected_suggestion = 0;
        }
    }

    fn log(&mut self, tone: EntryTone, title: impl Into<String>, body: Vec<String>) {
        self.transcript.push(TranscriptEntry {
            tone,
            title: title.into(),
            body,
        });
        if self.transcript.len() > 12 {
            let excess = self.transcript.len() - 12;
            self.transcript.drain(0..excess);
        }
    }
}

async fn run_shell(app: &mut InteractiveApp) -> anyhow::Result<InteractiveExit> {
    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = shell_loop(&mut terminal, app).await;

    terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn shell_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    app: &mut InteractiveApp,
) -> anyhow::Result<InteractiveExit> {
    loop {
        if app.last_refresh.elapsed() >= REFRESH_INTERVAL {
            app.refresh().await;
        }

        terminal.draw(|f| render_shell(f, app))?;

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
                if app.input.is_empty() {
                    return Ok(InteractiveExit::Quit);
                }
                app.input.clear();
                app.cursor = 0;
                app.sync_suggestions();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(InteractiveExit::Quit);
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.transcript.clear();
                app.log(
                    EntryTone::System,
                    "cleared",
                    vec![
                        "Transcript cleared. Type `/help` if you want a quick refresher."
                            .to_string(),
                    ],
                );
            }
            KeyCode::Up => {
                if app.selected_suggestion > 0 {
                    app.selected_suggestion -= 1;
                }
            }
            KeyCode::Down => {
                if app.selected_suggestion + 1 < app.suggestions.len() {
                    app.selected_suggestion += 1;
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
                autocomplete(app);
            }
            KeyCode::Enter => {
                if let Some(exit) = submit_input(app).await? {
                    return Ok(exit);
                }
            }
            KeyCode::Char(c) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.input.insert(app.cursor, c);
                    app.cursor += 1;
                    app.sync_suggestions();
                }
            }
            _ => {}
        }
    }
}

async fn submit_input(app: &mut InteractiveApp) -> anyhow::Result<Option<InteractiveExit>> {
    let query = app.input.trim().to_string();
    if query.is_empty() {
        return Ok(None);
    }

    app.log(EntryTone::Info, "you", vec![query.clone()]);
    app.input.clear();
    app.cursor = 0;
    app.sync_suggestions();

    let resolution = resolve_command(&query);
    let (command, guessed) = match resolution {
        Some(value) => value,
        None => {
            app.log(
                EntryTone::Warning,
                "not sure",
                vec![
                    "I couldn't map that to a strong command guess.".to_string(),
                    "Try `/help`, or start with `/health`, `/status`, `/agents`, `/doctor`, or `/top`."
                        .to_string(),
                ],
            );
            return Ok(None);
        }
    };

    if guessed {
        app.log(
            EntryTone::System,
            "interpreted",
            vec![format!("Treating that as `/{}`.", command.name)],
        );
    }

    execute_command(app, command).await
}

async fn execute_command(
    app: &mut InteractiveApp,
    command: &'static SlashCommand,
) -> anyhow::Result<Option<InteractiveExit>> {
    match command.name {
        "start" => {
            let message = start_local_service()?;
            app.log(EntryTone::Success, "service", vec![message]);
            tokio::time::sleep(Duration::from_millis(500)).await;
            app.refresh().await;
        }
        "health" => {
            app.refresh().await;
            let message = if app.overview.service_running {
                "Local service is healthy."
            } else {
                "Local service is not reachable."
            };
            app.log(
                if app.overview.service_running {
                    EntryTone::Success
                } else {
                    EntryTone::Warning
                },
                "health",
                vec![
                    message.to_string(),
                    format!("Endpoint: {}", app.overview.endpoint),
                ],
            );
        }
        "status" => {
            app.refresh().await;
            let tenant_label = app
                .overview
                .tenants
                .first()
                .map(|t| format!("{} ({})", t.name, t.slug))
                .unwrap_or_else(|| "not provisioned".to_string());
            let recommendations = build_recommendations(&app.overview);
            let mut body = vec![format!(
                "{}  tenant:{}  agents:{}  creds:{}  backups:{}",
                if app.overview.service_running {
                    "online"
                } else {
                    "offline"
                },
                tenant_label,
                app.overview.agents.len(),
                app.overview.credentials.len(),
                app.overview.backups.len(),
            )];
            if let Some(next) = recommendations.first() {
                body.push(format!("next: {}", next));
            }
            app.log(EntryTone::System, "status", body);
        }
        "agents" => {
            app.refresh().await;
            let body = if app.overview.agents.is_empty() {
                vec![
                    "No agents found.".to_string(),
                    "Create a credential first, then run `hiveloom agent create ...`.".to_string(),
                ]
            } else {
                app.overview
                    .agents
                    .iter()
                    .map(|agent| {
                        format!("{}  |  {}  |  {}", agent.name, agent.model_id, agent.status)
                    })
                    .collect()
            };
            app.log(EntryTone::Info, "agents", body);
        }
        "credentials" => {
            app.refresh().await;
            let body = if app.overview.credentials.is_empty() {
                vec![
                    "No credentials stored yet.".to_string(),
                    "Suggested command: hiveloom credential set anthropic-key --kind static --from-env ANTHROPIC_API_KEY"
                        .to_string(),
                ]
            } else {
                app.overview
                    .credentials
                    .iter()
                    .map(|cred| format!("{}  |  {}", cred.name, cred.kind))
                    .collect()
            };
            app.log(EntryTone::Info, "credentials", body);
        }
        "backups" => {
            app.refresh().await;
            let body = if app.overview.backups.is_empty() {
                vec![
                    "No backups recorded yet.".to_string(),
                    "Suggested command: hiveloom backup create --tenant default --output default-backup.tar.gz"
                        .to_string(),
                ]
            } else {
                app.overview
                    .backups
                    .iter()
                    .map(|b| format!("{}  |  {} bytes", file_label(&b.filename), b.size_bytes))
                    .collect()
            };
            app.log(EntryTone::Info, "backups", body);
        }
        "doctor" => {
            let data_dir = crate::cli::local::default_data_dir();
            app.log(EntryTone::System, "doctor", run_doctor_summary(&data_dir)?);
        }
        "create-agent" => {
            app.refresh().await;
            let body = if !app.overview.service_running {
                vec![
                    "Start the service first so agent commands can reach the admin API."
                        .to_string(),
                    "Use `/start`.".to_string(),
                ]
            } else if app.overview.credentials.is_empty() {
                vec![
                    "Create a provider credential first.".to_string(),
                    "Suggested command: hiveloom credential set anthropic-key --kind static --from-env ANTHROPIC_API_KEY"
                        .to_string(),
                ]
            } else {
                vec![
                    "Suggested next command:".to_string(),
                    "hiveloom agent create --name support-bot --model claude-sonnet-4-5-20250514 --system-prompt \"You are a helpful assistant.\" --scope-mode dual"
                        .to_string(),
                    format!("Using credential already present: {}", app.overview.credentials[0].name),
                ]
            };
            app.log(EntryTone::Success, "create-agent", body);
        }
        "top" => return Ok(Some(InteractiveExit::Run(DeferredAction::Dashboard))),
        "help" => {
            let body = COMMANDS
                .iter()
                .map(|cmd| format!("/{:<12} {}", cmd.name, cmd.description))
                .collect::<Vec<_>>();
            app.log(EntryTone::System, "help", {
                let mut lines = vec![
                    "Use full slash commands or plain requests.".to_string(),
                    "Examples: `/health`, `/status`, `/agents`, `/create-agent`, `/top`"
                        .to_string(),
                ];
                lines.extend(body);
                lines
            });
        }
        "quit" => return Ok(Some(InteractiveExit::Quit)),
        _ => {}
    }

    Ok(None)
}

fn render_shell(f: &mut ratatui::Frame, app: &InteractiveApp) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.size());

    render_header(f, root[0], app);
    render_transcript(f, root[1], app);
    render_suggestions_bar(f, root[2], app);
    render_composer(f, root[3], app);
    render_footer(f, root[4]);
}

fn render_header(f: &mut ratatui::Frame, area: Rect, app: &InteractiveApp) {
    let status = if app.overview.service_running {
        "online"
    } else {
        "offline"
    };
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "hiveloom",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            status,
            Style::default().fg(if app.overview.service_running {
                Color::Green
            } else {
                Color::Yellow
            }),
        ),
    ]));
    f.render_widget(header, area);
}

fn render_transcript(f: &mut ratatui::Frame, area: Rect, app: &InteractiveApp) {
    let max_lines = area.height as usize;
    let mut lines = Vec::new();

    for entry in &app.transcript {
        let label_style = match entry.tone {
            EntryTone::System => Style::default().fg(Color::Cyan),
            EntryTone::Success => Style::default().fg(Color::Green),
            EntryTone::Warning => Style::default().fg(Color::Yellow),
            EntryTone::Info => Style::default().fg(Color::Blue),
        }
        .add_modifier(Modifier::BOLD);

        if let Some(first) = entry.body.first() {
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", entry.title), label_style),
                Span::raw(first.clone()),
            ]));
        }
        for body in entry.body.iter().skip(1) {
            lines.push(Line::from(vec![Span::raw("  "), Span::raw(body.clone())]));
        }
        lines.push(Line::from(""));
    }

    let lines = if lines.len() > max_lines {
        lines.split_off(lines.len() - max_lines)
    } else {
        lines
    };

    let transcript = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(transcript, area);
}

fn render_composer(f: &mut ratatui::Frame, area: Rect, app: &InteractiveApp) {
    let content = if app.input.is_empty() {
        Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::styled(
                "try: /health, /status, /agents, /create-agent, /top",
                Style::default().fg(Color::DarkGray),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(app.input.clone()),
        ])
    };
    let composer = Paragraph::new(content).block(Block::default().borders(Borders::TOP));
    f.render_widget(composer, area);
    let cursor_x = area.x + 3 + app.cursor as u16;
    let cursor_y = area.y + 1;
    f.set_cursor(cursor_x, cursor_y);
}

fn render_footer(f: &mut ratatui::Frame, area: Rect) {
    let footer = Paragraph::new("Tab complete  Enter run  /help commands  Esc clear  Ctrl-C quit")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(footer, area);
}

fn render_suggestions_bar(f: &mut ratatui::Frame, area: Rect, app: &InteractiveApp) {
    let line = if app.input.trim().is_empty() {
        Line::from(vec![
            Span::styled("commands ", Style::default().fg(Color::DarkGray)),
            Span::styled("/health", Style::default().fg(Color::Cyan)),
            Span::styled(" check service  ", Style::default().fg(Color::DarkGray)),
            Span::styled("/status", Style::default().fg(Color::Cyan)),
            Span::styled(" show summary  ", Style::default().fg(Color::DarkGray)),
            Span::styled("/create-agent", Style::default().fg(Color::Cyan)),
            Span::styled(" next setup step", Style::default().fg(Color::DarkGray)),
        ])
    } else if app.suggestions.is_empty() {
        Line::from(vec![Span::styled(
            "No close command match yet. Try `/help` for the full command list.",
            Style::default().fg(Color::DarkGray),
        )])
    } else {
        let mut spans = vec![Span::styled(
            "suggestions ",
            Style::default().fg(Color::DarkGray),
        )];

        for (position, idx) in app.suggestions.iter().take(3).enumerate() {
            let cmd = &COMMANDS[*idx];
            let style = if position == app.selected_suggestion {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(18, 35, 49))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };

            if position > 0 {
                spans.push(Span::styled("  ", Style::default().fg(Color::DarkGray)));
            }

            spans.push(Span::styled(format!("/{}", cmd.name), style));
            spans.push(Span::styled(
                format!(" {}", cmd.title),
                Style::default().fg(Color::DarkGray),
            ));
        }

        Line::from(spans)
    };

    let suggestions = Paragraph::new(line).block(Block::default().borders(Borders::TOP));
    f.render_widget(suggestions, area);
}

fn autocomplete(app: &mut InteractiveApp) {
    if let Some(idx) = app.suggestions.first().copied() {
        let command = &COMMANDS[idx];
        app.input = format!("/{}", command.name);
        app.cursor = app.input.len();
        app.sync_suggestions();
    }
}

fn resolve_command(query: &str) -> Option<(&'static SlashCommand, bool)> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "?" {
        return COMMANDS
            .iter()
            .find(|cmd| cmd.name == "help")
            .map(|cmd| (cmd, false));
    }

    if let Some(name) = trimmed.strip_prefix('/') {
        let name = name.trim().to_ascii_lowercase();
        return COMMANDS
            .iter()
            .find(|cmd| cmd.name == name || cmd.aliases.iter().any(|alias| *alias == name))
            .map(|cmd| (cmd, false));
    }

    let ranked = ranked_commands(query);
    let best = ranked.first().copied()?;
    let score = score_command(query, &COMMANDS[best]);
    if score >= 60 {
        Some((&COMMANDS[best], true))
    } else if query.to_ascii_lowercase().contains("what")
        || query.to_ascii_lowercase().contains("next")
    {
        COMMANDS
            .iter()
            .find(|cmd| cmd.name == "status")
            .map(|cmd| (cmd, true))
    } else {
        None
    }
}

fn ranked_commands(query: &str) -> Vec<usize> {
    let mut scored: Vec<(usize, i32)> = COMMANDS
        .iter()
        .enumerate()
        .map(|(idx, cmd)| (idx, score_command(query, cmd)))
        .filter(|(_, score)| *score > 0)
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    scored.into_iter().map(|(idx, _)| idx).collect()
}

fn score_command(query: &str, command: &SlashCommand) -> i32 {
    let q = query.trim().trim_start_matches('/').to_ascii_lowercase();
    if q.is_empty() {
        return 1;
    }

    let title = command.title.to_ascii_lowercase();
    let desc = command.description.to_ascii_lowercase();

    if command.name == q {
        return 120;
    }
    if command.name.starts_with(&q) {
        return 110;
    }
    if command
        .aliases
        .iter()
        .any(|alias| alias.eq_ignore_ascii_case(&q))
    {
        return 105;
    }
    if command.aliases.iter().any(|alias| alias.starts_with(&q)) {
        return 98;
    }
    if title.starts_with(&q) {
        return 92;
    }
    if title.contains(&q) {
        return 84;
    }
    if command.aliases.iter().any(|alias| alias.contains(&q)) {
        return 78;
    }
    if desc.contains(&q) {
        return 62;
    }
    if q.contains("next") && command.name == "status" {
        return 95;
    }
    if q.contains("agent") && command.name == "agents" {
        return 88;
    }
    if q.contains("backup") && command.name == "backups" {
        return 88;
    }
    0
}

async fn fetch_overview(client: &ApiClient) -> Overview {
    let endpoint = crate::cli::local::default_endpoint();
    let service_running = client
        .get_raw("/healthz")
        .await
        .map(|status| status.is_success())
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
    let backups: Vec<BackupSummary> = client.get("/api/backups").await.unwrap_or_default();

    Overview {
        endpoint,
        service_running,
        tenants,
        agents,
        credentials,
        backups,
    }
}

fn build_recommendations(overview: &Overview) -> Vec<String> {
    let mut items = Vec::new();

    if !overview.service_running {
        items.push("Start the local service with `/start`.".to_string());
    }
    if overview.credentials.is_empty() {
        items.push("Store a provider credential before creating agents.".to_string());
    }
    if overview.agents.is_empty() {
        items.push("Create your first agent with `/create-agent` guidance.".to_string());
    } else {
        items.push("Use `/top` for the live dashboard or ask for `show agents`.".to_string());
    }
    if overview.backups.is_empty() {
        items.push("Create a backup once the instance looks good.".to_string());
    }

    if items.is_empty() {
        items.push(
            "Everything basic is in place. `/top` or `/status` are the best next moves."
                .to_string(),
        );
    }

    items
}

fn run_doctor_summary(data_dir: &str) -> anyhow::Result<Vec<String>> {
    let path = Path::new(data_dir);
    let mut lines = Vec::new();
    lines.push(format!("Data dir: {}", data_dir));
    lines.push(if path.exists() {
        "PASS data directory exists".to_string()
    } else {
        "FAIL data directory does not exist".to_string()
    });

    let key = path.join("master.key");
    lines.push(if key.exists() {
        "PASS master.key present".to_string()
    } else {
        "WARN master.key missing".to_string()
    });

    let db = path.join("platform.db");
    lines.push(if db.exists() {
        "PASS platform.db present".to_string()
    } else {
        "WARN platform.db missing".to_string()
    });

    let tenants = path.join("tenants");
    if tenants.exists() {
        let count = std::fs::read_dir(&tenants)?.filter_map(Result::ok).count();
        lines.push(format!("PASS tenant stores found: {}", count));
    } else {
        lines.push("WARN tenant stores missing".to_string());
    }

    Ok(lines)
}

fn start_local_service() -> anyhow::Result<String> {
    let data_dir = crate::cli::local::default_data_dir();
    let endpoint = crate::cli::local::default_endpoint();
    let pid_path = PathBuf::from(&data_dir).join("run").join("service.pid");

    if let Ok(pid) = std::fs::read_to_string(&pid_path) {
        let pid = pid.trim();
        if !pid.is_empty() && process_exists(pid) {
            return Ok(format!(
                "Service already appears to be running with pid {}.",
                pid
            ));
        }
    }

    let exe = std::env::current_exe()?;
    let logs_dir = PathBuf::from(&data_dir).join("logs");
    std::fs::create_dir_all(&logs_dir)?;
    let log_path = logs_dir.join("service.log");
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let stderr = stdout.try_clone()?;

    let child = Command::new(exe)
        .arg("serve")
        .env("HIVELOOM_DATA_DIR", &data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()?;

    Ok(format!(
        "Started local service in the background (pid {}) at {}. Logs: {}",
        child.id(),
        endpoint,
        log_path.display()
    ))
}

fn process_exists(pid: &str) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}
