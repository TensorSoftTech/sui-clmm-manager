//! Palette tuned for black / dark terminals (aligned with sol-clmm-tui).

use ratatui::style::{Color, Modifier, Style};

pub const FG: Color = Color::Rgb(210, 220, 215);
pub const DIM: Color = Color::Rgb(90, 110, 105);
pub const MUTED: Color = Color::Rgb(130, 150, 145);
pub const ACCENT: Color = Color::Rgb(0, 210, 170);
pub const ACCENT_DIM: Color = Color::Rgb(0, 140, 115);
pub const WARN: Color = Color::Rgb(230, 170, 70);
pub const OK: Color = Color::Rgb(120, 210, 140);
pub const TOKEN_A: Color = Color::Rgb(0, 200, 180);
pub const TOKEN_B: Color = Color::Rgb(230, 160, 50);
pub const OUTSIDE: Color = Color::Rgb(70, 85, 80);
pub const SELECT_FG: Color = Color::Rgb(10, 20, 18);
pub const SELECT_BG: Color = Color::Rgb(0, 210, 170);

pub fn title() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn border() -> Style {
    Style::default().fg(ACCENT_DIM)
}

pub fn body() -> Style {
    Style::default().fg(FG)
}

pub fn muted() -> Style {
    Style::default().fg(MUTED)
}

pub fn dim() -> Style {
    Style::default().fg(DIM)
}

pub fn warn() -> Style {
    Style::default().fg(WARN)
}

pub fn ok() -> Style {
    Style::default().fg(OK)
}

pub fn highlight() -> Style {
    Style::default()
        .fg(SELECT_FG)
        .bg(SELECT_BG)
        .add_modifier(Modifier::BOLD)
}

pub fn accent_text() -> Style {
    Style::default().fg(ACCENT)
}

pub fn header_key() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn token_a() -> Style {
    Style::default().fg(TOKEN_A).add_modifier(Modifier::BOLD)
}

pub fn token_b() -> Style {
    Style::default().fg(TOKEN_B).add_modifier(Modifier::BOLD)
}

pub fn outside() -> Style {
    Style::default().fg(OUTSIDE)
}
