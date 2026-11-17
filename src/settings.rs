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

/// The size of session and tab titles in the sidebar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarTextSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl SidebarTextSize {
    /// Session title size, tab title size, and tab row height, in pixels.
    pub fn metrics(self) -> (f32, f32, f32) {
        match self {
            Self::Small => (12., 11., 24.),
            Self::Medium => (14., 13., 28.),
            Self::Large => (16., 15., 32.),
        }
    }
}

/// Which touchpad swipe shows or hides the sidebar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarGesture {
    /// A two-finger swipe sideways; hold Alt to scroll content sideways instead.
    #[default]
    Swipe,
    /// A swipe sideways with Alt held.
    AltSwipe,
    Off,
}

/// Which tab icons draw in their program's colors; the rest draw in the text color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabIconColor {
    #[default]
    Always,
    /// The icon of each tab's focused pane.
    ActivePanes,
    /// Every icon in the selected tab.
    ActiveTabs,
    Never,
}

/// How a colored shell icon is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellIconTheme {
    /// Its own artwork: a light glyph on a dark tile.
    #[default]
    Default,
    /// The terminal theme's foreground on its background.
    Theme,
    /// The theme's accent.
    Accent,
}

/// How panes move, the active border travels, and the sidebar shows or hides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationStyle {
    /// Quick and direct.
    Snappy,
    /// Slower, easing in and out.
    #[default]
    Natural,
    /// No motion: everything moves at once.
    Off,
}

impl AnimationStyle {
    /// How long a motion takes, or `None` when nothing animates.
    pub fn duration(self) -> Option<std::time::Duration> {
        match self {
            Self::Snappy => Some(std::time::Duration::from_millis(120)),
            Self::Natural => Some(std::time::Duration::from_millis(260)),
            Self::Off => None,
        }
    }
}

/// Where terminal text sits in a pane taller than its whole lines: the leftover part of
/// a line goes below the text (Top) or above it (Bottom).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalAnchor {
    #[default]
    Top,
    Bottom,
}

/// Whether links that launch other apps, such as ssh://, open on ctrl+click.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppLinks {
    #[default]
    Ask,
    Always,
    Never,
}

/// A color for selected text or behind it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionColor {
    /// The terminal theme's selection color.
    #[default]
    Theme,
    /// The interface's accent.
    Accent,
    /// Text keeps the colors its program gave it (foreground only).
    Keep,
    /// One of the 16 terminal colors.
    Palette(u8),
}

/// How selections look: their colors, corners, and the shine that confirms a copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectionLook {
    pub foreground: SelectionColor,
    pub background: SelectionColor,
    pub rounded: bool,
    pub copy_animation: bool,
}

impl Default for SelectionLook {
    fn default() -> Self {
        Self {
            foreground: SelectionColor::Theme,
            background: SelectionColor::Theme,
            rounded: false,
            copy_animation: true,
        }
    }
}

