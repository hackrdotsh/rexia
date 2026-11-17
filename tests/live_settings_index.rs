//! Search, edit, reset and deep-link settings through the actual GPUI controls.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};
use rexia::ui::workspace::SettingsSection;
use std::{cell::RefCell, rc::Rc};

#[test]
fn settings_search_links_resets_and_palette_share_the_index() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-settings-index");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.toml");
    let mut h = Harness::open_with_path(
        common::test_settings(&address, &sessions.ids[0]),
        Some(path.clone()),
        1250.,
        850.,
    );
    h.wait("prompt and keymap", |m| {
        (!focused_text(m).is_empty() && m.keymap.is_some()).then_some(())
    });
    let window = h.window;
    let opened = Rc::new(RefCell::new(Vec::new()));
    let record = opened.clone();
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, _| {
                ws.set_link_opener(move |link, _| record.borrow_mut().push(link.to_owned()))
            })
            .unwrap()
    });

    h.run("client.settings", serde_json::Value::Null);
    h.press("ctrl-f");
    h.type_text("font size");
    h.screenshot("settings-search");
    h.press("enter");
    assert_eq!(
        h.workspace(|ws| ws.settings_target().map(str::to_owned))
            .as_deref(),
        Some("terminal.font_size")
    );
    assert_eq!(
        h.workspace(|ws| ws.settings_section()),
        Some(SettingsSection::Terminal)
    );
    h.press("ctrl-a");
    h.type_text("19");
    h.press("enter");
    assert_eq!(h.workspace(|ws| ws.settings.terminal.font_size), Some(19.));
    assert_eq!(
        rexia::settings::Settings::load(&path)
            .unwrap()
            .terminal
            .font_size,
        Some(19.)
    );
    h.screenshot("settings-search-edit");

    h.run(
        "client.setting.terminal.font_size",
        serde_json::json!({"increase": true}),
    );
    assert_eq!(h.workspace(|ws| ws.settings.terminal.font_size), Some(20.));
    h.run(
        "client.setting.terminal.font_size",
        serde_json::json!({"value": 19}),
    );
    h.run(
        "client.setting.interface.panes.density",
        serde_json::json!({"value": "compact"}),
    );
    assert_eq!(
        h.workspace(|ws| ws.settings.interface.pane_style),
        rexia::settings::PaneStyle::Compact
    );

    // Bad values don't silently clamp or overwrite the last good value.
    h.press("ctrl-a");
    h.type_text("-4");
    h.press("enter");
    assert!(h.workspace(|ws| ws.setting_error().is_some()));
    assert_eq!(h.workspace(|ws| ws.settings.terminal.font_size), Some(19.));

    // Click the real reset button; font inheritance returns, rather than saving a number.
    let at = h
        .workspace(|ws| ws.element_bounds("setting-reset:terminal.font_size"))
        .unwrap()
        .center();
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
    assert_eq!(h.workspace(|ws| ws.settings.terminal.font_size), None);
    h.press("escape");
    assert_eq!(
        rexia::settings::Settings::load(&path)
            .unwrap()
            .terminal
            .font_size,
        None
    );

    // Resetting a text preference updates its field too, so closing cannot restore it.
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| {
                ws.open_indexed_setting("terminal.font_family", window, cx)
                    .unwrap();
            })
            .unwrap()
    });
    h.type_text("monospace");
    h.press("enter");
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.reset_indexed_setting("terminal.font_family", cx)
                    .unwrap();
            })
            .unwrap()
    });
    h.press("escape");
    assert_eq!(
        rexia::settings::Settings::load(&path)
            .unwrap()
            .terminal
            .font_family,
        None
    );

    // The palette exposes the same setting and opens its editor.
    h.press("ctrl-shift-p");
    h.type_text("font size");
    assert!(
        h.workspace(|ws| ws.list_titles())
            .iter()
            .any(|title| title == "Font size")
    );
    h.press("enter");
    assert_eq!(
        h.workspace(|ws| ws.settings_target().map(str::to_owned))
            .as_deref(),
        Some("terminal.font_size")
    );
    h.press("escape");

    // A clicked internal URI stays in Rexia, not the desktop browser.
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.open_link("rex://settings/notifications/sounds".into(), cx)
            })
            .unwrap()
    });
    h.wait_ui("internal settings link", |ws| {
        (ws.settings_target() == Some("notifications.sounds")).then_some(())
    });
    assert_eq!(
        h.workspace(|ws| ws.settings_section()),
        Some(SettingsSection::Notifications)
    );
    assert!(opened.borrow().is_empty());
    h.screenshot("settings-deep-link");
    h.press("escape");
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.open_link("rex://settings/terminal/typography".into(), cx)
            })
            .unwrap()
    });
    h.wait_ui("settings group link", |ws| {
        (ws.settings_group() == Some("typography")).then_some(())
    });
    h.press("escape");
    let action = "client.setting.interface.panes.density";
    let mut edited = rexia::settings::Settings::load(&path).unwrap();
    edited
        .shortcuts
        .insert(action.into(), vec!["ctrl+alt+9".into()]);
    edited.save(&path).unwrap();
    h.wait_ui("externally edited shortcut", |ws| {
        ws.settings.shortcuts.get(action).cloned()
    });
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| ws.close_overlay(window, cx))
            .unwrap()
    });
    h.pump();
    assert!(h.workspace(|ws| ws.settings_section().is_none()));
    h.press("ctrl-alt-9");
    assert_eq!(
        h.workspace(|ws| ws.settings.interface.pane_style),
        rexia::settings::PaneStyle::Comfortable
    );
    println!(
        "Search, palette, scalar edit, invalid value, Reset and internal setting/group links passed"
    );
    drop(sessions);
}
