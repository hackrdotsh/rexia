//! Quick Look targets and desktop previews. Files go to GNOME's
//! previewer (Sushi, `org.gnome.NautilusPreviewer`) over D-Bus; without it, and for web
//! links, the desktop's opener (`xdg-open`) takes them.
use crate::rex::stream::{Frame, flags};
use std::{
    ops::Range,
    path::{Path, PathBuf},
};
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

/// The link at a character offset in a logical line, outside the terminal grid.
pub fn target_at(line: &str, col: usize) -> Option<Target> {
    targets(line)
        .into_iter()
        .find_map(|(range, target)| range.contains(&col).then_some(target))
}

fn delimiter(c: char) -> bool {
    c.is_whitespace() || "\"'`<>()[]{}|".contains(c)
}

fn location_suffix_len(chars: &[char]) -> usize {
    let mut end = 0;
    for _ in 0..2 {
        if chars.get(end) != Some(&':') {
            break;
        }
        let digits = chars[end + 1..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if digits == 0 {
            break;
        }
        end += digits + 1;
    }
    if chars
        .get(end)
        .is_none_or(|&c| delimiter(c) || ".,;!?".contains(c))
    {
        end
    } else {
        0
    }
}

fn path_without_location(mut path: &str) -> &str {
    for _ in 0..2 {
        match path.rsplit_once(':') {
            Some((name, suffix))
                if !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()) =>
            {
                path = name;
            }
            _ => break,
        }
    }
    path
}

fn text_target(word: &str, has_location: bool) -> Option<Target> {
    if ["http://", "https://", "file://"]
        .iter()
        .any(|prefix| word.starts_with(prefix))
    {
        return from_uri(word);
    }
    let path = path_without_location(word);
    let looks_like_path = path.contains('/')
        || has_location
        || path != word
        || Path::new(path).extension().is_some_and(|e| {
            let e = e.to_string_lossy();
            !e.is_empty() && !e.chars().any(char::is_whitespace)
        });
    (looks_like_path && !path.is_empty()).then(|| Target::Path(path.to_owned()))
}

fn quoted_word(chars: &[char], start: usize) -> Option<(String, usize)> {
    let quote = chars[start];
    let mut end = start + 1;
    let mut word = String::new();
    while end < chars.len() {
        let c = chars[end];
        if c == quote {
            return Some((word, end + 1));
        }
        if c == '\\' && quote == '"' && matches!(chars.get(end + 1), Some('"' | '\\')) {
            end += 1;
        }
        word.push(chars[end]);
        end += 1;
    }
    None
}

