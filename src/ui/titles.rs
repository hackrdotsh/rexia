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

/// A pane's title: the label someone gave it, else its program's title, else the program,
/// else its directory.
pub fn pane_title(block: &Block, state: Option<&BlockState>) -> String {
    let id = block.block_id.as_deref().unwrap_or_default();
    if !is_default_label(&block.label, id) {
        return block.label.clone();
    }
    if let Some(state) = state {
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

/// A Tabler icon for the program running in a pane.
pub fn program_icon(state: Option<&BlockState>) -> &'static str {
    let name = state
        .and_then(|s| s.process.name.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match name.as_str() {
        "git" | "lazygit" | "tig" | "jj" => "brand-git",
        "nvim" | "vim" | "vi" | "hx" | "helix" | "nano" | "emacs" | "kak" => "file-code",
        "ssh" | "mosh" | "et" => "world",
        "claude" | "codex" | "opencode" | "pi" | "aider" | "gemini" | "cli.js" => "command",
        "htop" | "btop" | "top" => "layout-grid",
        _ => "terminal-2",
    }
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
        assert_eq!(program_icon(Some(&state)), "file-code");
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
}
