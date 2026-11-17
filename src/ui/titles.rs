//! What to call tabs and panes, which icon to show, and the activity a tab reports.
use crate::rex::{
    activity::{ProgramState, StatusRecord},
    host::{BlockState, HostModel},
    view::{Block, Window},
};

/// Labels the server gives on its own; a tab or pane with one of these shows what runs in it.
fn is_default_label(label: &str, id: &str) -> bool {
    let label = label.trim();
    label.is_empty()
        || label.eq_ignore_ascii_case("shell")
        || (label.starts_with("Window ") && id.ends_with(label.trim_start_matches("Window ")))
}

fn basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/";
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

static PREFER_GENERATED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Prefer generated titles: name panes after their program and directory rather than
/// the titles shells and programs set.
pub fn set_prefer_generated(prefer: bool) {
    PREFER_GENERATED.store(prefer, std::sync::atomic::Ordering::Relaxed);
}

/// The running program and its directory, such as `nvim · rexia`.
fn generated_title(state: &BlockState) -> Option<String> {
    let program = state.process.name.as_deref().filter(|p| !p.is_empty());
    let dir = state
        .pwd
        .as_deref()
        .or(state.process.cwd.as_deref())
        .map(basename);
    match (program, dir) {
        (Some(program), Some(dir)) => Some(format!("{program} · {dir}")),
        (Some(program), None) => Some(program.to_owned()),
        (None, Some(dir)) => Some(dir.to_owned()),
        (None, None) => None,
    }
}

/// A pane's title: the label someone gave it, else its program's title, else the program,
/// else its directory. With generated titles preferred, the program and its directory
/// come before the program's own title.
pub fn pane_title(block: &Block, state: Option<&BlockState>) -> String {
    let id = block.block_id.as_deref().unwrap_or_default();
    if !is_default_label(&block.label, id) {
        return block.label.clone();
    }
    if let Some(state) = state {
        if PREFER_GENERATED.load(std::sync::atomic::Ordering::Relaxed)
            && let Some(title) = generated_title(state)
        {
            return title;
        }
        if let Some(title) = state.current_title().filter(|t| !t.trim().is_empty()) {
            return title.trim().to_owned();
        }
        if let Some(program) = &state.process.name {
            return program.clone();
        }
        if let Some(pwd) = state.pwd.as_deref().or(state.process.cwd.as_deref()) {
            return basename(pwd).to_owned();
        }
    }
    "Shell".into()
}

/// A tab's title: its own label, else its focused pane's title.
pub fn tab_title(window: &Window, model: &HostModel) -> String {
    if !is_default_label(&window.label, window.id()) {
        return window.label.clone();
    }
    let focused = window
        .focused_block_id
        .as_deref()
        .and_then(|id| window.block(id))
        .or_else(|| window.blocks().next());
    match focused {
        Some(block) => pane_title(
            block,
            block.block_id.as_ref().and_then(|id| model.blocks.get(id)),
        ),
        None => "Empty".into(),
    }
}

/// The most urgent program status among a tab's panes.
pub fn tab_activity<'m>(window: &Window, model: &'m HostModel) -> Option<&'m StatusRecord> {
    window
        .blocks()
        .filter_map(|b| model.blocks.get(b.block_id.as_ref()?)?.status())
        .filter(|r| r.state != ProgramState::Idle)
        .max_by_key(|r| r.state)
}

/// The most urgent program status in a whole session.
pub fn session_activity<'m>(session_id: &str, model: &'m HostModel) -> Option<&'m StatusRecord> {
    model
        .blocks
        .values()
        .filter(|b| b.session_id == session_id)
        .filter_map(BlockState::status)
        .filter(|r| r.state != ProgramState::Idle)
        .max_by_key(|r| r.state)
}

/// The kind of program running in a pane, which picks its icon and color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    Shell,
    Git,
    Editor,
    Remote,
    Agent,
    Monitor,
    Known(&'static super::program_icons::ProgramIcon),
}

