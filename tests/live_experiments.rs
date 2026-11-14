//! Developer › Experimental against a real Rex server named by REXIA_TEST_SERVER:
//! terminal anchoring, and the new quick switcher listing each reporting program.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::settings::{Settings, TerminalAnchor};
use serde_json::Value;
use std::time::{Duration, Instant};

fn settings(h: &mut Harness, change: impl FnOnce(&mut Settings) + 'static) {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.update_settings(change, cx))
            .unwrap()
    });
    h.pump();
}

fn grid_top(h: &mut Harness) -> f32 {
    let window = h.window;
    h.pump();
    h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        f32::from(pane.read(cx).grid_origin().y)
    })
}

/// Go to Anything's rows, once its list has filled.
fn go_to(h: &mut Harness) -> Vec<String> {
    h.run("pane.go_to", Value::Null);
    let titles = h.workspace(|ws| ws.list_titles());
    h.press("escape");
    titles
}

#[test]
fn anchoring_and_the_quick_switcher() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-experiments");
    let mut h = Harness::open(&address, &sessions.ids[0], 1100., 716.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // Anchoring: at the bottom, the text moves down by what's left under its last line.
    // First the terminal settles at the rows that fit.
    let window = h.window;
    let deadline = Instant::now() + Duration::from_secs(10);
    let line = loop {
        h.pump();
        let settled = h.cx.update(|cx| {
            let ws = window.read(cx).unwrap();
            let pane = ws.focused_terminal()?;
            let pane = pane.read(cx);
            let rows = ws.model().panes.get(&pane.block_id)?.snapshot().frame?.rows;
            let line = f32::from(pane.cell_size()?.height);
            (pane.requested_size()?.1 == rows).then_some(line)
        });
        if let Some(line) = settled {
            break line;
        }
        assert!(Instant::now() < deadline, "the terminal never settled");
        std::thread::sleep(Duration::from_millis(30));
    };
    let top = grid_top(&mut h);
    settings(&mut h, |s| s.terminal.anchor = TerminalAnchor::Bottom);
    let bottom = grid_top(&mut h);
    println!("grid top: anchored top {top}, anchored bottom {bottom}");
    assert!(
        bottom > top && bottom - top < line,
        "down by less than a line ({line}px)"
    );
    h.screenshot("anchor-bottom");
    settings(&mut h, |s| s.terminal.anchor = TerminalAnchor::Top);
    assert_eq!(grid_top(&mut h), top);

    // One pane reports two programs: a deploy to US East working, EU West blocked.
    let script = std::env::temp_dir().join(format!("rexia-quick-{}.sh", std::process::id()));
    std::fs::write(
        &script,
        "printf '\\033]7501;state=working:id=us-east:title=VVMgRWFzdA==:msg=UHVzaGluZyBpbWFnZQ==\\033\\\\'\n\
         printf '\\033]7501;state=blocked:kind=permission:id=eu-west:title=RVUgV2VzdA==:msg=QXBwcm92ZSBkZXBsb3k/\\033\\\\'\n\
         sleep 60\n",
    )
    .unwrap();
    h.type_text(&format!("sh {}", script.display()));
    h.press("enter");
    let deadline = Instant::now() + Duration::from_secs(20);
    while h
        .host
        .snapshot()
        .blocks
        .values()
        .map(|b| b.records.len())
        .sum::<usize>()
        < 2
    {
        assert!(Instant::now() < deadline, "the reports never arrived");
        h.pump();
        std::thread::sleep(Duration::from_millis(50));
    }

    // Off, Go to Anything lists sessions, tabs and panes.
    let classic = go_to(&mut h);
    println!("Go to Anything: {classic:?}");
    assert!(!classic.iter().any(|t| t == "EU West"));
    // On, each reporting program has its own row, the blocked one first.
    settings(&mut h, |s| s.interface.quick_switcher = true);
    let quick = go_to(&mut h);
    println!("quick switcher: {quick:?}");
    assert_eq!(&quick[..2], ["EU West", "US East"]);
    assert!(quick.contains(&"rexia-experiments".to_owned()));
    h.run("pane.go_to", Value::Null);
    h.screenshot("quick-switcher");
    h.press("escape");
    let _ = std::fs::remove_file(&script);
    drop(sessions);
}
