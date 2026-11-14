//! Short sounds for program activity, from the freedesktop sound theme.
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Blocked,
    Error,
    Done,
}

impl Cue {
    fn names(self) -> &'static [&'static str] {
        match self {
            Self::Blocked => &["window-question", "dialog-warning", "bell"],
            Self::Error => &["dialog-error", "dialog-warning", "bell"],
            Self::Done => &["complete", "message", "bell"],
        }
    }
}

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(home));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }
    let system =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(
        system
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from),
    );
    dirs
}

/// The theme's file for a cue, trying each fallback name in each data directory.
pub fn find(cue: Cue, dirs: &[PathBuf]) -> Option<PathBuf> {
    cue.names().iter().find_map(|name| {
        dirs.iter().find_map(|dir| {
            ["oga", "ogg", "wav"]
                .iter()
                .map(|ext| dir.join(format!("sounds/freedesktop/stereo/{name}.{ext}")))
                .find(|path| path.exists())
        })
    })
}

fn spawn(program: &str, args: &[String], file: &Path) -> bool {
    let child = std::process::Command::new(program)
        .args(args)
        .arg(file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match child {
        Ok(mut child) => {
            // Reap it in the background so no zombie stays behind.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// Play a cue at `volume` (1.0 is the file's own level) without waiting for it.
pub fn play(cue: Cue, volume: f32) {
    let Some(file) = find(cue, &data_dirs()) else {
        return;
    };
    let volume = volume.clamp(0., 2.);
    let played = spawn("pw-play", &[format!("--volume={volume:.2}")], &file)
        || spawn(
            "paplay",
            &[format!("--volume={}", (volume * 65536.) as u32)],
            &file,
        );
    if !played {
        eprintln!("rexia: no pw-play or paplay to play sounds with");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_fall_back_through_names_and_directories() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let theme = second.path().join("sounds/freedesktop/stereo");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(theme.join("dialog-warning.oga"), b"").unwrap();
        std::fs::write(theme.join("complete.oga"), b"").unwrap();
        let dirs = vec![first.path().to_owned(), second.path().to_owned()];
        assert_eq!(
            find(Cue::Blocked, &dirs),
            Some(theme.join("dialog-warning.oga"))
        );
        assert_eq!(find(Cue::Done, &dirs), Some(theme.join("complete.oga")));
        assert_eq!(
            find(Cue::Error, &dirs),
            Some(theme.join("dialog-warning.oga"))
        );
        assert_eq!(find(Cue::Done, &[first.path().to_owned()]), None);
    }
}
