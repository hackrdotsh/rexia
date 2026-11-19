mod common;

use common::{Harness, Sessions};
use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, ScrollDelta, ScrollEvent,
    ScrollSource, TouchPhase, point, px,
};
use rexia::{
    rex::host::HostCommand,
    settings::{HostConfig, Settings},
    ui::workspace::Workspace,
};
use serde_json::{Value, json};
use std::sync::Mutex;

static FIXTURE: Mutex<()> = Mutex::new(());

fn click(h: &mut Harness, key: &str) {
    let at = h.wait_ui(key, |ws| ws.element_bounds(key)).center();
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        position: at,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    }));
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        position: at,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: 1,
    }));
    h.pump();
}

#[test]
fn collapsed_rows_survive_activity_and_reopening() {
    let Some(address) = common::server() else {
        return;
    };
    let _fixture = FIXTURE.lock().unwrap_or_else(|error| error.into_inner());
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-sidebar-disclosure");
    let session = &sessions.ids[0];
    let settings = common::test_settings(&address, session);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.toml");
    settings.save(&path).unwrap();
    let mut h = Harness::open_with_path(settings, Some(path.clone()), 1100., 760.);
    let tab = h.wait("the tab", |m| {
        Some(m.view()?.active_window()?.id().to_owned())
    });
    let tab = format!("side-tab:{tab}");
    let row = format!("session:0:{session}");

    h.wait_ui("the expanded tab row", |ws| ws.element_bounds(&tab));
    click(&mut h, &row);
    h.wait_ui("the session collapses", |ws| {
        ws.element_bounds(&tab).is_none().then_some(())
    });
    h.run("client.sidebar.activity.toggle", Value::Null);
    h.run("client.sidebar.activity.toggle", Value::Null);
    assert!(h.workspace(|ws| ws.element_bounds(&tab).is_none()));
    click(&mut h, "host-disclosure:test");
    h.wait_ui("the host collapses", |ws| {
        ws.element_bounds(&row).is_none().then_some(())
    });
    drop(h);

    let mut reopened = Harness::open_with(Settings::load(&path).unwrap(), 1100., 760.);
    reopened.wait("the same session", |m| {
        (m.selected.as_ref() == Some(session)).then_some(())
    });
    reopened.wait_ui("the host row", |ws| {
        ws.element_bounds("host-disclosure:test")
    });
    assert!(
        reopened.workspace(|ws| ws.element_bounds(&row).is_none()),
        "host reopened expanded"
    );
    click(&mut reopened, "host-disclosure:test");
    reopened.wait_ui("the session row", |ws| ws.element_bounds(&row));
    assert!(
        reopened.workspace(|ws| ws.element_bounds(&tab).is_none()),
        "session reopened expanded"
    );
}

