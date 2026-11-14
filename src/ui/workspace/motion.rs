//! Interface › Animation: panes gliding to their new places, new panes fading in, and the
//! sidebar sliding in and out, at the chosen animation style. While things move,
//! terminals keep their size and resize once at the end, so the server isn't asked for a
//! new size on every frame.
use super::Workspace;
use crate::rex::view::Rect;
use gpui::Context;
use std::{collections::HashMap, time::Instant};

/// Where a pane is and, while it moves, where it came from.
#[derive(Clone, Copy, Debug)]
pub struct PaneMotion {
    pub rect: Rect,
    pub from: Option<Rect>,
    /// The pane is new: it fades in rather than moving.
    pub opening: bool,
    pub started: Instant,
    /// Bumped on each move, so each one animates afresh.
    pub moves: u64,
}

/// What's moving: panes, and the sidebar showing (true) or hiding (false).
#[derive(Default)]
pub struct Motion {
    pub panes: HashMap<String, PaneMotion>,
    pub sidebar_shown: Option<bool>,
    pub sidebar: Option<(bool, Instant, u64)>,
    /// The tab drawn last: switching tabs shows its panes at once rather than fading in.
    pub window: Option<String>,
    moves: u64,
}

/// The easing curve for a style: Snappy starts fast and settles; Natural eases in and out.
pub fn easing(style: crate::settings::AnimationStyle) -> fn(f32) -> f32 {
    match style {
        crate::settings::AnimationStyle::Snappy => |t| 1. - (1. - t).powi(4),
        _ => gpui::ease_in_out,
    }
}

/// What a pane draws now: its place, and the move it's making.
pub enum PanePlace {
    Still,
    Moving { from: Rect, moves: u64 },
    Opening { moves: u64 },
}

impl Workspace {
    fn motion_running(&self, started: Instant) -> bool {
        self.settings
            .interface
            .animation
            .duration()
            .is_some_and(|d| started.elapsed() < d)
    }

    /// Note where a pane is drawn, and whether it's moving: a pane whose place changed
    /// glides there; a new one fades in. `first` is true while the tab first shows, so its
    /// panes don't all fade in at once.
    pub(super) fn place_pane(
        &mut self,
        block: &str,
        rect: Rect,
        dragging: bool,
        first: bool,
        cx: &mut Context<Self>,
    ) -> PanePlace {
        let animate = self.settings.interface.animation.duration().is_some() && !dragging;
        let now = Instant::now();
        let previous = self.motion.panes.get(block).copied();
        let changed = match previous {
            None => !first,
            Some(p) => p.rect != rect,
        };
        if changed && animate {
            self.motion.moves += 1;
            let moves = self.motion.moves;
            self.motion.panes.insert(
                block.to_owned(),
                PaneMotion {
                    rect,
                    from: previous.map(|p| p.rect),
                    opening: previous.is_none(),
                    started: now,
                    moves,
                },
            );
            self.hold_terminals(cx);
        } else if changed || previous.is_none() || dragging {
            self.motion.panes.insert(
                block.to_owned(),
                PaneMotion {
                    rect,
                    from: None,
                    opening: false,
                    started: now,
                    moves: previous.map_or(0, |p| p.moves),
                },
            );
        }
        let motion = self.motion.panes[block];
        if !self.motion_running(motion.started) {
            return PanePlace::Still;
        }
        match (motion.from, motion.opening) {
            (Some(from), _) => PanePlace::Moving {
                from,
                moves: motion.moves,
            },
            (None, true) => PanePlace::Opening {
                moves: motion.moves,
            },
            (None, false) => PanePlace::Still,
        }
    }

    /// Forget panes that aren't shown any more.
    pub(super) fn keep_pane_motion(&mut self, shown: &[String]) {
        self.motion.panes.retain(|block, _| shown.contains(block));
    }

    /// The sidebar's slide: whether it's showing or hiding, its progress so far being
    /// GPUI's to draw, and its key. `None` when it isn't moving.
    pub(super) fn sidebar_slide(
        &mut self,
        shown: bool,
        cx: &mut Context<Self>,
    ) -> Option<(bool, u64)> {
        let last = self.motion.sidebar_shown.replace(shown);
        if last.is_some_and(|last| last != shown)
            && self.settings.interface.animation.duration().is_some()
        {
            self.motion.moves += 1;
            self.motion.sidebar = Some((shown, Instant::now(), self.motion.moves));
            self.hold_terminals(cx);
        }
        let (showing, started, moves) = self.motion.sidebar?;
        if self.motion_running(started) {
            Some((showing, moves))
        } else {
            self.motion.sidebar = None;
            None
        }
    }

    /// Keep every terminal's size until motion ends, then draw once more so they fit.
    fn hold_terminals(&mut self, cx: &mut Context<Self>) {
        let Some(duration) = self.settings.interface.animation.duration() else {
            return;
        };
        let until = Instant::now() + duration;
        for pane in self.panes.values() {
            pane.update(cx, |pane, _| pane.hold_size_until(until));
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(duration + std::time::Duration::from_millis(20))
                .await;
            let _ = this.update(cx, |ws, cx| {
                for pane in ws.panes.values() {
                    pane.update(cx, |_, cx| cx.notify());
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Panes moving now, and where from, for tests.
    pub fn moving_panes(&self) -> Vec<(String, Option<Rect>, Rect)> {
        self.motion
            .panes
            .iter()
            .filter(|(_, m)| self.motion_running(m.started) && (m.from.is_some() || m.opening))
            .map(|(block, m)| (block.clone(), m.from, m.rect))
            .collect()
    }

    /// Whether the sidebar is sliding, and which way (true showing), for tests.
    pub fn sidebar_sliding(&self) -> Option<bool> {
        let (showing, started, _) = self.motion.sidebar?;
        self.motion_running(started).then_some(showing)
    }
}

/// `from` moved toward `to` by `t`, from 0 to 1.
pub fn lerp(from: Rect, to: Rect, t: f32) -> Rect {
    let t = f64::from(t);
    Rect {
        x: from.x + (to.x - from.x) * t,
        y: from.y + (to.y - from.y) * t,
        w: from.w + (to.w - from.w) * t,
        h: from.h + (to.h - from.h) * t,
    }
}