/// The effect cursor shaders draw behind a cursor that jumps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorShader {
    #[default]
    Warp,
    Sweep,
    Comet,
    Blaze,
    Tapered,
    Rainbow,
    Gradient,
    Frozen,
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
    /// The weight of ordinary terminal text, 100 to 900: a face of the font such as Light,
    /// or a step of a variable font. None draws the regular weight.
    pub font_weight: Option<u16>,
    pub line_height: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub copy_on_select: bool,
    pub selection: SelectionLook,
    /// Show the keys that can come next after the first key of a sequence.
    pub key_hints: bool,
    /// Move through history by pixels instead of whole lines.
    pub smooth_scrolling: bool,
    /// Reflow text on the frame a pane changes size, without waiting for the server.
    pub smooth_resizing: bool,
    /// Experimental: pin terminal text to the top or the bottom of each pane.
    pub anchor: TerminalAnchor,
    /// List recently chosen paths first in the path browser.
    pub recent_paths: bool,
    pub app_links: AppLinks,
    /// App links chosen to always open, by scheme and host (`ssh://build`).
    pub trusted_links: Vec<String>,
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
    /// The program and arguments new local terminals run; empty runs the login shell.
    pub startup_command: String,
    /// Name panes after their program and directory, not titles shells set.
    pub prefer_generated_titles: bool,
    /// Draw a trail behind the cursor when it jumps.
    pub cursor_shaders: bool,
    pub cursor_shader: CursorShader,
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
            font_weight: None,
            line_height: 1.0,
            padding_x: 8.,
            padding_y: 6.,
            copy_on_select: false,
            selection: SelectionLook::default(),
            key_hints: true,
            smooth_scrolling: true,
            smooth_resizing: true,
            anchor: TerminalAnchor::Top,
            recent_paths: true,
            app_links: AppLinks::Ask,
            trusted_links: Vec::new(),
            padding_fill: PaddingFill::Off,
            padding_fill_header: true,
            cursor_style: crate::rex::stream::CursorShape::Block,
            force_cursor_style: false,
            cursor_blink: BlinkSpeed::Normal,
            cursor_blink_ms: 600,
            startup_command: String::new(),
            prefer_generated_titles: false,
            cursor_shaders: false,
            cursor_shader: CursorShader::Warp,
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
    /// The sidebar's width in pixels, as dragged.
    pub sidebar_width: f32,
    pub toolbar_layout: ToolbarLayout,
    pub toolbar_behavior: ToolbarBehavior,
    /// The side Toggle Toolbar Layout goes back to from Above.
    pub vertical_side: ToolbarLayout,
    /// Horizontal tabs share the tab bar equally.
    pub tabs_fill: bool,
    pub animation: AnimationStyle,
    /// Experimental: Go to Anything lists every session, pane and reporting program.
    pub quick_switcher: bool,
    pub sidebar_text_size: SidebarTextSize,
    pub sidebar_gesture: SidebarGesture,
    /// Spread or pinch over a pane to zoom or restore it, and swipe for the sidebar.
    pub trackpad_gestures: bool,
    pub tab_icon_color: TabIconColor,
    /// Legacy preference retained for config compatibility; shell icons now follow their pane.
    pub shell_icon_theme: ShellIconTheme,
    /// How many pane icons a tab shows: up to three in vertical tabs, five in horizontal.
    pub tab_icons: u8,
    pub pane_style: PaneStyle,
    pub pane_headers: PaneHeaders,
    /// Default-background alpha, from 0 (clear) to 1 (opaque); program colors stay opaque.
    pub active_pane_opacity: f32,
    pub inactive_pane_opacity: f32,
    pub highlight_active_pane: bool,
    pub highlight_color: HighlightColor,
    /// How strongly the highlight draws, from 0.2 to 1.
    pub highlight_opacity: f32,
    pub highlight_animation: HighlightAnimation,
    pub fade_inactive: FadeInactive,
    /// Fade the panes of a window that isn't focused, as inactive panes fade.
    pub fade_inactive_windows: bool,
    pub focus_follows_mouse: bool,
    /// Even out a tab's splits when a pane opens or closes, keeping dividers people place.
    pub autobalance_splits: bool,
    /// Show the activity filter in the sidebar instead of sessions.
    pub activity_view: bool,
    /// Sort the activity filter by urgency instead of grouping it by session.
    pub activity_by_priority: bool,
}

