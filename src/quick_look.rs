//! Quick Look targets and desktop previews. Files go to GNOME's
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

#[proxy(
    interface = "org.freedesktop.FileManager1",
    default_service = "org.freedesktop.FileManager1",
    default_path = "/org/freedesktop/FileManager1"
)]
trait FileManager {
    fn show_items(&self, uris: &[&str], startup_id: &str) -> zbus::Result<()>;
}

async fn bus(address: Option<&str>) -> anyhow::Result<zbus::Connection> {
    Ok(match address {
        Some(address) => zbus::connection::Builder::address(address)?.build().await?,
        None => zbus::Connection::session().await?,
    })
}

/// Show a file in the desktop's file manager (`org.freedesktop.FileManager1`), selected
/// in its folder, on the session bus or the bus at `address`.
pub fn show_in_files(address: Option<&str>, uri: &str) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let manager = FileManagerProxy::new(&bus(address).await?).await?;
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            manager.show_items(&[uri], ""),
        )
        .await
        .map_err(|_| anyhow::anyhow!("the file manager didn't answer"))??;
        Ok(())
    })
}

/// Show `uri` in GNOME's previewer, on the session bus or the bus at `address`.
pub fn preview(address: Option<&str>, uri: &str) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let previewer = PreviewerProxy::new(&bus(address).await?).await?;
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

/// Decode a program-supplied link. A file URI refers to the terminal's machine,
/// not necessarily this client, even when its authority names that machine.
pub fn from_uri(link: &str) -> Option<Target> {
    let url = url::Url::parse(link).ok()?;
    if url.scheme() == "file" {
        let local = url::Url::parse(&format!("file://{}", url.path())).ok()?;
        Some(Target::Path(
            local.to_file_path().ok()?.display().to_string(),
        ))
    } else {
        Some(Target::Web(link.to_owned()))
    }
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
    if word.starts_with("file://") {
        return from_uri(&word);
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

/// Candidate links in reading order, with character ranges in the displayed line.
pub fn targets(line: &str) -> Vec<(std::ops::Range<usize>, Target)> {
    let chars: Vec<char> = line.chars().collect();
    let mut found = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        if chars[start].is_whitespace() || "\"'`<>()[]{}|".contains(chars[start]) {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < chars.len()
            && !chars[end].is_whitespace()
            && !"\"'`<>()[]{}|".contains(chars[end])
        {
            end += 1;
        }
        if let Some(target) = target_at(line, start) {
            found.push((start..end, target));
        }
        start = end;
    }
    found
}

#[derive(Clone, Debug)]
pub struct VisibleLink {
    pub target: Target,
    pub row: u16,
    pub columns: std::ops::Range<u16>,
}

/// Combine text links with OSC 8 targets. The latter win when a label also looks
/// like a path. Ranges use terminal columns, including wide-character spacing.
pub fn visible_links(
    frame: &crate::rex::stream::Frame,
    program_links: &[(u16, u16, String)],
    cwd: Option<&str>,
    local: bool,
) -> Vec<VisibleLink> {
    use crate::rex::stream::flags;
    let mut result = Vec::new();
    for (row, cells) in frame.lines.iter().enumerate() {
        let mut text = String::new();
        let mut columns = Vec::new();
        for (column, cell) in cells.iter().enumerate() {
            if cell.has(flags::SPACER) {
                continue;
            }
            let content = if cell.text.is_empty() {
                " "
            } else {
                &cell.text
            };
            text.push_str(content);
            columns.extend(content.chars().map(|_| column as u16));
        }
        for (range, target) in targets(&text) {
            let start = columns[range.start];
            let end = columns[range.end - 1] + 1;
            if cells[usize::from(start)..usize::from(end)]
                .iter()
                .any(|c| c.has(flags::LINK))
            {
                continue;
            }
            if let Target::Path(path) = &target
                && local
                && resolve(path, cwd).is_none()
            {
                continue;
            }
            result.push(VisibleLink {
                target,
                row: row as u16,
                columns: start..end,
            });
        }
    }
    let mut previous: Option<VisibleLink> = None;
    for (column, row, uri) in program_links {
        let Some(target) = from_uri(uri) else {
            continue;
        };
        if let Some(link) = &mut previous
            && link.row == *row
            && link.columns.end == *column
            && link.target == target
        {
            link.columns.end += 1;
            continue;
        }
        if let Some(link) = previous.take() {
            result.push(link);
        }
        previous = Some(VisibleLink {
            target,
            row: *row,
            columns: *column..*column + 1,
        });
    }
    if let Some(link) = previous {
        result.push(link);
    }
    result.sort_by_key(|link| (link.row, link.columns.start));
    result
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
    fn links_keep_terminal_columns_and_program_targets() {
        use crate::rex::stream::{Cell, Frame, flags};
        let cell = |text: &str, flags| Cell {
            text: text.into(),
            fg: 0,
            bg: None,
            flags,
        };
        let mut line = vec![
            cell("界", flags::WIDE),
            cell("", flags::SPACER),
            cell(" ", 0),
        ];
        line.extend("https://a.test ".chars().map(|c| cell(&c.to_string(), 0)));
        let label_start = line.len() as u16;
        line.extend(
            "label.txt"
                .chars()
                .map(|c| cell(&c.to_string(), flags::LINK)),
        );
        let program: Vec<_> = (label_start..line.len() as u16)
            .map(|x| (x, 0, "https://b.test".into()))
            .collect();
        let frame = Frame {
            lines: vec![line],
            ..Default::default()
        };
        let links = visible_links(&frame, &program, None, false);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].columns.start, 3);
        assert_eq!(links[0].target, Target::Web("https://a.test".into()));
        assert_eq!(links[1].columns.start, label_start);
        assert_eq!(links[1].target, Target::Web("https://b.test".into()));
        assert_eq!(
            from_uri("file://remote/tmp/a%20b.txt"),
            Some(Target::Path("/tmp/a b.txt".into()))
        );
    }

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
