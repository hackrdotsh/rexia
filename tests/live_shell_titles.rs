//! Terminal › Shell and Titles against a real Rex server named by REXIA_TEST_SERVER: a
//! startup command for new terminals, and generated titles over titles programs set.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{settings::Settings, ui::titles};

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

fn tab_title(m: &rexia::rex::host::HostModel) -> Option<String> {
    let window = m.view()?.active_window()?;
    Some(titles::tab_title(window, m))
}

#[test]
fn startup_command_and_generated_titles() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-shell-titles");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // A startup command with arguments runs in new local terminals.
    settings(&mut h, |s| {
        s.terminal.startup_command = r#"sh -c 'echo "rexia startup $((40 + 2))"; exec sh'"#.into()
    });
    let window = h.window;
    h.cx.update(|cx| window.update(cx, |ws, _, cx| ws.new_tab(cx)).unwrap());
    let text = h.wait("the startup command's output", |m| {
        let text = focused_text(m);
        text.contains("rexia startup 42").then_some(text)
    });
    println!(
        "new tab shows: {:?}",
        text.lines().find(|l| l.contains("startup")).unwrap()
    );

    // A title the program sets wins, until generated titles are preferred.
    h.type_text(r"printf '\033]0;set by the shell\007'");
    h.press("enter");
    let set = h.wait("the program's title", |m| {
        tab_title(m).filter(|t| t == "set by the shell")
    });
    println!("title set by the program: {set}");
    settings(&mut h, |s| s.terminal.prefer_generated_titles = true);
    let generated = h.wait("a generated title", |m| {
        tab_title(m).filter(|t| t != "set by the shell" && t.contains(" · "))
    });
    println!("generated title: {generated}");
    assert!(generated.starts_with("sh · "), "{generated}");
    h.screenshot("generated-titles");
    settings(&mut h, |s| {
        s.terminal.prefer_generated_titles = false;
        s.terminal.startup_command.clear();
    });
    assert_eq!(
        h.workspace(|ws| tab_title(&ws.model())).as_deref(),
        Some("set by the shell")
    );
    drop(sessions);
}
