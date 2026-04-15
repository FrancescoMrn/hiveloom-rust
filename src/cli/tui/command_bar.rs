use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use super::theme;

pub fn render(
    f: &mut Frame,
    area: Rect,
    input: &str,
    _cursor: usize,
    suggestions: &[String],
) {
    // Suggestions line above input
    if !suggestions.is_empty() {
        let sug_area = Rect::new(area.x, area.y, area.width, 1);
        let spans: Vec<Span> = suggestions
            .iter()
            .take(5)
            .enumerate()
            .flat_map(|(i, s)| {
                let mut v = Vec::new();
                if i > 0 {
                    v.push(Span::styled("  ", Style::default()));
                }
                v.push(Span::styled(s, theme::dim()));
                v
            })
            .collect();
        f.render_widget(Paragraph::new(Line::from(spans)), sug_area);
    }

    // Input line
    let input_area = Rect::new(area.x, area.y + if suggestions.is_empty() { 0 } else { 1 }, area.width, 1);
    let line = Line::from(vec![
        Span::styled(": ", theme::accent()),
        Span::raw(input),
    ]);
    f.render_widget(Paragraph::new(line), input_area);
}