#[test]
fn host_filter_keeps_checkboxes_independent_and_saves_the_solo_choice() {
    let (Some(address), Ok(alternate)) =
        (common::server(), std::env::var("REXIA_TEST_SECOND_HOST"))
    else {
        eprintln!("skipped: requires a second disposable server in REXIA_TEST_SECOND_HOST");
        return;
    };
    let _fixture = FIXTURE.lock().unwrap_or_else(|error| error.into_inner());
    let control = common::connect(&address);
    let alternate_control = common::connect(&alternate);
    let sessions = Sessions::create(&control, "rexia-sidebar-primary");
    let other_sessions = Sessions::create(&alternate_control, "rexia-sidebar-alternate");
    let primary = &sessions.ids[0];
    let other = &other_sessions.ids[0];
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.toml");
    let mut settings = common::test_settings(&address, primary);
    settings.hosts.extend([
        HostConfig {
            id: "alt".into(),
            label: "Alternate".into(),
            endpoint: alternate,
        },
        HostConfig {
            id: "offline".into(),
            label: "Offline server".into(),
            endpoint: format!("unix://{}/no-server.sock", directory.path().display()),
        },
    ]);
    let mut h = Harness::open_with_path(settings, Some(path.clone()), 1100., 760.);
    h.wait_ui("both servers load their session", |ws| {
        ws.element_bounds(&format!("session:0:{primary}"))?;
        ws.element_bounds(&format!("session:1:{other}"))
    });
    click(&mut h, "host-filter");
    click(&mut h, "host-filter:online");
    h.wait_ui("Online Hosts hides the unreachable host", |ws| {
        ws.element_bounds("host-disclosure:offline")
            .is_none()
            .then_some(())
    });
    click(&mut h, "host-filter:check:alt");
    h.wait_ui("unchecking Alternate hides only that host", |ws| {
        ws.element_bounds("host-disclosure:test")?;
        ws.element_bounds("host-disclosure:alt")
            .is_none()
            .then_some(())
    });
    click(&mut h, "host-filter:check:alt");
    h.wait_ui("checking Alternate keeps both online hosts", |ws| {
        ws.element_bounds("host-disclosure:test")?;
        ws.element_bounds("host-disclosure:alt")
    });
    click(&mut h, "host-filter:online");
    h.wait_ui(
        "turning off Online Hosts restores the checked offline host",
        |ws| ws.element_bounds("host-disclosure:offline"),
    );
    click(&mut h, "host-filter:solo:alt");
    h.wait_ui("clicking the host row shows only Alternate", |ws| {
        ws.element_bounds("host-disclosure:alt")?;
        (ws.element_bounds("host-disclosure:test").is_none()
            && ws.element_bounds("host-disclosure:offline").is_none())
        .then_some(())
    });
    assert_eq!(h.workspace(|ws| ws.host().config.id.clone()), "test");
    h.screenshot("sidebar-host-filter-solo");

    let loaded = Settings::load(&path).unwrap();
    let mut relaunched = Harness::open_with(loaded, 1100., 760.);
    relaunched.wait_ui("relaunched window restores Alternate", |ws| {
        ws.element_bounds(&format!("session:1:{other}"))
    });
    assert!(relaunched.workspace(|ws| ws.element_bounds("host-disclosure:test").is_none()));

    h.run("client.window.new", Value::Null);
    let first = h.window;
    let second = h.cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter_map(|w| w.downcast::<Workspace>())
            .find(|w| *w != first)
            .unwrap()
    });
    assert!(h.cx.update(|cx| {
        let ws = second.read(cx).unwrap();
        ws.settings.sidebar.host_selected("alt") && !ws.settings.sidebar.host_selected("test")
    }));
    click(&mut h, "host-filter:check:test");
    assert!(h.cx.update(|cx| {
        let ws = second.read(cx).unwrap();
        ws.settings.sidebar.host_selected("alt") && ws.settings.sidebar.host_selected("test")
    }));

    // Searching is scoped to the checked hosts too.
    click(&mut h, "host-filter:solo:test");
    h.run("client.sidebar.filter.focus", Value::Null);
    h.type_text("rexia-sidebar-alternate");
    assert!(h.workspace(|ws| ws.filter_titles().is_empty()));
}

