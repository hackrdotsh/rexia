//! Interface › Animation and Terminal › Scrolling against a real Rex server named by
//! REXIA_TEST_SERVER: panes gliding and opening, the sidebar sliding, and touchpad
//! scrolling moving history by pixels.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{PlatformInput, ScrollDelta, ScrollEvent, ScrollSource, TouchPhase, point, px};
use rexia::settings::{AnimationStyle, Settings};
use serde_json::Value;

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

fn toggle_sidebar(h: &mut Harness) -> Option<bool> {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.toggle_sidebar(cx))
            .unwrap()
    });
    h.pump();
    h.workspace(|ws| ws.sidebar_sliding())
}

fn pane<R>(h: &mut Harness, read: impl FnOnce(&rexia::ui::terminal::TerminalPane) -> R) -> R {
    let window = h.window;
    h.pump();
    h.cx.update(|cx| {
        read(
            window
                .read(cx)
                .unwrap()
                .focused_terminal()
                .unwrap()
                .read(cx),
        )
    })
}

/// The focused pane's scrollback position: how many lines are above the viewport.
fn history_offset(h: &mut Harness) -> u64 {
    h.wait("the frame", |m| {
        let block = m.view()?.active_window()?.focused_block_id.clone()?;
        Some(m.panes.get(&block)?.snapshot().frame?.scrollbar.offset)
    })
}

/// A touchpad scroll of `lines` lines (positive moves into history).
fn touchpad(h: &mut Harness, lines: f32) {
    let at = pane(h, |p| p.cell_point(10, 5)).unwrap();
    let height = pane(h, |p| p.cell_size().unwrap().height);
    h.input(PlatformInput::Scroll(ScrollEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(0.), height * lines)),
        modifiers: Default::default(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    }));
    std::thread::sleep(std::time::Duration::from_millis(150));
    h.pump();
}

#[test]
fn panes_glide_sidebar_slides_and_history_scrolls_smoothly() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-motion");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    let first = h.wait("a prompt", |m| {
        let block = m.view()?.active_window()?.focused_block_id.clone()?;
        (!focused_text(m).trim().is_empty()).then_some(block)
    });

    // Natural: splitting moves the first pane over and the new one fades in.
    h.run("pane.split.right", Value::Null);
    let moving = h.wait_ui("panes moving", |ws| {
        let moving = ws.moving_panes();
        (moving.len() == 2).then_some(moving)
    });
    for (block, from, to) in &moving {
        println!(
            "moving {}: {from:?} -> {to:?}",
            if *block == first { "first" } else { "new" }
        );
    }
    let glide = moving.iter().find(|(b, _, _)| *b == first).unwrap();
    assert!(
        glide.1.is_some_and(|from| from.w > glide.2.w),
        "the first pane narrows"
    );
    assert!(
        moving
            .iter()
            .any(|(b, from, _)| *b != first && from.is_none()),
        "the new one opens"
    );

    // The sidebar slides out and back in.
    let hiding = toggle_sidebar(&mut h);
    let showing = toggle_sidebar(&mut h);
    println!("sidebar sliding: hiding {hiding:?}, showing {showing:?}");
    assert_eq!((hiding, showing), (Some(false), Some(true)));

    // Off: nothing moves.
    settings(&mut h, |s| s.interface.animation = AnimationStyle::Off);
    h.run("pane.split.down", Value::Null);
    std::thread::sleep(std::time::Duration::from_millis(400));
    h.pump();
    let still = h.workspace(|ws| ws.moving_panes().len());
    let slide = toggle_sidebar(&mut h);
    println!("with Off: {still} moving, sidebar {slide:?}");
    assert_eq!((still, slide), (0, None));
    toggle_sidebar(&mut h);

    // Smooth scrolling: history to scroll through, then touchpad scrolls of part of a line.
    h.type_text("seq 1 300");
    h.press("enter");
    h.wait("the output", |m| {
        focused_text(m).contains("300").then_some(())
    });
    // Wait until the output has pushed lines into history.
    let start = h.wait("history", |m| {
        let block = m.view()?.active_window()?.focused_block_id.clone()?;
        let bar = m.panes.get(&block)?.snapshot().frame?.scrollbar;
        (bar.offset > 0).then_some(bar.offset)
    });
    touchpad(&mut h, 0.4);
    let partial = (history_offset(&mut h), pane(&mut h, |p| p.scroll_offset()));
    touchpad(&mut h, 0.8);
    let whole = (history_offset(&mut h), pane(&mut h, |p| p.scroll_offset()));
    let height = pane(&mut h, |p| p.cell_size().unwrap().height);
    println!(
        "history at {start}; after 0.4 line: {partial:?}; after 1.2 lines: {whole:?} (line {height:?})"
    );
    // Less than a line only shifts the text; the next 0.8 completes a line.
    assert!((partial.1 - height * 0.4).abs() < px(0.5));
    assert_eq!(whole.0 + 1, partial.0, "a whole line moved");
    assert!((whole.1 - height * 0.2).abs() < px(0.5));

    // Off: a part of a line does nothing; whole lines move three at a time.
    settings(&mut h, |s| s.terminal.smooth_scrolling = false);
    let before = history_offset(&mut h);
    touchpad(&mut h, 0.4);
    let after = (history_offset(&mut h), pane(&mut h, |p| p.scroll_offset()));
    println!("smooth off, 0.4 line: history {before} -> {after:?}");
    assert_eq!(after, (before, px(0.)));
    drop(sessions);
}
