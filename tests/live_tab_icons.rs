//! Tab icons: how many pane icons each tab shows and which draw in color, against a real
//! Rex server named by REXIA_TEST_SERVER. Screenshots go to REXIA_SHOT_DIR.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{
    settings::{Settings, ShellIconTheme, TabIconColor, ToolbarLayout},
    ui::{
        theme::Theme,
        titles::Program,
        workspace::tab_icons::{TabIcon, icon_look},
    },
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

fn icons(h: &mut Harness, tab: &str, vertical: bool) -> Vec<TabIcon> {
    h.workspace(|ws| ws.tab_icon_list(tab, vertical, &ws.model()))
}

fn colored(icons: &[TabIcon]) -> Vec<bool> {
    icons.iter().map(|i| i.colored).collect()
}

#[test]
fn tab_icons_count_color_and_follow_programs() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-tab-icons");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1280., 800.);
    let (first_tab, first) = h.wait("a prompt", |m| {
        let window = m.view()?.active_window()?;
        let block = window.focused_block_id.clone()?;
        (!focused_text(m).trim().is_empty()).then(|| (window.id().to_owned(), block))
    });
    // Four panes in the first tab, and a second tab in the background.
    for _ in 0..3 {
        common::request(
            &control,
            "session.new_split",
            json!({"session_id": session, "anchor_id": first, "direction": "horizontal",
                "side": "after", "layout": rexia::rex::host::shell_layout()}),
        )
        .unwrap();
    }
    common::request(
        &control,
        "session.new_window",
        json!({"session_id": session, "window_label": "other",
            "layout": rexia::rex::host::shell_layout(), "focus": false}),
    )
    .unwrap();
    let second_tab = h.wait("four panes and two tabs", |m| {
        let view = m.view()?;
        let ready = view.windows.len() == 2 && view.active_window()?.panes().len() == 4;
        ready.then(|| {
            view.windows
                .iter()
                .find(|w| w.id() != first_tab)
                .map(|w| w.id().to_owned())
        })?
    });
    common::request(
        &control,
        "session.focus_block",
        json!({"session_id": session, "block_id": first}),
    )
    .unwrap();
    h.wait("the first pane focused", |m| {
        (m.view()?.active_window()?.focused_block_id.as_deref() == Some(first.as_str()))
            .then_some(())
    });

    // Three icons by default; horizontal tabs go up to five, vertical tabs stop at three.
    assert_eq!(icons(&mut h, &first_tab, false).len(), 3);
    settings(&mut h, |s| s.interface.tab_icons = 5);
    let horizontal = icons(&mut h, &first_tab, false);
    let vertical = icons(&mut h, &first_tab, true);
    println!(
        "5 icons: horizontal {}, vertical {}",
        horizontal.len(),
        vertical.len()
    );
    assert_eq!((horizontal.len(), vertical.len()), (4, 3));
    settings(&mut h, |s| s.interface.tab_icons = 0);
    assert!(icons(&mut h, &first_tab, false).is_empty());
    settings(&mut h, |s| s.interface.tab_icons = 3);

    // Which icons draw in color.
    let modes = [
        TabIconColor::Always,
        TabIconColor::ActivePanes,
        TabIconColor::ActiveTabs,
        TabIconColor::Never,
    ];
    let mut seen = Vec::new();
    for mode in modes {
        settings(&mut h, move |s| s.interface.tab_icon_color = mode);
        let selected = colored(&icons(&mut h, &first_tab, false));
        let background = colored(&icons(&mut h, &second_tab, false));
        println!("{mode:?}: selected tab {selected:?}, other tab {background:?}");
        seen.push((selected, background));
    }
    assert_eq!(seen[0], (vec![true; 3], vec![true]));
    assert_eq!(seen[1], (vec![true, false, false], vec![true]));
    assert_eq!(seen[2], (vec![true; 3], vec![false]));
    assert_eq!(seen[3], (vec![false; 3], vec![false]));

    // The shell icon theme: the accent tile for a colored shell.
    settings(&mut h, |s| {
        s.interface.tab_icon_color = TabIconColor::Always;
        s.interface.shell_icon_theme = ShellIconTheme::Accent;
    });
    let theme = h.cx.update(|cx| Theme::get(cx).clone());
    let shell = icons(&mut h, &first_tab, false)[0];
    assert_eq!(shell.program, Program::Shell);
    assert_eq!(
        icon_look(shell, ShellIconTheme::Accent, theme.text, &theme).tile,
        Some(theme.primary)
    );

    // A program in the focused pane changes its icon, first in the stack.
    h.type_text("top");
    h.press("enter");
    let program = h.wait_ui("top's icon", |ws| {
        let first = *ws.tab_icon_list(&first_tab, false, &ws.model()).first()?;
        (first.program != Program::Shell).then_some(first.program)
    });
    println!("running top: {program:?}");
    assert_eq!(program, Program::Monitor);
    settings(&mut h, |s| {
        s.interface.shell_icon_theme = ShellIconTheme::Default
    });
    h.screenshot("tab-icons-default-artwork");
    settings(&mut h, |s| s.interface.toolbar_layout = ToolbarLayout::Left);
    h.screenshot("tab-icons-vertical");
    h.press("q");
    drop(sessions);
}
