//! rexia's preferences, saved as TOML in `$XDG_CONFIG_HOME/rexia/settings.toml`.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostConfig {
    /// Stable across renames; `local` for this machine's server.
    pub id: String,
    pub label: String,
    /// A server address, as `rex -S` takes it. Empty means the local server.
    #[serde(default)]
    pub endpoint: String,
}

impl HostConfig {
    pub fn local() -> Self {
        Self {
            id: "local".into(),
            label: hostname(),
            endpoint: String::new(),
        }
    }

    pub fn is_local(&self) -> bool {
        self.endpoint.is_empty()
    }
}

pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "This computer".into())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolbarLayout {
    #[default]
    Above,
    Left,
    Right,
}

impl ToolbarLayout {
    /// Tabs listed down the sidebar instead of across the top.
    pub fn vertical(self) -> bool {
        self != Self::Above
    }
}

/// What color the active pane's highlight draws in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighlightColor {
    /// Noctalia's primary color.
    #[default]
    Accent,
    /// One of the terminal's 16 colors.
    Palette(u8),
    /// The terminal's text color.
    Text,
}

/// How the highlight reaches a newly focused pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighlightAnimation {
    #[default]
    Fade,
    Move,
    Instant,
}

/// When the toolbar shows: always, when the pointer reaches the top of the window, or only
/// after Reveal Toolbar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolbarBehavior {
    #[default]
    Always,
    Hover,
    Hidden,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneStyle {
    #[default]
    Comfortable,
    Compact,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneHeaders {
    #[default]
    Always,
    Hover,
    Hidden,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FadeInactive {
    #[default]
    Off,
    Surface,
    Content,
    Both,
}

/// Whether a program's background colors reach into the padding around its text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaddingFill {
    #[default]
    Off,
    /// While a full-screen program runs, along each edge that has one color.
    Automatic,
    /// Every edge cell's color, whatever is running.
    Always,
}

/// Which of bold and italic text draws in its style; the rest draws plain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextStyles {
    #[default]
    Both,
    BoldOnly,
    ItalicOnly,
    Neither,
}

impl TextStyles {
    pub fn bold(self) -> bool {
        matches!(self, Self::Both | Self::BoldOnly)
    }

    pub fn italic(self) -> bool {
        matches!(self, Self::Both | Self::ItalicOnly)
    }
}

/// How fast cursors blink when their program asks for a blinking cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlinkSpeed {
    Off,
    Slow,
    #[default]
    Normal,
    Fast,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalSettings {
    /// Overrides the font found in the Ghostty config or fontconfig.
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub line_height: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub copy_on_select: bool,
    pub padding_fill: PaddingFill,
    /// Color the pane header to match the program too, while the padding fills.
    pub padding_fill_header: bool,
    /// The cursor programs get until they pick one.
    pub cursor_style: crate::rex::stream::CursorShape,
    /// Ignore programs that change the cursor's shape.
    pub force_cursor_style: bool,
    pub cursor_blink: BlinkSpeed,
    /// The time between blinks with a custom speed, in milliseconds.
    pub cursor_blink_ms: u32,
    /// Pixels added to (or, negative, taken from) each character's width.
    pub character_spacing: f32,
    /// Join characters like -> and != into single symbols.
    pub ligatures: bool,
    pub text_styles: TextStyles,
    /// Fonts, in order, for characters the terminal font doesn't have.
    pub fallback_fonts: Vec<String>,
    /// Lift text that's too faint to read against its background.
    pub improve_contrast: bool,
    /// Map program colors to the nearest theme color (and improve contrast).
    pub theme_palette: bool,
}

