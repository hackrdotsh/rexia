//! Several rexia windows, against a real Rex server named by REXIA_TEST_SERVER: a tab moved
//! to its own window stays there while the first window switches tabs, preferences reach
//! every window, and New Window and Close Window open and close them.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{
    AnyWindowHandle, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, PlatformInput, Point,
    WindowHandle, point, px,
};
use rexia::{settings::PaneStyle, ui::workspace::Workspace};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn windows(h: &mut Harness) -> Vec<WindowHandle<Workspace>> {
    h.cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter_map(|w: AnyWindowHandle| w.downcast::<Workspace>())
            .collect()
    })
}

/// Wait for something in another window, as rexia keeps running.
fn wait_in<T>(
    h: &mut Harness,
    window: WindowHandle<Workspace>,
    what: &str,
    check: impl Fn(&Workspace) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        h.cx.update(|cx| {
            let _ = window.update(cx, |ws, window, cx| {
                ws.sync(window, cx);
                window.refresh();
            });
        });
        h.pump();
        let found = h.cx.update(|cx| window.read(cx).ok().and_then(&check));
        if let Some(found) = found {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// The label of the tab a window shows.
fn shown_tab(ws: &Workspace) -> Option<String> {
    Some(ws.model().view()?.active_window()?.label.clone())
}

fn click(h: &mut Harness, position: Point<Pixels>) {
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position,
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    }));
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position,
        modifiers: Default::default(),
        click_count: 1,
    }));
}

#[test]
fn uneven_floating_split_keeps_title_and_controls_usable() {
    exercise_floating_header(false);
}

#[test]
fn uneven_floating_split_keeps_actions_accessible_with_sidebar() {
    exercise_floating_header(true);
}

