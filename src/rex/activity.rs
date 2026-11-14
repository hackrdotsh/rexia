//! What programs in a terminal report about themselves (Program Status Protocol, OSC 7501),
//! and the terminal events that arrive as `block_event`s on the control connection.
use super::data::TERMINAL_CREATOR;
use serde_json::Value;
use std::collections::BTreeMap;

/// A record's state. Ordered by urgency: a tab shows its most urgent record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProgramState {
    #[default]
    Idle,
    Done,
    Working,
    Error,
    Blocked,
}

impl ProgramState {
    /// Unknown states read as idle, so a newer server's states never raise an alarm.
    pub fn parse(value: &str) -> Self {
        match value {
            "working" => Self::Working,
            "done" => Self::Done,
            "blocked" => Self::Blocked,
            "error" => Self::Error,
            _ => Self::Idle,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Done => "Done",
            Self::Working => "Working",
            Self::Error => "Error",
            Self::Blocked => "Needs attention",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

impl BlockedKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Permission => "Needs approval",
            Self::Question => "Question",
            Self::Auth => "Needs sign-in",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatusRecord {
    pub state: ProgramState,
    pub kind: Option<BlockedKind>,
    pub progress: Option<u8>,
    /// The program that reported, as `effective_app` (or `app`) names it.
    pub app: Option<String>,
    /// The program's own words; shown only as plain text.
    pub title: Option<String>,
    pub msg: Option<String>,
    pub updated_at: Option<String>,
}

impl StatusRecord {
    pub fn parse(value: &Value) -> Self {
        let text = |key: &str| {
            value[key]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        Self {
            state: ProgramState::parse(value["state"].as_str().unwrap_or_default()),
            kind: match value["kind"].as_str() {
                Some("permission") => Some(BlockedKind::Permission),
                Some("question") => Some(BlockedKind::Question),
                Some("auth") => Some(BlockedKind::Auth),
                _ => None,
            },
            progress: value["progress"].as_u64().map(|p| p.min(100) as u8),
            app: text("effective_app").or_else(|| text("app")),
            title: text("title"),
            msg: text("msg"),
            updated_at: text("updated_at"),
        }
    }

    /// The word a tab shows for this record.
    pub fn badge(&self) -> &'static str {
        match (self.state, self.kind) {
            (ProgramState::Blocked, Some(kind)) => kind.label(),
            (ProgramState::Blocked, None) => "Needs input",
            (state, _) => state.name(),
        }
    }
}

/// The most urgent record of a block, which decides its tab indicator.
pub fn summary(records: &BTreeMap<String, StatusRecord>) -> Option<&StatusRecord> {
    records.values().max_by_key(|r| r.state)
}

/// The name to show for a process: the basename of the first of `script`, `argv0` and
/// `name` that is present, without a login shell's leading `-`.
pub fn program_name(process: &Value) -> Option<String> {
    ["script", "argv0", "name"]
        .iter()
        .filter_map(|key| process[*key].as_str())
        .find(|s| !s.is_empty())
        .map(|s| s.rsplit('/').next().unwrap_or(s).trim_start_matches('-'))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Process {
    pub name: Option<String>,
    pub cwd: Option<String>,
    /// The foreground process and its ancestors.
    pub pids: Vec<u64>,
}

impl Process {
    pub fn parse(payload: &Value) -> Self {
        let foreground = &payload["foreground"];
        let mut pids: Vec<u64> = foreground["pid"].as_u64().into_iter().collect();
        pids.extend(
            payload["ancestors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a["pid"].as_u64()),
        );
        Self {
            name: program_name(foreground),
            cwd: foreground["cwd"].as_str().map(str::to_owned),
            pids,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlockEvent {
    StatusChanged {
        id: String,
        record: StatusRecord,
    },
    StatusRemoved {
        ids: Vec<String>,
    },
    Process(Process),
    /// A program set the title (empty: cleared), with the pid that set it.
    Title {
        text: String,
        owner: Option<u64>,
    },
    Pwd(String),
    Notification {
        title: String,
        body: String,
    },
    Progress {
        state: String,
        progress: Option<u8>,
    },
    Bell,
    Exited {
        code: i64,
    },
}

/// A terminal `block_event`'s block and meaning; none for other creators and events.
pub fn parse_block_event(payload: &Value) -> Option<(String, BlockEvent)> {
    if payload["creator_name"] != TERMINAL_CREATOR {
        return None;
    }
    let block = payload["block_id"].as_str()?.to_owned();
    let inner = &payload["payload"];
    let event = match payload["name"].as_str()? {
        "program_status_changed" => BlockEvent::StatusChanged {
            id: inner["id"].as_str().unwrap_or_default().to_owned(),
            record: StatusRecord::parse(&inner["record"]),
        },
        "program_status_removed" => BlockEvent::StatusRemoved {
            ids: inner["ids"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|id| id.as_str().unwrap_or_default().to_owned())
                .collect(),
        },
        "process_changed" => BlockEvent::Process(Process::parse(inner)),
        "title_changed" => BlockEvent::Title {
            text: inner["title"].as_str().unwrap_or_default().to_owned(),
            owner: inner["owner"]["pid"].as_u64(),
        },
        "pwd_changed" => BlockEvent::Pwd(pwd_path(inner["pwd"].as_str().unwrap_or_default())),
        "desktop_notification" => BlockEvent::Notification {
            title: inner["title"].as_str().unwrap_or_default().to_owned(),
            body: inner["body"].as_str().unwrap_or_default().to_owned(),
        },
        "progress_report" => BlockEvent::Progress {
            state: inner["state"].as_str().unwrap_or_default().to_owned(),
            progress: inner["progress"].as_u64().map(|p| p.min(100) as u8),
        },
        "bell" => BlockEvent::Bell,
        "child_exited" => BlockEvent::Exited {
            code: inner["exit_code"].as_i64().unwrap_or(0),
        },
        _ => return None,
    };
    Some((block, event))
}

/// `program_status` replies: every record by id (the root record has an empty id).
pub fn parse_status(reply: &Value) -> BTreeMap<String, StatusRecord> {
    reply["records"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|record| {
            (
                record["id"].as_str().unwrap_or_default().to_owned(),
                StatusRecord::parse(record),
            )
        })
        .collect()
}

/// OSC 7 reports `file://host/path`; the path is what's useful to show.
pub fn pwd_path(value: &str) -> String {
    let Some(rest) = value.strip_prefix("file://") else {
        return value.to_owned();
    };
    let path = rest.find('/').map_or("/", |i| &rest[i..]);
    percent_decode(path)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&value[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(name: &str, payload: Value) -> Value {
        json!({"block_id": "b", "creator_name": TERMINAL_CREATOR, "name": name, "payload": payload})
    }

    #[test]
    fn status_records_parse_and_rank() {
        let (block, parsed) = parse_block_event(&event(
            "program_status_changed",
            json!({"id": "", "record": {"state": "blocked", "kind": "permission",
                "app": "claude", "msg": "Allow edit?", "updated_at": "2026-11-14T00:00:00Z"}}),
        ))
        .unwrap();
        assert_eq!(block, "b");
        let BlockEvent::StatusChanged { record, .. } = parsed else {
            panic!("status event")
        };
        assert_eq!(record.badge(), "Needs approval");
        assert_eq!(record.app.as_deref(), Some("claude"));

        let mut records = BTreeMap::new();
        records.insert(
            "a".into(),
            StatusRecord::parse(&json!({"state": "working", "progress": 40})),
        );
        records.insert("b".into(), record);
        records.insert(
            "c".into(),
            StatusRecord::parse(&json!({"state": "from-the-future"})),
        );
        assert_eq!(summary(&records).unwrap().state, ProgramState::Blocked);
        assert_eq!(records["a"].progress, Some(40));
        assert_eq!(records["c"].state, ProgramState::Idle);
    }

    #[test]
    fn process_title_and_pwd() {
        let (_, process) = parse_block_event(&event(
            "process_changed",
            json!({"foreground": {"pid": 9, "name": "node", "script": "/usr/lib/claude/cli.js",
                "cwd": "/home/me"}, "ancestors": [{"pid": 3, "argv0": "-zsh"}]}),
        ))
        .unwrap();
        assert_eq!(
            process,
            BlockEvent::Process(Process {
                name: Some("cli.js".into()),
                cwd: Some("/home/me".into()),
                pids: vec![9, 3],
            })
        );
        assert_eq!(
            program_name(&json!({"argv0": "-zsh"})).as_deref(),
            Some("zsh")
        );
        let (_, pwd) = parse_block_event(&event(
            "pwd_changed",
            json!({"pwd": "file://box/home/me/my%20dir"}),
        ))
        .unwrap();
        assert_eq!(pwd, BlockEvent::Pwd("/home/me/my dir".into()));
        assert!(
            parse_block_event(&json!({"creator_name": "other", "block_id": "b", "name": "bell"}))
                .is_none()
        );
    }

    #[test]
    fn status_query_reply() {
        let records = parse_status(&json!({"records": [
            {"state": "working", "updated_at": "x"},
            {"id": "build/test", "state": "error", "msg": "2 failed", "updated_at": "x"}]}));
        assert_eq!(records.len(), 2);
        assert_eq!(records["build/test"].msg.as_deref(), Some("2 failed"));
        assert_eq!(records[""].state, ProgramState::Working);
    }
}
