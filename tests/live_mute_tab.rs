//! Mute Tab, against a real Rex server named by REXIA_TEST_SERVER and a private session bus:
//! right-clicking a tab offers Mute Tab; a muted tab's program sends no desktop notification
//! (neither its status nor its own OSC 9) and adds nothing to the launcher badge, but keeps
//! its status mark; the mute is saved and comes back with the settings; Unmute Tab brings
//! notifications back.
mod common;

use common::{Harness, Sessions, desktop::Desktop, focused_text};
use gpui::{Modifiers, MouseButton, MouseDownEvent, PlatformInput};
use rexia::{
    rex::{
        host::{HostCommand, HostHandle},
        view::View,
    },
    settings::Settings,
};
use serde_json::json;
use std::time::{Duration, Instant};

fn type_into(h: &mut Harness, session: &str, block: &str, line: &str) {
    let host = h.host.clone();
    type_via(h, &host, session, block, line);
}

/// Type a line through `host`'s connection, as another client would.
fn type_via(h: &mut Harness, host: &HostHandle, session: &str, block: &str, line: &str) {
    for (method, payload) in [
        (
            "block.event",
            json!({"session_id": session, "block_id": block,
                "event": {"type": "paste", "payload": {"text": line}}}),
        ),
        (
            "client.key",
            json!({"session_id": session, "block_id": block,
                "key": {"key": "enter", "mods": "", "text": "\r", "unshifted_codepoint": 13, "action": "press"}}),
        ),
    ] {
        host.send(HostCommand::Input {
            session_id: session.into(),
            block_id: block.into(),
            method,
            payload,
            claim: true,
        });
    }
    h.pump();
}

fn right_click_tab(h: &mut Harness, window_id: &str) -> Vec<String> {
    let key = format!("tab:{window_id}");
    let at = h
        .wait_ui("the tab drawn", move |ws| ws.element_bounds(&key))
        .center();
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Right,
        position: at,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    }));
    h.wait_ui("the tab menu", |ws| ws.context_menu_labels())
}

fn choose(h: &mut Harness, label: &str) {
    let window = h.window;
    let label = label.to_owned();
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.choose_context_item(&label, window, cx)
            })
            .unwrap()
    });
    h.pump();
}