impl Program {
    pub fn of(state: Option<&BlockState>) -> Self {
        let name = state
            .and_then(|s| s.process.name.as_deref())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let is_wrapper = |program: &super::program_icons::ProgramIcon| {
            matches!(program.id, "shell" | "node" | "python" | "bun" | "ruby")
        };
        let identities = state
            .map(|state| state.process.identities.as_slice())
            .unwrap_or_default();
        let known = identities
            .iter()
            .filter_map(|identity| super::program_icons::find(identity))
            .find(|program| !is_wrapper(program))
            .or_else(|| super::program_icons::find(&name))
            .or_else(|| {
                identities
                    .iter()
                    .find_map(|identity| super::program_icons::find(identity))
            });
        // Scripts report their own effective_app while the foreground process can
        // still be a shell or runtime. Specific foreground tools beat stale reports.
        let wrapper = known.is_none_or(is_wrapper);
        if wrapper
            && let Some(program) = state
                .into_iter()
                .flat_map(|state| state.records.values())
                .filter(|record| !matches!(record.state, ProgramState::Idle | ProgramState::Done))
                .filter_map(|record| {
                    Some((
                        record.state,
                        super::program_icons::find(record.app.as_deref()?)?,
                    ))
                })
                .max_by_key(|(state, _)| *state)
                .map(|(_, program)| program)
        {
            return Self::Known(program);
        }
        if let Some(program) = known {
            return if program.id == "shell" {
                Self::Shell
            } else {
                Self::Known(program)
            };
        }
        match name.as_str() {
            "git" | "lazygit" | "tig" | "jj" => Self::Git,
            "nvim" | "vim" | "vi" | "hx" | "helix" | "nano" | "emacs" | "kak" => Self::Editor,
            "ssh" | "mosh" | "et" => Self::Remote,
            "claude" | "codex" | "opencode" | "pi" | "aider" | "gemini" | "cli.js" => Self::Agent,
            "htop" | "btop" | "top" => Self::Monitor,
            _ => Self::Shell,
        }
    }

    /// Its original program icon, or a generic category icon for unknown tools.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Shell => "programs/shell",
            Self::Git => "programs/git",
            Self::Editor => "file-code",
            Self::Remote => "world",
            Self::Agent => "command",
            Self::Monitor => "layout-grid",
            Self::Known(program) => program.icon,
        }
    }

    /// The terminal color its icon takes when tab icons use color: one of the bright
    /// 16-color entries, so it follows the theme's palette.
    pub fn ansi(self) -> usize {
        match self {
            Self::Shell => 10,
            Self::Git => 9,
            Self::Editor => 10,
            Self::Remote => 12,
            Self::Agent => 13,
            Self::Monitor => 14,
            Self::Known(program) => program.ansi,
        }
    }

    pub fn is_shell(self) -> bool {
        matches!(self, Self::Shell) || matches!(self, Self::Known(program) if program.is_shell())
    }
}

