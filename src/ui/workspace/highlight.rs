//! The active pane's highlight: its color and strength, how it reaches a newly focused pane,
//! and fading panes while the window isn't focused.
use super::Workspace;
use crate::{
    rex::view::Rect,
    settings::{FadeInactive, HighlightAnimation, HighlightColor},
    ui::theme::{Theme, color},
};
use gpui::{
    Animation, AnimationExt, AnyElement, Hsla, IntoElement, ParentElement, Styled, div, px,
    relative,
};
use std::time::Duration;

/// How long the highlight takes to fade or move to a newly focused pane.
pub const HIGHLIGHT_MOTION: Duration = Duration::from_millis(180);

/// Where the highlight is, and where it came from when focus last moved.
#[derive(Default)]
pub struct Track {
    pub block: Option<String>,
    pub rect: Option<Rect>,
    pub from: Option<Rect>,
    /// Bumped on each focus change, so each change animates afresh.
    pub moves: u64,
}

fn lerp(from: Rect, to: Rect, t: f32) -> Rect {
    let t = f64::from(t);
    Rect {
        x: from.x + (to.x - from.x) * t,
        y: from.y + (to.y - from.y) * t,
        w: from.w + (to.w - from.w) * t,
        h: from.h + (to.h - from.h) * t,
    }
}

impl Workspace {
    /// The highlight's color at its chosen strength.
    pub fn highlight_color(&self, theme: &Theme) -> Hsla {
        let interface = &self.settings.interface;
        let mut tint = match interface.highlight_color {
            HighlightColor::Accent => theme.primary,
            HighlightColor::Palette(index) => {
                color(theme.terminal.ansi[usize::from(index.min(15))])
            }
            HighlightColor::Text => color(theme.terminal.foreground),
        };
        tint.a *= interface.highlight_opacity.clamp(0.2, 1.0);
        tint
    }

    /// Note the focused pane, remembering where the highlight was so it can move.
    pub(super) fn track_highlight(&mut self, block: Option<String>, rect: Option<Rect>) {
        if self.highlight.block != block {
            self.highlight.from = self.highlight.rect;
            self.highlight.block = block;
            self.highlight.moves += 1;
        }
        self.highlight.rect = rect;
    }

    /// Where a moving highlight goes from and to, while it uses Move.
    pub fn highlight_motion(&self) -> Option<(Rect, Rect)> {
        (self.settings.interface.highlight_animation == HighlightAnimation::Move)
            .then(|| Some((self.highlight.from?, self.highlight.rect?)))
            .flatten()
    }

    /// Which parts of every pane fade because the window isn't focused: (header, content).
    pub fn window_fade(&self) -> Option<(bool, bool)> {
        if !self.settings.interface.fade_inactive_windows || self.window_active {
            return None;
        }
        Some(match self.settings.interface.fade_inactive {
            FadeInactive::Surface => (true, false),
            FadeInactive::Both => (true, true),
            // Without a pane fade to follow, the text fades.
            FadeInactive::Content | FadeInactive::Off => (false, true),
        })
    }

    /// A highlight that moves from the previous pane to the focused one, drawn over both.
    pub(super) fn render_moving_highlight(&self, gap: f32, theme: &Theme) -> Option<AnyElement> {
        let (from, to) = self.highlight_motion()?;
        let motion = self.settings.interface.animation.duration();
        let tint = self.highlight_color(theme);
        let radius = theme.radius();
        let width = theme.border_width.max(px(1.));
        let place = move |rect: Rect| {
            div()
                .absolute()
                .left(relative(rect.x as f32))
                .top(relative(rect.y as f32))
                .w(relative(rect.w as f32))
                .h(relative(rect.h as f32))
                .p(px(gap / 2.))
                .child(
                    div()
                        .size_full()
                        .rounded(radius)
                        .border(width)
                        .border_color(tint),
                )
        };
        // With animation off, the highlight is simply where focus is.
        let Some(motion) = motion else {
            return Some(place(to).into_any_element());
        };
        Some(
            place(to)
                .with_animation(
                    ("highlight-move", self.highlight.moves as usize),
                    Animation::new(motion).with_easing(gpui::ease_in_out),
                    move |_, t| place(lerp(from, to, t)),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_highlight_moves_in_a_straight_line() {
        let a = Rect {
            x: 0.,
            y: 0.,
            w: 0.5,
            h: 1.,
        };
        let b = Rect {
            x: 0.5,
            y: 0.,
            w: 0.5,
            h: 0.5,
        };
        assert_eq!(lerp(a, b, 0.), a);
        assert_eq!(lerp(a, b, 1.), b);
        let mid = lerp(a, b, 0.5);
        assert!((mid.x - 0.25).abs() < 1e-9 && (mid.h - 0.75).abs() < 1e-9);
    }
}