#[test]
fn muted_tabs_stay_quiet_and_keep_their_mark() {
    let Some(address) = common::server() else {
        return;
    };
    let Some(desktop) = Desktop::start() else {
        eprintln!("skipped: no dbus-daemon");
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-mute");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1280., 800.);
    h.attach_desktop(&desktop.address);
    h.activate();
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // A second tab in the background.
    h.call_from(
        &control,
        "session.new_window",
        json!({"session_id": session, "window_label": "agent",
            "layout": rexia::rex::host::shell_layout(), "focus": false}),
    )
    .unwrap();
    let (tab, block) = h.wait("the background tab", |m| {
        let window = m.view()?.windows.iter().find(|w| w.label == "agent")?;
        let block = window.blocks().next()?.block_id.clone()?;
        Some((window.id().to_owned(), block))
    });
    let client = h.wait("the Rexia client", |m| m.client_id.clone());
    for muted in [true, true, false, false] {
        h.call_from(
            &control,
            "client.perform",
            json!({"client_id": client, "action": "client.tab.mute",
                "args": {"window_id": tab, "muted": muted}}),
        )
        .unwrap();
        assert_eq!(h.workspace(|ws| ws.tab_mutes()), [false, muted]);
    }
    h.wait("the background shell", |m| {
        m.blocks.get(&block).and_then(|b| b.process.name.clone())
    });

    // Right-clicking it offers Mute Tab.
    let menu = right_click_tab(&mut h, &tab);
    println!("tab menu: {menu:?}");
    assert_eq!(
        menu,
        ["Rename Tab…", "Mute Tab", "Close Other Tabs", "Close Tab"]
    );
    choose(&mut h, "Mute Tab");
    assert_eq!(h.workspace(|ws| ws.tab_mutes()), [false, true]);
    let saved = h.workspace(|ws| ws.settings.muted_tabs.clone());
    println!("saved mutes: {saved:?}");
    assert_eq!(saved.get("test"), Some(&vec![tab.clone()]));

    // Its program sends its own notification, then needs the user, then waits for a line.
    let dir = std::env::temp_dir().join(format!("rexia-mute-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("agent.sh");
    // A name of its own, so reports from other sessions on the server can't be mistaken
    // for it.
    let app = format!("agent{}", std::process::id());
    std::fs::write(
        &script,
        format!(
            "printf '\\033]9;{app} hello\\007'\n\
             printf '\\033]7501;state=blocked:kind=question:app={app}:msg=QWxsb3cgZWRpdD8=\\033\\\\'\n\
             read line\n\
             printf '\\033]7501;state=error:app={app}:msg=RmFpbGVk\\033\\\\'\n\
             sleep 120\n"
        ),
    )
    .unwrap();
    let ours = |s: &common::desktop::Shown| s.summary.contains(&app) || s.body.contains(&app);
    let badge_before = h.workspace(|ws| ws.badge_count());
    type_into(
        &mut h,
        &session,
        &block,
        &format!("sh {}", script.display()),
    );
    let marks = h.wait_ui("the muted tab's mark", |ws| {
        let marks = ws.tab_marks();
        marks.iter().any(Option::is_some).then_some(marks)
    });
    println!("tab marks while muted: {marks:?}");
    assert_eq!(marks, [None, Some("Question")], "the mark stays");
    let counted = h.workspace(|ws| ws.badge_panes());
    let records: Vec<_> = h
        .host
        .snapshot()
        .blocks
        .iter()
        .filter(|(_, b)| !b.records.is_empty())
        .map(|(id, b)| (id.clone(), b.records.keys().cloned().collect::<Vec<_>>()))
        .collect();
    assert_eq!(
        counted.len(),
        badge_before,
        "no badge count: counted {counted:?}, muted pane {block}, reports {records:?}"
    );
    // Give a notification time to arrive, were one sent.
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        h.pump();
        std::thread::sleep(Duration::from_millis(50));
    }
    let shown: Vec<_> = desktop.shown().into_iter().filter(|s| ours(s)).collect();
    println!("shown while muted: {shown:?}");
    assert!(shown.is_empty(), "a muted tab notifies nothing: {shown:?}");
    h.screenshot("mute-tab");

    // The mute comes back with the saved settings, as after a relaunch.
    let path = dir.join("settings.toml");
    h.workspace(|ws| ws.settings.save(&path)).unwrap();
    let reloaded = Settings::load(&path).unwrap();
    assert_eq!(reloaded.muted_tabs.get("test"), Some(&vec![tab.clone()]));
    let mut again = Harness::open_with(reloaded, 1280., 800.);
    let mutes = again.wait_ui("the relaunched tabs", |ws| {
        let mutes = ws.tab_mutes();
        (mutes.len() == 2).then_some(mutes)
    });
    println!("after relaunch: {mutes:?}");
    assert_eq!(mutes, [false, true]);
    drop(again);

    // Unmuted, the next change notifies and the badge counts again.
    let menu = right_click_tab(&mut h, &tab);
    assert!(menu.contains(&"Unmute Tab".to_owned()), "{menu:?}");
    choose(&mut h, "Unmute Tab");
    assert_eq!(h.workspace(|ws| ws.tab_mutes()), [false, false]);
    assert_eq!(h.workspace(|ws| ws.badge_count()), badge_before + 1);
    type_into(&mut h, &session, &block, "");
    let note = desktop.wait_shown(
        "the error notification",
        || h.pump(),
        |s| s.summary.contains(&app),
    );
    println!("after unmuting: {note:?}");
    assert_eq!(note.summary, format!("{app}: Error"));

    // A muted tab in a session rexia isn't showing, as one muted before a relaunch: rexia
    // looks up where its panes sit, so its program stays quiet too.
    let mut sessions = sessions;
    let elsewhere = sessions.add("rexia-mute-elsewhere");
    let view: View = serde_json::from_value(
        common::request(&control, "session.view", json!({"session_id": elsewhere})).unwrap(),
    )
    .unwrap();
    let far_tab = view.windows[0].id().to_owned();
    let far_block = view.windows[0]
        .blocks()
        .next()
        .unwrap()
        .block_id
        .clone()
        .unwrap();
    let window = h.window;
    let muted = far_tab.clone();
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.update_settings(
                    move |s| s.muted_tabs.entry("test".into()).or_default().push(muted),
                    cx,
                )
            })
            .unwrap()
    });
    let far_shell = far_block.clone();
    h.wait("the other session's shell", |m| {
        m.blocks
            .get(&far_shell)
            .and_then(|b| b.process.name.clone())
    });
    let far_app = format!("far{}", std::process::id());
    let far_script = dir.join("far.sh");
    std::fs::write(
        &far_script,
        format!(
            "printf '\\033]7501;state=blocked:kind=permission:app={far_app}:msg=T2s/\\033\\\\'\n\
             sleep 120\n"
        ),
    )
    .unwrap();
    let badge_now = h.workspace(|ws| ws.badge_count());
    type_via(
        &mut h,
        &control,
        &elsewhere,
        &far_block,
        &format!("sh {}", far_script.display()),
    );
    let far_report = far_block.clone();
    h.wait("the other session's report", |m| {
        (!m.blocks.get(&far_report)?.records.is_empty()).then_some(())
    });
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        h.pump();
        std::thread::sleep(Duration::from_millis(50));
    }
    let far_shown: Vec<_> = desktop
        .shown()
        .into_iter()
        .filter(|s| s.summary.contains(&far_app))
        .collect();
    println!("shown from the muted tab in another session: {far_shown:?}");
    assert!(
        far_shown.is_empty(),
        "a muted tab elsewhere notifies nothing"
    );
    assert_eq!(h.workspace(|ws| ws.badge_count()), badge_now, "nor counts");

    let _ = std::fs::remove_dir_all(&dir);
    drop(sessions);
}
