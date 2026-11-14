//! Follow the user's Noctalia theme: its palette and its shell style, reloaded whenever
//! Noctalia changes them.
//!
//! The palette comes from the first of:
//! 1. rexia's own template output, `$XDG_CONFIG_HOME/rexia/noctalia.toml`, which Noctalia
//!    rewrites on every theme change once `resources/noctalia/rexia.toml` is registered as
//!    a user template;
//! 2. another enabled user template whose output has the same `mode` + `[colors]` form;
//! 3. Noctalia v4's `colors.json`;
//! 4. Noctalia's default palette.
//!
//! The shell style (font, corner rounding, borders, shadows) comes from
//! `noctalia msg config-get`.
pub mod palette;
pub mod shell;

pub use palette::{Palette, TerminalColors};
pub use shell::ShellStyle;

use anyhow::Result;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Template(PathBuf),
    LegacyJson(PathBuf),
    /// Noctalia's palette couldn't be read.
    Default,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoctaliaTheme {
    pub palette: Palette,
    pub shell: ShellStyle,
    pub source: Source,
    /// The shell style came from a running Noctalia.
    pub shell_live: bool,
}

impl Default for NoctaliaTheme {
    fn default() -> Self {
        Self {
            palette: Palette::default(),
            shell: ShellStyle::default(),
            source: Source::Default,
            shell_live: false,
        }
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/".into())
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
}

fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".local/state"))
}

/// Where Noctalia writes rexia's palette.
pub fn template_output() -> PathBuf {
    config_home().join("rexia/noctalia.toml")
}

/// The Noctalia config that registers rexia's template, for the user's config.toml.
pub fn template_registration(template: &Path) -> String {
    format!(
        "[theme.templates.user.rexia]\ninput_path = \"{}\"\noutput_path = \"~/.config/rexia/noctalia.toml\"\n",
        template.display()
    )
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(path),
    }
}

/// The Wayland displays a Noctalia instance serves, from its sockets in the runtime directory
/// (`noctalia-wayland-0.sock`). Its dmenu socket isn't an instance.
pub fn instance_displays(runtime_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(runtime_dir) else {
        return Vec::new();
    };
    let mut displays: Vec<String> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter_map(|name| {
            let display = name.strip_prefix("noctalia-")?.strip_suffix(".sock")?;
            (!display.starts_with("dmenu-")).then(|| display.to_owned())
        })
        .collect();
    displays.sort();
    displays
}

