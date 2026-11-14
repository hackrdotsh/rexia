//! Files dropped on a terminal on another machine are uploaded there, and the terminal types
//! their paths; a long upload shows its progress and can be cancelled. The other machine is
//! an ssh:// host reached through a stand-in for ssh that runs the remote command here, so
//! the test reaches REXIA_TEST_SERVER (unix://) as a remote host. Needs REXIA_TEST_REX.
mod common;

use common::{Harness, Sessions, focused_text};
use gpui::{ExternalPaths, FileDropEvent, PlatformInput};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// A stand-in for ssh that runs the remote command here, passing what rexia sends at about
/// 3 MB/s, as a slow link would, so an upload takes long enough to watch and cancel.
fn fake_ssh(dir: &Path, rex: &str) -> PathBuf {
    let rex_dir = Path::new(rex).parent().unwrap().display().to_string();
    let pacer = dir.join("pace.py");
    std::fs::write(
        &pacer,
        "import subprocess, sys, time\n\
         child = subprocess.Popen(['sh', '-c', sys.argv[1]], stdin=subprocess.PIPE)\n\
         while True:\n\
         \x20   chunk = sys.stdin.buffer.read1(65536)\n\
         \x20   if not chunk:\n\
         \x20       break\n\
         \x20   child.stdin.write(chunk)\n\
         \x20   child.stdin.flush()\n\
         \x20   time.sleep(0.02)\n\
         child.stdin.close()\n\
         sys.exit(child.wait())\n",
    )
    .unwrap();
    let script = dir.join("ssh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nfor a; do last=$a; done\nPATH='{rex_dir}':\"$PATH\"; export PATH\nexec python3 '{}' \"$last\"\n",
            pacer.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn drop_files(h: &mut Harness, files: Vec<PathBuf>) {
    let window = h.window;
    let at = h.cx.update(|cx| {
        let pane = window.read(cx).unwrap().focused_terminal().unwrap();
        pane.read(cx).cell_point(4, 2).unwrap()
    });
    h.input(PlatformInput::FileDrop(FileDropEvent::Entered {
        position: at,
        paths: ExternalPaths(files.into()),
    }));
    h.input(PlatformInput::FileDrop(FileDropEvent::Pending {
        position: at,
    }));
    h.input(PlatformInput::FileDrop(FileDropEvent::Submit {
        position: at,
    }));
}

/// The path the terminal typed for a file named `name`, read off the screen. The terminal
/// escapes spaces with backslashes, as a shell reads them.
fn typed_path(text: &str, name: &str) -> Option<String> {
    let joined = text.replace('\n', "");
    let escaped = name.replace(' ', "\\ ");
    let end = joined.find(&escaped)? + escaped.len();
    let bytes = joined.as_bytes();
    let mut start = end - escaped.len();
    while start > 0 && !(bytes[start - 1] == b' ' && (start < 2 || bytes[start - 2] != b'\\')) {
        start -= 1;
    }
    Some(joined[start..end].replace("\\ ", " "))
}

#[test]
fn dropped_files_upload_to_remote_terminals() {
    let Some(address) = common::server() else {
        return;
    };
    let (Some(socket), Ok(rex)) = (
        address.strip_prefix("unix://").map(str::to_owned),
        std::env::var("REXIA_TEST_REX"),
    ) else {
        eprintln!("skipped: needs a unix:// REXIA_TEST_SERVER and REXIA_TEST_REX");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: this test binary runs only this test, so nothing reads the environment
    // concurrently.
    unsafe { std::env::set_var("REXIA_SSH", fake_ssh(dir.path(), &rex)) };
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-upload");
    let mut h = Harness::open(
        &format!("ssh://tester@farhost{socket}"),
        &sessions.ids[0],
        1100.,
        700.,
    );
    h.wait("a prompt over ssh", |m| {
        (!focused_text(m).trim().is_empty()).then_some(())
    });

    // Two files in one drop: uploaded as a batch, their paths typed together.
    let first = dir.path().join("upload-one.txt");
    let second = dir.path().join("upload two.txt");
    std::fs::write(&first, "first file's words\n").unwrap();
    std::fs::write(&second, vec![7u8; 300_000]).unwrap();
    drop_files(&mut h, vec![first.clone(), second.clone()]);
    for _ in 0..30 {
        h.pump();
        std::thread::sleep(Duration::from_millis(100));
    }
    let (one, two) = h.wait("both paths typed", |m| {
        let text = focused_text(m);
        Some((
            typed_path(&text, "upload-one.txt")?,
            typed_path(&text, "upload two.txt")?,
        ))
    });
    println!("typed: {one} and {two}");
    assert_ne!(
        Path::new(&one),
        first,
        "a copy on the far side, not the original"
    );
    assert_eq!(std::fs::read(&one).unwrap(), std::fs::read(&first).unwrap());
    assert_eq!(
        std::fs::read(&two).unwrap(),
        std::fs::read(&second).unwrap()
    );
    let ended = h.wait_ui("the upload's end", |ws| ws.last_upload().cloned());
    println!("the upload ended: {ended:?}");
    assert_eq!(ended, Ok(()));
    h.press("ctrl-c");

    // A big file (the server takes up to 100 MiB) shows its progress, and Cancel stops it
    // with nothing typed.
    let big = dir.path().join("big.bin");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(95 * 1024 * 1024)
        .unwrap();
    drop_files(&mut h, vec![big.clone()]);
    let (label, sent, total) = h.wait_ui("the upload under way", |ws| {
        ws.uploads().into_iter().find(|(_, sent, _)| *sent > 0)
    });
    println!("under way: {label} ({sent} of {total} bytes)");
    std::thread::sleep(Duration::from_millis(500));
    h.pump();
    h.screenshot("upload-progress");
    let window = h.window;
    h.cx.update(|cx| window.update(cx, |ws, _, _| ws.cancel_uploads()).unwrap());
    let ended = h.wait_ui("the cancelled upload", |ws| {
        ws.uploads()
            .is_empty()
            .then(|| ws.last_upload().cloned())
            .flatten()
    });
    println!("after Cancel: {ended:?}");
    assert_eq!(ended, Err("Upload cancelled".to_owned()));
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        h.pump();
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        typed_path(&focused_text(&h.host.snapshot()), "big.bin").is_none(),
        "a cancelled upload types nothing"
    );
    drop(sessions);
}
