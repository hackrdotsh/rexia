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
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceSettings {
    pub sidebar: bool,
    pub toolbar_layout: ToolbarLayout,
    pub pane_style: PaneStyle,
    pub pane_headers: PaneHeaders,
    pub highlight_active_pane: bool,
    pub fade_inactive: FadeInactive,
    pub focus_follows_mouse: bool,
    /// Show the activity filter in the sidebar instead of sessions.
    pub activity_view: bool,
}

impl Default for InterfaceSettings {
    fn default() -> Self {
        Self {
            sidebar: true,
            toolbar_layout: ToolbarLayout::Above,
            pane_style: PaneStyle::Comfortable,
            pane_headers: PaneHeaders::Always,
            highlight_active_pane: true,
            fade_inactive: FadeInactive::Off,
            focus_follows_mouse: false,
            activity_view: false,
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
    /// The host and session shown last, restored at launch.
    pub last_host: Option<String>,
    pub last_session: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hosts: vec![HostConfig::local()],
            client_label: None,
            interface: InterfaceSettings::default(),
            terminal: TerminalSettings::default(),
            last_host: None,
            last_session: None,
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
    }
}
