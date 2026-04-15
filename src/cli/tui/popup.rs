use ratatui::prelude::*;
use ratatui::widgets::{Clear, Paragraph};

use super::theme;

pub struct PopupItem {
    pub label: String,
    pub dangerous: bool,
}

pub fn render(
    f: &mut Frame,
    anchor_y: u16,
    anchor_x: u16,
    items: &[PopupItem],
    selected: usize,
) {
    let width = items.iter().map(|i| i.label.len()).max().unwrap_or(10) + 8;
    let height = items.len() as u16 + 2; // +2 for border
    let term_h = f.size().height;

    // Position: below anchor if space, above if near bottom
    let y = if anchor_y + height + 2 < term_h {
        anchor_y + 1
    } else {
        anchor_y.saturating_sub(height)
    };
    let x = anchor_x.min(f.size().width.saturating_sub(width as u16 + 2));

    let popup_area = Rect::new(x, y, width as u16, height);

    // Clear background
    f.render_widget(Clear, popup_area);

    let mut lines = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let is_selected = i == selected;
        let cursor = if is_selected {
            format!(" {} ", theme::ICON_CURSOR)
        } else {
            "   ".to_string()
        };

        let label_style = if item.dangerous && is_selected {
            Style::default()
                .fg(theme::DANGER)
                .bg(theme::FOCUS_BG)
                .add_modifier(Modifier::BOLD)
        } else if item.dangerous {
            Style::default().fg(theme::DANGER)
        } else if is_selected {
            theme::focused_bold()
        } else {
            Style::default()
        };

        let bg = if is_selected { theme::focused() } else { Style::default() };

        lines.push(
            Line::from(vec![
                Span::styled(cursor, if is_selected { theme::accent() } else { Style::default() }),
                Span::styled(item.label.clone(), label_style),
            ])
            .style(bg),
        );
    }

    let block = theme::rounded_block_plain();
    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, popup_area);
}
