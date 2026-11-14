//! Noctalia's shell style: font, corner rounding, borders, shadows and animation.
use anyhow::{Context, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct ShellStyle {
    pub font_family: String,
    /// Multiplies Noctalia's base corner radii; 0 draws square corners.
    pub corner_radius_scale: f32,
    pub button_borders: bool,
    pub card_borders: bool,
    pub input_borders: bool,
    pub popup_borders: bool,
    pub popup_shadows: bool,
    pub panel_borders: bool,
    pub panel_border_width: f32,
    /// The palette role panel borders use, such as `outline_variant`.
    pub panel_border_role: String,
    pub shadow_alpha: f32,
    pub animation: bool,
    pub animation_speed: f32,
    pub ui_scale: f32,
    pub high_contrast: bool,
}

impl Default for ShellStyle {
    /// Noctalia's defaults.
    fn default() -> Self {
        Self {
            font_family: "Inter".into(),
            corner_radius_scale: 1.0,
            button_borders: true,
            card_borders: true,
            input_borders: true,
            popup_borders: true,
            popup_shadows: true,
            panel_borders: true,
            panel_border_width: 1.0,
            panel_border_role: "outline_variant".into(),
            shadow_alpha: 0.55,
            animation: true,
            animation_speed: 1.0,
            ui_scale: 1.0,
            high_contrast: false,
        }
    }
}

impl ShellStyle {
    /// Read `noctalia msg config-get shell` and `… accessibility` output. Missing keys keep
    /// their defaults, so older and newer Noctalia versions both work.
    pub fn parse(shell: &str, accessibility: &str) -> Result<Self> {
        let shell: toml::Table = toml::from_str(shell).context("unreadable shell config")?;
        let access: toml::Table = toml::from_str(accessibility).unwrap_or_default();
        let defaults = Self::default();
        let table = |name: &str| shell.get(name).and_then(|v| v.as_table());
        let boolean = |t: Option<&toml::Table>, key: &str, default: bool| {
            t.and_then(|t| t.get(key))
                .and_then(|v| v.as_bool())
                .unwrap_or(default)
        };
        let number = |t: Option<&toml::Table>, key: &str, default: f32| {
            t.and_then(|t| t.get(key))
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                .map_or(default, |v| v as f32)
        };
        let root = Some(&shell);
        let panel = table("panel");
        Ok(Self {
            font_family: shell
                .get("font_family")
                .and_then(|v| v.as_str())
                .filter(|f| !f.trim().is_empty())
                .map_or(defaults.font_family.clone(), str::to_owned),
            corner_radius_scale: number(root, "corner_radius_scale", 1.0).clamp(0.0, 4.0),
            button_borders: boolean(root, "button_borders", true),
            card_borders: boolean(root, "card_borders", true),
            input_borders: boolean(root, "input_borders", true),
            popup_borders: boolean(root, "popup_borders", true),
            popup_shadows: boolean(root, "popup_shadows", true),
            panel_borders: boolean(panel, "borders", true),
            panel_border_width: number(panel, "border_width", 1.0).clamp(0.0, 8.0),
            panel_border_role: panel
                .and_then(|p| p.get("border_color"))
                .and_then(|v| v.as_str())
                .map_or(defaults.panel_border_role, str::to_owned),
            shadow_alpha: number(table("shadow"), "alpha", 0.55).clamp(0.0, 1.0),
            animation: boolean(table("animation"), "enabled", true),
            animation_speed: number(table("animation"), "speed", 1.0).clamp(0.1, 4.0),
            ui_scale: number(Some(&access), "ui_scale", 1.0).clamp(0.5, 3.0),
            high_contrast: boolean(Some(&access), "high_contrast", false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_live_shape_of_noctalia_config() {
        let shell = r#"
button_borders = true
card_borders = false
corner_radius_scale = 0.0
font_family = 'DM Sans'
input_borders = true
popup_borders = true
popup_shadows = false

[animation]
enabled = true
speed = 1.5

[panel]
border_color = 'outline'
border_width = 2.0
borders = true

[shadow]
alpha = 0.55000001192092896
direction = 'down_right'
"#;
        let style = ShellStyle::parse(shell, "high_contrast = false\nui_scale = 1.25\n").unwrap();
        assert_eq!(style.font_family, "DM Sans");
        assert_eq!(style.corner_radius_scale, 0.0);
        assert!(!style.card_borders);
        assert!(!style.popup_shadows);
        assert_eq!(style.panel_border_role, "outline");
        assert_eq!(style.panel_border_width, 2.0);
        assert_eq!(style.animation_speed, 1.5);
        assert_eq!(style.ui_scale, 1.25);
    }

    #[test]
    fn missing_keys_keep_defaults() {
        let style = ShellStyle::parse("", "").unwrap();
        assert_eq!(style, ShellStyle::default());
        let integer = ShellStyle::parse("corner_radius_scale = 2", "").unwrap();
        assert_eq!(integer.corner_radius_scale, 2.0);
    }
}