fn run_msg(args: &[&str], display: Option<&str>) -> Option<String> {
    let program = std::env::var("REXIA_NOCTALIA").unwrap_or_else(|_| "noctalia".into());
    let mut command = Command::new(program);
    command
        .arg("msg")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(display) = display {
        command.env("WAYLAND_DISPLAY", display);
    }
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                std::io::Read::read_to_string(child.stdout.as_mut()?, &mut out).ok()?;
                return status.success().then_some(out);
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Run `noctalia msg …`, giving up after two seconds. Noctalia finds its instance by
/// `WAYLAND_DISPLAY`; when rexia runs on another display (a nested compositor, or a
/// launcher without the session's environment) the instance is found by its socket.
fn msg(args: &[&str]) -> Option<String> {
    if let Some(out) = run_msg(args, None) {
        return Some(out);
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    let current = std::env::var("WAYLAND_DISPLAY").ok();
    instance_displays(&runtime)
        .into_iter()
        .filter(|display| Some(display) != current.as_ref())
        .find_map(|display| run_msg(args, Some(&display)))
}

/// Outputs of the user templates Noctalia has enabled, rexia's own first.
pub fn template_outputs(templates: &str) -> Vec<PathBuf> {
    let Ok(table) = toml::from_str::<toml::Table>(templates) else {
        return Vec::new();
    };
    let Some(user) = table.get("user").and_then(|u| u.as_table()) else {
        return Vec::new();
    };
    let mut outputs: Vec<(bool, PathBuf)> = Vec::new();
    for (name, template) in user {
        if template.get("enabled").and_then(|e| e.as_bool()) == Some(false) {
            continue;
        }
        let ours = name == "rexia";
        match template.get("output_path") {
            Some(toml::Value::String(path)) => outputs.push((ours, expand(path))),
            Some(toml::Value::Array(paths)) => {
                for path in paths.iter().filter_map(|p| p.as_str()) {
                    outputs.push((ours, expand(path)));
                }
            }
            _ => {}
        }
    }
    outputs.sort_by_key(|(ours, _)| !ours);
    outputs.into_iter().map(|(_, path)| path).collect()
}

fn read_palette(templates: Option<&str>) -> (Palette, Source) {
    let mut candidates = vec![template_output()];
    if let Some(templates) = templates {
        candidates.extend(template_outputs(templates));
    }
    if let Ok(path) = std::env::var("REXIA_NOCTALIA_PALETTE") {
        candidates.insert(0, PathBuf::from(path));
    }
    for path in candidates {
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(palette) = Palette::from_template(&text)
        {
            return (palette, Source::Template(path));
        }
    }
    let legacy = config_home().join("noctalia/colors.json");
    if let Ok(text) = std::fs::read_to_string(&legacy)
        && let Ok(palette) = Palette::from_legacy_json(&text)
    {
        return (palette, Source::LegacyJson(legacy));
    }
    (Palette::default(), Source::Default)
}

/// Read the current theme. Never fails: anything unreadable falls back to defaults.
pub fn load() -> NoctaliaTheme {
    let templates = msg(&["config-get", "theme.templates"]);
    let (palette, source) = read_palette(templates.as_deref());
    let shell_text = msg(&["config-get", "shell"]);
    let accessibility = msg(&["config-get", "accessibility"]).unwrap_or_default();
    let shell = shell_text
        .as_deref()
        .and_then(|text| ShellStyle::parse(text, &accessibility).ok());
    NoctaliaTheme {
        palette,
        shell_live: shell.is_some(),
        shell: shell.unwrap_or_default(),
        source,
    }
}

/// Directories whose changes can change the theme.
fn watched_dirs(theme: &NoctaliaTheme) -> Vec<PathBuf> {
    let mut dirs = vec![
        config_home().join("rexia"),
        config_home().join("noctalia"),
        state_home().join("noctalia"),
    ];
    if let Some(path) = std::env::var_os("REXIA_NOCTALIA_PALETTE").map(PathBuf::from)
        && let Some(parent) = path.parent()
    {
        dirs.push(parent.to_owned());
    }
    if let Source::Template(path) | Source::LegacyJson(path) = &theme.source
        && let Some(parent) = path.parent()
    {
        dirs.push(parent.to_owned());
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Keeps watching while alive.
pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
}

/// Call `changed` with the new theme whenever Noctalia's palette or shell style changes.
/// Noctalia writes several files per change, so events are gathered for a moment first.
pub fn watch(
    initial: &NoctaliaTheme,
    changed: impl Fn(NoctaliaTheme) + Send + 'static,
) -> Result<Watcher> {
    use notify::Watcher as _;
    let (events, received) = std::sync::mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|e| !e.kind.is_access()) {
            let _ = events.send(());
        }
    })?;
    let _ = std::fs::create_dir_all(config_home().join("rexia"));
    for dir in watched_dirs(initial) {
        // A directory that doesn't exist yet is simply not watched.
        let _ = watcher.watch(&dir, notify::RecursiveMode::NonRecursive);
    }
    let mut current = initial.clone();
    std::thread::Builder::new()
        .name("noctalia-watch".into())
        .spawn(move || {
            while received.recv().is_ok() {
                while received.recv_timeout(Duration::from_millis(200)).is_ok() {}
                let next = load();
                if next != current {
                    current = next.clone();
                    changed(next);
                }
            }
        })?;
    Ok(Watcher { _watcher: watcher })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests that change the environment run one at a time.
    static ENV: Mutex<()> = Mutex::new(());

    fn palette_file(primary: &str) -> String {
        format!(
            "mode = \"dark\"\n[colors]\nprimary = \"{primary}\"\non_primary = \"#263500\"\nsurface = \"#000000\"\non_surface = \"#e2e2e2\"\n"
        )
    }

    #[test]
    fn instances_are_found_by_their_sockets() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "noctalia-wayland-0.sock",
            "noctalia-dmenu-wayland-0.sock",
            "noctalia-wayland-0.lock",
            "wayland-1",
        ] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        assert_eq!(instance_displays(dir.path()), vec!["wayland-0".to_owned()]);
        assert!(instance_displays(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn template_outputs_put_rexia_first_and_skip_disabled() {
        let templates = r#"
builtin_ids = []
[user.nordend]
enabled = true
output_path = [ '~/.config/nordend/colors.toml' ]
[user.off]
enabled = false
output_path = '/tmp/off.toml'
[user.rexia]
enabled = true
output_path = '/tmp/rexia.toml'
"#;
        let outputs = template_outputs(templates);
        assert_eq!(outputs[0], PathBuf::from("/tmp/rexia.toml"));
        assert_eq!(outputs[1], home().join(".config/nordend/colors.toml"));
        assert_eq!(outputs.len(), 2);
        assert!(template_outputs("not toml [").is_empty());
    }

    #[test]
    fn explicit_palette_file_wins() {
        let _env = ENV.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("palette.toml");
        std::fs::write(&path, palette_file("#b2d26b")).unwrap();
        // SAFETY: ENV serializes every test that touches the environment.
        unsafe { std::env::set_var("REXIA_NOCTALIA_PALETTE", &path) };
        let (palette, source) = read_palette(None);
        unsafe { std::env::remove_var("REXIA_NOCTALIA_PALETTE") };
        assert_eq!(palette.primary, 0xb2d26b);
        assert_eq!(source, Source::Template(path));
    }

    #[test]
    fn registration_names_the_output_rexia_reads() {
        let text = template_registration(Path::new("/share/rexia/noctalia/rexia.toml"));
        let table: toml::Table = toml::from_str(&text).unwrap();
        let rexia = &table["theme"]["templates"]["user"]["rexia"];
        assert_eq!(
            rexia["output_path"].as_str(),
            Some("~/.config/rexia/noctalia.toml")
        );
    }

    #[test]
    fn a_rewritten_palette_reaches_the_watcher() {
        let _env = ENV.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("palette.toml");
        std::fs::write(&path, palette_file("#111111")).unwrap();
        // SAFETY: ENV serializes every test that touches the environment.
        unsafe { std::env::set_var("REXIA_NOCTALIA_PALETTE", &path) };
        let initial = load();
        assert_eq!(initial.palette.primary, 0x111111);
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher = watch(&initial, move |theme| {
            let _ = tx.send(theme.palette.primary);
        })
        .unwrap();
        std::fs::write(&path, palette_file("#22aa44")).unwrap();
        let primary = rx.recv_timeout(Duration::from_secs(10));
        unsafe { std::env::remove_var("REXIA_NOCTALIA_PALETTE") };
        drop(watcher);
        assert_eq!(primary, Ok(0x22aa44));
    }
}
