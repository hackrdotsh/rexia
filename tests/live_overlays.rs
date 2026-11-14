//! Palettes, dialogs and the find bar, driven by keys against a real Rex server named by
//! REXIA_TEST_SERVER. Screenshots go to REXIA_SHOT_DIR.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::ui::workspace::ListKind;
use serde_json::{Value, json};

#[test]
fn palettes_switchers_dialogs_and_find() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let mut sessions = Sessions::create(&control, "rexia-palettes");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1280., 800.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // The command palette: initials find Split Pane Right; Enter runs it.
    h.press("ctrl-shift-p");
    assert_eq!(h.workspace(|ws| ws.list_kind()), Some(ListKind::Commands));
    h.type_text("spr");
    let first = h.workspace(|ws| ws.list_titles().first().cloned());
    println!("palette 'spr' -> {first:?}");
    assert_eq!(first.as_deref(), Some("Split Pane Right"));
    h.screenshot("palette");
    h.press("enter");
    h.wait("the split", |m| {
        (m.view()?.active_window()?.panes().len() == 2).then_some(())
    });
    assert_eq!(h.workspace(|ws| ws.list_kind()), None);
    // The same key that opened the palette closes it.
    h.press("ctrl-shift-p");
    h.press("ctrl-shift-p");
    assert_eq!(h.workspace(|ws| ws.list_kind()), None);
    println!("palette: Enter split the pane; ctrl+shift+p toggles it");

    // Find in the focused pane: matches are counted and highlighted; Esc clears them.
    h.wait("the new pane's prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    h.type_text("echo needle-one needle-two");
    h.press("enter");
    h.wait("the echo", |m| {
        (focused_text(m).matches("needle").count() >= 4).then_some(())
    });
    h.press("ctrl-shift-f");
    h.type_text("needle");
    let found = h.wait("search results", |m| {
        let block = m.view()?.active_window()?.focused_block_id.clone()?;
        let (total, _) = m.panes.get(&block)?.snapshot().frame?.search?;
        (total >= 4).then_some(total)
    });
    println!("find 'needle': {found} matches");
    h.screenshot("find");
    h.press("escape");
    assert!(!h.workspace(|ws| ws.has_find()));

    // Rename the tab.
    h.run("client.tab.rename", Value::Null);
    h.press("ctrl-a");
    h.type_text("build");
    h.press("enter");
    let label = h.wait("the new tab name", |m| {
        let window = m.view()?.active_window()?;
        (window.label == "build").then(|| window.label.clone())
    });
    println!("rename tab -> {label}");

    // The session switcher names a new session.
    h.press("ctrl-shift-k");
    h.type_text("rexia-made-here");
    let offer = h.workspace(|ws| ws.list_titles().last().cloned());
    println!("switcher offers {offer:?}");
    h.screenshot("switcher");
    h.press("enter");
    let created = h.wait("the new session", |m| {
        let s = m.sessions.iter().find(|s| s.label == "rexia-made-here")?;
        (m.selected.as_ref() == Some(&s.session_id)).then(|| s.session_id.clone())
    });
    sessions.track(created.clone());
    println!("switcher created and showed rexia-made-here");

    // Go to Directory lists directories from the server and opens a tab there.
    h.wait("the new session's prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    let tabs = h.host.snapshot().view().map_or(0, |v| v.windows.len());
    h.press("ctrl-alt-g");
    h.type_text("/tm");
    let listed = h.wait_ui("the directory listing", |ws| {
        let titles = ws.list_titles();
        titles.iter().any(|t| t == "tmp/").then_some(titles)
    });
    println!("go to directory '/tm' lists {listed:?}");
    h.screenshot("directory");
    h.press("enter");
    h.wait("a tab in /tmp", |m| {
        (m.view()?.windows.len() == tabs + 1).then_some(())
    });

    // Go to Anything lists sessions, tabs and panes.
    h.press("ctrl-shift-space");
    let everything = h.workspace(|ws| ws.list_titles().len());
    println!("go to anything lists {everything} items");
    assert!(everything >= 4);
    h.screenshot("go-to");
    h.press("escape");

    // Closing a session asks first, and Enter keeps it running.
    h.run("session.close", Value::Null);
    assert!(h.workspace(|ws| ws.has_dialog()));
    h.screenshot("confirm-close");
    h.press("enter");
    assert!(!h.workspace(|ws| ws.has_dialog()));
    assert!(
        h.host.snapshot().session(&created).is_some(),
        "Enter kept the session"
    );
    h.run("session.close", Value::Null);
    h.press("tab");
    h.press("enter");
    h.wait("the session to close", |m| {
        m.session(&created).is_none().then_some(())
    });
    println!("close session: Enter keeps it; Tab then Enter closes it");

    // Add a remote host: an unusable address is explained; a good one is saved on the server.
    h.run("client.host.add", Value::Null);
    h.type_text("studio");
    h.press("tab");
    h.type_text("ssh://me@studio");
    h.press("enter");
    let problem = h.workspace(|ws| ws.dialog_error());
    println!("add host ssh:// -> {problem:?}");
    assert!(problem.is_some());
    h.screenshot("add-host");
    h.press("ctrl-a");
    h.type_text("https://studio.example.ts.net");
    h.press("enter");
    let saved = h.wait("the saved host", |m| {
        m.hosts
            .iter()
            .find(|host| host["label"] == "studio")
            .map(|host| host["endpoints"].clone())
    });
    h.wait_ui("the host in the workspace", |ws| {
        (ws.hosts.len() == 2).then_some(())
    });
    println!("add host saved on the home server: {saved}");
    h.screenshot("hosts");

    // Remove it again, through the list and the confirmation.
    h.run("client.host.remove", Value::Null);
    h.press("enter");
    h.press("tab");
    h.press("enter");
    h.wait("the host to go", |m| {
        (!m.hosts.iter().any(|host| host["label"] == "studio")).then_some(())
    });
    h.wait_ui("the workspace to drop it", |ws| {
        (ws.hosts.len() == 1).then_some(())
    });
    println!("remove host: gone from the server and the sidebar");

    let leftover: Vec<Value> = h.host.snapshot().hosts;
    assert!(leftover.iter().all(|host| host["label"] != json!("studio")));
}
