use std::io;
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use serde::Deserialize;

use super::client::ApiClient;
use super::tui::{
    chat_view::{self, ChatMessage},
    command_bar, context_panel, form,
    form::{FieldKind, FormField, FormState},
    menu::{self, MenuItem},
    popup::{self, PopupItem},
    status_bar::{self, StatusInfo},
    theme, wizard,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

// ── Data types ──────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
struct Overview {
    endpoint: String,
    service_running: bool,
    tenants: Vec<TenantInfo>,
    agents: Vec<AgentInfo>,
    credentials: Vec<CredentialInfo>,
    mcp_identities: Vec<McpInfo>,
}

#[derive(Debug, Deserialize, Clone)]
struct TenantInfo { #[serde(default)] slug: String }
#[derive(Debug, Deserialize, Clone)]
struct AgentInfo {
    #[serde(default)] id: String,
    #[serde(default)] name: String,
    #[serde(default)] model_id: String,
    #[serde(default)] status: String,
}
#[derive(Debug, Deserialize, Clone)]
struct CredentialInfo { #[serde(default)] name: String, #[serde(default)] kind: String }
#[derive(Debug, Deserialize, Clone)]
struct McpInfo {
    #[serde(default)] id: String,
    #[serde(default)] name: String,
    #[serde(default)] status: String,
    #[serde(default)] agent_id: Option<String>,
}
#[derive(Debug, Deserialize, Clone)]
struct ChatResponse {
    response: String,
    conversation_id: String,
    #[serde(default)] capabilities_used: Vec<String>,
}

// ── Screen state machine ────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Category { Setup, Agents, Chat, Credentials, Mcp, System }

impl Category {
    fn label(&self) -> &str {
        match self {
            Self::Setup => "Setup",
            Self::Agents => "Agents",
            Self::Chat => "Chat",
            Self::Credentials => "Credentials",
            Self::Mcp => "MCP",
            Self::System => "System",
        }
    }
    fn description(&self) -> &str {
        match self {
            Self::Setup => "Get started with guided setup",
            Self::Agents => "Create and manage AI agents",
            Self::Chat => "Talk to your agents",
            Self::Credentials => "API keys and secrets",
            Self::Mcp => "External client access",
            Self::System => "Health, backups, logs",
        }
    }
    fn all() -> &'static [Category] {
        &[Self::Setup, Self::Agents, Self::Chat, Self::Credentials, Self::Mcp, Self::System]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelFocus { Actions, Context }

enum Screen {
    MainMenu { selected: usize },
    Submenu {
        category: Category,
        actions: Vec<String>,
        action_idx: usize,
        item_idx: usize,
        focus: PanelFocus,
    },
    Form {
        parent: Category,
        title: String,
        form: FormState,
        form_kind: FormKind,
    },
    Popup {
        parent: Category,
        item_id: String,
        item_name: String,
        items: Vec<PopupItem>,
        selected: usize,
        anchor_y: u16,
    },
    ChatMode {
        agent_name: String,
        agent_id: String,
        conversation_id: Option<String>,
        messages: Vec<ChatMessage>,
        input: String,
        cursor: usize,
    },
    Wizard {
        step: usize,
        form: Option<FormState>,
        info_lines: Vec<String>,
    },
    CmdBar {
        parent_selected: usize,
        input: String,
        cursor: usize,
        suggestions: Vec<String>,
    },
}

#[derive(Clone)]
enum FormKind {
    CreateAgent,
    SetCredential,
    CreateMcpIdentity,
    AddSkill { agent_id: String },
}

// ── App ─────────────────────────────────────────────────────────────────

struct App {
    client: ApiClient,
    overview: Overview,
    screen: Screen,
    last_refresh: Instant,
    status_msg: Option<(String, bool)>, // (message, is_success)
    cmd_registry: Vec<String>,
}

impl App {
    async fn new(client: ApiClient) -> Self {
        let overview = fetch_overview(&client).await;
        let is_fresh = overview.credentials.is_empty() && overview.agents.is_empty();
        let cmd_registry = build_cmd_registry();
        Self {
            client,
            overview,
            screen: Screen::MainMenu { selected: if is_fresh { 0 } else { 1 } },
            last_refresh: Instant::now(),
            status_msg: if is_fresh {
                Some(("Welcome! Select Setup to get started.".to_string(), true))
            } else {
                None
            },
            cmd_registry,
        }
    }
    async fn refresh(&mut self) {
        self.overview = fetch_overview(&self.client).await;
        self.last_refresh = Instant::now();
    }
    fn status_info(&self, breadcrumb: Option<&str>) -> StatusInfo {
        StatusInfo {
            service_running: self.overview.service_running,
            agent_count: self.overview.agents.len(),
            credential_count: self.overview.credentials.len(),
            tenant: self.overview.tenants.first().map(|t| t.slug.clone()).unwrap_or_else(|| "—".into()),
            breadcrumb: breadcrumb.map(|s| s.to_string()),
        }
    }
}

// ── Main entry ──────────────────────────────────────────────────────────

pub async fn run() -> anyhow::Result<()> {
    let client = ApiClient::new(None, None);
    let mut app = App::new(client).await;

    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = event_loop(&mut terminal, &mut app).await;

    terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn event_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    loop {
        if app.last_refresh.elapsed() >= REFRESH_INTERVAL {
            app.refresh().await;
        }
        terminal.draw(|f| render(f, app))?;

        if !event::poll(Duration::from_millis(150))? { continue; }
        let Event::Key(key) = event::read()? else { continue; };
        if key.kind != KeyEventKind::Press { continue; }

        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(());
        }

        if handle_key(app, key.code, key.modifiers).await? {
            return Ok(());
        }
    }
}

// ── Key handling ────────────────────────────────────────────────────────

async fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) -> anyhow::Result<bool> {
    app.status_msg = None;

    match &mut app.screen {
        Screen::MainMenu { selected } => match code {
            KeyCode::Esc => return Ok(true),
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Down => *selected = (*selected + 1).min(Category::all().len() - 1),
            KeyCode::Char(':') => {
                let sel = *selected;
                app.screen = Screen::CmdBar { parent_selected: sel, input: String::new(), cursor: 0, suggestions: Vec::new() };
            }
            KeyCode::Enter => {
                let cat = Category::all()[*selected];
                app.refresh().await;
                enter_category(app, cat);
            }
            _ => {}
        },

        Screen::Submenu { category, actions, action_idx, item_idx, focus } => match code {
            KeyCode::Esc => app.screen = Screen::MainMenu { selected: 0 },
            KeyCode::Tab => *focus = if *focus == PanelFocus::Actions { PanelFocus::Context } else { PanelFocus::Actions },
            KeyCode::Up => {
                if *focus == PanelFocus::Actions { *action_idx = action_idx.saturating_sub(1); }
                else { *item_idx = item_idx.saturating_sub(1); }
            }
            KeyCode::Down => {
                if *focus == PanelFocus::Actions {
                    *action_idx = (*action_idx + 1).min(actions.len().saturating_sub(1));
                } else {
                    let max = context_item_count(&app.overview, *category);
                    *item_idx = (*item_idx + 1).min(max.saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                let cat = *category;
                let fidx = *focus;
                let aidx = *action_idx;
                let iidx = *item_idx;
                handle_submenu_enter(app, cat, fidx, aidx, iidx).await?;
            }
            KeyCode::Char(':') => {
                app.screen = Screen::CmdBar { parent_selected: 0, input: String::new(), cursor: 0, suggestions: Vec::new() };
            }
            _ => {}
        },

        Screen::Form { parent, title: _, form, form_kind } => match code {
            KeyCode::Esc => { let p = *parent; app.refresh().await; enter_category(app, p); }
            KeyCode::Tab => form.focus_next(),
            KeyCode::BackTab => form.focus_prev(),
            KeyCode::Up => form.cycle_select(false),
            KeyCode::Down => form.cycle_select(true),
            KeyCode::Backspace => form.backspace(),
            KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => form.insert_char(c),
            KeyCode::Enter => {
                let kind = form_kind.clone();
                let parent = *parent;
                submit_form(app, kind, parent).await?;
            }
            _ => {}
        },

        Screen::Popup { parent, item_id, item_name, items, selected, .. } => match code {
            KeyCode::Esc => { let p = *parent; app.refresh().await; enter_category(app, p); }
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Down => *selected = (*selected + 1).min(items.len().saturating_sub(1)),
            KeyCode::Enter => {
                let cat = *parent;
                let sel = *selected;
                let iid = item_id.clone();
                let iname = item_name.clone();
                let label = items[sel].label.clone();
                handle_popup_action(app, cat, &label, &iid, &iname).await?;
            }
            _ => {}
        },

        Screen::ChatMode { agent_name, agent_id, conversation_id, messages, input, cursor } => match code {
            KeyCode::Esc => app.screen = Screen::MainMenu { selected: 2 },
            KeyCode::Backspace => { if *cursor > 0 { input.remove(*cursor - 1); *cursor -= 1; } }
            KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => { input.insert(*cursor, c); *cursor += 1; }
            KeyCode::Enter => {
                let msg = input.trim().to_string();
                if msg == "/exit" || msg.is_empty() {
                    if msg == "/exit" { app.screen = Screen::MainMenu { selected: 2 }; }
                    return Ok(false);
                }
                messages.push(ChatMessage { role: "user".into(), content: msg.clone(), capabilities: vec![] });
                input.clear(); *cursor = 0;

                let tid = app.overview.tenants.first().map(|t| t.slug.as_str()).unwrap_or("default");
                let mut body = serde_json::json!({ "message": msg });
                if let Some(cid) = conversation_id.as_ref() {
                    body["conversation_id"] = serde_json::Value::String(cid.clone());
                }
                match app.client.post::<_, ChatResponse>(&format!("/api/tenants/{}/agents/{}/chat", tid, agent_id), &body).await {
                    Ok(resp) => {
                        *conversation_id = Some(resp.conversation_id);
                        messages.push(ChatMessage { role: agent_name.clone(), content: resp.response, capabilities: resp.capabilities_used });
                    }
                    Err(e) => {
                        messages.push(ChatMessage { role: "system".into(), content: format!("Error: {}", e), capabilities: vec![] });
                    }
                }
            }
            _ => {}
        },

        Screen::Wizard { step, form, info_lines } => match code {
            KeyCode::Esc => app.screen = Screen::MainMenu { selected: 0 },
            KeyCode::Tab => {
                // Skip step
                *step += 1;
                if *step >= 5 {
                    app.status_msg = Some(("Setup complete!".into(), true));
                    app.refresh().await;
                    app.screen = Screen::MainMenu { selected: 2 };
                } else {
                    *form = None; info_lines.clear();
                    setup_wizard_step(app).await;
                }
            }
            KeyCode::Enter => {
                handle_wizard_enter(app).await?;
            }
            KeyCode::Backspace => {
                if let Some(ref mut f) = form { f.backspace(); }
            }
            KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => {
                if let Some(ref mut f) = form { f.insert_char(c); }
            }
            _ => {
                if let Some(ref mut f) = form {
                    match code {
                        KeyCode::Up => f.cycle_select(false),
                        KeyCode::Down => f.cycle_select(true),
                        _ => {}
                    }
                }
            }
        },

        Screen::CmdBar { parent_selected, input, cursor, suggestions } => match code {
            KeyCode::Esc => {
                let sel = *parent_selected;
                app.screen = Screen::MainMenu { selected: sel };
            }
            KeyCode::Backspace => { if *cursor > 0 { input.remove(*cursor - 1); *cursor -= 1; } *suggestions = filter_cmd(&app.cmd_registry, input); }
            KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => { input.insert(*cursor, c); *cursor += 1; *suggestions = filter_cmd(&app.cmd_registry, input); }
            KeyCode::Tab => {
                if let Some(first) = suggestions.first() { *input = first.clone(); *cursor = input.len(); }
            }
            KeyCode::Enter => {
                let cmd = input.clone();
                let sel = *parent_selected;
                app.screen = Screen::MainMenu { selected: sel };
                run_cli_command(app, &cmd);
            }
            _ => {}
        },
    }
    Ok(false)
}

// ── Screen transitions ──────────────────────────────────────────────────

fn enter_category(app: &mut App, cat: Category) {
    match cat {
        Category::Setup => {
            app.screen = Screen::Wizard { step: 0, form: None, info_lines: Vec::new() };
            // Will set up first step on next render cycle
            let rt = tokio::runtime::Handle::current();
            rt.block_on(setup_wizard_step(app));
        }
        Category::Chat => {
            if let Some(agent) = app.overview.agents.first() {
                app.screen = Screen::ChatMode {
                    agent_name: agent.name.clone(), agent_id: agent.id.clone(),
                    conversation_id: None, messages: vec![], input: String::new(), cursor: 0,
                };
            } else {
                app.status_msg = Some(("No agents. Create one first via Agents > Create.".into(), false));
                app.screen = Screen::MainMenu { selected: 1 };
            }
        }
        _ => {
            let actions = category_actions(cat);
            app.screen = Screen::Submenu {
                category: cat, actions, action_idx: 0, item_idx: 0, focus: PanelFocus::Context,
            };
        }
    }
}

fn category_actions(cat: Category) -> Vec<String> {
    match cat {
        Category::Agents => vec!["Create new agent".into(), "Add skill".into(), "Export".into()],
        Category::Credentials => vec!["Set new credential".into(), "Rotate".into(), "Remove".into()],
        Category::Mcp => vec!["Create identity".into(), "Reissue code".into()],
        Category::System => vec!["Health".into(), "Status".into(), "Doctor".into(), "Backup".into(), "Compaction log".into()],
        _ => vec![],
    }
}

fn context_item_count(ov: &Overview, cat: Category) -> usize {
    match cat {
        Category::Agents => ov.agents.len(),
        Category::Credentials => ov.credentials.len(),
        Category::Mcp => ov.mcp_identities.len(),
        _ => 0,
    }
}

fn context_rows(ov: &Overview, cat: Category) -> (Vec<&'static str>, Vec<Vec<String>>) {
    match cat {
        Category::Agents => (
            vec!["NAME", "MODEL", "STATUS"],
            ov.agents.iter().map(|a| vec![
                a.name.clone(),
                a.model_id.split('-').take(3).collect::<Vec<_>>().join("-"),
                format!("{} {}", if a.status == "active" { theme::ICON_ONLINE } else { theme::ICON_DOT }, a.status),
            ]).collect(),
        ),
        Category::Credentials => (
            vec!["NAME", "TYPE"],
            ov.credentials.iter().map(|c| vec![c.name.clone(), c.kind.clone()]).collect(),
        ),
        Category::Mcp => (
            vec!["NAME", "STATUS"],
            ov.mcp_identities.iter().map(|m| vec![
                m.name.clone(),
                format!("{} {}", if m.status == "active" { theme::ICON_ONLINE } else { theme::ICON_DOT }, m.status),
            ]).collect(),
        ),
        _ => (vec![], vec![]),
    }
}

// ── Submenu enter ───────────────────────────────────────────────────────

async fn handle_submenu_enter(app: &mut App, cat: Category, focus: PanelFocus, aidx: usize, iidx: usize) -> anyhow::Result<()> {
    if focus == PanelFocus::Context {
        // Open popup for selected item
        let (id, name) = match cat {
            Category::Agents => app.overview.agents.get(iidx).map(|a| (a.id.clone(), a.name.clone())).unwrap_or_default(),
            Category::Credentials => app.overview.credentials.get(iidx).map(|c| (c.name.clone(), c.name.clone())).unwrap_or_default(),
            Category::Mcp => app.overview.mcp_identities.get(iidx).map(|m| (m.id.clone(), m.name.clone())).unwrap_or_default(),
            _ => return Ok(()),
        };
        if id.is_empty() { return Ok(()); }
        let items = popup_items_for(cat);
        app.screen = Screen::Popup { parent: cat, item_id: id, item_name: name, items, selected: 0, anchor_y: (iidx as u16 + 6).min(20) };
    } else {
        // Action panel
        match cat {
            Category::Agents => match aidx {
                0 => { // Create
                    app.screen = Screen::Form {
                        parent: cat, title: "Create Agent".into(),
                        form: FormState::new(vec![
                            FormField::text("Name", "support-bot"),
                            FormField::select("Model", vec!["claude-sonnet-4-20250514".into(), "claude-haiku-4-5-20251001".into(), "gpt-4o".into()], 0),
                            FormField::text_with_default("System Prompt", "You are a helpful assistant."),
                        ]),
                        form_kind: FormKind::CreateAgent,
                    };
                }
                1 => { // Add skill — need agent selection first
                    if let Some(agent) = app.overview.agents.first() {
                        app.screen = Screen::Form {
                            parent: cat, title: format!("Add Skill to {}", agent.name),
                            form: FormState::new(vec![
                                FormField::text("Skill Name", "product-faq"),
                                FormField::text("Description", "Knowledge base"),
                                FormField::text("File Path", "skills/faq.md"),
                            ]),
                            form_kind: FormKind::AddSkill { agent_id: agent.id.clone() },
                        };
                    }
                }
                _ => {
                    app.status_msg = Some(("Use the context panel to select an agent first.".into(), false));
                }
            },
            Category::Credentials => if aidx == 0 {
                app.screen = Screen::Form {
                    parent: cat, title: "Set Credential".into(),
                    form: FormState::new(vec![
                        FormField::text_with_default("Name", "anthropic"),
                        FormField::masked("API Key", "sk-ant-..."),
                    ]),
                    form_kind: FormKind::SetCredential,
                };
            },
            Category::Mcp => if aidx == 0 {
                let agent_names: Vec<String> = app.overview.agents.iter().map(|a| a.name.clone()).collect();
                if agent_names.is_empty() {
                    app.status_msg = Some(("Create an agent first.".into(), false));
                } else {
                    app.screen = Screen::Form {
                        parent: cat, title: "Create MCP Identity".into(),
                        form: FormState::new(vec![
                            FormField::text("Identity Name", "desktop-user"),
                            FormField::select("Bind to Agent", agent_names, 0),
                        ]),
                        form_kind: FormKind::CreateMcpIdentity,
                    };
                }
            },
            Category::System => {
                let cmds = ["health", "status", "doctor", "backup create", "compaction-log"];
                if let Some(cmd) = cmds.get(aidx) {
                    run_cli_command(app, cmd);
                    enter_category(app, cat);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// ── Popup actions ───────────────────────────────────────────────────────

fn popup_items_for(cat: Category) -> Vec<PopupItem> {
    match cat {
        Category::Agents => vec![
            PopupItem { label: "Chat".into(), dangerous: false },
            PopupItem { label: "Edit".into(), dangerous: false },
            PopupItem { label: "Add Skill".into(), dangerous: false },
            PopupItem { label: "Versions".into(), dangerous: false },
            PopupItem { label: "Delete".into(), dangerous: true },
        ],
        Category::Credentials => vec![
            PopupItem { label: "Rotate".into(), dangerous: false },
            PopupItem { label: "Remove".into(), dangerous: true },
        ],
        Category::Mcp => vec![
            PopupItem { label: "Reissue Code".into(), dangerous: false },
            PopupItem { label: "Revoke".into(), dangerous: true },
        ],
        _ => vec![],
    }
}

async fn handle_popup_action(app: &mut App, cat: Category, action: &str, item_id: &str, item_name: &str) -> anyhow::Result<()> {
    match (cat, action) {
        (Category::Agents, "Chat") => {
            app.screen = Screen::ChatMode {
                agent_name: item_name.into(), agent_id: item_id.into(),
                conversation_id: None, messages: vec![], input: String::new(), cursor: 0,
            };
        }
        (Category::Agents, "Delete") => {
            let tid = app.overview.tenants.first().map(|t| t.slug.as_str()).unwrap_or("default");
            match app.client.delete(&format!("/api/tenants/{}/agents/{}", tid, item_id)).await {
                Ok(()) => app.status_msg = Some((format!("Deleted agent '{}'", item_name), true)),
                Err(e) => app.status_msg = Some((format!("Failed: {}", e), false)),
            }
            app.refresh().await;
            enter_category(app, cat);
        }
        (Category::Agents, "Versions") => {
            run_cli_command(app, &format!("agent versions {}", item_id));
            enter_category(app, cat);
        }
        (Category::Agents, "Add Skill") => {
            app.screen = Screen::Form {
                parent: cat, title: format!("Add Skill to {}", item_name),
                form: FormState::new(vec![
                    FormField::text("Skill Name", "product-faq"),
                    FormField::text("Description", "Knowledge base"),
                    FormField::text("File Path", "skills/faq.md"),
                ]),
                form_kind: FormKind::AddSkill { agent_id: item_id.into() },
            };
        }
        (Category::Credentials, "Remove") => {
            let tid = app.overview.tenants.first().map(|t| t.slug.as_str()).unwrap_or("default");
            match app.client.delete(&format!("/api/tenants/{}/credentials/{}", tid, item_id)).await {
                Ok(()) => app.status_msg = Some((format!("Removed credential '{}'", item_name), true)),
                Err(e) => app.status_msg = Some((format!("Failed: {}", e), false)),
            }
            app.refresh().await;
            enter_category(app, cat);
        }
        (Category::Mcp, "Reissue Code") => {
            run_cli_command(app, &format!("mcp-identity reissue-setup-code {} --tenant default", item_id));
            enter_category(app, cat);
        }
        (Category::Mcp, "Revoke") => {
            run_cli_command(app, &format!("mcp-identity revoke {} --tenant default", item_id));
            app.refresh().await;
            enter_category(app, cat);
        }
        _ => {
            app.status_msg = Some((format!("Action '{}' not yet implemented", action), false));
            enter_category(app, cat);
        }
    }
    Ok(())
}

// ── Form submission ─────────────────────────────────────────────────────

async fn submit_form(app: &mut App, kind: FormKind, parent: Category) -> anyhow::Result<()> {
    let form = match &app.screen { Screen::Form { form, .. } => form, _ => return Ok(()) };
    let tid = app.overview.tenants.first().map(|t| t.slug.as_str()).unwrap_or("default");

    match kind {
        FormKind::CreateAgent => {
            let name = &form.fields[0].value;
            let model = &form.fields[1].display_value();
            let prompt = &form.fields[2].value;
            if name.is_empty() {
                if let Screen::Form { form, .. } = &mut app.screen { form.error = Some("Name is required".into()); }
                return Ok(());
            }
            let body = serde_json::json!({ "name": name, "model_id": model, "system_prompt": prompt, "scope_mode": "dual" });
            match app.client.post::<_, serde_json::Value>(&format!("/api/tenants/{}/agents", tid), &body).await {
                Ok(_) => { app.status_msg = Some((format!("{} Agent '{}' created", theme::ICON_CHECK, name), true)); app.refresh().await; enter_category(app, parent); }
                Err(e) => { if let Screen::Form { form, .. } = &mut app.screen { form.error = Some(format!("{}", e)); } }
            }
        }
        FormKind::SetCredential => {
            let name = &form.fields[0].value;
            let secret = &form.fields[1].value;
            if secret.is_empty() {
                if let Screen::Form { form, .. } = &mut app.screen { form.error = Some("API key is required".into()); }
                return Ok(());
            }
            let body = serde_json::json!({ "name": name, "kind": "static", "value": secret });
            match app.client.post::<_, serde_json::Value>(&format!("/api/tenants/{}/credentials", tid), &body).await {
                Ok(_) => { app.status_msg = Some((format!("{} Credential '{}' stored", theme::ICON_CHECK, name), true)); app.refresh().await; enter_category(app, parent); }
                Err(e) => { if let Screen::Form { form, .. } = &mut app.screen { form.error = Some(format!("{}", e)); } }
            }
        }
        FormKind::CreateMcpIdentity => {
            let name = &form.fields[0].value;
            let agent = &form.fields[1].display_value();
            if name.is_empty() {
                if let Screen::Form { form, .. } = &mut app.screen { form.error = Some("Name is required".into()); }
                return Ok(());
            }
            let body = serde_json::json!({ "name": name, "agent_slug": agent });
            match app.client.post::<_, serde_json::Value>(&format!("/api/tenants/{}/mcp-identities", tid), &body).await {
                Ok(resp) => {
                    let code = resp.get("setup_code").and_then(|v| v.as_str()).unwrap_or("(none)");
                    let url = format!("{}/mcp/{}/{}", app.overview.endpoint, tid, agent);
                    app.status_msg = Some((format!("{} Identity created. URL: {}  Code: {}", theme::ICON_CHECK, url, code), true));
                    app.refresh().await; enter_category(app, parent);
                }
                Err(e) => { if let Screen::Form { form, .. } = &mut app.screen { form.error = Some(format!("{}", e)); } }
            }
        }
        FormKind::AddSkill { agent_id } => {
            let name = &form.fields[0].value;
            let desc = &form.fields[1].value;
            let path = &form.fields[2].value;
            if name.is_empty() || path.is_empty() {
                if let Screen::Form { form, .. } = &mut app.screen { form.error = Some("Name and file path are required".into()); }
                return Ok(());
            }
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => { if let Screen::Form { form, .. } = &mut app.screen { form.error = Some(format!("Cannot read file: {}", e)); } return Ok(()); }
            };
            let body = serde_json::json!({ "name": name, "description": desc, "auth_type": "markdown", "instruction_content": content });
            match app.client.post::<_, serde_json::Value>(&format!("/api/tenants/{}/agents/{}/capabilities", tid, agent_id), &body).await {
                Ok(_) => { app.status_msg = Some((format!("{} Skill '{}' added ({} bytes)", theme::ICON_CHECK, name, content.len()), true)); app.refresh().await; enter_category(app, parent); }
                Err(e) => { if let Screen::Form { form, .. } = &mut app.screen { form.error = Some(format!("{}", e)); } }
            }
        }
    }
    Ok(())
}

// ── Wizard ──────────────────────────────────────────────────────────────

const WIZARD_TITLES: [&str; 5] = ["Service", "API Key", "Agent", "MCP Access", "Test Chat"];

async fn setup_wizard_step(app: &mut App) {
    let step = match &app.screen { Screen::Wizard { step, .. } => *step, _ => return };
    app.refresh().await;
    match step {
        0 => { // Service check
            if app.overview.service_running {
                if let Screen::Wizard { info_lines, .. } = &mut app.screen {
                    *info_lines = vec![format!("{} Service is running at {}", theme::ICON_CHECK, app.overview.endpoint)];
                }
            } else {
                if let Screen::Wizard { info_lines, .. } = &mut app.screen {
                    *info_lines = vec!["Service is offline. Press Enter to start.".into()];
                }
            }
        }
        1 => { // API key
            if !app.overview.credentials.is_empty() {
                if let Screen::Wizard { info_lines, form, .. } = &mut app.screen {
                    *info_lines = vec![format!("{} Credential already stored: {}", theme::ICON_CHECK, app.overview.credentials[0].name)];
                    *form = None;
                }
            } else {
                if let Screen::Wizard { form, info_lines, .. } = &mut app.screen {
                    *form = Some(FormState::new(vec![FormField::masked("API Key", "sk-ant-...")]));
                    *info_lines = vec!["Paste your Anthropic or OpenAI API key:".into()];
                }
            }
        }
        2 => { // Agent
            if !app.overview.agents.is_empty() {
                if let Screen::Wizard { info_lines, form, .. } = &mut app.screen {
                    *info_lines = vec![format!("{} Agent exists: {}", theme::ICON_CHECK, app.overview.agents[0].name)];
                    *form = None;
                }
            } else {
                if let Screen::Wizard { form, info_lines, .. } = &mut app.screen {
                    *form = Some(FormState::new(vec![
                        FormField::text_with_default("Agent Name", "support-bot"),
                        FormField::text_with_default("System Prompt", "You are a helpful assistant."),
                    ]));
                    *info_lines = vec!["Create your first agent:".into()];
                }
            }
        }
        3 => { // MCP
            if let Screen::Wizard { info_lines, form, .. } = &mut app.screen {
                *form = None;
                if let Some(agent) = app.overview.agents.first() {
                    *info_lines = vec![
                        "MCP identity will be created for external access.".into(),
                        format!("Agent: {}", agent.name),
                        "Press Enter to create, Tab to skip.".into(),
                    ];
                }
            }
        }
        4 => { // Test chat
            if let Screen::Wizard { form, info_lines, .. } = &mut app.screen {
                *form = Some(FormState::new(vec![FormField::text("Message", "Hello!")]));
                *info_lines = vec!["Send a test message to your agent:".into()];
            }
        }
        _ => {}
    }
}

fn extract_wizard_state(screen: &Screen) -> Option<(usize, Option<FormState>)> {
    match screen {
        Screen::Wizard { step, form, .. } => Some((*step, form.clone())),
        _ => None,
    }
}

async fn handle_wizard_enter(app: &mut App) -> anyhow::Result<()> {
    let (step, form_snapshot) = match extract_wizard_state(&app.screen) {
        Some(v) => v,
        None => return Ok(()),
    };
    let tid = app.overview.tenants.first().map(|t| t.slug.clone()).unwrap_or_else(|| "default".into());

    match step {
        0 => { // Start service
            if !app.overview.service_running {
                let exe = std::env::current_exe()?;
                let data_dir = crate::cli::local::default_data_dir();
                let logs_dir = std::path::PathBuf::from(&data_dir).join("logs");
                std::fs::create_dir_all(&logs_dir)?;
                let log_path = logs_dir.join("service.log");
                let stdout = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;
                let stderr = stdout.try_clone()?;
                ProcessCommand::new(exe).arg("serve").env("HIVELOOM_DATA_DIR", &data_dir)
                    .stdin(Stdio::null()).stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr)).spawn()?;
                tokio::time::sleep(Duration::from_millis(2000)).await;
                app.refresh().await;
            }
            // Advance
            if let Screen::Wizard { step, .. } = &mut app.screen { *step = 1; }
            setup_wizard_step(app).await;
        }
        1 => { // Submit API key
            if let Some(ref f) = form_snapshot {
                let key = &f.fields[0].value;
                if !key.is_empty() {
                    let cred_name = if key.starts_with("sk-ant-") { "anthropic" } else { "openai" };
                    let body = serde_json::json!({ "name": cred_name, "kind": "static", "value": key });
                    match app.client.post::<_, serde_json::Value>(&format!("/api/tenants/{}/credentials", tid), &body).await {
                        Ok(_) => {}
                        Err(e) => { if let Screen::Wizard { form: Some(f), .. } = &mut app.screen { f.error = Some(format!("{}", e)); } return Ok(()); }
                    }
                }
            }
            if let Screen::Wizard { step, .. } = &mut app.screen { *step = 2; }
            setup_wizard_step(app).await;
        }
        2 => { // Submit agent
            if let Some(ref f) = form_snapshot {
                let name = &f.fields[0].value;
                let prompt = &f.fields[1].value;
                if !name.is_empty() {
                    let model = if app.overview.credentials.iter().any(|c| c.name == "anthropic") { "claude-sonnet-4-20250514" } else { "gpt-4o" };
                    let body = serde_json::json!({ "name": name, "model_id": model, "system_prompt": prompt, "scope_mode": "dual" });
                    let _ = app.client.post::<_, serde_json::Value>(&format!("/api/tenants/{}/agents", tid), &body).await;
                }
            }
            if let Screen::Wizard { step, .. } = &mut app.screen { *step = 3; }
            setup_wizard_step(app).await;
        }
        3 => { // Create MCP identity
            app.refresh().await;
            if let Some(agent) = app.overview.agents.first() {
                let body = serde_json::json!({ "name": "desktop-user", "agent_slug": agent.name });
                if let Ok(resp) = app.client.post::<_, serde_json::Value>(&format!("/api/tenants/{}/mcp-identities", tid), &body).await {
                    let code = resp.get("setup_code").and_then(|v| v.as_str()).unwrap_or("—");
                    if let Screen::Wizard { info_lines, .. } = &mut app.screen {
                        *info_lines = vec![
                            format!("{} MCP identity created!", theme::ICON_CHECK),
                            String::new(),
                            format!("  URL:   {}/mcp/{}/{}", app.overview.endpoint, tid, agent.name),
                            format!("  Code:  {}", code),
                            String::new(),
                            "Add the URL to Claude Desktop. Enter the code when prompted.".into(),
                            "Press Enter to continue to test chat.".into(),
                        ];
                    }
                }
            }
            // Don't auto-advance — let user read the info
            // On next Enter, advance to step 4
            if let Screen::Wizard { step, .. } = &mut app.screen {
                if *step == 3 { *step = 4; } // next Enter will run step 4
            }
            // Actually re-setup for step 4
            setup_wizard_step(app).await;
        }
        4 => { // Test chat
            if let Some(ref f) = form_snapshot {
                let msg = &f.fields[0].value;
                if !msg.is_empty() {
                    app.refresh().await;
                    if let Some(agent) = app.overview.agents.first() {
                        let body = serde_json::json!({ "message": msg });
                        match app.client.post::<_, ChatResponse>(&format!("/api/tenants/{}/agents/{}/chat", tid, agent.id), &body).await {
                            Ok(resp) => {
                                if let Screen::Wizard { info_lines, form, .. } = &mut app.screen {
                                    *info_lines = vec![
                                        format!("  you: {}", msg),
                                        format!("  {}: {}", agent.name, resp.response),
                                        String::new(),
                                        format!("{} Setup complete! Press Enter to finish.", theme::ICON_CHECK),
                                    ];
                                    *form = None;
                                }
                            }
                            Err(e) => {
                                if let Screen::Wizard { info_lines, .. } = &mut app.screen {
                                    info_lines.push(format!("Chat error: {}", e));
                                }
                            }
                        }
                    }
                }
            } else {
                // Finish wizard
                app.status_msg = Some((format!("{} Setup complete!", theme::ICON_CHECK), true));
                app.refresh().await;
                app.screen = Screen::MainMenu { selected: 2 }; // highlight Chat
            }
        }
        _ => {}
    }
    Ok(())
}

// ── CLI command dispatch ────────────────────────────────────────────────

fn run_cli_command(app: &mut App, input: &str) {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return,
    };
    let args: Vec<&str> = input.split_whitespace().collect();
    if args.is_empty() { return; }
    let result = ProcessCommand::new(&exe).args(&args)
        .env("HIVELOOM_ENDPOINT", &app.overview.endpoint)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).output();
    match result {
        Ok(output) => {
            let out = String::from_utf8_lossy(&output.stdout);
            let err = String::from_utf8_lossy(&output.stderr);
            let text = format!("{}{}", out, err).trim().to_string();
            app.status_msg = Some((if text.is_empty() { "(done)".into() } else { text }, output.status.success()));
        }
        Err(e) => app.status_msg = Some((format!("Failed: {}", e), false)),
    }
}

fn build_cmd_registry() -> Vec<String> {
    use clap::CommandFactory;
    let mut entries = Vec::new();
    let cmd = super::Cli::command();
    for sub in cmd.get_subcommands() {
        let name = sub.get_name().to_string();
        if name == "interactive" || name == "serve" || name == "version" { continue; }
        if sub.get_subcommands().next().is_some() {
            for ss in sub.get_subcommands() {
                entries.push(format!("{} {}", name, ss.get_name()));
            }
        } else {
            entries.push(name);
        }
    }
    entries
}

fn filter_cmd(registry: &[String], query: &str) -> Vec<String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() { return vec![]; }
    registry.iter().filter(|e| e.to_lowercase().starts_with(&q) || e.to_lowercase().contains(&q))
        .take(5).cloned().collect()
}

// ── Data fetching ───────────────────────────────────────────────────────

async fn fetch_overview(client: &ApiClient) -> Overview {
    let endpoint = crate::cli::local::default_endpoint();
    let service_running = client.get_raw("/healthz").await.map(|s| s.is_success()).unwrap_or(false);
    let tenants: Vec<TenantInfo> = client.get("/api/tenants").await.unwrap_or_default();
    let agents: Vec<AgentInfo> = client.get("/api/tenants/default/agents").await.unwrap_or_default();
    let credentials: Vec<CredentialInfo> = client.get("/api/tenants/default/credentials").await.unwrap_or_default();
    let mcp_identities: Vec<McpInfo> = client.get("/api/tenants/default/mcp-identities").await.unwrap_or_default();
    Overview { endpoint, service_running, tenants, agents, credentials, mcp_identities }
}

// ── Rendering ───────────────────────────────────────────────────────────

fn render(f: &mut Frame, app: &App) {
    let size = f.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Status bar
            Constraint::Min(6),     // Content
            Constraint::Length(if app.status_msg.is_some() { 1 } else { 0 }), // Status message
            Constraint::Length(1),  // Key hints
        ])
        .split(size);

    // Status bar
    let breadcrumb = match &app.screen {
        Screen::MainMenu { .. } => None,
        Screen::Submenu { category, .. } => Some(category.label()),
        Screen::Form { parent, title, .. } => Some(Box::leak(format!("{} {} {}", parent.label(), theme::ICON_ARROW, title).into_boxed_str()) as &str),
        Screen::ChatMode { agent_name, .. } => Some(Box::leak(format!("Chat {} {}", theme::ICON_ARROW, agent_name).into_boxed_str()) as &str),
        Screen::Wizard { step, .. } => Some(Box::leak(format!("Setup {} {}", theme::ICON_ARROW, WIZARD_TITLES.get(*step).unwrap_or(&"")).into_boxed_str()) as &str),
        Screen::CmdBar { .. } => None,
        Screen::Popup { parent, .. } => Some(parent.label()),
    };
    status_bar::render(f, chunks[0], &app.status_info(breadcrumb));

    // Content
    match &app.screen {
        Screen::MainMenu { selected } => render_main_menu(f, chunks[1], *selected),
        Screen::Submenu { category, actions, action_idx, item_idx, focus } => {
            render_submenu(f, chunks[1], &app.overview, *category, actions, *action_idx, *item_idx, *focus);
        }
        Screen::Form { form, .. } => form::render(f, chunks[1], form),
        Screen::Popup { parent, items, selected, anchor_y, .. } => {
            render_submenu(f, chunks[1], &app.overview, *parent, &category_actions(*parent), 0, 0, PanelFocus::Context);
            popup::render(f, *anchor_y, chunks[1].width / 2, items, *selected);
        }
        Screen::ChatMode { agent_name, messages, input, .. } => {
            chat_view::render(f, chunks[1], messages, input, agent_name);
        }
        Screen::Wizard { step, form, info_lines } => {
            render_wizard(f, chunks[1], *step, form.as_ref(), info_lines);
        }
        Screen::CmdBar { input, cursor, suggestions, .. } => {
            render_main_menu(f, chunks[1], 0);
            let cmd_area = Rect::new(chunks[1].x, chunks[1].bottom().saturating_sub(2), chunks[1].width, 2);
            command_bar::render(f, cmd_area, input, *cursor, suggestions);
        }
    }

    // Status message
    if let Some((ref msg, is_success)) = app.status_msg {
        let style = if is_success { theme::success() } else { theme::warning() };
        f.render_widget(Paragraph::new(Span::styled(format!("  {}", msg), style)), chunks[2]);
    }

    // Key hints
    let hints = match &app.screen {
        Screen::MainMenu { .. } => "  ↑↓ navigate   Enter select   : command   Esc quit",
        Screen::Submenu { .. } => "  ↑↓ navigate   Tab switch panel   Enter select   Esc back",
        Screen::Form { .. } => "  Tab next field   ↑↓ select option   Enter submit   Esc cancel",
        Screen::Popup { .. } => "  ↑↓ navigate   Enter select   Esc close",
        Screen::ChatMode { .. } => "  Enter send   /exit or Esc back",
        Screen::Wizard { .. } => "  Enter continue   Tab skip   Esc back",
        Screen::CmdBar { .. } => "  Enter execute   Tab complete   Esc cancel",
    };
    f.render_widget(Paragraph::new(Span::styled(hints, theme::dim())), chunks[3]);
}

fn render_main_menu(f: &mut Frame, area: Rect, selected: usize) {
    let items: Vec<MenuItem> = Category::all().iter().map(|c| MenuItem {
        label: c.label().to_string(),
        description: c.description().to_string(),
        badge: if *c == Category::Setup { Some(theme::ICON_ARROW.to_string()) } else { None },
    }).collect();

    let menu_area = Rect::new(area.x + 2, area.y + 1, area.width.saturating_sub(4), area.height.saturating_sub(2));
    menu::render(f, menu_area, &items, selected);
}

fn render_submenu(
    f: &mut Frame, area: Rect, overview: &Overview, cat: Category,
    actions: &[String], action_idx: usize, item_idx: usize, focus: PanelFocus,
) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    // Left: actions
    let action_items: Vec<MenuItem> = actions.iter().map(|a| MenuItem {
        label: a.clone(), description: String::new(), badge: None,
    }).collect();
    let action_area = Rect::new(cols[0].x + 1, cols[0].y + 1, cols[0].width.saturating_sub(2), cols[0].height.saturating_sub(2));
    menu::render(f, action_area, &action_items, if focus == PanelFocus::Actions { action_idx } else { usize::MAX });

    // Right: context
    let (columns, rows) = context_rows(overview, cat);
    context_panel::render(f, cols[1], cat.label(), &columns, &rows, item_idx, focus == PanelFocus::Context);
}

fn render_wizard(f: &mut Frame, area: Rect, step: usize, form: Option<&FormState>, info_lines: &[String]) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(4)])
        .split(area);

    let title = WIZARD_TITLES.get(step).unwrap_or(&"");
    wizard::render_step_indicator(f, chunks[0], step, 5, title);

    let content_area = Rect::new(chunks[1].x + 2, chunks[1].y, chunks[1].width.saturating_sub(4), chunks[1].height);

    let mut lines: Vec<Line> = info_lines.iter().map(|l| {
        Line::from(Span::styled(format!("  {}", l), Style::default()))
    }).collect();

    if let Some(fs) = form {
        let info_height = lines.len() as u16;
        let info_area = Rect::new(content_area.x, content_area.y, content_area.width, info_height);
        f.render_widget(Paragraph::new(lines), info_area);
        let form_area = Rect::new(content_area.x, content_area.y + info_height + 1, content_area.width, content_area.height.saturating_sub(info_height + 1));
        form::render(f, form_area, fs);
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  Enter: continue   Tab: skip", theme::dim())));
        f.render_widget(Paragraph::new(lines), content_area);
    }
}
