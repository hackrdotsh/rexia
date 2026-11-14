//! Trackpad gestures: a sideways swipe shows or hides the sidebar, and spreading two
//! fingers over a pane zooms it while pinching restores it.
use super::Workspace;
use crate::settings::SidebarGesture;
use gpui::{Context, PinchEvent, ScrollDelta, ScrollEvent, TouchPhase};
use std::time::{Duration, Instant};

/// How far fingers travel sideways before a swipe counts, in pixels.
const SWIPE_DISTANCE: f32 = 90.;
/// A pause this long ends a swipe; Wayland doesn't report when fingers lift.
const SWIPE_GAP: Duration = Duration::from_millis(250);
/// How much a pinch must spread or close before it zooms or restores a pane.
const PINCH_AMOUNT: f32 = 0.2;

/// Adds up one swipe's scrolling and reports it once, when it has gone far enough sideways.
#[derive(Default)]
pub struct SwipeTracker {
    dx: f32,
    dy: f32,
    last: Option<Instant>,
    fired: bool,
}

impl SwipeTracker {
    /// Feed a touchpad scroll; true the first time this swipe counts.
    pub fn feed(&mut self, dx: f32, dy: f32, now: Instant) -> bool {
        if self
            .last
            .is_none_or(|last| now.duration_since(last) > SWIPE_GAP)
        {
            *self = Self::default();
        }
        self.last = Some(now);
        self.dx += dx;
        self.dy += dy;
        if self.fired || self.dx.abs() < SWIPE_DISTANCE || self.dx.abs() < 2. * self.dy.abs() {
            return false;
        }
        self.fired = true;
        true
    }
}

/// Adds up one pinch on one pane.
#[derive(Default)]
pub struct PinchTracker {
    block: Option<String>,
    amount: f32,
    fired: bool,
}

/// What a pinch asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pinch {
    Zoom,
    Restore,
}

impl PinchTracker {
    /// Feed a pinch over `block`; what it asks for, the first time it goes far enough.
    pub fn feed(&mut self, block: &str, delta: f32, phase: TouchPhase) -> Option<Pinch> {
        if phase == TouchPhase::Started || self.block.as_deref() != Some(block) {
            *self = Self {
                block: Some(block.to_owned()),
                ..Self::default()
            };
        }
        if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.block = None;
            return None;
        }
        self.amount += delta;
        if self.fired || self.amount.abs() < PINCH_AMOUNT {
            return None;
        }
        self.fired = true;
        Some(if self.amount > 0. {
            Pinch::Zoom
        } else {
            Pinch::Restore
        })
    }
}

/// Whether a sideways swipe with Alt held (or not) toggles the sidebar.
pub fn swipe_toggles(gesture: SidebarGesture, alt: bool) -> bool {
    match gesture {
        // Hold Alt to scroll content sideways instead.
        SidebarGesture::Swipe => !alt,
        SidebarGesture::AltSwipe => alt,
        SidebarGesture::Off => false,
    }
}

impl Workspace {
    /// A scroll anywhere in the window: a touchpad swipe sideways may toggle the sidebar.
    pub fn on_swipe(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let interface = &self.settings.interface;
        if !interface.trackpad_gestures
            || !swipe_toggles(interface.sidebar_gesture, event.modifiers.alt)
        {
            return;
        }
        // Mouse wheels scroll by lines; only touchpads scroll by pixels.
        let ScrollDelta::Pixels(delta) = event.delta else {
            return;
        };
        if self
            .swipe
            .feed(f32::from(delta.x), f32::from(delta.y), Instant::now())
        {
            self.swipes += 1;
            self.toggle_sidebar(cx);
        }
    }

    /// A pinch over a pane: spreading zooms it, pinching restores it.
    pub fn on_pane_pinch(&mut self, block: &str, event: &PinchEvent, cx: &mut Context<Self>) {
        if !self.settings.interface.trackpad_gestures {
            return;
        }
        let Some(pinch) = self.pinch.feed(block, event.delta, event.phase) else {
            return;
        };
        let zoomed = self
            .model()
            .view()
            .and_then(|v| v.active_window())
            .and_then(|w| w.zoomed_block_id.clone());
        let wanted = match pinch {
            Pinch::Zoom => zoomed.is_none(),
            Pinch::Restore => zoomed.is_some(),
        };
        if wanted {
            self.toggle_zoom(zoomed.unwrap_or_else(|| block.to_owned()));
            cx.notify();
        }
    }

    /// How many swipes have toggled the sidebar, for tests.
    pub fn swipe_count(&self) -> usize {
        self.swipes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swipes_count_once_when_far_and_sideways_enough() {
        let start = Instant::now();
        let mut swipe = SwipeTracker::default();
        let at = |ms| start + Duration::from_millis(ms);
        assert!(!swipe.feed(40., 5., at(0)));
        assert!(swipe.feed(60., 5., at(10)), "100 px sideways counts");
        assert!(!swipe.feed(60., 0., at(20)), "only once per swipe");
        // After a pause, a new swipe; a mostly vertical one doesn't count.
        assert!(!swipe.feed(100., 80., at(400)));
        assert!(!swipe.feed(0., 0., at(410)));
        assert!(swipe.feed(-120., 0., at(800)), "either direction");
    }

    #[test]
    fn pinches_zoom_or_restore_once() {
        let mut pinch = PinchTracker::default();
        assert_eq!(pinch.feed("a", 0., TouchPhase::Started), None);
        assert_eq!(pinch.feed("a", 0.1, TouchPhase::Moved), None);
        assert_eq!(pinch.feed("a", 0.15, TouchPhase::Moved), Some(Pinch::Zoom));
        assert_eq!(pinch.feed("a", 0.3, TouchPhase::Moved), None);
        assert_eq!(pinch.feed("a", 0., TouchPhase::Ended), None);
        assert_eq!(
            pinch.feed("a", -0.3, TouchPhase::Started),
            Some(Pinch::Restore)
        );
    }

    #[test]
    fn alt_picks_which_swipes_toggle() {
        assert!(swipe_toggles(SidebarGesture::Swipe, false));
        assert!(!swipe_toggles(SidebarGesture::Swipe, true));
        assert!(swipe_toggles(SidebarGesture::AltSwipe, true));
        assert!(!swipe_toggles(SidebarGesture::AltSwipe, false));
        assert!(!swipe_toggles(SidebarGesture::Off, false));
    }
}
