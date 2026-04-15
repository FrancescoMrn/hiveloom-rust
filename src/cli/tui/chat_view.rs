use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Wrap};

use super::theme;

pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub capabilities: Vec<String>,
}

pub fn render(
    f: &mut Frame,
    area: Rect,
    messages: &[ChatMessage],
    input: &str,
    agent_name: &str,
) {
    // Split: messages area + input area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(area);

    // Messages
    let mut lines = Vec::new();
    for msg in messages {
        let (label_style, label) = if msg.role == "user" {
            (theme::bold(), "  you")
        } else {
            (
                Style::default()
                    .fg(theme::SUCCESS)
                    .add_modifier(Modifier::BOLD),
                agent_name,
            )
        };
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {}", label),
            label_style,
        )));
        for content_line in msg.content.lines() {
            lines.push(Line::from(Span::raw(format!("    {}", content_line))));
        }
        if !msg.capabilities.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("    {} {}", theme::ICON_CHECK, msg.capabilities.join(", ")),
                theme::dim(),
            )));
        }
    }

    if messages.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Start typing to chat...",
            theme::dim(),
        )));
    }

    // Auto-scroll to bottom
    let visible = chunks[0].height as usize;
    let scroll = lines.len().saturating_sub(visible) as u16;
    let msg_widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(msg_widget, chunks[0]);

    // Input
    let display = if input.is_empty() {
        Line::from(Span::styled("  Type a message...", theme::dim()))
    } else {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::raw(input),
        ])
    };
    let input_block = theme::rounded_block_plain();
    let input_widget = Paragraph::new(display).block(input_block);
    f.render_widget(input_widget, chunks[1]);
}