/// Candidate links in reading order, with character ranges in the displayed line.
pub fn targets(line: &str) -> Vec<(Range<usize>, Target)> {
    let chars: Vec<_> = line.chars().collect();
    let mut found = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        if matches!(chars[start], '"' | '\'' | '`')
            && (start == 0 || delimiter(chars[start - 1]))
            && let Some((word, end)) = quoted_word(&chars, start)
        {
            let suffix = location_suffix_len(&chars[end..]);
            if let Some(target) = text_target(&word, suffix != 0) {
                found.push((start..end + suffix, target));
                start = end + suffix;
                continue;
            }
        }
        if delimiter(chars[start]) {
            start += 1;
            continue;
        }
        let mut end = start;
        let mut word = String::new();
        while end < chars.len() && !delimiter(chars[end]) {
            if chars[end] == '\\'
                && chars
                    .get(end + 1)
                    .is_some_and(|&c| delimiter(c) || c == '\\')
            {
                end += 1;
            }
            word.push(chars[end]);
            end += 1;
        }
        let mut link_end = end;
        while word.ends_with(['.', ',', ';', '!', '?']) {
            word.pop();
            link_end -= 1;
        }
        if let Some(target) = text_target(&word, false) {
            found.push((start..link_end, target));
        }
        start = end;
    }
    found
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkSpan {
    pub row: u16,
    pub columns: Range<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleLink {
    pub target: Target,
    pub spans: Vec<LinkSpan>,
}

impl VisibleLink {
    pub fn contains(&self, x: u16, y: u16) -> bool {
        self.spans
            .iter()
            .any(|span| span.row == y && span.columns.contains(&x))
    }
}

fn continues(frame: &Frame, row: usize) -> bool {
    frame.wrap_continuations.get(row).copied().unwrap_or(false)
}

fn logical_end(frame: &Frame, start: usize) -> usize {
    (start + 1..frame.lines.len())
        .find(|&row| !continues(frame, row))
        .unwrap_or(frame.lines.len())
}

/// Hit-test only the logical line under the pointer, using the frame being drawn.
pub fn link_at(frame: &Frame, x: u16, y: u16) -> Option<VisibleLink> {
    let row = usize::from(y);
    frame.lines.get(row)?;
    let first = (1..=row)
        .rev()
        .find(|&row| !continues(frame, row))
        .unwrap_or(0);
    links_in_rows(frame, first..logical_end(frame, row))
        .into_iter()
        .find(|link| link.contains(x, y))
}

pub fn visible_links(frame: &Frame) -> Vec<VisibleLink> {
    let mut links = Vec::new();
    let mut first = 0;
    while first < frame.lines.len() {
        let end = logical_end(frame, first);
        links.extend(links_in_rows(frame, first..end));
        first = end;
    }
    links
}

/// Join only soft-wrapped rows, preserving the cell span of each character.
/// Program links win over text that happens to look like a path.
fn links_in_rows(frame: &Frame, rows: Range<usize>) -> Vec<VisibleLink> {
    let mut text = String::new();
    let mut characters = Vec::new();
    for row in rows.clone() {
        let cells = &frame.lines[row];
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
            let end = (column + if cell.has(flags::WIDE) { 2 } else { 1 }).min(cells.len());
            characters.extend(
                content
                    .chars()
                    .map(|_| (row as u16, column as u16, end as u16, cell.has(flags::LINK))),
            );
        }
    }
    let mut result = Vec::new();
    for (range, target) in targets(&text) {
        if (range.start == 0 && continues(frame, rows.start))
            || (range.end == characters.len()
                && rows.end == frame.lines.len()
                && frame.last_row_wrapped)
        {
            continue;
        }
        let characters = &characters[range];
        if characters.iter().any(|&(_, _, _, program)| program) {
            continue;
        }
        let spans = characters
            .chunk_by(|a, b| a.0 == b.0)
            .map(|cells| LinkSpan {
                row: cells[0].0,
                columns: cells[0].1..cells[cells.len() - 1].2,
            })
            .collect();
        result.push(VisibleLink { target, spans });
    }
    let mut previous_uri = None;
    let mut program_links: Vec<VisibleLink> = Vec::new();
    for link in frame
        .hyperlinks
        .iter()
        .filter(|link| rows.contains(&usize::from(link.row)))
    {
        let span = LinkSpan {
            row: link.row,
            columns: link.columns.clone(),
        };
        if let Some(previous) = program_links.last_mut()
            && previous_uri == Some(&link.uri)
            && let Some(last) = previous.spans.last()
            && last.row + 1 == span.row
            && span.columns.start == 0
            && frame.lines[usize::from(last.row)][usize::from(last.columns.end)..]
                .iter()
                .all(|cell| cell.has(flags::SPACER))
        {
            previous.spans.push(span);
        } else if let Some(target) = from_uri(&link.uri) {
            program_links.push(VisibleLink {
                target,
                spans: vec![span],
            });
        }
        previous_uri = Some(&link.uri);
    }
    result.extend(program_links);
    result.sort_by_key(|link| (link.spans[0].row, link.spans[0].columns.start));
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
        let cwd = Path::new(cwd?);
        if !cwd.is_absolute() {
            return None;
        }
        cwd.join(path)
    };
    full.exists().then_some(full)
}

pub fn needs_cwd(path: &str) -> bool {
    !Path::new(path).is_absolute() && path != "~" && !path.starts_with("~/")
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
        use crate::rex::stream::{Cell, Hyperlink};
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
        let frame = Frame {
            hyperlinks: vec![Hyperlink {
                row: 0,
                columns: label_start..line.len() as u16,
                uri: "https://b.test".into(),
            }],
            lines: vec![line],
            ..Default::default()
        };
        let links = visible_links(&frame);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].spans[0].columns.start, 3);
        assert_eq!(links[0].target, Target::Web("https://a.test".into()));
        assert_eq!(links[1].spans[0].columns.start, label_start);
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
    fn quoted_and_escaped_paths_include_spaces_and_location_suffixes() {
        let path = Some(Target::Path("src/my notes.rs".into()));
        assert_eq!(target_at("error: 'src/my notes.rs':12:4,", 14), path);
        assert_eq!(target_at("error: \"src/my notes.rs:12:4\"", 24), path);
        assert_eq!(target_at(r"open src/my\ notes.rs:12", 12), path);
        assert_eq!(target_at("see (src/main.rs:12:4),", 22), None);
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
