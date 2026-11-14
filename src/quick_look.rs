//! Quick Look: preview the file or web link under the pointer. Files go to GNOME's
//! previewer (Sushi, `org.gnome.NautilusPreviewer`) over D-Bus; without it, and for web
//! links, the desktop's opener (`xdg-open`) takes them.
use std::path::{Path, PathBuf};
use zbus::proxy;

#[proxy(
    interface = "org.gnome.NautilusPreviewer2",
    default_service = "org.gnome.NautilusPreviewer",
    default_path = "/org/gnome/NautilusPreviewer"
)]
trait Previewer {
    fn show_file(&self, uri: &str, window_handle: &str, close_if_shown: bool) -> zbus::Result<()>;
}

/// Show `uri` in GNOME's previewer, on the session bus or the bus at `address`.
pub fn preview(address: Option<&str>, uri: &str) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let connection = match address {
            Some(address) => zbus::connection::Builder::address(address)?.build().await?,
            None => zbus::Connection::session().await?,
        };
        let previewer = PreviewerProxy::new(&connection).await?;
        // Sushi may need starting; give it a moment, not forever.
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            previewer.show_file(uri, "", false),
        )
        .await
        .map_err(|_| anyhow::anyhow!("the previewer didn't answer"))??;
        Ok(())
    })
}

/// What the pointer is on: a web link, or a path as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Web(String),
    Path(String),
}

/// The word around column `col` of `line`, as a link or path, if it looks like one:
/// `https://…`, `file://…`, or something with a `/` or a file extension. Quotes,
/// brackets and trailing punctuation are left off, and so is a `:line:col` suffix.
pub fn target_at(line: &str, col: usize) -> Option<Target> {
    let chars: Vec<char> = line.chars().collect();
    if col >= chars.len() || chars[col].is_whitespace() {
        return None;
    }
    let stop = |c: char| c.is_whitespace() || "\"'`<>()[]{}|".contains(c);
    let mut start = col;
    while start > 0 && !stop(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < chars.len() && !stop(chars[end]) {
        end += 1;
    }
    let mut word: String = chars[start..end].iter().collect();
    while word.ends_with(['.', ',', ';', '!', '?']) {
        word.pop();
    }
    if word.starts_with("http://") || word.starts_with("https://") {
        return Some(Target::Web(word));
    }
    if let Some(path) = word.strip_prefix("file://") {
        return Some(Target::Path(path.to_owned()));
    }
    // file.rs:12:4 points at a place in the file; the file is what's previewed.
    while let Some((head, tail)) = word.rsplit_once(':') {
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            word = head.to_owned();
        } else {
            break;
        }
    }
    let looks_like_path = word.contains('/')
        || Path::new(&word)
            .extension()
            .is_some_and(|e| !e.is_empty() && e.len() <= 8);
    (looks_like_path && !word.is_empty()).then_some(Target::Path(word))
}

/// A path as a program printed it, resolved from `cwd` with `~` expanded; `None` if
/// nothing's there.
pub fn resolve(path: &str, cwd: Option<&str>) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let path = match (path.strip_prefix("~/"), &home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if path == "~" => home?,
        _ => PathBuf::from(path),
    };
    let full = if path.is_absolute() {
        path
    } else {
        PathBuf::from(cwd?).join(path)
    };
    full.exists().then_some(full)
}

/// A `file://` URI for a local path.
pub fn file_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| format!("file://{}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_found_around_the_pointer() {
        let line = "see (src/main.rs:12:4), https://rex.dev/docs. and notes";
        assert_eq!(target_at(line, 8), Some(Target::Path("src/main.rs".into())));
        assert_eq!(
            target_at(line, 30),
            Some(Target::Web("https://rex.dev/docs".into()))
        );
        assert_eq!(target_at(line, 50), None, "a plain word");
        assert_eq!(target_at(line, 3), None, "a space");
        assert_eq!(
            target_at("open README.md", 7),
            Some(Target::Path("README.md".into()))
        );
        assert_eq!(
            target_at("x file:///tmp/a b", 4),
            Some(Target::Path("/tmp/a".into()))
        );
    }

    #[test]
    fn paths_resolve_from_the_pane_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.md"), "hi").unwrap();
        let cwd = dir.path().to_str().unwrap();
        assert_eq!(
            resolve("notes.md", Some(cwd)),
            Some(dir.path().join("notes.md"))
        );
        assert_eq!(resolve("missing.md", Some(cwd)), None);
        assert!(file_uri(&dir.path().join("a b.md")).ends_with("a%20b.md"));
    }
}
