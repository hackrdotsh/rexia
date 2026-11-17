//! Shared names, destinations and validation for searchable client settings.
use crate::settings::Settings;
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: String,
    pub id: String,
    pub title: String,
    pub section: &'static str,
    pub group: &'static str,
    pub default: Value,
}

pub fn group(path: &str) -> &'static str {
    if matches!(path, "interface.quick_switcher" | "terminal.anchor") {
        return "experimental";
    }
    let field = path.rsplit('.').next().unwrap_or(path);
    if path.starts_with("interface.") {
        if field.starts_with("highlight") || field.starts_with("fade") {
            "active-pane"
        } else if field.starts_with("toolbar") || field == "tabs_fill" {
            "toolbar"
        } else if field.starts_with("sidebar")
            || field.starts_with("activity_")
            || field == "trackpad_gestures"
        {
            "sidebar"
        } else if matches!(field, "focus_follows_mouse" | "autobalance_splits") {
            "splits-focus"
        } else {
            "panes"
        }
    } else if path.starts_with("terminal.") {
        if field.starts_with("font")
            || matches!(
                field,
                "character_spacing" | "ligatures" | "text_styles" | "fallback_fonts"
            )
        {
            "typography"
        } else if field.starts_with("cursor") || field == "force_cursor_style" {
            "cursor"
        } else if path.contains("selection") || field == "copy_on_select" {
            "selection"
        } else if field.starts_with("padding") {
            "padding"
        } else if field == "smooth_scrolling" {
            "scrolling"
        } else if field == "smooth_resizing" {
            "resizing"
        } else if field == "app_links" {
            "links"
        } else if field == "startup_command" {
            "shell"
        } else if field == "prefer_generated_titles" {
            "titles"
        } else {
            "advanced"
        }
    } else if path.starts_with("notifications.") {
        if path.matches('.').count() > 1
            || matches!(field, "activity_in_tabs" | "include_idle" | "each_process")
        {
            "program-activity"
        } else if field == "notice_position" {
            "in-app-notifications"
        } else {
            "system-notifications"
        }
    } else {
        "general"
    }
}

