use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use super::theme;

pub struct MenuItem {
    pub label: String,
    pub description: String,
    pub badge: Option<String>,
}

pub fn render(f: &mut Frame, area: Rect, items: &[MenuItem], selected: usize) {
    let mut lines = Vec::new();

    for (i, item) in items.iter().enumerate() {
        let is_selected = i == selected;
        let cursor = if is_selected {
            Span::styled(format!("  {} ", theme::ICON_CURSOR), theme::accent())
        } else {
            Span::raw("    ")
        };

        let label_style = if is_selected {
            theme::focused_bold()
        } else {
            theme::bold()
        };
        let desc_style = if is_selected {
            theme::focused()
        } else {
            theme::dim()
        };
        let bg_style = if is_selected {
            theme::focused()
        } else {
            Style::default()
        };

        let mut spans = vec![
            Span::styled(
                cursor.content.to_string(),
                if is_selected {
                    theme::accent()
                } else {
                    Style::default()
                },
            ),
            Span::styled(format!("{:<16}", item.label), label_style),
            Span::styled(&item.description, desc_style),
        ];

        if let Some(ref badge) = item.badge {
            spans.push(Span::styled(
                format!("  {}", badge),
                if is_selected {
                    Style::default().fg(theme::ACCENT).bg(theme::FOCUS_BG)
                } else {
                    theme::accent()
                },
            ));
        }

        // Fill the rest of the line with background color for selected item
        let line = Line::from(spans).style(bg_style);
        lines.push(line);
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}