#[test]
fn activity_groups_each_report_by_priority() {
    let Some(address) = common::server() else {
        return;
    };
    let _fixture = FIXTURE.lock().unwrap_or_else(|error| error.into_inner());
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-sidebar-activity");
    let session = sessions.ids[0].clone();
    let mut h = Harness::open(&address, &session, 1100., 760.);
    let block = h.wait("the shell prompt", |model| {
        if common::focused_text(model).trim().is_empty() {
            return None;
        }
        model.view()?.active_window()?.focused_block_id.clone()
    });
    let line = r#"sh -c 'printf "\033]7501;state=working:id=builder:app=make\007\033]7501;state=blocked:id=agent:app=claude\007"; sleep 60'"#;
    for (method, payload) in [
        (
            "block.event",
            json!({"session_id": session, "block_id": block,
            "event": {"type": "paste", "payload": {"text": line}}}),
        ),
        (
            "client.key",
            json!({"session_id": session, "block_id": block,
            "key": {"key": "enter", "mods": "", "text": "\r", "unshifted_codepoint": 13,
                "action": "press"}}),
        ),
    ] {
        h.host.send(HostCommand::Input {
            session_id: session.clone(),
            block_id: block.clone(),
            method,
            payload,
            claim: true,
        });
    }
    h.wait("two status records from the same pane", |m| {
        (m.blocks.get(&block)?.records.len() == 2).then_some(())
    });
    h.run("client.sidebar.activity.toggle", Value::Null);
    let (blocked_heading, blocked, working_heading, working) =
        h.wait_ui("two reports with their priority headings", |ws| {
            Some((
                ws.element_bounds("activity-group:Needs attention")?,
                ws.element_bounds(&format!("activity:0:{block}:agent"))?,
                ws.element_bounds("activity-group:Working")?,
                ws.element_bounds(&format!("activity:0:{block}:builder"))?,
            ))
        });
    assert!(blocked_heading.top() < blocked.top());
    assert!(blocked.top() < working_heading.top());
    assert!(working_heading.top() < working.top());
    h.screenshot("sidebar-activity-priorities");
    assert!(h.workspace(|ws| ws.shortcut_rows("group activity by session").is_empty()));
}

#[test]
fn last_session_scrolls_past_the_fade() {
    let Some(address) = common::server() else {
        return;
    };
    let _fixture = FIXTURE.lock().unwrap_or_else(|error| error.into_inner());
    let control = common::connect(&address);
    let mut sessions = Sessions::create(&control, "rexia-sidebar-scroll");
    for i in 1..14 {
        sessions.add(&format!("rexia-sidebar-scroll-{i}"));
    }
    let last = format!("session:0:{}", sessions.ids.last().unwrap());
    let mut h = Harness::open(&address, &sessions.ids[0], 900., 360.);
    let (row, fade) = h.wait_ui("last session initially overflows the sidebar", |ws| {
        let fade = ws.element_bounds("sidebar-fade:bottom")?;
        let row = ws.element_bounds(&last)?;
        (row.top() > fade.bottom()).then_some((row, fade))
    });
    h.input(PlatformInput::Scroll(ScrollEvent {
        position: point(fade.center().x, fade.top() - px(30.)),
        delta: ScrollDelta::Pixels(point(px(0.), -px(2400.))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    }));
    let scrolled = h.wait_ui("last row fully above the fade", |ws| {
        let last = ws.element_bounds(&last)?;
        (last.bottom() <= fade.top() && last.top() < row.top()).then_some(last)
    });
    println!(
        "Last row bottom: {:?}; bottom fade starts at {:?}",
        scrolled.bottom(),
        fade.top()
    );
    h.screenshot("sidebar-scroll-bottom");

    let last_id = sessions.ids.last().unwrap();
    h.input(PlatformInput::Scroll(ScrollEvent {
        position: point(fade.center().x, fade.top() - px(30.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(2400.))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    }));
    h.wait_ui("the last row goes out of view again", |ws| {
        (ws.element_bounds(&last)?.top() > fade.bottom()).then_some(())
    });
    h.run("client.sidebar.activity.toggle", Value::Null);
    h.run("session.select", json!({"session_id": last_id}));
    h.wait("the last session becomes active", |model| {
        (model.selected.as_ref()? == last_id).then_some(())
    });
    h.run("client.sidebar.activity.toggle", Value::Null);
    h.wait_ui(
        "the selected row auto-scrolls clear of the bottom fade",
        |ws| {
            let last = ws.element_bounds(&last)?;
            let fade = ws.element_bounds("sidebar-fade:bottom")?;
            (last.bottom() <= fade.top() - px(3.)).then_some(())
        },
    );
}
