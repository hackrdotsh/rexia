//! Asking before closing busy work, against a real Rex server named by REXIA_TEST_SERVER:
//! an idle pane closes at once; a pane whose program reports it's working asks first and
//! names the program; Escape keeps it, a click on the program goes to its pane, and Enter
//! closes. Closing other tabs asks the same about the busy ones.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::rex::host::HostModel;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn panes(m: &HostModel) -> Vec<String> {
    m.view()
        .and_then(|v| v.active_window())
        .map(|w| w.panes().into_iter().map(|p| p.block_id).collect())
        .unwrap_or_default()
}

fn focused(m: &HostModel) -> Option<String> {
    m.view()?.active_window()?.focused_block_id.clone()
}

fn asking(h: &mut Harness) -> Option<Vec<String>> {
    h.workspace(|ws| ws.confirm_programs())
}

#[test]
fn closing_busy_work_asks_first() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-close-busy");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1100., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // The first pane runs a program that says it's working.
    let busy = h.wait("the first pane", focused);
    let script = std::env::temp_dir().join(format!("rexia-busy-{}.sh", std::process::id()));
    std::fs::write(
        &script,
        "printf '\\033]7501;state=working:app=deploy:msg=UHVzaGluZw==\\033\\\\'\nsleep 120\n",
    )
    .unwrap();
    h.type_text(&format!("sh {}", script.display()));
    h.press("enter");
    let deadline = Instant::now() + Duration::from_secs(20);
    while h
        .host
        .snapshot()
        .blocks
        .get(&busy)
        .is_none_or(|b| b.records.is_empty())
    {
        assert!(Instant::now() < deadline, "the report never arrived");
        h.pump();
        std::thread::sleep(Duration::from_millis(50));
    }

    // An idle pane closes without a question.
    h.run("pane.split.right", Value::Null);
    let idle = h.wait("the new pane", |m| {
        focused(m).filter(|f| *f != busy && panes(m).len() == 2)
    });
    h.run(
        "pane.close",
        json!({"block_id": idle, "session_id": session}),
    );
    h.wait("the idle pane closed", |m| {
        (panes(m).len() == 1).then_some(())
    });
    assert_eq!(asking(&mut h), None, "no question for an idle pane");
    println!("idle pane closed without asking");

    // The busy one asks, naming its program; Escape keeps it.
    h.run(
        "pane.close",
        json!({"block_id": busy, "session_id": session}),
    );
    let named = h.wait_ui("the question", |ws| ws.confirm_programs());
    let title = h.workspace(|ws| ws.confirm_title());
    println!("closing the busy pane asks {title:?} about {named:?}");
    assert_eq!(named.len(), 1);
    assert!(named[0].ends_with("(Working)"));
    h.screenshot("close-busy");
    h.press("escape");
    assert_eq!(asking(&mut h), None);
    std::thread::sleep(Duration::from_millis(300));
    h.pump();
    assert_eq!(
        h.wait("the pane", |m| Some(panes(m))),
        std::slice::from_ref(&busy),
        "kept"
    );

    // A click on the program in the question goes to its pane, closing nothing.
    h.run("client.tab.new", Value::Null);
    let tabs = |m: &HostModel| m.view().map_or(0, |v| v.windows.len());
    h.wait("a second tab", |m| (tabs(m) == 2).then_some(()));
    h.run("client.tab.close_others", Value::Null);
    let named = h.wait_ui("the question about other tabs", |ws| ws.confirm_programs());
    println!("closing other tabs asks about {named:?}");
    let window = h.window;
    let target = busy.clone();
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| ws.go_to_busy(target, window, cx))
            .unwrap()
    });
    h.wait("the busy pane shown", |m| {
        (focused(m).as_deref() == Some(busy.as_str())).then_some(())
    });
    println!(
        "clicking the program showed its pane; tabs still {}",
        h.wait("tabs", |m| Some(tabs(m)))
    );
    assert_eq!(h.wait("tabs", |m| Some(tabs(m))), 2);

    // From the idle tab, Enter closes the busy one.
    h.run("client.tab.next", Value::Null);
    h.wait("the idle tab", |m| {
        focused(m).filter(|f| *f != busy).map(|_| ())
    });
    h.run("client.tab.close_others", Value::Null);
    h.wait_ui("the question again", |ws| ws.confirm_programs());
    h.press("enter");
    h.wait("one tab left", |m| (tabs(m) == 1).then_some(()));
    println!("Enter closed the busy tab");
    let _ = std::fs::remove_file(&script);
    drop(sessions);
}