fn exercise_floating_header(sidebar: bool) {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-uneven-floating-split");
    let session = &sessions.ids[0];
    let mut settings = common::test_settings(&address, session);
    settings.interface.sidebar = sidebar;
    settings.interface.animation = rexia::settings::AnimationStyle::Off;
    let mut h = Harness::open_with(settings, 480., 320.);
    h.run("pane.new_floating", Value::Null);
    let (layer_id, left) = h.wait("the floating terminal", |m| {
        let pane = m
            .view()?
            .active_window()?
            .panes()
            .into_iter()
            .find(|p| p.floating.is_some())?;
        Some((pane.floating?, pane.block_id))
    });
    h.run("pane.split.right", json!({"block_id": left}));
    let right = h.wait("the second floating terminal", |m| {
        let layer = m.view()?.active_window()?.layer_of(&left)?;
        rexia::rex::view::last_block(layer.layout.as_ref()?)
            .filter(|block| *block != left)
            .map(str::to_owned)
    });
    common::request(
        &control,
        "session.resize_layer",
        json!({
            "session_id": session, "layer_id": layer_id, "bounds": [0.78, 0.12, 0.2, 0.62]
        }),
    )
    .unwrap();
    common::request(
        &control,
        "session.resize_pane",
        json!({
            "session_id": session, "direction": "horizontal", "ratio": 0.95,
            "before_block_id": left, "after_block_id": right
        }),
    )
    .unwrap();
    h.wait("the uneven split at the right edge", |m| {
        let layer = m.view()?.active_window()?.layer_of(&left)?;
        (layer.rect().x > 0.7 && layer.layout.as_ref()?.split.as_ref()?.ratio > 0.9).then_some(())
    });

    let title = |h: &mut Harness, block: &str| {
        h.wait_ui("the floating title", |ws| {
            ws.element_bounds(&format!("header:{block}"))
        })
    };
    for block in [&left, &right] {
        let bounds = title(&mut h, block);
        println!(
            "Uneven floating split: title width {}px",
            f32::from(bounds.size.width)
        );
        assert!(
            bounds.size.width >= px(47.),
            "floating title and controls do not fit: {bounds:?}"
        );
    }
    screenshot(
        &mut h,
        if sidebar {
            "uneven-floating-split-sidebar"
        } else {
            "uneven-floating-split"
        },
    );

    let click_control = |h: &mut Harness, block, index: u8| {
        let bounds = title(h, block);
        let inline_index = if sidebar { 0 } else { index };
        let at = point(
            bounds.right() + px(17. + f32::from(inline_index) * 28.),
            bounds.center().y,
        );
        let area = h.workspace(Workspace::pane_area);
        assert!(
            at.x + px(11.) <= area.right(),
            "floating control is offscreen"
        );
        click(h, at);
        if sidebar {
            assert_eq!(
                h.workspace(Workspace::context_menu_labels).unwrap(),
                [
                    "Minimize",
                    "Split Right",
                    "Zoom Pane",
                    "Dock Pane",
                    "Close Pane"
                ],
            );
            if index == 0 {
                screenshot(h, "uneven-floating-split-menu");
            }
            // The menu anchors here, snapping to the window margin near the right edge.
            let x = (at.x + px(20.)).min(px(450.));
            click(h, point(x, at.y + px(17. + f32::from(index) * 26.)));
        }
    };
    click_control(&mut h, &right, 0);
    h.wait("Minimize clicked on the narrow pane", |m| {
        let layer = m.view()?.active_window()?.layer_of(&right)?;
        if layer.state.as_deref() != Some("minimized") {
            return None;
        }
        assert!(
            layer.rect().x > 0.7,
            "a button click saved the expanded layer bounds"
        );
        Some(())
    });
    h.run("pane.minimize_floating", json!({"block_id": right}));
    h.wait("the floating layer restored", |m| {
        (m.view()?
            .active_window()?
            .layer_of(&right)?
            .state
            .as_deref()
            != Some("minimized"))
        .then_some(())
    });

    click_control(&mut h, &right, 2);
    h.wait("Zoom clicked on the narrow pane", |m| {
        (m.view()?.active_window()?.zoomed_block_id.as_ref() == Some(&right)).then_some(())
    });
    h.run("pane.zoom", json!({"block_id": right}));
    h.wait("the split unzoomed", |m| {
        m.view()?
            .active_window()?
            .zoomed_block_id
            .is_none()
            .then_some(())
    });

    click_control(&mut h, &right, 1);
    let added = h.wait("Split Right clicked on the narrow pane", |m| {
        let layer = m.view()?.active_window()?.layer_of(&right)?;
        layer
            .blocks
            .iter()
            .filter_map(|block| block.block_id.as_ref())
            .find(|block| *block != &left && *block != &right)
            .cloned()
    });
    if !sidebar {
        common::request(
            &control,
            "block.close",
            json!({"session_id": session, "block_id": added}),
        )
        .unwrap();
        h.wait("the original two-pane split", |m| {
            m.view()?
                .active_window()?
                .block(&added)
                .is_none()
                .then_some(())
        });
    }

    click_control(&mut h, &right, 4);
    h.wait("Close clicked on the narrow pane", |m| {
        m.view()?
            .active_window()?
            .block(&right)
            .is_none()
            .then_some(())
    });
    click_control(&mut h, &left, 3);
    h.wait("Dock clicked on the remaining floating header", |m| {
        (m.view()?.active_window()?.layer_of(&left)?.kind == "tiled").then_some(())
    });
    println!("All five floating header actions clicked (sidebar: {sidebar})");
}

fn screenshot(h: &mut Harness, name: &str) {
    let path = common::shot_dir().join(format!("{name}.png"));
    h.cx.capture_screenshot(h.window.into())
        .unwrap()
        .save(&path)
        .unwrap();
    println!("saved {}", path.display());
}

