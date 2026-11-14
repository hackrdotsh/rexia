//! Several rexia windows, against a real Rex server named by REXIA_TEST_SERVER: a tab moved
//! to its own window stays there while the first window switches tabs, preferences reach
//! every window, and New Window and Close Window open and close them.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{AnyWindowHandle, WindowHandle};
use rexia::{settings::PaneStyle, ui::workspace::Workspace};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn windows(h: &mut Harness) -> Vec<WindowHandle<Workspace>> {
    h.cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter_map(|w: AnyWindowHandle| w.downcast::<Workspace>())
            .collect()
    })
}

/// Wait for something in another window, as rexia keeps running.
fn wait_in<T>(
    h: &mut Harness,
    window: WindowHandle<Workspace>,
    what: &str,
    check: impl Fn(&Workspace) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        h.pump();
        let found = h.cx.update(|cx| window.read(cx).ok().and_then(&check));
        if let Some(found) = found {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// The label of the tab a window shows.
fn shown_tab(ws: &Workspace) -> Option<String> {
    Some(ws.model().view()?.active_window()?.label.clone())
}

#[test]
fn tabs_move_to_their_own_windows_and_windows_open_and_close() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let mut sessions = Sessions::create(&control, "rexia-windows");
    let session = sessions.ids[0].clone();
    for label in ["build", "logs"] {
        common::request(
            &control,
            "session.new_window",
            json!({"session_id": session, "window_label": label,
                "layout": rexia::rex::host::shell_layout(), "focus": label == "logs"}),
        )
        .unwrap();
    }
    let mut h = Harness::open(&address, &session, 1100., 700.);
    h.wait("a prompt in the logs tab", |m| {
        let view = m.view()?;
        (view.windows.len() == 3
            && view.active_window()?.label == "logs"
            && !focused_text(m).trim().is_empty())
        .then_some(())
    });
    let first = h.window;

    // Move the logs tab out: a second window shows it, the first shows the tab before it.
    h.run("client.tab.move_to_new_window", Value::Null);
    let all = windows(&mut h);
    assert_eq!(all.len(), 2, "a second window opened");
    let second = *all.iter().find(|w| **w != first).unwrap();
    let moved = wait_in(&mut h, second, "the moved tab", |ws| {
        shown_tab(ws).filter(|l| l == "logs")
    });
    let stayed = wait_in(&mut h, first, "the tab left behind", |ws| {
        shown_tab(ws).filter(|l| l == "build")
    });
    println!("moved: second window shows {moved}, first shows {stayed}");

    // Switching tabs in the first window leaves the second where it is.
    h.run("client.tab.previous", Value::Null);
    let switched = wait_in(&mut h, first, "the first window's new tab", |ws| {
        // The first tab, whose label the server made up.
        shown_tab(ws).filter(|l| l != "build" && l != "logs")
    });
    let kept = wait_in(&mut h, second, "the second window's tab", shown_tab);
    println!("after Previous Tab in the first: first shows {switched}, second shows {kept}");
    assert_eq!(kept, "logs");
    let shot = h.cx.capture_screenshot(second.into()).unwrap();
    let path = common::shot_dir().join("window-moved-tab.png");
    shot.save(&path).unwrap();
    println!("saved {}", path.display());

    // A preference changed in one window reaches the other.
    h.cx.update(|cx| {
        first
            .update(cx, |ws, _, cx| {
                ws.update_settings(|s| s.interface.pane_style = PaneStyle::Compact, cx)
            })
            .unwrap()
    });
    let style = wait_in(&mut h, second, "the shared preference", |ws| {
        (ws.settings.interface.pane_style == PaneStyle::Compact).then_some(PaneStyle::Compact)
    });
    println!("pane style in the second window: {style:?}");

    // New Window opens a third window with a session of its own.
    h.press("ctrl-shift-n");
    let third = {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            h.pump();
            if let Some(w) = windows(&mut h)
                .into_iter()
                .find(|w| *w != first && *w != second)
            {
                break w;
            }
            assert!(Instant::now() < deadline, "no new window");
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    let fresh = wait_in(&mut h, third, "the new window's session", |ws| {
        ws.model().selected.filter(|s| *s != session)
    });
    sessions.track(fresh.clone());
    println!("new window shows its own session {fresh}");

    // Close Window closes it; the others stay.
    h.cx.update(|cx| {
        third
            .update(cx, |ws, window, cx| {
                ws.run_action(
                    "client.window.close",
                    Value::Null,
                    "palette",
                    Default::default(),
                    window,
                    cx,
                )
                .unwrap();
            })
            .unwrap()
    });
    h.pump();
    let left = windows(&mut h).len();
    println!("after Close Window: {left} windows");
    assert_eq!(left, 2);
    drop(sessions);
}
