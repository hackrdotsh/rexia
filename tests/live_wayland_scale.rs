//! Opt-in test of actual Wayland event ordering under an isolated umbriel compositor.
//! Set REXIA_TEST_UMBRIEL to its executable. No real Rex server or desktop is touched.
use std::{
    fs,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn numbers(line: &str, marker: &str) -> Vec<u32> {
    line.split_once(marker)
        .map(|(_, rest)| {
            rest.trim_end_matches(')')
                .split(", ")
                .filter_map(|part| part.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn fractional_scale_survives_surface_enter() {
    let Ok(compositor) = std::env::var("REXIA_TEST_UMBRIEL") else {
        eprintln!("skipped: set REXIA_TEST_UMBRIEL to test actual Wayland scaling");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let trace = root.join("wayland.log");
    let launcher = root.join("launch.sh");
    let settings = root.join("settings.toml");
    fs::write(&settings, "[interface]\nsidebar = false\n").unwrap();
    fs::write(
        &launcher,
        format!(
            "WAYLAND_DEBUG=1 {} --config {} --server {} 2>{} &\n\
             sleep 5\n{} msg session-quit skip-confirmation\n",
            quoted(env!("CARGO_BIN_EXE_rexia")),
            quoted(settings.to_str().unwrap()),
            quoted(&format!("unix://{}/absent.sock", root.display())),
            quoted(trace.to_str().unwrap()),
            quoted(&compositor),
        ),
    )
    .unwrap();
    let config = root.join("compositor.toml");
    fs::write(
        &config,
        format!(
            "[general]\nautostart = [{}]\nxwayland = false\nshow_cheatsheet = false\n\
             [animation]\nenabled = false\n\
             [output.HEADLESS-1]\nmode = \"2880x1920@60\"\nscale = 1.5\n",
            serde_json::to_string(&format!("sh {}", quoted(launcher.to_str().unwrap()))).unwrap()
        ),
    )
    .unwrap();
    let log = root.join("compositor.log");
    let mut child = Command::new(&compositor)
        .args(["-c", config.to_str().unwrap()])
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("XDG_RUNTIME_DIR", root)
        .env("WLR_BACKENDS", "headless")
        .env("WLR_HEADLESS_OUTPUTS", "1")
        .env("WLR_LIBINPUT_NO_DEVICES", "1")
        .stdout(Stdio::from(fs::File::create(&log).unwrap()))
        .stderr(Stdio::from(
            fs::File::create(root.join("errors.log")).unwrap(),
        ))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(25);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "isolated compositor did not exit: {}",
                fs::read_to_string(&log).unwrap()
            );
        }
        thread::sleep(Duration::from_millis(50));
    };
    assert!(
        status.success(),
        "compositor failed: {}",
        fs::read_to_string(root.join("errors.log")).unwrap()
    );
    let trace = fs::read_to_string(trace).unwrap();
    let (_, after_scale) = trace
        .split_once("preferred_scale(180)")
        .expect("compositor must supply the 150% scale");
    assert!(
        !after_scale.contains("set_buffer_scale(2)"),
        "an output event replaced fractional scale with integer scale"
    );
    let destination = trace
        .lines()
        .rev()
        .find(|l| l.contains(".set_destination("))
        .unwrap();
    let logical = numbers(destination, ".set_destination(");
    let buffer = trace
        .lines()
        .rev()
        .find(|l| l.contains(".create_immed(new id wl_buffer"))
        .unwrap();
    let dimensions = numbers(buffer, ", ");
    assert!(logical.len() >= 2 && dimensions.len() >= 2);
    println!(
        "150%: logical {logical:?}, buffer {} × {}",
        dimensions[0], dimensions[1]
    );
    for axis in 0..2 {
        assert!((f64::from(dimensions[axis]) - f64::from(logical[axis]) * 1.5).abs() <= 1.);
    }
}