const NAMES: &[(&str, &str, &str)] = &[
    (
        "interface.toolbar_layout",
        "toolbar-layout",
        "Toolbar layout",
    ),
    (
        "interface.toolbar_behavior",
        "toolbar-behavior",
        "Toolbar behavior",
    ),
    ("interface.animation", "animation-style", "Animation style"),
    (
        "interface.quick_switcher",
        "quick-switcher",
        "New quick switcher",
    ),
    ("interface.sidebar_width", "sidebar-width", "Sidebar width"),
    (
        "interface.tabs_fill",
        "tabs-fill",
        "Tabs fill available space",
    ),
    (
        "interface.tab_icon_color",
        "tab-icon-color",
        "Use color for tab icons",
    ),
    (
        "interface.shell_icon_theme",
        "shell-icon-theme",
        "Shell icon theme",
    ),
    ("interface.tab_icons", "tab-icons", "Show tab icons"),
    ("interface.sidebar", "sidebar", "Show sidebar"),
    (
        "interface.sidebar_text_size",
        "sidebar-text-size",
        "Sidebar text size",
    ),
    (
        "interface.sidebar_gesture",
        "sidebar-gesture",
        "Sidebar gesture",
    ),
    (
        "interface.trackpad_gestures",
        "trackpad-gestures",
        "Use trackpad gestures",
    ),
    (
        "interface.activity_view",
        "activity-view",
        "Show activity in the sidebar",
    ),
    (
        "interface.activity_below_sessions",
        "activity-below-sessions",
        "Show Activity below Sessions",
    ),
    (
        "interface.activity_by_priority",
        "activity-priority",
        "Sort activity by priority",
    ),
    ("interface.pane_style", "pane-style", "Pane style"),
    (
        "interface.minimized_corner",
        "minimized-corner",
        "Move Minimized Floating Windows",
    ),
    ("interface.pane_headers", "pane-headers", "Pane headers"),
    (
        "interface.active_pane_opacity",
        "active-pane-opacity",
        "Active pane background opacity",
    ),
    (
        "interface.inactive_pane_opacity",
        "inactive-pane-opacity",
        "Inactive pane background opacity",
    ),
    (
        "interface.highlight_active_pane",
        "highlight",
        "Highlight active pane edges",
    ),
    (
        "interface.highlight_color",
        "highlight-color",
        "Highlight color",
    ),
    (
        "interface.highlight_opacity",
        "highlight-opacity",
        "Highlight opacity",
    ),
    (
        "interface.highlight_animation",
        "highlight-animation",
        "Animate active pane highlight",
    ),
    ("interface.fade_inactive", "fade", "Fade inactive panes"),
    (
        "interface.fade_inactive_windows",
        "fade-inactive-windows",
        "Fade panes for inactive windows",
    ),
    (
        "interface.focus_follows_mouse",
        "focus-follows-mouse",
        "Focus follows mouse",
    ),
    (
        "interface.autobalance_splits",
        "autobalance-splits",
        "Autobalance splits",
    ),
    ("terminal.font_family", "font", "Terminal font"),
    ("terminal.anchor", "terminal-anchor", "Terminal anchoring"),
    (
        "terminal.cursor_shaders",
        "cursor-shaders",
        "Enable cursor shaders",
    ),
    ("terminal.cursor_shader", "cursor-shader", "Cursor shader"),
    ("terminal.font_size", "font-size", "Font size"),
    ("terminal.font_weight", "font-weight", "Font weight"),
    ("terminal.line_height", "line-height", "Line height"),
    ("terminal.padding_x", "padding-x", "Padding X"),
    ("terminal.padding_y", "padding-y", "Padding Y"),
    (
        "terminal.padding_fill",
        "padding-fill",
        "Fill with program colors",
    ),
    (
        "terminal.padding_fill_header",
        "padding-fill-header",
        "Include pane header",
    ),
    ("terminal.cursor_style", "cursor-style", "Cursor style"),
    (
        "terminal.force_cursor_style",
        "force-cursor",
        "Force cursor style",
    ),
    ("terminal.cursor_blink", "cursor-blink", "Blink speed"),
    (
        "terminal.cursor_blink_ms",
        "blink-interval",
        "Blink interval",
    ),
    (
        "terminal.copy_on_select",
        "copy-on-select",
        "Copy on select",
    ),
    (
        "terminal.selection.foreground",
        "selection-foreground",
        "Selection text color",
    ),
    (
        "terminal.selection.background",
        "selection-background",
        "Selection background color",
    ),
    (
        "terminal.selection.rounded",
        "selection-rounded",
        "Rounded selection corners",
    ),
    (
        "terminal.selection.copy_animation",
        "copy-animation",
        "Play animation on copy",
    ),
    (
        "terminal.smooth_scrolling",
        "smooth-scrolling",
        "Smooth scrolling",
    ),
    (
        "terminal.smooth_resizing",
        "smooth-resizing",
        "Smooth terminal resizing",
    ),
    ("terminal.key_hints", "key-hints", "Show key sequence hints"),
    (
        "terminal.recent_paths",
        "recent-paths",
        "Show recently used paths",
    ),
    ("terminal.app_links", "app-links", "Links that open apps"),
    (
        "terminal.prefer_generated_titles",
        "prefer-generated-titles",
        "Prefer generated titles",
    ),
    (
        "terminal.startup_command",
        "startup-command",
        "Startup command",
    ),
    (
        "terminal.character_spacing",
        "character-spacing",
        "Character spacing",
    ),
    ("terminal.ligatures", "ligatures", "Ligatures"),
    (
        "terminal.text_styles",
        "text-styles",
        "Render bold and italic",
    ),
    (
        "terminal.improve_contrast",
        "improve-contrast",
        "Improve text contrast",
    ),
    (
        "terminal.theme_palette",
        "theme-palette",
        "Force theme palette for text",
    ),
    (
        "notifications.activity_in_tabs",
        "activity-tabs",
        "Show activity in tabs",
    ),
    (
        "notifications.include_idle",
        "include-idle",
        "Include idle processes in activity",
    ),
    (
        "notifications.each_process",
        "each-process",
        "Show each process",
    ),
    (
        "notifications.system_notifications",
        "system-notifications",
        "Show desktop notifications",
    ),
    (
        "notifications.terminal_notifications",
        "terminal-notifications",
        "Terminal notifications",
    ),
    (
        "notifications.tailscale_sign_in",
        "tailscale-sign-in",
        "Tailscale sign-in",
    ),
    (
        "notifications.tailscale_approval",
        "tailscale-approval",
        "Tailscale device approval",
    ),
    (
        "notifications.tailscale_failures",
        "tailscale-failures",
        "Tailscale failures",
    ),
    ("notifications.badge", "badge", "Badge launcher icon"),
    ("notifications.sounds", "sounds", "Play sounds"),
    ("notifications.sound_volume", "volume", "Sound volume"),
    (
        "notifications.notice_position",
        "notice-position",
        "Notification position",
    ),
    ("client_label", "client-label", "Client name"),
    ("ssh_agent", "ssh-agent-socket", "SSH agent socket"),
    (
        "rex_location",
        "rex-source",
        "Rex executable or source directory",
    ),
];