impl Default for InterfaceSettings {
    fn default() -> Self {
        Self {
            sidebar: true,
            sidebar_width: 260.,
            toolbar_layout: ToolbarLayout::Above,
            toolbar_behavior: ToolbarBehavior::Always,
            vertical_side: ToolbarLayout::Left,
            tabs_fill: false,
            animation: AnimationStyle::Natural,
            quick_switcher: false,
            sidebar_text_size: SidebarTextSize::Medium,
            sidebar_gesture: SidebarGesture::Swipe,
            trackpad_gestures: true,
            tab_icon_color: TabIconColor::Always,
            shell_icon_theme: ShellIconTheme::Default,
            tab_icons: 3,
            pane_style: PaneStyle::Comfortable,
            pane_headers: PaneHeaders::Always,
            active_pane_opacity: 1.,
            inactive_pane_opacity: 1.,
            highlight_active_pane: true,
            highlight_color: HighlightColor::Accent,
            highlight_opacity: 1.0,
            highlight_animation: HighlightAnimation::Fade,
            fade_inactive: FadeInactive::Off,
            fade_inactive_windows: false,
            focus_follows_mouse: false,
            autobalance_splits: true,
            activity_view: false,
            activity_by_priority: false,
        }
    }
}

impl InterfaceSettings {
    pub fn pane_opacity(&self, selected: bool, window_active: bool) -> f32 {
        let value = if selected && window_active {
            self.active_pane_opacity
        } else {
            self.inactive_pane_opacity
        };
        if value.is_finite() {
            value.clamp(0., 1.)
        } else {
            1.
        }
    }

    pub fn translucent_panes(&self) -> bool {
        self.pane_opacity(true, true) < 1. || self.pane_opacity(false, true) < 1.
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
    /// Desktop notifications about a server's built-in Tailscale: needing a sign-in,
    /// needing an admin's approval, and stopping or reporting a problem.
    pub tailscale_sign_in: bool,
    pub tailscale_approval: bool,
    pub tailscale_failures: bool,
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
            tailscale_sign_in: true,
            tailscale_approval: true,
            tailscale_failures: true,
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
    /// The order hosts are listed in, by ID, once someone moves one.
    pub host_order: Vec<String>,
    /// Local image paths keyed by stable host ID; never sent to the Rex server.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub host_icons: std::collections::BTreeMap<String, PathBuf>,
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
    /// The SSH agent socket ssh asks for keys for ssh:// hosts; none leaves it to ssh.
    pub ssh_agent: Option<std::path::PathBuf>,
    /// Paths chosen in the path browser, newest first, by host ID: every session on a
    /// host shares them. Directories end in `/`.
    pub recent_paths: std::collections::BTreeMap<String, Vec<String>>,
    /// Browser origins allowed to open Rex WebSocket streams, passed to the server when
    /// rexia starts it.
    pub web_origins: Vec<String>,
    /// The rex rexia runs for Restart Server and `rex skill`: an executable, or a source
    /// checkout holding one; none runs the rex on PATH.
    pub rex_location: Option<std::path::PathBuf>,
    /// The origins rexia last started each host's server with (by host ID), and which
    /// server process that was, so changes since show as pending.
    pub applied_origins: std::collections::BTreeMap<String, AppliedOrigins>,
    /// Muted tabs, by host ID: their window IDs. A muted tab sends no notifications or
    /// sounds and adds nothing to the launcher badge.
    pub muted_tabs: std::collections::BTreeMap<String, Vec<String>>,
}

/// Web client origins a server was started with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppliedOrigins {
    pub origins: Vec<String>,
    /// The server process that was running before the restart, until the new one is seen.
    pub before: Option<String>,
    /// The server process started with these origins, once seen.
    pub instance: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hosts: vec![HostConfig::local()],
            host_order: Vec::new(),
            host_icons: Default::default(),
            client_label: None,
            interface: InterfaceSettings::default(),
            terminal: TerminalSettings::default(),
            notifications: NotificationSettings::default(),
            last_host: None,
            last_session: None,
            shortcuts: Default::default(),
            ssh_agent: None,
            recent_paths: Default::default(),
            web_origins: Vec::new(),
            rex_location: None,
            applied_origins: Default::default(),
            muted_tabs: Default::default(),
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
        assert_eq!(partial.interface.active_pane_opacity, 1.);
        assert_eq!(partial.interface.inactive_pane_opacity, 1.);
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