/// The icon for the program running in a pane.
pub fn program_icon(state: Option<&BlockState>) -> &'static str {
    Program::of(state).icon()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rex::activity::Process;
    use std::collections::BTreeMap;

    fn block(id: &str, label: &str) -> Block {
        Block {
            block_id: Some(id.into()),
            label: label.into(),
            ..Default::default()
        }
    }

    #[test]
    fn pane_titles_prefer_labels_then_program_reports() {
        assert_eq!(pane_title(&block("b", "logs"), None), "logs");
        assert_eq!(pane_title(&block("b", "shell"), None), "Shell");
        let state = BlockState {
            process: Process {
                name: Some("nvim".into()),
                cwd: Some("/home/me/rexia".into()),
                pids: vec![9],
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(pane_title(&block("b", "shell"), Some(&state)), "nvim");
        let titled = BlockState {
            title: Some(("notes.md - NVIM".into(), Some(9))),
            ..state.clone()
        };
        assert_eq!(
            pane_title(&block("b", "shell"), Some(&titled)),
            "notes.md - NVIM"
        );
        let idle = BlockState {
            pwd: Some("/home/me/rexia/".into()),
            ..Default::default()
        };
        assert_eq!(pane_title(&block("b", ""), Some(&idle)), "rexia");
        assert_eq!(program_icon(Some(&state)), "programs/neovim");
    }

    #[test]
    fn tabs_follow_their_focused_pane_unless_renamed() {
        let mut model = HostModel::default();
        model.blocks.insert(
            "b2".into(),
            BlockState {
                process: Process {
                    name: Some("cargo".into()),
                    ..Default::default()
                },
                records: BTreeMap::from([(
                    String::new(),
                    StatusRecord {
                        state: ProgramState::Working,
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
        );
        let window = Window {
            window_id: Some("window:0123-89abcdef".into()),
            label: "Window 89abcdef".into(),
            focused_block_id: Some("b2".into()),
            layers: vec![crate::rex::view::Layer {
                blocks: vec![block("b1", "shell"), block("b2", "shell")],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(tab_title(&window, &model), "cargo");
        assert_eq!(
            tab_activity(&window, &model).unwrap().state,
            ProgramState::Working
        );
        let renamed = Window {
            label: "build".into(),
            ..window
        };
        assert_eq!(tab_title(&renamed, &model), "build");
    }

    #[test]
    fn specific_processes_and_wrapped_apps_select_distinct_icons() {
        let mut state = BlockState::default();
        for (command, icon) in [
            ("claude", "claude-code"),
            ("co", "co-codes"),
            ("gojo", "gojo"),
            ("/usr/bin/btop", "btop"),
            ("jj", "jj"),
        ] {
            state.process.name = Some(command.into());
            assert_eq!(Program::of(Some(&state)).icon(), format!("programs/{icon}"));
        }
        state.process.name = Some("node".into());
        state.records.insert(
            "agent".into(),
            StatusRecord {
                state: ProgramState::Working,
                app: Some("OpenCode".into()),
                ..Default::default()
            },
        );
        assert_eq!(Program::of(Some(&state)).icon(), "programs/opencode");
        state.process.name = Some("/usr/bin/node".into());
        assert_eq!(Program::of(Some(&state)).icon(), "programs/opencode");
        state.process.name = Some("gojo".into());
        assert_eq!(Program::of(Some(&state)).icon(), "programs/gojo");
        state.records.clear();
        state.process.name = Some("unknown-tool".into());
        state.title = Some(("Claude Code - my project".into(), None));
        assert_eq!(Program::of(Some(&state)), Program::Shell);
    }

    #[test]
    fn wrapper_icons_use_executable_identity_without_rewriting_the_label() {
        for (label, executable, expected) in [
            ("oc", "/nix/store/package/bin/.opencode-wrapped", "opencode"),
            (
                "opencode2",
                "/nix/store/package/bin/.opencode2-wrapped_",
                "opencode",
            ),
            ("launcher", "/usr/bin/claude", "claude-code"),
            ("python", "/usr/bin/codex", "codex"),
            ("git", "/usr/bin/claude", "claude-code"),
            ("launcher", "/usr/bin/python", "python"),
        ] {
            let state = BlockState {
                process: crate::rex::activity::Process::parse(&serde_json::json!({
                    "foreground": {"pid": 10, "argv0": label, "path": executable},
                    "ancestors": [{"pid": 1, "name": "nu"}]
                })),
                ..Default::default()
            };
            assert_eq!(state.process.name.as_deref(), Some(label));
            assert_eq!(
                Program::of(Some(&state)).icon(),
                format!("programs/{expected}")
            );
        }
    }

    #[test]
    fn wrapper_detection_does_not_guess_from_ancestors_or_directory_names() {
        let state = BlockState {
            process: crate::rex::activity::Process::parse(&serde_json::json!({
                "foreground": {"pid": 10, "argv0": "launcher", "path": "/tmp/opencode/unknown-tool"},
                "ancestors": [{"pid": 1, "name": "opencode"}]
            })),
            ..Default::default()
        };
        assert_eq!(Program::of(Some(&state)), Program::Shell);
    }
}
