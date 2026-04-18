use ratatui::prelude::*;

use super::theme;

pub fn render_step_indicator(f: &mut Frame, area: Rect, current: usize, total: usize, title: &str) {
    let mut spans = vec![
        Span::styled(
            format!("  Step {} of {} ", current + 1, total),
            theme::bold(),
        ),
        Span::styled("── ", theme::dim()),
    ];

    for i in 0..total {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        if i < current {
            spans.push(Span::styled(theme::ICON_CHECK, theme::success()));
        } else if i == current {
            spans.push(Span::styled(theme::ICON_STEP_CURRENT, theme::accent()));
        } else {
            spans.push(Span::styled(theme::ICON_STEP_TODO, theme::dim()));
        }
    }

    spans.push(Span::styled(format!("  {}", title), theme::dim()));

    let line = Line::from(spans);
    f.render_widget(ratatui::widgets::Paragraph::new(line), area);
}
