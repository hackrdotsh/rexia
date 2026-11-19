//! Remote file previews use the server's download stream, not a matching local path.
mod common;

use common::{Harness, Sessions, desktop::Desktop, focused_text};
use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput};
use rexia::ui::workspace::quick_look::Shown;
use std::{cell::RefCell, os::unix::fs::PermissionsExt, rc::Rc};

#[test]
fn remote_files_download_before_preview_or_open() {
    let Some(address) = common::server() else {
        return;
    };
    let (Some(socket), Ok(rex)) = (
        address.strip_prefix("unix://"),
        std::env::var("REXIA_TEST_REX"),
    ) else {
        return;
    };
    let Some(desktop) = Desktop::start_with(true) else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let ssh = directory.path().join("ssh");
    std::fs::write(&ssh, format!(
        "#!/bin/sh\nfor a; do last=$a; done\nPATH='{}':\"$PATH\"; export PATH\nexec sh -c \"$last\"\n",
        std::path::Path::new(&rex).parent().unwrap().display()
    )).unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: this binary runs one test, before any ssh worker starts.
    unsafe { std::env::set_var("REXIA_SSH", &ssh) };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-remote-preview");
    let mut h = Harness::open(
        &format!("ssh://tester@preview-host{socket}"),
        &sessions.ids[0],
        1200.,
        700.,
    );
    h.wait("remote prompt", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    let original = directory.path().join("remote notes.txt");
    let content = "A file received through Rex's remote download stream.\n";
    std::fs::write(&original, content).unwrap();
    let uri = rexia::quick_look::file_uri(&original);
    let downloads = directory.path().join("downloads");
    let opened = Rc::new(RefCell::new(Vec::new()));
    let record = opened.clone();
    let window = h.window;
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, _| {
                ws.set_download_directory(downloads.clone());
                ws.set_preview_bus(Some(desktop.address.clone()));
                ws.set_link_opener(move |link, _| record.borrow_mut().push(link.to_owned()));
            })
            .unwrap()
    });

    h.type_text(&format!(
        "printf '%b\\n' '\\033]8;;{uri}\\033\\\\remote-notes\\033]8;;\\033\\\\'"
    ));
    h.press("enter");
    let row = h.wait("remote file link", |m| {
        focused_text(m)
            .lines()
            .position(|line| line.trim() == "remote-notes")
    });
    let at = h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        pane.read(cx).cell_point(3, row as u16).unwrap()
    });
    h.input(PlatformInput::MouseMove(MouseMoveEvent {
        position: at,
        pressed_button: None,
        modifiers: Default::default(),
    }));
    h.run("client.quick_look", serde_json::Value::Null);
    let preview = h.wait_ui("downloaded file preview", |ws| {
        ws.quick_looks().last().cloned()
    });
    let Shown::Previewer(preview) = preview else {
        panic!("expected native file preview")
    };
    let copy = url::Url::parse(&preview).unwrap().to_file_path().unwrap();
    println!("preview downloaded copy: {}", copy.display());
    assert!(copy.starts_with(&downloads));
    assert_ne!(copy, original);
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), content);
    assert_eq!(
        std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
        0o600
    );

    // Opening a remote file link also downloads, and never overwrites an existing copy.
    let modifiers = Modifiers {
        control: true,
        ..Default::default()
    };
    h.input(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers,
        click_count: 1,
        first_mouse: false,
    }));
    h.input(PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position: at,
        modifiers,
        click_count: 1,
    }));
    h.wait_ui("remote file opened", |_| {
        (!opened.borrow().is_empty()).then_some(())
    });
    let open_copy = url::Url::parse(&opened.borrow()[0])
        .unwrap()
        .to_file_path()
        .unwrap();
    assert_ne!(open_copy, copy);
    assert!(open_copy.starts_with(&downloads));
    assert_eq!(std::fs::read_to_string(open_copy).unwrap(), content);
    assert_eq!(std::fs::read_to_string(copy).unwrap(), content);
    assert_eq!(desktop.previewed().len(), 1);
    let opened_before = opened.borrow().len();
    let missing = rexia::quick_look::file_uri(&directory.path().join("missing.txt"));
    h.cx.update(|cx| {
        window
            .update(cx, |ws, _, cx| ws.open_link(missing, cx))
            .unwrap()
    });
    h.wait_ui("missing remote file reported", |ws| {
        ws.notice
            .as_ref()
            .filter(|notice| notice.contains("Couldn't download"))
            .cloned()
    });
    assert_eq!(
        opened.borrow().len(),
        opened_before,
        "a failed download must not open a partial file"
    );

    // With the downloaded copy's execute bits stripped, a script is still detected.
    let script = directory.path().join("remote-helper");
    let script_text = "#!/bin/sh\nexit 0\n";
    std::fs::write(&script, script_text).unwrap();
    let script_uri = rexia::quick_look::file_uri(&script);
    h.type_text(&format!(
        "printf '%b\\n' '\\033]8;;{script_uri}\\033\\\\remote-script\\033]8;;\\033\\\\'"
    ));
    h.press("enter");
    let row = h.wait("remote script link", |m| {
        focused_text(m)
            .lines()
            .position(|line| line.trim() == "remote-script")
    });
    let at = h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        pane.read(cx).cell_point(3, row as u16).unwrap()
    });
    h.input(PlatformInput::MouseMove(MouseMoveEvent {
        position: at,
        pressed_button: None,
        modifiers: Default::default(),
    }));
    h.run("client.quick_look", serde_json::Value::Null);
    h.wait_ui("remote script confirmation", |ws| ws.confirm_title());
    assert_eq!(opened.borrow().len(), opened_before);
    assert_eq!(desktop.previewed().len(), 1, "the script was not previewed");
    h.press("tab");
    h.press("enter");
    let script_copy = url::Url::parse(opened.borrow().last().unwrap())
        .unwrap()
        .to_file_path()
        .unwrap();
    assert_ne!(script_copy, script);
    assert!(script_copy.starts_with(&downloads));
    assert_eq!(std::fs::read_to_string(&script_copy).unwrap(), script_text);
    assert_eq!(
        std::fs::metadata(&script_copy)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    println!("Open Anyway opened remote script copy {script_copy:?}");
    drop(sessions);
}