fn words(name: &str) -> String {
    let name = name.replace('_', " ");
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

pub fn entries() -> Vec<Entry> {
    fn walk(value: &Value, path: &str, section: &'static str, entries: &mut Vec<Entry>) {
        if let Some(object) = value.as_object() {
            for (key, value) in object {
                walk(value, &format!("{path}.{key}"), section, entries);
            }
        } else if !value.is_array()
            && !matches!(
                path,
                "interface.vertical_side" | "interface.shell_icon_theme"
            )
        {
            let named = NAMES.iter().find(|(key, _, _)| *key == path);
            let id = named
                .map(|(_, id, _)| (*id).to_owned())
                .unwrap_or_else(|| path.replace(['.', '_'], "-"));
            let title = named
                .map(|(_, _, title)| (*title).to_owned())
                .unwrap_or_else(|| {
                    path.split('.')
                        .skip(1)
                        .map(words)
                        .collect::<Vec<_>>()
                        .join(" · ")
                });
            entries.push(Entry {
                path: path.into(),
                id,
                title,
                section: if matches!(path, "interface.quick_switcher" | "terminal.anchor") {
                    "Developer"
                } else {
                    section
                },
                group: group(path),
                default: value.clone(),
            });
        }
    }
    let settings = serde_json::to_value(Settings::default()).expect("settings serialize");
    let mut result = Vec::new();
    for (key, section) in [
        ("interface", "Interface"),
        ("terminal", "Terminal"),
        ("notifications", "Notifications"),
    ] {
        walk(&settings[key], key, section, &mut result);
    }
    for key in ["client_label", "ssh_agent", "rex_location"] {
        walk(
            &settings[key],
            key,
            if key == "rex_location" {
                "Developer"
            } else {
                "Client & Hosts"
            },
            &mut result,
        );
    }
    result.push(Entry {
        path: "terminal.fallback_fonts".into(),
        id: "fallback-fonts".into(),
        title: "Fallback fonts".into(),
        section: "Terminal",
        group: "typography",
        default: json!([]),
    });
    result
}

pub fn find(path_or_id: &str) -> Option<Entry> {
    entries()
        .into_iter()
        .find(|entry| entry.path == path_or_id || entry.id == path_or_id)
}

pub fn value(settings: &Settings, path: &str) -> Value {
    let data = serde_json::to_value(settings).expect("settings serialize");
    data.pointer(&format!("/{}", path.replace('.', "/")))
        .cloned()
        .unwrap_or(Value::Null)
}

pub fn display(value: &Value) -> String {
    match value {
        Value::Null => "Default".into(),
        Value::Bool(true) => "On".into(),
        Value::Bool(false) => "Off".into(),
        Value::String(text) if text.is_empty() => "Default".into(),
        Value::String(text) => text.clone(),
        Value::Object(object) if object.contains_key("palette") => {
            format!("Palette {}", object["palette"])
        }
        value => value.to_string(),
    }
}

pub fn choice_label(value: &Value) -> String {
    match value {
        Value::String(text) => words(text),
        _ => display(value),
    }
}

pub fn search(query: &str) -> Vec<Entry> {
    let terms: Vec<_> = query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    entries()
        .into_iter()
        .filter(|entry| {
            let haystack = format!(
                "{} {} {} {}",
                entry.title, entry.path, entry.section, entry.group
            )
            .replace(['_', '.', '-'], " ")
            .to_lowercase();
            terms.iter().all(|term| haystack.contains(term))
        })
        .collect()
}

/// Enumerated values use the same serialized spelling as Settings.
pub fn choices(path: &str) -> Vec<Value> {
    let strings: &[&str] = match path {
        "interface.toolbar_layout" => &["above", "left", "right"],
        "interface.toolbar_behavior" | "interface.pane_headers" => &["always", "hover", "hidden"],
        "interface.animation" => &["snappy", "natural", "off"],
        "interface.sidebar_text_size" => &["small", "medium", "large"],
        "interface.sidebar_gesture" => &["swipe", "alt_swipe", "off"],
        "interface.tab_icon_color" => &["always", "active_panes", "active_tabs", "never"],
        "interface.shell_icon_theme" => &["default", "theme", "accent"],
        "interface.pane_style" => &["comfortable", "compact"],
        "interface.minimized_corner" => &["top_left", "top_right", "bottom_left", "bottom_right"],
        "interface.highlight_animation" => &["fade", "move", "instant"],
        "interface.fade_inactive" => &["off", "surface", "content", "both"],
        "terminal.anchor" => &["top", "bottom"],
        "terminal.app_links" => &["ask", "always", "never"],
        "terminal.padding_fill" => &["off", "automatic", "always"],
        "terminal.cursor_style" => &["block", "bar", "underline", "hollow_block"],
        "terminal.cursor_blink" => &["off", "slow", "normal", "fast", "custom"],
        "terminal.cursor_shader" => &[
            "warp", "sweep", "comet", "blaze", "tapered", "rainbow", "gradient", "frozen",
        ],
        "terminal.text_styles" => &["both", "bold_only", "italic_only", "neither"],
        "notifications.notice_position" => &[
            "top_left",
            "top_center",
            "top_right",
            "bottom_left",
            "bottom_center",
            "bottom_right",
        ],
        "interface.highlight_color" => &["accent", "text"],
        "terminal.selection.foreground" => &["theme", "accent", "keep"],
        "terminal.selection.background" => &["theme", "accent"],
        _ => &[],
    };
    let mut result: Vec<Value> = strings.iter().map(|s| json!(s)).collect();
    if matches!(
        path,
        "interface.highlight_color"
            | "terminal.selection.foreground"
            | "terminal.selection.background"
    ) {
        result.extend((0..16).map(|i| json!({"palette": i})));
    }
    result
}

pub fn number_range(path: &str) -> Option<(f64, f64, f64)> {
    Some(match path {
        "interface.sidebar_width" => (180., 480., 10.),
        "interface.tab_icons" => (0., 5., 1.),
        "interface.highlight_opacity" => (0.2, 1., 0.1),
        "interface.active_pane_opacity" | "interface.inactive_pane_opacity" => (0., 1., 0.05),
        "terminal.font_size" => (6., 72., 1.),
        "terminal.font_weight" => (100., 900., 100.),
        "terminal.line_height" => (0.8, 2., 0.05),
        "terminal.padding_x" | "terminal.padding_y" => (0., 40., 1.),
        "terminal.character_spacing" => (-4., 8., 0.5),
        "terminal.cursor_blink_ms" => (100., 5000., 50.),
        "notifications.sound_volume" => (0., 1., 0.1),
        _ => return None,
    })
}

pub fn changed(settings: &Settings, entry: &Entry) -> bool {
    value(settings, &entry.path) != entry.default
}

pub fn set(settings: &Settings, path: &str, value: Value) -> Result<Settings, String> {
    let entry = find(path).ok_or("Unknown setting")?;
    let path = entry.path.as_str();
    if value != entry.default {
        if let Some((low, high, _)) = number_range(path) {
            let number = value.as_f64().ok_or("Enter a number")?;
            if !number.is_finite() || number < low || number > high {
                return Err(format!("Enter a number from {low} to {high}"));
            }
        }
        let options = choices(path);
        if !options.is_empty() && !options.contains(&value) {
            return Err("Choose one of the listed values".into());
        }
    }
    let mut data = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    let target = data
        .pointer_mut(&format!("/{}", path.replace('.', "/")))
        .ok_or("Unknown setting")?;
    *target = value;
    let mut settings: Settings = serde_json::from_value(data).map_err(|e| e.to_string())?;
    if path == "interface.toolbar_layout" && settings.interface.toolbar_layout.vertical() {
        settings.interface.vertical_side = settings.interface.toolbar_layout;
        settings.interface.sidebar = true;
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_opacity_validates_persists_and_resets() {
        let defaults = Settings::default();
        assert!(!defaults.interface.translucent_panes());
        for path in [
            "interface.active_pane_opacity",
            "interface.inactive_pane_opacity",
        ] {
            for bad in [-0.1, 1.1] {
                assert!(set(&defaults, path, json!(bad)).is_err());
            }
            for value in [0., 0.45, 1.] {
                let changed = set(&defaults, path, json!(value)).unwrap();
                let saved = toml::to_string(&changed).unwrap();
                let loaded: Settings = toml::from_str(&saved).unwrap();
                assert_eq!(super::value(&loaded, path), json!(value as f32));
                let reset = set(&loaded, path, find(path).unwrap().default).unwrap();
                assert_eq!(reset.interface, defaults.interface);
            }
        }
        assert_eq!(
            search("active pane background opacity")[0].path,
            "interface.active_pane_opacity"
        );
        let mut settings = defaults.interface;
        settings.active_pane_opacity = 0.75;
        settings.inactive_pane_opacity = 0.25;
        assert_eq!(settings.pane_opacity(true, true), 0.75);
        assert_eq!(settings.pane_opacity(false, true), 0.25);
        assert_eq!(settings.pane_opacity(true, false), 0.25);
        assert_eq!(settings.pane_opacity(false, false), 0.25);
        settings.active_pane_opacity = f32::NAN;
        assert_eq!(settings.pane_opacity(true, true), 1.);
    }

    #[test]
    fn index_covers_simple_preferences_and_round_trips_values() {
        let defaults = Settings::default();
        assert!(entries().len() > 70);
        println!("indexed settings: {}", entries().len());
        for entry in entries() {
            assert_eq!(value(&defaults, &entry.path), entry.default);
            for option in choices(&entry.path) {
                assert!(
                    set(&defaults, &entry.path, option).is_ok(),
                    "{}",
                    entry.path
                );
            }
        }
        let changed = set(&defaults, "terminal.font_size", json!(19.)).unwrap();
        assert!(changed.terminal.font_size.is_some());
        assert_eq!(
            set(&changed, "terminal.font_size", Value::Null)
                .unwrap()
                .terminal
                .font_size,
            None
        );
        assert!(set(&defaults, "terminal.font_size", json!(-5)).is_err());
        assert_eq!(
            set(&defaults, "font-size", json!(19))
                .unwrap()
                .terminal
                .font_size,
            Some(19.)
        );
        for entry in entries() {
            let updated = if entry.default.is_boolean() {
                json!(!entry.default.as_bool().unwrap())
            } else if let Some((low, _, _)) = number_range(&entry.path) {
                if matches!(
                    entry.path.as_str(),
                    "terminal.font_weight" | "terminal.cursor_blink_ms" | "interface.tab_icons"
                ) {
                    json!(low as u32)
                } else {
                    json!(low)
                }
            } else {
                continue;
            };
            let updated = set(&defaults, &entry.path, updated).unwrap();
            let restored = set(&updated, &entry.path, entry.default.clone()).unwrap();
            assert_eq!(value(&restored, &entry.path), entry.default);
        }
        assert_eq!(search("font size")[0].path, "terminal.font_size");
    }
}