#[test]
fn switching_tabs_draws_warm_frames_and_reuses_the_terminal() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-warm-tabs");
    let session = &sessions.ids[0];
    let mut h = Harness::open(&address, session, 920., 540.);
    let first = h
        .wait_ui("first terminal entity", |ws| ws.focused_terminal())
        .entity_id();
    let other = common::request(
        &control,
        "session.new_window",
        json!({
            "session_id": session, "window_label": "Warm", "focus": false,
        "layout": {"block": {"flavor": rexia::rex::data::SHELL_FLAVOR, "options": {
            "shell": "none", "command": ["sh", "-c", "printf 'warm-tab-ready\\n'; exec cat"]
        }}},
        }),
    )
    .unwrap();
    let block = other["block_ids"][0].as_str().unwrap();
    h.wait("an unpublished tab to have a frame before focus", |m| {
        let frame = m.panes.get(block)?.snapshot().frame?;
        (0..usize::from(frame.rows))
            .any(|row| frame.line_text(row).contains("warm-tab-ready"))
            .then_some(())
    });
    h.run("client.tab.next", Value::Null);
    h.wait("first view of the new active tab", |m| {
        (m.view()?.active_window()?.label == "Warm").then_some(())
    });
    assert!(focused_text(&h.host.snapshot()).contains("warm-tab-ready"));
    assert_ne!(
        h.workspace(|ws| ws.focused_terminal().unwrap().entity_id()),
        first
    );
    h.run("client.tab.previous", Value::Null);
    h.wait_ui("the first terminal entity to return", |ws| {
        (ws.focused_terminal()?.entity_id() == first).then_some(())
    });
}

