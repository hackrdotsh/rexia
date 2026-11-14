//! The sidebar's tab filter, driven by keys against a real Rex server named by
//! REXIA_TEST_SERVER. Screenshots go to REXIA_SHOT_DIR.
mod common;

use common::{Harness, Sessions, focused_text};
use serde_json::{Value, json};

fn filter_focused(h: &mut Harness) -> bool {
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, window, cx| ws.filter_focused(window, cx))
            .unwrap()
    })
}

#[test]
fn tab_filter_narrows_sessions_and_tabs_and_shows_the_choice() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let mut sessions = Sessions::create(&control, "rexia-filter-alpha");
    let alpha = sessions.ids[0].clone();
    let beta = sessions.add("rexia-filter-beta");
    for label in ["build", "logs"] {
        common::request(
            &control,
            "session.new_window",
            json!({"session_id": alpha, "window_label": label,
                "layout": rexia::rex::host::shell_layout(), "focus": false}),
        )
        .unwrap();
    }
    let mut h = Harness::open(&address, &alpha, 1280., 800.);
    h.wait("a prompt and the tabs", |m| {
        let view = m.view()?;
        (view.windows.len() == 3 && !focused_text(m).trim().is_empty()).then_some(())
    });

    // The palette's Focus Tab Filter puts the cursor in the sidebar's field.
    h.run("client.sidebar.filter.focus", Value::Null);
    assert!(filter_focused(&mut h), "the filter field has focus");
    h.type_text("logs");
    let titles = h.workspace(|ws| ws.filter_titles());
    println!("filter 'logs' -> {titles:?}");
    assert_eq!(
        titles,
        vec!["rexia-filter-alpha".to_owned(), "logs".to_owned()]
    );
    h.screenshot("tab-filter");

    // Enter shows the highlighted tab, then the filter empties and the pane has focus.
    h.press("enter");
    h.wait("the logs tab to show", |m| {
        (m.view()?.active_window()?.label == "logs").then_some(())
    });
    assert!(h.workspace(|ws| ws.filter_titles().is_empty()));
    assert!(!filter_focused(&mut h));

    // A session name matches too, and its tabs come with it; arrows move the highlight.
    h.run("client.sidebar.filter.focus", Value::Null);
    h.type_text("filter-be");
    let titles = h.workspace(|ws| ws.filter_titles());
    println!("filter 'filter-be' -> {titles:?}");
    assert_eq!(
        titles.first().map(String::as_str),
        Some("rexia-filter-beta")
    );
    assert!(!titles.iter().any(|t| t == "rexia-filter-alpha"));
    h.press("down");
    h.press("up");
    h.press("enter");
    h.wait("the beta session to show", |m| {
        (m.selected.as_deref() == Some(beta.as_str())).then_some(())
    });
    println!("enter on 'filter-be' showed {beta}");

    // Nothing matching says so; Esc clears it and gives the pane focus back.
    h.run("client.sidebar.filter.focus", Value::Null);
    h.type_text("zzqx");
    assert!(h.workspace(|ws| ws.filter_titles().is_empty()));
    h.press("escape");
    assert!(!filter_focused(&mut h));
    assert!(h.workspace(|ws| ws.filter_titles().is_empty()));
    sessions.track(beta);
    drop(sessions);
}
