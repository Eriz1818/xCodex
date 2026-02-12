use crate::color::blend;
use crate::color::is_light;
use crate::terminal_palette::best_color;
use crate::terminal_palette::default_bg;
use ratatui::style::Color;
use ratatui::style::Style;

pub fn user_message_style() -> Style {
    user_message_style_for(default_bg())
}

pub fn proposed_plan_style() -> Style {
    proposed_plan_style_for(crate::theme::transcript_bg_rgb())
}

/// Returns the style for a user-authored message using the provided terminal background.
pub fn user_message_style_for(terminal_bg: Option<(u8, u8, u8)>) -> Style {
    match terminal_bg {
        Some(bg) => Style::default().bg(user_message_bg(bg)),
        None => Style::default(),
    }
}

/// Returns proposed-plan style using transcript surface as the color anchor.
pub fn proposed_plan_style_for(transcript_bg: Option<(u8, u8, u8)>) -> Style {
    let mut style = crate::theme::transcript_style();
    if let Some(bg) = transcript_bg {
        style = style.bg(proposed_plan_bg(bg));
    }
    style
}

#[allow(clippy::disallowed_methods)]
pub fn user_message_bg(terminal_bg: (u8, u8, u8)) -> Color {
    let top = if is_light(terminal_bg) {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    };
    best_color(blend(top, terminal_bg, 0.1))
}

#[allow(clippy::disallowed_methods)]
pub fn proposed_plan_bg(transcript_bg: (u8, u8, u8)) -> Color {
    user_message_bg(transcript_bg)
}