#[test]
fn tabs_move_to_their_own_windows_and_windows_open_and_close() {
    let Some(address) = common::server() else {
        return;
    };
    let control = common::connect(&address);
    let mut sessions = Sessions::create(&control, "rexia-windows");
    let session = sessions.ids[0].clone();
    for label in ["build", "logs"] {
        common::request(
            &control,
            "session.new_window",
            json!({"session_id": session, "window_label": label,
                "layout": rexia::rex::host::shell_layout(), "focus": label == "logs"}),
        )
        .unwrap();
    }
    let mut h = Harness::open(&address, &session, 1100., 700.);
    h.wait("a prompt in the logs tab", |m| {
        let view = m.view()?;
        (view.windows.len() == 3
            && view.active_window()?.label == "logs"
            && !focused_text(m).trim().is_empty())
        .then_some(())
    });
    let first = h.window;

    // The moved tab also owns a minimized floating pane.
    h.run("pane.new_floating", Value::Null);
    let layer = h.wait("the floating layer", |m| {
        m.view()?
            .active_window()?
            .layers
            .iter()
            .find(|layer| layer.kind != "tiled")?
            .layer_id
            .clone()
    });
    h.run("pane.minimize_floating", Value::Null);
    h.wait("the floating layer to minimize", |m| {
        m.view()?
            .active_window()?
            .minimized_layers()
            .iter()
            .any(|minimized| minimized.layer_id.as_deref() == Some(&layer))
            .then_some(())
    });

    // Move the logs tab out: a second window shows it, the first shows the tab before it.
    h.run("client.tab.move_to_new_window", Value::Null);
    let all = windows(&mut h);
    assert_eq!(all.len(), 2, "a second window opened");
    let second = *all.iter().find(|w| **w != first).unwrap();
    let moved = wait_in(&mut h, second, "the moved tab", |ws| {
        shown_tab(ws).filter(|l| l == "logs")
    });
    let stayed = wait_in(&mut h, first, "the tab left behind", |ws| {
        shown_tab(ws).filter(|l| l == "build")
    });
    println!("moved: second window shows {moved}, first shows {stayed}");

    // Switching tabs in the first window leaves the second where it is.
    h.run("client.tab.previous", Value::Null);
    let switched = wait_in(&mut h, first, "the first window's new tab", |ws| {
        // The first tab, whose label the server made up.
        shown_tab(ws).filter(|l| l != "build" && l != "logs")
    });
    let kept = wait_in(&mut h, second, "the second window's tab", shown_tab);
    println!("after Previous Tab in the first: first shows {switched}, second shows {kept}");
    assert_eq!(kept, "logs");
    wait_in(&mut h, second, "its minimized layer", |ws| {
        let model = ws.model();
        model
            .view()?
            .active_window()?
            .minimized_layers()
            .iter()
            .any(|minimized| minimized.layer_id.as_deref() == Some(&layer))
            .then_some(())
    });
    h.cx.update(|cx| {
        second
            .update(cx, |ws, _, _| ws.restore_layer(layer.clone()))
            .unwrap()
    });
    let floated = wait_in(
        &mut h,
        second,
        "a restored floating terminal instead of a placeholder",
        |ws| {
            let model = ws.model();
            let tab = model.view()?.active_window()?;
            let pane = tab
                .panes()
                .into_iter()
                .find(|pane| pane.floating.as_deref() == Some(&layer))?;
            model
                .panes
                .get(&pane.block_id)?
                .snapshot()
                .frame
                .map(|_| pane.block_id)
        },
    );
    common::request(
        &control,
        "session.resize_layer",
        json!({
            "session_id": session, "layer_id": layer, "bounds": [0.88, 0.15, 0.10, 0.4]
        }),
    )
    .unwrap();
    let width = wait_in(
        &mut h,
        second,
        "a narrow floating header with room for the title",
        |ws| {
            let model = ws.model();
            let current = model.view()?.active_window()?.layer_of(&floated)?;
            if current.bounds.get(2).is_none_or(|width| *width > 0.13) {
                return None;
            }
            let title_width =
                f32::from(ws.element_bounds(&format!("header:{floated}"))?.size.width);
            (47. ..200.).contains(&title_width).then_some(title_width)
        },
    );
    println!("Narrow floating pane leaves {width}px for its title");
    h.cx.update(|cx| {
        second
            .update(cx, |ws, _, cx| {
                ws.split(Some(floated.clone()), true, true, cx)
            })
            .unwrap();
    });
    let widths = wait_in(&mut h, second, "both split floating headers to fit", |ws| {
        let model = ws.model();
        let current = model.view()?.active_window()?.layer_of(&floated)?;
        if current.blocks.len() != 2 {
            return None;
        }
        let mut widths = Vec::new();
        for block in &current.blocks {
            let bounds = ws.element_bounds(&format!("header:{}", block.block_id.as_deref()?))?;
            widths.push(f32::from(bounds.size.width));
        }
        widths.iter().all(|width| *width >= 47.).then_some(widths)
    });
    println!("Split floating pane title widths: {widths:?}");
    let shot = h.cx.capture_screenshot(second.into()).unwrap();
    let path = common::shot_dir().join("window-moved-tab.png");
    shot.save(&path).unwrap();
    println!("saved {}", path.display());

    // A preference changed in one window reaches the other.
    h.cx.update(|cx| {
        first
            .update(cx, |ws, _, cx| {
                ws.update_settings(|s| s.interface.pane_style = PaneStyle::Compact, cx)
            })
            .unwrap()
    });
    let style = wait_in(&mut h, second, "the shared preference", |ws| {
        (ws.settings.interface.pane_style == PaneStyle::Compact).then_some(PaneStyle::Compact)
    });
    println!("pane style in the second window: {style:?}");

    // New Window opens a third window with a session of its own.
    h.press("ctrl-shift-n");
    let third = {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            h.pump();
            if let Some(w) = windows(&mut h)
                .into_iter()
                .find(|w| *w != first && *w != second)
            {
                break w;
            }
            assert!(Instant::now() < deadline, "no new window");
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    let fresh = wait_in(&mut h, third, "the new window's session", |ws| {
        ws.model().selected.filter(|s| *s != session)
    });
    sessions.track(fresh.clone());
    println!("new window shows its own session {fresh}");

    h.cx.update(|cx| {
        third
            .update(cx, |ws, window, cx| {
                ws.run_action(
                    "client.close",
                    Value::Null,
                    "palette",
                    Default::default(),
                    window,
                    cx,
                )
                .unwrap();
            })
            .unwrap();
    });
    wait_in(
        &mut h,
        third,
        "the previous session after closing the only tab",
        |ws| {
            let model = ws.model();
            (model.selected.as_ref() == Some(&session) && model.session(&fresh).is_none())
                .then_some(())
        },
    );

    // Close Window closes it; the others stay.
    h.cx.update(|cx| {
        third
            .update(cx, |ws, window, cx| {
                ws.run_action(
                    "client.window.close",
                    Value::Null,
                    "palette",
                    Default::default(),
                    window,
                    cx,
                )
                .unwrap();
            })
            .unwrap()
    });
    h.pump();
    let left = windows(&mut h).len();
    println!("after Close Window: {left} windows");
    assert_eq!(left, 2);
    drop(sessions);
}
