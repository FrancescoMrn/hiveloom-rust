use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use super::theme;

pub struct StatusInfo {
    pub service_running: bool,
    pub agent_count: usize,
    pub credential_count: usize,
    pub tenant: String,
    pub breadcrumb: Option<String>,
}

pub fn render(f: &mut Frame, area: Rect, info: &StatusInfo) {
    let status_icon = if info.service_running {
        Span::styled(format!("{} online", theme::ICON_ONLINE), theme::success())
    } else {
        Span::styled(format!("{} offline", theme::ICON_ONLINE), theme::warning())
    };

    let title = match &info.breadcrumb {
        Some(crumb) => format!("  hiveloom {} {}", theme::ICON_ARROW, crumb),
        None => "  hiveloom".to_string(),
    };

    let line = Line::from(vec![
        Span::styled(&title, theme::accent_bold()),
        Span::raw("   "),
        status_icon,
        Span::styled(
            format!(
                "   {} agents   {} credentials   {}",
                info.agent_count, info.credential_count, info.tenant
            ),
            theme::dim(),
        ),
    ]);

    let block = theme::rounded_block_plain();
    let paragraph = Paragraph::new(line).block(block);
    f.render_widget(paragraph, area);
}
