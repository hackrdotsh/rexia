//! Settings › Shortcuts against a real Rex server named by REXIA_TEST_SERVER (whose test
//! init.lua binds keys too): every action with its keys and their source, a warning for a
//! key two actions share, the filter, changing and removing a key from the page, resetting
//! one another action has taken (it asks first), and the changes as Lua.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::ui::workspace::{
    SettingsSection,
    shortcuts_page::{ShortcutRow, Source},
};

fn rows(h: &mut Harness, filter: &str) -> Vec<ShortcutRow> {
    let filter = filter.to_owned();
    h.workspace(move |ws| ws.shortcut_rows(&filter))
}

fn find(h: &mut Harness, action: &str) -> ShortcutRow {
    rows(h, "")
        .into_iter()
        .find(|r| r.action == action)
        .unwrap_or_else(|| panic!("no row for {action}"))
}

/// GPUI's spelling of keys shown as "Ctrl+Shift+F".
fn spelled(shown: &str) -> String {
    shown.to_lowercase().replace('+', "-")
}

fn record(h: &mut Harness, action: &str, key: &str) {
    let window = h.window;
    let action = action.to_owned();
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.record_from_settings(action.clone(), action.clone(), window, cx)
            })
            .unwrap()
    });
    h.pump();
    h.press(key);
    h.wait_ui("back on the Shortcuts page", |ws| {
        (ws.settings_section() == Some(SettingsSection::Shortcuts)).then_some(())
    });
    // The keymap comes back from the server with the change.
    std::thread::sleep(std::time::Duration::from_millis(300));
    h.pump();
}

#[test]
fn shortcuts_page_lists_changes_resets_and_exports() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-shortcuts-page");
    let mut h = Harness::open(&address, &sessions.ids[0], 1200., 900.);
    h.wait("a prompt and the keymap", |m| {
        (!focused_text(m).trim().is_empty() && m.keymap.is_some()).then_some(())
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

    // Every action, with its keys and where they come from.
    let all = rows(&mut h, "");
    let split_right = find(&mut h, "pane.split.right");
    let from_config = all
        .iter()
        .find(|r| matches!(r.source, Source::Config(_)))
        .cloned();
    println!(
        "{} actions; Split Right: {:?} ({:?}); from init.lua: {:?}",
        all.len(),
        split_right.keys,
        split_right.source,
        from_config
            .as_ref()
            .map(|r| (&r.action, &r.keys, &r.source))
    );
    assert!(all.len() > 100);
    assert_eq!(split_right.source, Source::Default);
    assert!(from_config.is_some(), "init.lua's bindings show their line");

    // The test init.lua gives ctrl+shift+o to a split too: two actions, one key.
    let conflicts = h.workspace(|ws| ws.shortcut_conflicts());
    println!("conflicts: {conflicts:?}");
    assert!(conflicts.iter().any(|c| c.contains("Split Pane Right")));

    // The filter narrows the list.
    let zoom = rows(&mut h, "zoom");
    println!(
        "filter \"zoom\": {:?}",
        zoom.iter().map(|r| &r.title).collect::<Vec<_>>()
    );
    assert!(
        !zoom.is_empty()
            && zoom.iter().all(|r| r.title.to_lowercase().contains("zoom")
                || r.action.contains("zoom")
                || r.keys.iter().any(|k| k.to_lowercase().contains("zoom")))
    );
    h.screenshot("settings-shortcuts");

    // Change a key from the page, then remove another.
    record(&mut h, "pane.zoom", "ctrl-alt-z");
    let zoomed = find(&mut h, "pane.zoom");
    println!(
        "Zoom Pane after Change…: {:?} ({:?}, and {:?})",
        zoomed.keys, zoomed.source, zoomed.config
    );
    // The test init.lua binds ctrl+a z to it as well; both show, yours first.
    assert!(zoomed.keys.contains(&"Ctrl+Alt+Z".to_owned()));
    assert_eq!(zoomed.source, Source::Yours);
    assert!(zoomed.config.is_some(), "init.lua's binding is noted");
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.remove_shortcut("client.tab.new", cx))
            .unwrap()
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    h.pump();
    let new_tab = find(&mut h, "client.tab.new");
    println!(
        "New Tab after Remove: {:?} ({:?})",
        new_tab.keys, new_tab.source
    );
    // Only init.lua's own key for it is left.
    assert_eq!(new_tab.keys, ["Ctrl+A C"]);
    assert!(matches!(new_tab.source, Source::Config(_)));

    // Find moves to another key; Zoom Pane takes Find's old one; resetting Find asks.
    let find_default = find(&mut h, "client.find.open").keys[0].clone();
    record(&mut h, "client.find.open", "ctrl-alt-f");
    record(&mut h, "pane.zoom", &spelled(&find_default));
    println!(
        "Find moved to {:?}; Zoom Pane took {find_default}: {:?}",
        find(&mut h, "client.find.open").keys,
        find(&mut h, "pane.zoom").keys
    );
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.reset_shortcut("client.find.open", window, cx)
            })
            .unwrap()
    });
    let question = h.wait_ui("the reset question", |ws| ws.confirm_title());
    println!("reset asks: {question:?}");
    h.press("tab");
    h.press("enter");
    std::thread::sleep(std::time::Duration::from_millis(300));
    h.pump();
    let (find_after, zoom_after) = (find(&mut h, "client.find.open"), find(&mut h, "pane.zoom"));
    println!(
        "after Reset: Find {:?} ({:?}); Zoom Pane {:?} ({:?})",
        find_after.keys, find_after.source, zoom_after.keys, zoom_after.source
    );
    assert_eq!(find_after.keys[0], find_default);
    assert_eq!(find_after.source, Source::Default);
    assert!(
        !zoom_after.keys.contains(&find_default),
        "Zoom Pane gave the key back"
    );

    // The changes as Lua.
    let lua = h.workspace(|ws| ws.shortcuts_as_lua());
    println!("as Lua:\n{lua}");
    assert!(lua.contains("rex.unbind(") && lua.contains("\"client.tab.new\""));
    drop(sessions);
}
