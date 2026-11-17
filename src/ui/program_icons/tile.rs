//! Rounded logo tiles; shell tiles keep their terminal's exact colors.
use crate::ui::{icons::icon, text_color::luminance, theme::color};
use gpui::{AnyElement, IntoElement, div, prelude::*, px};

pub fn foreground(background: u32) -> u32 {
    let brightness = luminance(background);
    // Prefer a light logomark when it meets non-text contrast; use black on pale fills.
    if 1.05 / (brightness + 0.05) >= 3.0 {
        0xffffff
    } else {
        0x000000
    }
}

pub fn render(name: &'static str, side: f32, background: u32, ink: Option<u32>) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded(px(side * 0.24))
        .overflow_hidden()
        .border_1()
        .border_color(color(0xffffff).opacity(0.14))
        .bg(color(background))
        .child(
            icon(name)
                .size(px(side * if side < 24. { 0.72 } else { 0.6 }))
                .text_color(color(ink.unwrap_or_else(|| foreground(background)))),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_foreground_contrasts_with_flat_backgrounds() {
        for program in super::super::PROGRAMS
            .iter()
            .filter(|program| !program.is_shell())
        {
            let a = luminance(program.background);
            let b = luminance(
                program
                    .foreground
                    .unwrap_or_else(|| foreground(program.background)),
            );
            assert!(
                (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 3.0,
                "{}",
                program.id
            );
        }
    }
}
