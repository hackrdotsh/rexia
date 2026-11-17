//! Keys, modes, init.lua bindings and remote control against a real Rex server. Set
//! REXIA_TEST_SERVER to a disposable server whose config is tests/fixtures/init.lua
//! (`rex server run --config tests/fixtures/init.lua`).
mod common;

use common::{Harness, Sessions, focused_text};
use serde_json::json;

#[test]
fn keys_modes_lua_bindings_and_remote_control() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-keys");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1200., 760.);

    let keymap = h.wait("the keymap and a live pane", |m| {
        let pane = m.view()?.active_window()?.focused_block_id.clone()?;
        m.panes.get(&pane)?.snapshot().frame?;
        m.keymap.clone()
    });
    let bindings = keymap["bindings"].as_array().unwrap();
    println!(
        "keymap: {} bindings, {} from init.lua, diagnostics {}",
        bindings.len(),
        bindings
            .iter()
            .filter(|b| b["source"]
                .as_str()
                .is_some_and(|s| s.starts_with("init.lua")))
            .count(),
        keymap["diagnostics"]
    );
    // Let the shell draw its prompt before typing into it.
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // A rexia default: ctrl+shift+t opens a tab.
    let before = h.host.snapshot().view().map_or(0, |v| v.windows.len());
    h.press("ctrl-shift-t");
    h.wait("ctrl+shift+t to open a tab", |m| {
        (m.view()?.windows.len() == before + 1).then_some(())
    });
    println!(
        "ctrl+shift+t (rexia default): tabs {before} -> {}",
        before + 1
    );

    // An init.lua key sequence: ctrl+a then c also opens a tab, with a hint in between.
    h.press("ctrl-a");
    assert!(
        h.workspace(|ws| ws.pending_keys() == 1),
        "ctrl+a waits for the next key"
    );
    h.press("c");
    h.wait("ctrl+a c to open a tab", |m| {
        (m.view()?.windows.len() == before + 2).then_some(())
    });
    println!("ctrl+a c (init.lua sequence): tabs -> {}", before + 2);

    // init.lua rebinds ctrl+shift+o from Split Right to a downward split.
    h.press("ctrl-shift-o");
    let direction = h.wait("ctrl+shift+o to split", |m| {
        let window = m.view()?.active_window()?;
        (window.panes().len() == 2).then(|| window.dividers()[0].horizontal)
    });
    assert!(
        !direction,
        "init.lua's downward split wins over rexia's default"
    );
    println!("ctrl+shift+o (init.lua override): split down");

    // An exclusive mode: keys it doesn't bind are swallowed; its own binding runs.
    h.press("ctrl-alt-r");
    assert_eq!(
        h.workspace(|ws| ws.active_mode()),
        Some("resize".to_owned())
    );
    h.press("enter");
    assert_eq!(h.workspace(|ws| ws.active_mode()), None);
    println!("ctrl+alt+r enters resize mode; enter (bound in the mode) leaves it");

    // A key bound to a Lua function goes to the server, which writes into the pane.
    h.press("ctrl-shift-u");
    h.wait("the Lua binding's output", |m| {
        focused_text(m).contains("rexia-lua-key").then_some(())
    });
    println!("ctrl+shift+u (Lua function): wrote rexia-lua-key");

    // Remote control: Quick Look no longer requires a pointer position; an unknown
    // action still fails rather than being acknowledged as performed.
    let client_id = h.host.snapshot().client_id.unwrap();
    let zoom = h.call_from(
        &control,
        "client.perform",
        json!({"client_id": client_id, "action": "pane.zoom", "session_id": session}),
    );
    assert!(zoom.is_ok(), "pane.zoom: {zoom:?}");
    h.wait("the zoom", |m| {
        m.view()?.active_window()?.zoomed_block_id.clone()
    });
    let preview = h.call_from(
        &control,
        "client.perform",
        json!({"client_id": client_id, "action": "client.quick_look"}),
    );
    println!("client.perform pane.zoom -> ok; client.quick_look -> {preview:?}");
    assert!(preview.is_ok());
    let missing = h.call_from(
        &control,
        "client.perform",
        json!({"client_id": client_id, "action": "client.nonexistent"}),
    );
    assert!(missing.is_err());

    // rex do: the CLI drives rexia by its client label.
    let inspect = h
        .call_from(&control, "client.inspect", json!({"client_id": client_id}))
        .unwrap();
    let declared = inspect["actions"].as_array().map_or(0, Vec::len);
    println!("client.inspect lists {declared} rexia actions");
    assert!(declared > 50);
}
