//! Autobalance splits against a real Rex server named by REXIA_TEST_SERVER: opening a pane
//! evens out the tab's columns, a divider moved from the keyboard stays where it was put
//! while the others even out, Balance Splits lets it go, and with the setting off nothing
//! moves.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::rex::host::HostModel;
use serde_json::{Value, json};

/// The tab's tiled panes, left to right, as (block, left edge, width).
fn columns(m: &HostModel) -> Option<Vec<(String, f64, f64)>> {
    let window = m.view()?.active_window()?;
    let mut panes: Vec<_> = window
        .panes()
        .into_iter()
        .filter(|p| p.floating.is_none())
        .map(|p| (p.block_id, p.rect.x, p.rect.w))
        .collect();
    panes.sort_by(|a, b| a.1.total_cmp(&b.1));
    Some(panes)
}

fn widths(m: &HostModel, count: usize) -> Option<Vec<f64>> {
    let columns = columns(m)?;
    (columns.len() == count).then(|| columns.iter().map(|c| c.2).collect())
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.02
}

#[test]
fn autobalance_evens_columns_and_keeps_placed_dividers() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-autobalance");
    let mut h = Harness::open(&address, &sessions.ids[0], 1200., 700.);
    h.wait("a prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });
    assert!(
        h.workspace(|ws| ws.settings.interface.autobalance_splits),
        "on by default"
    );

    // Splitting the right pane again would leave halves of a half; autobalance evens them.
    h.run("pane.split.right", Value::Null);
    h.wait("two panes", |m| widths(m, 2));
    h.run("pane.split.right", Value::Null);
    let even = h.wait("three even columns", |m| {
        let w = widths(m, 3)?;
        w.iter().all(|w| near(*w, 1. / 3.)).then_some(w)
    });
    println!("three columns: {even:.3?}");

    // Move the middle divider from the keyboard: it's pinned.
    let middle = h.wait("the middle pane", |m| Some(columns(m)?.get(1)?.0.clone()));
    let session = sessions.ids[0].clone();
    h.run(
        "pane.resize",
        json!({"direction": "right", "amount": 0.2, "block_id": middle, "session_id": session}),
    );
    let placed = h.wait("the moved divider", |m| {
        let w = widths(m, 3)?;
        (!near(w[1], 1. / 3.)).then_some(w)
    });
    let pinned = h.workspace(|ws| ws.pinned_splits());
    println!("after moving the middle divider: {placed:.3?}, pinned {pinned:?}");
    assert_eq!(pinned.len(), 1);

    // A fourth column: the pinned divider stays, the others even out.
    let last = h.wait("the last pane", |m| Some(columns(m)?.last()?.0.clone()));
    h.run(
        "pane.split.right",
        json!({"block_id": last, "session_id": session}),
    );
    let four = h.wait("four columns, rebalanced", |m| {
        let w = widths(m, 4)?;
        near(w[2], w[3]).then_some(w)
    });
    let edges = h.wait("the edges", columns);
    println!("four columns: {four:.3?}");
    println!(
        "middle divider before and after: {:.3} -> {:.3}",
        placed[0] + placed[1],
        edges[2].1
    );
    assert!(
        near(placed[0] + placed[1], edges[2].1),
        "the placed divider stayed"
    );

    // Balance Splits lets it go: four even columns.
    h.run("pane.balance", Value::Null);
    let balanced = h.wait("four even columns", |m| {
        let w = widths(m, 4)?;
        w.iter().all(|w| near(*w, 0.25)).then_some(w)
    });
    println!(
        "after Balance Splits: {balanced:.3?}, pinned {:?}",
        h.workspace(|ws| ws.pinned_splits())
    );
    assert!(h.workspace(|ws| ws.pinned_splits().is_empty()));

    // Off: closing a pane leaves the others as they are.
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| {
                ws.update_settings(|s| s.interface.autobalance_splits = false, cx)
            })
            .unwrap()
    });
    let first = h.wait("the first pane", |m| Some(columns(m)?.first()?.0.clone()));
    h.run(
        "pane.close",
        json!({"block_id": first, "session_id": session}),
    );
    let off = h.wait("three columns", |m| widths(m, 3));
    std::thread::sleep(std::time::Duration::from_millis(500));
    let still = h.wait("three columns again", |m| widths(m, 3));
    println!("with autobalance off, after closing one: {still:.3?}");
    assert_eq!(off, still);
    assert!(!still.iter().all(|w| near(*w, 1. / 3.)), "not rebalanced");
    drop(sessions);
}
