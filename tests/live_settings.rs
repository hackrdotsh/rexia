//! The settings page, driven by keys against a real Rex server named by REXIA_TEST_SERVER.
//! Screenshots go to REXIA_SHOT_DIR.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{settings::PaneStyle, ui::workspace::SettingsSection};
use serde_json::json;

#[test]
fn settings_page_opens_switches_sections_and_applies_changes() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-settings");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1280., 800.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // ctrl+, opens the page on Interface; Tab moves through the sections.
    h.press("ctrl-,");
    assert_eq!(
        h.workspace(|ws| ws.settings_section()),
        Some(SettingsSection::Interface)
    );
    h.screenshot("settings-interface");
    h.press("tab");
    assert_eq!(
        h.workspace(|ws| ws.settings_section()),
        Some(SettingsSection::Terminal)
    );

    // Changes apply at once: the panes take the new font size.
    let before = h.workspace(|ws| ws.terminal_font_size());
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.update_settings(
                    |s| {
                        s.terminal.font_size = Some(before + 2.);
                        s.interface.pane_style = PaneStyle::Compact;
                    },
                    cx,
                )
            })
            .unwrap()
    });
    h.pump();
    assert_eq!(h.workspace(|ws| ws.terminal_font_size()), before + 2.);
    h.screenshot("settings-terminal");
    // Terminal, Shortcuts, Notifications, then Client & Hosts.
    h.press("tab");
    assert_eq!(
        h.workspace(|ws| ws.settings_section()),
        Some(SettingsSection::Shortcuts)
    );
    h.press("tab");
    h.screenshot("settings-notifications");
    h.press("tab");
    assert_eq!(
        h.workspace(|ws| ws.settings_section()),
        Some(SettingsSection::Hosts)
    );
    h.screenshot("settings-hosts");

    // Esc closes it; the palette entry opens it again and its key closes it.
    h.press("escape");
    assert_eq!(h.workspace(|ws| ws.settings_section()), None);
    h.run("client.settings", json!({}));
    assert!(h.workspace(|ws| ws.settings_section().is_some()));
    h.press("ctrl-,");
    assert_eq!(h.workspace(|ws| ws.settings_section()), None);
    drop(sessions);
}