impl TerminalSettings {
    /// The time between blinks, or none when cursors never blink.
    pub fn blink_interval(&self) -> Option<std::time::Duration> {
        let ms = match self.cursor_blink {
            BlinkSpeed::Off => return None,
            BlinkSpeed::Slow => 900,
            BlinkSpeed::Normal => 600,
            BlinkSpeed::Fast => 300,
            BlinkSpeed::Custom => self.cursor_blink_ms.clamp(100, 5000),
        };
        Some(std::time::Duration::from_millis(ms as u64))
    }
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: None,
            line_height: 1.0,
            padding_x: 8.,
            padding_y: 6.,
            copy_on_select: false,
            padding_fill: PaddingFill::Off,
            padding_fill_header: true,
            cursor_style: crate::rex::stream::CursorShape::Block,
            force_cursor_style: false,
            cursor_blink: BlinkSpeed::Normal,
            cursor_blink_ms: 600,
            character_spacing: 0.,
            ligatures: false,
            text_styles: TextStyles::Both,
            fallback_fonts: Vec::new(),
            improve_contrast: false,
            theme_palette: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceSettings {
    pub sidebar: bool,
    pub toolbar_layout: ToolbarLayout,
    pub toolbar_behavior: ToolbarBehavior,
    /// The side Toggle Toolbar Layout goes back to from Above.
    pub vertical_side: ToolbarLayout,
    /// Horizontal tabs share the tab bar equally.
    pub tabs_fill: bool,
    pub pane_style: PaneStyle,
    pub pane_headers: PaneHeaders,
    pub highlight_active_pane: bool,
    pub highlight_color: HighlightColor,
    /// How strongly the highlight draws, from 0.2 to 1.
    pub highlight_opacity: f32,
    pub highlight_animation: HighlightAnimation,
    pub fade_inactive: FadeInactive,
    /// Fade the panes of a window that isn't focused, as inactive panes fade.
    pub fade_inactive_windows: bool,
    pub focus_follows_mouse: bool,
    /// Show the activity filter in the sidebar instead of sessions.
    pub activity_view: bool,
    /// Sort the activity filter by urgency instead of grouping it by session.
    pub activity_by_priority: bool,
}

impl Default for InterfaceSettings {
    fn default() -> Self {
        Self {
            sidebar: true,
            toolbar_layout: ToolbarLayout::Above,
            toolbar_behavior: ToolbarBehavior::Always,
            vertical_side: ToolbarLayout::Left,
            tabs_fill: false,
            pane_style: PaneStyle::Comfortable,
            pane_headers: PaneHeaders::Always,
            highlight_active_pane: true,
            highlight_color: HighlightColor::Accent,
            highlight_opacity: 1.0,
            highlight_animation: HighlightAnimation::Fade,
            fade_inactive: FadeInactive::Off,
            fade_inactive_windows: false,
            focus_follows_mouse: false,
            activity_view: false,
            activity_by_priority: false,
        }
    }
}

/// What rexia does when a program reports one state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StateActivity {
    pub tabs: bool,
    pub notify: bool,
    pub sound: bool,
}

impl Default for StateActivity {
    fn default() -> Self {
        Self {
            tabs: true,
            notify: true,
            sound: true,
        }
    }
}

/// Where notices float in the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticePosition {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
}

impl NoticePosition {
    pub const ALL: [NoticePosition; 6] = [
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomCenter,
        Self::BottomRight,
    ];

