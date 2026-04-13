use clap::Args;
use crossterm::{
    event::{self, Event, KeyCode},
    terminal,
};
use ratatui::{prelude::*, widgets::*};
use serde::Deserialize;

use super::client::ApiClient;

#[derive(Args)]
pub struct TopArgs {
    /// API endpoint
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Bearer token for remote API access
    #[arg(long)]
    pub token: Option<String>,

    /// Refresh interval in seconds
    #[arg(long, default_value = "2")]
    pub interval: u64,
}

#[derive(Debug, Default, Deserialize)]
struct DashboardData {
    #[serde(default)]
    agents: Vec<AgentSummary>,
    #[serde(default)]
    conversations_active: u64,
    #[serde(default)]
    jobs: Vec<JobSummary>,
    #[serde(default)]
    healthy: bool,
}

#[derive(Debug, Deserialize)]
struct AgentSummary {
    name: String,
    status: String,
    #[serde(default)]
    id: String,
}

#[derive(Debug, Deserialize)]
struct JobSummary {
    #[serde(default)]
    id: String,
    #[serde(default)]
    agent_id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    next_fire_at: Option<String>,
    #[serde(default)]
    last_fired_at: Option<String>,
}

async fn fetch_dashboard(client: &ApiClient) -> DashboardData {
    // Best-effort fetches; failures yield defaults
    let healthy = client
        .get::<serde_json::Value>("/healthz")
        .await
        .is_ok();

    // We cannot enumerate all tenants' agents from a single API call easily;
    // fetch from "default" tenant as representative overview.
    let agents: Vec<AgentSummary> = client
        .get("/api/tenants/default/agents")
        .await
        .unwrap_or_default();

    DashboardData {
        agents,
        conversations_active: 0,
        jobs: Vec::new(),
        healthy,
    }
}

pub async fn run(args: TopArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let interval = std::time::Duration::from_secs(args.interval);

    // Initialize terminal
    terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let mut last_fetch = std::time::Instant::now() - interval;
    let mut data = DashboardData::default();

    loop {
        // Refresh data periodically
        if last_fetch.elapsed() >= interval {
            data = fetch_dashboard(&client).await;
            last_fetch = std::time::Instant::now();
        }

        // Render
        terminal.draw(|f| render_dashboard(f, &data))?;

        // Handle input (non-blocking with 200ms timeout)
        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
                    break;
                }
            }
        }
    }

    // Restore terminal
    terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn render_dashboard(f: &mut ratatui::Frame, data: &DashboardData) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(8),    // Agents
            Constraint::Length(5), // Stats
            Constraint::Length(3), // Footer
        ])
        .split(f.size());

    // Title bar
    let health_indicator = if data.healthy { "OK" } else { "DEGRADED" };
    let title = Paragraph::new(format!(
        " hiveloom top  |  Health: {}  |  Press 'q' to quit",
        health_indicator
    ))
    .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    .block(Block::default().borders(Borders::ALL).title("Hiveloom Dashboard"));
    f.render_widget(title, chunks[0]);

    // Agents table
    let header = Row::new(vec!["NAME", "STATUS", "ID"])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = data
        .agents
        .iter()
        .map(|a| {
            let status_style = match a.status.as_str() {
                "active" => Style::default().fg(Color::Green),
                "disabled" => Style::default().fg(Color::Red),
                _ => Style::default(),
            };
            Row::new(vec![
                Cell::from(a.name.clone()),
                Cell::from(a.status.clone()).style(status_style),
                Cell::from(a.id.clone()),
            ])
        })
        .collect();
    let agents_table = Table::new(
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Percentage(20),
            Constraint::Percentage(50),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("Active Agents"));
    f.render_widget(agents_table, chunks[1]);

    // Stats bar
    let stats = Paragraph::new(format!(
        " Agents: {}  |  Active conversations: {}  |  Scheduled jobs: {}",
        data.agents.len(),
        data.conversations_active,
        data.jobs.len(),
    ))
    .block(Block::default().borders(Borders::ALL).title("Summary"));
    f.render_widget(stats, chunks[2]);

    // Footer
    let footer = Paragraph::new(" Refresh: 2s  |  q: quit")
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, chunks[3]);
}
