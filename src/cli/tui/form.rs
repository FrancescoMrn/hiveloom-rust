use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use super::theme;

#[derive(Clone)]
pub enum FieldKind {
    Text,
    Masked,
    Select {
        options: Vec<String>,
        selected: usize,
    },
}

#[derive(Clone)]
pub struct FormField {
    pub label: String,
    pub value: String,
    pub placeholder: String,
    pub kind: FieldKind,
    pub cursor: usize,
}

impl FormField {
    pub fn text(label: &str, placeholder: &str) -> Self {
        Self {
            label: label.to_string(),
            value: String::new(),
            placeholder: placeholder.to_string(),
            kind: FieldKind::Text,
            cursor: 0,
        }
    }
    pub fn text_with_default(label: &str, default: &str) -> Self {
        Self {
            label: label.to_string(),
            value: default.to_string(),
            placeholder: String::new(),
            kind: FieldKind::Text,
            cursor: default.len(),
        }
    }
    pub fn masked(label: &str, placeholder: &str) -> Self {
        Self {
            label: label.to_string(),
            value: String::new(),
            placeholder: placeholder.to_string(),
            kind: FieldKind::Masked,
            cursor: 0,
        }
    }
    pub fn select(label: &str, options: Vec<String>, default: usize) -> Self {
        let value = options.get(default).cloned().unwrap_or_default();
        Self {
            label: label.to_string(),
            value,
            placeholder: String::new(),
            kind: FieldKind::Select {
                options,
                selected: default,
            },
            cursor: 0,
        }
    }

    pub fn display_value(&self) -> String {
        match &self.kind {
            FieldKind::Text => {
                if self.value.is_empty() {
                    self.placeholder.clone()
                } else {
                    self.value.clone()
                }
            }
            FieldKind::Masked => {
                if self.value.is_empty() {
                    self.placeholder.clone()
                } else {
                    "●".repeat(self.value.len())
                }
            }
            FieldKind::Select { options, selected } => {
                options.get(*selected).cloned().unwrap_or_default()
            }
        }
    }
}

#[derive(Clone)]
pub struct FormState {
    pub fields: Vec<FormField>,
    pub focused: usize,
    pub error: Option<String>,
    pub success: Option<String>,
}

impl FormState {
    pub fn new(fields: Vec<FormField>) -> Self {
        Self {
            fields,
            focused: 0,
            error: None,
            success: None,
        }
    }

    pub fn focus_next(&mut self) {
        if self.focused + 1 < self.fields.len() {
            self.focused += 1;
        }
    }

    pub fn focus_prev(&mut self) {
        self.focused = self.focused.saturating_sub(1);
    }

    pub fn insert_char(&mut self, c: char) {
        let field = &mut self.fields[self.focused];
        match field.kind {
            FieldKind::Text | FieldKind::Masked => {
                field.value.insert(field.cursor, c);
                field.cursor += 1;
            }
            FieldKind::Select { .. } => {}
        }
    }

    pub fn backspace(&mut self) {
        let field = &mut self.fields[self.focused];
        if field.cursor > 0 {
            field.value.remove(field.cursor - 1);
            field.cursor -= 1;
        }
    }

    pub fn cycle_select(&mut self, forward: bool) {
        let field = &mut self.fields[self.focused];
        if let FieldKind::Select {
            ref options,
            ref mut selected,
        } = field.kind
        {
            if forward {
                *selected = (*selected + 1) % options.len();
            } else {
                *selected = selected.checked_sub(1).unwrap_or(options.len() - 1);
            }
            field.value = options[*selected].clone();
        }
    }
}

pub fn render(f: &mut Frame, area: Rect, state: &FormState) {
    let mut lines = Vec::new();

    for (i, field) in state.fields.iter().enumerate() {
        let is_focused = i == state.focused;
        let label_style = theme::bold();
        let value_display = field.display_value();
        let is_placeholder =
            field.value.is_empty() && !matches!(field.kind, FieldKind::Select { .. });

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {}", field.label),
            label_style,
        )));

        let field_style = if is_focused {
            theme::focused()
        } else if is_placeholder {
            theme::dim()
        } else {
            Style::default()
        };

        let prefix = if is_focused { "  > " } else { "    " };
        let select_hint = if matches!(field.kind, FieldKind::Select { .. }) && is_focused {
            "  (↑↓ to change)"
        } else {
            ""
        };

        lines.push(Line::from(vec![
            Span::styled(
                prefix.to_string(),
                if is_focused {
                    theme::accent()
                } else {
                    Style::default()
                },
            ),
            Span::styled(value_display, field_style),
            Span::styled(select_hint.to_string(), theme::dim()),
        ]));
    }

    // Error / success
    if let Some(ref err) = state.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  ✗ {}", err),
            theme::warning(),
        )));
    }
    if let Some(ref msg) = state.success {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {} {}", theme::ICON_CHECK, msg),
            theme::success(),
        )));
    }

    // Hint
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Tab: next field   Enter: submit   Esc: cancel",
        theme::dim(),
    )));

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}
