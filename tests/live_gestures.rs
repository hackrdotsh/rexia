//! Trackpad gestures and the sidebar's text size, against a real Rex server named by
//! REXIA_TEST_SERVER: swipes toggle the sidebar, pinches zoom and restore a pane.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{
    Modifiers, PinchEvent, PlatformInput, ScrollDelta, ScrollEvent, ScrollSource, TouchPhase,
    point, px,
};
use rexia::settings::{Settings, SidebarGesture, SidebarTextSize};
use serde_json::json;
use std::time::Duration;

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

fn modifiers(alt: bool) -> Modifiers {
    Modifiers {
        alt,
        ..Default::default()
    }
}

/// A two-finger swipe sideways over the panes: five touchpad scrolls of 30 px, then a pause.
fn swipe(h: &mut Harness, alt: bool) {
    for _ in 0..5 {
        h.input(PlatformInput::Scroll(ScrollEvent {
            position: point(px(900.), px(400.)),
            delta: ScrollDelta::Pixels(point(px(30.), px(2.))),
            modifiers: modifiers(alt),
            touch_phase: TouchPhase::Moved,
            source: ScrollSource::Pointer,
        }));
    }
    std::thread::sleep(Duration::from_millis(300));
}

/// Spread (positive) or pinch (negative) two fingers over the point `x`, `y`.
fn pinch(h: &mut Harness, x: f32, y: f32, amount: f32) {
    for (phase, delta) in [
        (TouchPhase::Started, 0.),
        (TouchPhase::Moved, amount / 2.),
        (TouchPhase::Moved, amount / 2.),
        (TouchPhase::Ended, 0.),
    ] {
        h.input(PlatformInput::Pinch(PinchEvent {
            position: point(px(x), px(y)),
            delta,
            modifiers: Modifiers::default(),
            phase,
        }));
    }
}

fn sidebar(h: &mut Harness) -> (bool, usize) {
    h.workspace(|ws| (ws.settings.interface.sidebar, ws.swipe_count()))
}

#[test]
fn swipes_toggle_the_sidebar_and_pinches_zoom_panes() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-gestures");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1280., 800.);
    let first = h.wait("a prompt", |m| {
        let block = m.view()?.active_window()?.focused_block_id.clone()?;
        (!focused_text(m).trim().is_empty()).then_some(block)
    });
    common::request(
        &control,
        "session.new_split",
        json!({"session_id": session, "anchor_id": first, "direction": "horizontal",
            "side": "after", "layout": rexia::rex::host::shell_layout()}),
    )
    .unwrap();
    h.wait("two panes", |m| {
        (m.view()?.active_window()?.panes().len() == 2).then_some(())
    });
    assert!(sidebar(&mut h).0, "the sidebar starts shown");

    // Swipe: a sideways swipe hides the sidebar and another shows it; Alt scrolls instead.
    swipe(&mut h, false);
    let hidden = sidebar(&mut h);
    swipe(&mut h, true);
    let alt = sidebar(&mut h);
    swipe(&mut h, false);
    let shown = sidebar(&mut h);
    println!("Swipe: after swipe {hidden:?}, after alt-swipe {alt:?}, after swipe {shown:?}");
    assert_eq!((hidden, alt, shown), ((false, 1), (false, 1), (true, 2)));
    // A mouse wheel turned sideways scrolls by lines and never toggles.
    h.input(PlatformInput::Scroll(ScrollEvent {
        position: point(px(900.), px(400.)),
        delta: ScrollDelta::Lines(point(10., 0.)),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    }));
    assert_eq!(sidebar(&mut h), (true, 2));

    // Alt-swipe: only swipes with Alt held toggle.
    settings(&mut h, |s| {
        s.interface.sidebar_gesture = SidebarGesture::AltSwipe
    });
    swipe(&mut h, false);
    let plain = sidebar(&mut h);
    swipe(&mut h, true);
    let alt = sidebar(&mut h);
    println!("Alt-swipe: after swipe {plain:?}, after alt-swipe {alt:?}");
    assert_eq!((plain, alt), ((true, 2), (false, 3)));
    // Off, or trackpad gestures off: nothing toggles.
    settings(&mut h, |s| {
        s.interface.sidebar_gesture = SidebarGesture::Off
    });
    swipe(&mut h, true);
    settings(&mut h, |s| {
        s.interface.sidebar_gesture = SidebarGesture::Swipe;
        s.interface.trackpad_gestures = false;
    });
    swipe(&mut h, false);
    assert_eq!(sidebar(&mut h), (false, 3));
    settings(&mut h, |s| {
        s.interface.trackpad_gestures = true;
        s.interface.sidebar = true;
    });

    // Spreading over the right pane zooms it; pinching restores it.
    let zoomed = |h: &mut Harness| {
        h.workspace(|ws| {
            ws.model()
                .view()
                .and_then(|v| v.active_window())
                .and_then(|w| w.zoomed_block_id.clone())
        })
    };
    pinch(&mut h, 1100., 400., 0.4);
    let zoom = h.wait("a zoomed pane", |m| {
        m.view()?.active_window()?.zoomed_block_id.clone()
    });
    println!("spread over the right pane: zoomed {zoom}");
    assert_ne!(zoom, first, "the right pane, not the first");
    pinch(&mut h, 900., 400., -0.4);
    h.wait("the pane restored", |m| {
        m.view()?
            .active_window()?
            .zoomed_block_id
            .is_none()
            .then_some(())
    });
    println!("pinch: restored, zoomed {:?}", zoomed(&mut h));
    // With trackpad gestures off, a spread does nothing.
    settings(&mut h, |s| s.interface.trackpad_gestures = false);
    pinch(&mut h, 1100., 400., 0.4);
    std::thread::sleep(Duration::from_millis(300));
    h.pump();
    assert_eq!(zoomed(&mut h), None);

    // Sidebar text size: a picture of each.
    for (size, name) in [
        (SidebarTextSize::Small, "sidebar-text-small"),
        (SidebarTextSize::Large, "sidebar-text-large"),
    ] {
        settings(&mut h, move |s| s.interface.sidebar_text_size = size);
        h.screenshot(name);
    }
    drop(sessions);
}