    pub fn top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopCenter | Self::TopRight)
    }

    /// 0 for left, 1 for center, 2 for right.
    pub fn column(self) -> usize {
        match self {
            Self::TopLeft | Self::BottomLeft => 0,
            Self::TopCenter | Self::BottomCenter => 1,
            Self::TopRight | Self::BottomRight => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "Top left",
            Self::TopCenter => "Top center",
            Self::TopRight => "Top right",
            Self::BottomLeft => "Bottom left",
            Self::BottomCenter => "Bottom center",
            Self::BottomRight => "Bottom right",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    /// Where in-app notices float in the window.
    pub notice_position: NoticePosition,
    /// Show working dots and attention badges on tabs.
    pub activity_in_tabs: bool,
    /// Keep tabs whose programs report idle in the activity list.
    pub include_idle: bool,
    /// List each reporting pane of a tab on its own row.
    pub each_process: bool,
    /// Desktop notifications for program status while the pane isn't in view.
    pub system_notifications: bool,
    /// The count of unseen attention on rexia's launcher icon.
    pub badge: bool,
    pub sounds: bool,
    pub sound_volume: f32,
    /// Let programs send desktop notifications with OSC 9 and OSC 777.
    pub terminal_notifications: bool,
    pub working: StateActivity,
    pub blocked: StateActivity,
    pub error: StateActivity,
    pub done: StateActivity,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            activity_in_tabs: true,
            include_idle: false,
            each_process: false,
            system_notifications: true,
            badge: true,
            sounds: false,
            sound_volume: 1.0,
            terminal_notifications: true,
            notice_position: NoticePosition::BottomCenter,
            working: StateActivity {
                tabs: true,
                notify: false,
                sound: false,
            },
            blocked: StateActivity::default(),
            error: StateActivity::default(),
            done: StateActivity::default(),
        }
    }
}

impl NotificationSettings {
    pub fn for_state(&self, state: crate::rex::activity::ProgramState) -> StateActivity {
        use crate::rex::activity::ProgramState;
        match state {
            ProgramState::Working => self.working,
            ProgramState::Blocked => self.blocked,
            ProgramState::Error => self.error,
            ProgramState::Done => self.done,
            ProgramState::Idle => StateActivity {
                tabs: self.include_idle,
                notify: false,
                sound: false,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub hosts: Vec<HostConfig>,
    /// The label this client shows in `rex client ls`.
    pub client_label: Option<String>,
    pub interface: InterfaceSettings,
    pub terminal: TerminalSettings,
    pub notifications: NotificationSettings,
    /// The host and session shown last, restored at launch.
    pub last_host: Option<String>,
    pub last_session: Option<String>,
    /// Keys the user gave rexia's actions with Change Shortcut, replacing their defaults.
    /// An empty list leaves the action without a key.
    pub shortcuts: std::collections::BTreeMap<String, Vec<String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hosts: vec![HostConfig::local()],
            client_label: None,
            interface: InterfaceSettings::default(),
            terminal: TerminalSettings::default(),
            notifications: NotificationSettings::default(),
            last_host: None,
            last_session: None,
            shortcuts: Default::default(),
        }
    }
}

pub fn default_path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    config.join("rexia/settings.toml")
}

impl Settings {
    /// Read settings; a missing file gives the defaults.
    pub fn load(path: &Path) -> Result<Self> {
        let mut settings: Settings = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .with_context(|| format!("couldn't read {}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(error.into()),
        };
        if settings.hosts.is_empty() {
            settings.hosts.push(HostConfig::local());
        }
        Ok(settings)
    }

    /// Write atomically, so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("toml.tmp");
        std::fs::write(&temporary, toml::to_string_pretty(self)?)?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_fills_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rexia/settings.toml");
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        let mut settings = Settings::default();
        settings.interface.sidebar = false;
        settings.terminal.font_size = Some(13.);
        settings.hosts.push(HostConfig {
            id: "studio".into(),
            label: "studio".into(),
            endpoint: "https://studio.tail.ts.net".into(),
        });
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        std::fs::write(&path, "[interface]\npane_headers = \"hidden\"\n").unwrap();
        let partial = Settings::load(&path).unwrap();
        assert_eq!(partial.interface.pane_headers, PaneHeaders::Hidden);
        assert!(partial.interface.sidebar);
        assert_eq!(partial.hosts.len(), 1, "the local host is always there");
        std::fs::write(&path, "[notifications.done]\nsound = false\n").unwrap();
        let quiet = Settings::load(&path).unwrap();
        assert!(!quiet.notifications.done.sound);
        assert!(
            quiet.notifications.done.notify,
            "unset fields keep their defaults"
        );
        assert!(!quiet.notifications.working.notify);
    }
}
