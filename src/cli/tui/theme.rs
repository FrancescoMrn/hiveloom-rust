use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders},
};

// ── Color palette ───────────────────────────────────────────────────────
pub const ACCENT: Color = Color::Cyan;
pub const DIM: Color = Color::DarkGray;
pub const SUCCESS: Color = Color::Green;
pub const WARNING: Color = Color::Yellow;
pub const TEXT: Color = Color::White;
pub const FOCUS_BG: Color = Color::Rgb(30, 50, 70);
pub const DANGER: Color = Color::Red;

// ── Icons ───────────────────────────────────────────────────────────────
pub const ICON_ONLINE: &str = "●";
pub const ICON_CURSOR: &str = "▸";
pub const ICON_CHECK: &str = "✓";
pub const ICON_ARROW: &str = "→";
pub const ICON_STEP_CURRENT: &str = "●";
pub const ICON_STEP_TODO: &str = "○";
pub const ICON_DOT: &str = "●";

// ── Styles ──────────────────────────────────────────────────────────────
pub fn accent() -> Style {
    Style::default().fg(ACCENT)
}

pub fn accent_bold() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(DIM)
}

pub fn bold() -> Style {
    Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
}

pub fn success() -> Style {
    Style::default().fg(SUCCESS)
}

pub fn warning() -> Style {
    Style::default().fg(WARNING)
}

pub fn focused() -> Style {
    Style::default().fg(TEXT).bg(FOCUS_BG)
}

pub fn focused_bold() -> Style {
    Style::default()
        .fg(TEXT)
        .bg(FOCUS_BG)
        .add_modifier(Modifier::BOLD)
}

// ── Block helpers ───────────────────────────────────────────────────────
pub fn rounded_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(format!(" {} ", title), accent_bold()))
}

pub fn rounded_block_plain() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM))
}
