use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use super::theme;

pub fn render(
    f: &mut Frame,
    area: Rect,
    title: &str,
    columns: &[&str],
    rows: &[Vec<String>],
    selected: usize,
    focused: bool,
) {
    let mut lines = Vec::new();

    // Header
    let header: Vec<Span> = columns
        .iter()
        .map(|c| Span::styled(format!("  {:<18}", c), theme::bold()))
        .collect();
    lines.push(Line::from(header));
    lines.push(Line::from(""));

    // Rows
    for (i, row) in rows.iter().enumerate() {
        let is_selected = i == selected && focused;
        let cursor = if is_selected {
            Span::styled(format!("  {} ", theme::ICON_CURSOR), theme::accent())
        } else {
            Span::raw("    ")
        };

        let row_style = if is_selected {
            theme::focused()
        } else {
            Style::default()
        };

        let mut spans = vec![cursor];
        for cell in row {
            spans.push(Span::styled(format!("{:<18}", cell), row_style));
        }

        lines.push(Line::from(spans).style(row_style));
    }

    if rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "    (none)",
            theme::dim(),
        )));
    }

    // Footer hint
    let remaining = area.height as usize - lines.len().min(area.height as usize) - 1;
    for _ in 0..remaining {
        lines.push(Line::from(""));
    }
    if focused {
        lines.push(Line::from(Span::styled(
            "  Enter: actions  Esc: back",
            theme::dim(),
        )));
    }

    let block = theme::rounded_block(title);
    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}
