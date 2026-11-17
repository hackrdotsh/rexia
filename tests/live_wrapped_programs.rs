//! Follow actual executable identity through a Nushell alias and a Nix wrapper.
mod common;

use common::{Harness, Sessions, focused_text};
use rexia::{
    rex::data::{SHELL_FLAVOR, TERMINAL_CREATOR},
    ui::titles::Program,
};
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, process::Command};

#[test]
fn nushell_alias_and_nix_wrapper_show_the_launched_program() {
    let Some(address) = common::server() else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join(".opencode2-wrapped_");
    let source = directory.path().join("fixture.c");
    std::fs::write(
        &source,
        "#include <stdio.h>\n#include <unistd.h>\nint main(void) { puts(\"WRAPPED_ICON_READY\"); fflush(stdout); sleep(60); return 0; }\n",
    ).unwrap();
    assert!(
        Command::new("cc")
            .arg(source)
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap()
            .success()
    );
    let control = common::connect(&address);
    let sessions = Sessions::create(&control, "rexia-wrapped-icons");
    let session = &sessions.ids[0];
    common::request(
        &control,
        "session.new_window",
        json!({
            "session_id": session,
            "layout": {"block": {"flavor": SHELL_FLAVOR, "options": {
                "shell": "none", "command": ["nu", "--no-config-file"]
            }}}
        }),
    )
    .unwrap();
    let mut h = Harness::open(&address, session, 1000., 700.);
    let block = h.wait("shell prompt", |model| {
        (!focused_text(model).trim().is_empty())
            .then(|| model.view()?.active_window()?.focused_block_id.clone())?
    });
    for name in ["opencode2", "custom-launcher"] {
        let wrapper = directory.path().join(name);
        std::fs::write(
            &wrapper,
            format!(
                "#!/usr/bin/env bash\nexec -a \"$0\" '{}'\n",
                binary.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
        h.type_text(&format!("alias oc = {}; oc", wrapper.display()));
        h.press("enter");
        let program = h.wait("reported wrapped process", |model| {
            let state = model.blocks.get(&block)?;
            (state.process.name.as_deref() == Some(name)).then(|| Program::of(Some(state)))
        });
        let report = common::request(
            &control,
            &format!("{TERMINAL_CREATOR}.process"),
            json!({"session_id": session, "block_id": block, "args": {}}),
        )
        .unwrap();
        println!("Wrapped process report: {report}");
        h.screenshot(&format!("wrapped-opencode-{name}"));
        h.press("ctrl-c");
        assert_eq!(program.icon(), "programs/opencode");
        h.wait("shell icon after wrapper exits", |model| {
            (Program::of(model.blocks.get(&block)) == Program::Shell).then_some(())
        });
    }
}
