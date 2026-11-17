//! The active pane highlight and inactive-window fading, against a real Rex server named by
//! REXIA_TEST_SERVER.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{
    settings::{HighlightAnimation, HighlightColor, Settings},
    ui::theme::{Theme, color},
};
use serde_json::json;

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

fn tint(h: &mut Harness) -> gpui::Hsla {
    let window = h.window;
    h.cx.update(|cx| {
        let theme = Theme::get(cx).clone();
        window.read(cx).unwrap().highlight_color(&theme)
    })
}

#[test]
fn highlight_color_strength_motion_and_window_fade() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-highlight");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1100., 700.);
    let first = h.wait("a prompt", |m| {
        let block = m.view()?.active_window()?.focused_block_id.clone()?;
        (!focused_text(m).trim().is_empty()).then_some(block)
    });
    let split = common::request(
        &control,
        "session.new_split",
        json!({"session_id": session, "anchor_id": first, "direction": "horizontal",
            "side": "after", "layout": rexia::rex::host::shell_layout()}),
    )
    .unwrap();
    let second = split["block_ids"][0].as_str().unwrap().to_owned();
    h.wait("two panes", |m| {
        (m.view()?.active_window()?.panes().len() == 2).then_some(())
    });
    let theme = h.cx.update(|cx| Theme::get(cx).clone());

    // Colors: the accent by default, then a terminal color, then the text color.
    assert_eq!(tint(&mut h), theme.primary);
    settings(&mut h, |s| {
        s.interface.highlight_color = HighlightColor::Palette(4)
    });
    assert_eq!(tint(&mut h), color(theme.terminal.ansi[4]));
    settings(&mut h, |s| {
        s.interface.highlight_color = HighlightColor::Text
    });
    assert_eq!(tint(&mut h), color(theme.terminal.foreground));
    // Strength scales the color's opacity.
    settings(&mut h, |s| {
        s.interface.highlight_color = HighlightColor::Palette(4);
        s.interface.highlight_opacity = 0.5;
    });
    let half = tint(&mut h);
    println!("palette 4 at 50%: {half:?}");
    assert!((half.a - color(theme.terminal.ansi[4]).a * 0.5).abs() < 1e-3);
    // Headless windows don't advance animations, so the picture uses Instant.
    settings(&mut h, |s| {
        s.interface.highlight_animation = HighlightAnimation::Instant
    });
    h.screenshot("highlight-palette-instant");

    // Move: focusing the other pane moves the highlight from the old pane to the new one.
    settings(&mut h, |s| {
        s.interface.highlight_opacity = 1.;
        s.interface.highlight_animation = HighlightAnimation::Move;
    });
    let focus = |block: &str| json!({"session_id": session, "block_id": block});
    common::request(&control, "session.focus_block", focus(&first)).unwrap();
    h.wait("the first pane focused", |m| {
        (m.view()?.active_window()?.focused_block_id.as_deref() == Some(first.as_str()))
            .then_some(())
    });
    h.pump();
    common::request(&control, "session.focus_block", focus(&second)).unwrap();
    let (from, to) = h.wait_ui("the highlight to move", |ws| {
        // A prior split or left-focus animation can still be present until sync.
        ws.highlight_motion()
            .filter(|(from, to)| from != to && to.x > 0.)
    });
    println!("highlight moves from {from:?} to {to:?}");
    assert!(to.x > from.x, "to the pane on the right");

    // Fade panes for inactive windows: faded while unfocused, clear once focused.
    settings(&mut h, |s| s.interface.fade_inactive_windows = true);
    let unfocused = h.workspace(|ws| ws.window_fade());
    println!("window not focused: fade {unfocused:?}");
    assert_eq!(unfocused, Some((false, true)));
    h.activate();
    let focused = h.workspace(|ws| ws.window_fade());
    println!("window focused: fade {focused:?}");
    assert_eq!(focused, None);
    drop(sessions);
}
