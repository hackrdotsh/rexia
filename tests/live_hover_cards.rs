//! Hover cards, against a real Rex server named by REXIA_TEST_SERVER: resting on a tab in
//! the background shows each of its panes with what its programs report (sub-agents each
//! on their own line, with messages and progress) and its directory; clicking a pane in
//! the card goes to it; resting on a pane header's title shows that pane alone.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{
    Bounds, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    PlatformInput, Point, point, px,
};
use rexia::{
    rex::host::HostModel,
    ui::workspace::hover_card::{CardTarget, ShownCard},
};
use serde_json::Value;
use std::time::{Duration, Instant};

const WIDTH: f32 = 1200.;
const HEIGHT: f32 = 760.;

fn focused(m: &HostModel) -> Option<String> {
    m.view()?.active_window()?.focused_block_id.clone()
}

fn active_tab(m: &HostModel) -> Option<String> {
    m.view()?.active_window_id.clone()
}

fn move_to(h: &mut Harness, at: Point<Pixels>) {
    h.input(PlatformInput::MouseMove(MouseMoveEvent {
        position: at,
        pressed_button: None,
        modifiers: Modifiers::default(),
    }));
}

fn bounds(h: &mut Harness, key: &str) -> Bounds<Pixels> {
    let key = key.to_owned();
    h.wait_ui(&format!("{key} drawn"), move |ws| ws.element_bounds(&key))
}

/// Rest the pointer on `at` and return the card that appears.
fn rest_on(h: &mut Harness, at: Point<Pixels>) -> (CardTarget, Vec<String>) {
    // Away first, long enough for an open card to close.
    move_to(h, point(px(WIDTH / 2.), px(HEIGHT - 40.)));
    h.cx.advance_clock(Duration::from_millis(1000));
    h.pump();
    h.cx.update(|cx| cx.set_global(ShownCard::default()));
    move_to(h, at);
    // GPUI's test clock moves only when told to.
    h.cx.advance_clock(Duration::from_millis(300));
    h.pump();
    let early = h.cx.update(|cx| cx.global::<ShownCard>().target.clone());
    assert_eq!(early, None, "cards wait before showing");
    h.cx.advance_clock(Duration::from_millis(300));
    h.pump();
    h.cx.update(|cx| {
        let card = cx.global::<ShownCard>();
        (
            card.target.clone().expect("a hover card"),
            card.lines.clone(),
        )
    })
}

#[test]
fn tabs_and_pane_headers_show_hover_cards() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-hover-cards");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, WIDTH, HEIGHT);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // The first pane runs a program with two sub-agents reporting, in its own directory.
    let reporter = h.wait("the first pane", focused);
    let dir = std::env::temp_dir().join(format!("rexia-card-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("report.sh");
    std::fs::write(
        &script,
        "printf '\\033]7501;state=working:app=builder:id=us-east:progress=40:msg=UHVzaGluZyBpbWFnZQ==\\033\\\\'\n\
         printf '\\033]7501;state=blocked:kind=permission:app=reviewer:id=eu-west:msg=QXBwcm92ZSBkZXBsb3k/\\033\\\\'\n\
         sleep 120\n",
    )
    .unwrap();
    // The shell reports its directory at its next prompt.
    h.type_text(&format!("cd {}", dir.display()));
    h.press("enter");
    let wanted = dir.display().to_string();
    h.wait("the shell's directory", |m| {
        m.blocks
            .get(&reporter)?
            .pwd
            .as_deref()?
            .ends_with(&wanted)
            .then_some(())
    });
    h.type_text("sh report.sh");
    h.press("enter");
    let deadline = Instant::now() + Duration::from_secs(20);
    while h
        .host
        .snapshot()
        .blocks
        .get(&reporter)
        .is_none_or(|b| b.records.len() < 2)
    {
        assert!(
            Instant::now() < deadline,
            "the reports never arrived: {:?}\n{}",
            h.host
                .snapshot()
                .blocks
                .get(&reporter)
                .map(|b| b.records.clone()),
            focused_text(&h.host.snapshot())
        );
        h.pump();
        std::thread::sleep(Duration::from_millis(50));
    }

    // A second pane beside it, then a second tab in front.
    h.run("pane.split.right", Value::Null);
    let other = h.wait("the second pane", |m| focused(m).filter(|f| *f != reporter));
    let first_tab = h.wait("the first tab", active_tab);
    h.run("client.tab.new", Value::Null);
    h.wait("the second tab", |m| {
        active_tab(m).filter(|t| *t != first_tab).map(|_| ())
    });

    // Resting on the first tab shows both its panes.
    let tab = bounds(&mut h, &format!("tab:{first_tab}"));
    let (target, lines) = rest_on(&mut h, tab.center());
    println!("tab card {target:?}:\n  {}", lines.join("\n  "));
    assert_eq!(target, CardTarget::Tab(first_tab.clone()));
    assert!(lines.contains(&"builder: Working · Pushing image · 40%".to_owned()));
    assert!(lines.contains(&"reviewer: Needs approval · Approve deploy?".to_owned()));
    assert!(
        lines.iter().any(|l| l.ends_with(&wanted)),
        "the pane's directory"
    );
    h.screenshot("hover-card-tab");

    // Clicking the reporting pane in the card goes to it.
    let rows = h.cx.update(|cx| cx.global::<ShownCard>().rows.clone());
    println!("card rows: {rows:?}");
    let (_, at) = rows
        .iter()
        .find(|(block, _)| *block == reporter)
        .expect("the reporting pane's row");
    let at = at.center();
    move_to(&mut h, at);
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
    h.wait("the reporting pane shown", |m| {
        (active_tab(m).as_deref() == Some(first_tab.as_str())
            && focused(m).as_deref() == Some(reporter.as_str()))
        .then_some(())
    });
    println!("clicking the pane in the card went to it");

    // A pane header's title shows that pane alone.
    let header = bounds(&mut h, &format!("header:{reporter}"));
    let (target, lines) = rest_on(&mut h, header.center());
    println!("pane card {target:?}:\n  {}", lines.join("\n  "));
    assert_eq!(target, CardTarget::Pane(reporter.clone()));
    assert!(lines.contains(&"builder: Working · Pushing image · 40%".to_owned()));
    assert!(lines.iter().any(|l| l.ends_with(&wanted)));
    let header = bounds(&mut h, &format!("header:{other}"));
    let (target, lines) = rest_on(&mut h, header.center());
    println!("other pane card {target:?}: {lines:?}");
    assert_eq!(
        lines,
        [wanted],
        "the inherited directory title isn't repeated"
    );
    assert_eq!(target, CardTarget::Pane(other.clone()));
    assert!(!lines.iter().any(|l| l.contains("builder")));
    h.screenshot("hover-card-pane");

    let _ = std::fs::remove_dir_all(&dir);
    drop(sessions);
}
