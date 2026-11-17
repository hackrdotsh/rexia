//! Clicks on what floats over the panes stay there, against a real Rex server named by
//! REXIA_TEST_SERVER: clicking inside Settings (over the terminal behind it) doesn't
//! focus that terminal or close Settings, and neither does clicking a notice.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, PlatformInput, Point, point, px,
};
use rexia::ui::workspace::SettingsSection;

fn click(h: &mut Harness, at: Point<Pixels>) {
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    }));
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::default(),
        click_count: 1,
    }));
}

fn terminal_focused(h: &mut Harness) -> bool {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.focused_terminal()
                    .is_some_and(|t| t.read(cx).has_focus(window))
            })
            .unwrap()
    })
}

#[test]
fn clicks_inside_floating_things_stay_there() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-overlay-clicks");
    let mut h = Harness::open(&address, &sessions.ids[0], 1200., 800.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.open_settings(window, cx);
                ws.show_settings_section(SettingsSection::Shortcuts, window, cx);
            })
            .unwrap()
    });
    h.pump();
    let before = terminal_focused(&mut h);

    // Inside the panel, over the terminal behind it: a row's empty middle, then a row lower down.
    for at in [point(px(700.), px(420.)), point(px(760.), px(600.))] {
        click(&mut h, at);
        let (focused, section) = (
            terminal_focused(&mut h),
            h.workspace(|ws| ws.settings_section()),
        );
        println!(
            "click at {at:?}: terminal focused {focused} (before {before}), settings {section:?}"
        );
        assert!(!focused, "the terminal behind Settings took the click");
        assert_eq!(
            section,
            Some(SettingsSection::Shortcuts),
            "Settings stays open"
        );
    }
    h.screenshot("overlay-click");

    // Outside the panel, on the dimmed backdrop: Settings closes, as before.
    click(&mut h, point(px(1180.), px(780.)));
    let closed = h.workspace(|ws| ws.settings_section());
    println!("click on the backdrop: settings {closed:?}");
    assert_eq!(closed, None);

    // A notice floating over the pane: clicking it doesn't reach the terminal either.
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.notice = Some("A notice to click on".into());
                ws.open_settings(window, cx);
                ws.close_overlay(window, cx);
                window.blur(cx);
                cx.notify();
            })
            .unwrap()
    });
    h.pump();
    let notice = h.wait_ui("the notice drawn", |ws| ws.notice_bounds());
    click(&mut h, point(notice.origin.x + px(60.), notice.center().y));
    let focused = terminal_focused(&mut h);
    println!("click on the notice: terminal focused {focused}");
    assert!(!focused, "the terminal under the notice took the click");
    drop(sessions);
}
