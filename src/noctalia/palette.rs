//! Noctalia's color palette: Material 3 roles plus a terminal palette.
use anyhow::{Context, Result, bail};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalColors {
    pub foreground: u32,
    pub background: u32,
    pub cursor: u32,
    pub cursor_text: u32,
    pub selection_background: u32,
    pub selection_foreground: u32,
    /// Normal colors 0–7, then bright colors 8–15.
    pub ansi: [u32; 16],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub primary: u32,
    pub on_primary: u32,
    pub primary_container: u32,
    pub on_primary_container: u32,
    pub secondary: u32,
    pub on_secondary: u32,
    pub tertiary: u32,
    pub on_tertiary: u32,
    pub error: u32,
    pub on_error: u32,
    pub surface: u32,
    pub on_surface: u32,
    pub surface_variant: u32,
    pub on_surface_variant: u32,
    pub surface_container_lowest: u32,
    pub surface_container_low: u32,
    pub surface_container: u32,
    pub surface_container_high: u32,
    pub surface_container_highest: u32,
    pub outline: u32,
    pub outline_variant: u32,
    pub shadow: u32,
    pub hover: u32,
    pub on_hover: u32,
    /// Present when the source carries a terminal palette.
    pub terminal: Option<TerminalColors>,
}

impl Default for Palette {
    /// Noctalia's own dark palette, for when no theme can be read.
    fn default() -> Self {
        Self::from_legacy(&HashMap::from([
            ("mPrimary".into(), 0x009fff),
            ("mOnPrimary".into(), 0x0a0a0a),
            ("mSecondary".into(), 0xe130ac),
            ("mOnSecondary".into(), 0x0a0a0a),
            ("mTertiary".into(), 0x5ecc71),
            ("mOnTertiary".into(), 0x0a0a0a),
            ("mError".into(), 0xff2e3f),
            ("mOnError".into(), 0x0a0a0a),
            ("mSurface".into(), 0x171717),
            ("mOnSurface".into(), 0xfafafa),
            ("mSurfaceVariant".into(), 0x1d1d1d),
            ("mOnSurfaceVariant".into(), 0xa3a3a3),
            ("mOutline".into(), 0x404040),
            ("mShadow".into(), 0x000000),
            ("mHover".into(), 0x5ecc71),
            ("mOnHover".into(), 0x0a0a0a),
        ]))
    }
}

pub fn parse_hex(value: &str) -> Option<u32> {
    let hex = value.trim().trim_start_matches('#');
    match hex.len() {
        6 => u32::from_str_radix(hex, 16).ok(),
        // #rrggbbaa: Noctalia's palettes are opaque, so alpha is dropped.
        8 => u32::from_str_radix(&hex[..6], 16).ok(),
        _ => None,
    }
}

/// Mix two colors: `amount` 0 is `a`, 1 is `b`.
pub fn mix(a: u32, b: u32, amount: f32) -> u32 {
    let channel = |shift: u32| {
        let from = ((a >> shift) & 0xff) as f32;
        let to = ((b >> shift) & 0xff) as f32;
        ((from + (to - from) * amount).round() as u32).min(255) << shift
    };
    channel(16) | channel(8) | channel(0)
}

impl Palette {
    /// A template rendering: `mode = "dark"` and a `[colors]` table of hex values, the
    /// format of rexia's Noctalia template (and of other templates written the same way).
    pub fn from_template(text: &str) -> Result<Self> {
        let value: toml::Table = toml::from_str(text).context("not a TOML palette")?;
        let colors = value
            .get("colors")
            .and_then(|c| c.as_table())
            .context("the palette has no [colors] table")?;
        let map: HashMap<String, u32> = colors
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), parse_hex(v.as_str()?)?)))
            .collect();
        for required in ["primary", "on_primary", "surface", "on_surface"] {
            if !map.contains_key(required) {
                bail!("the palette has no {required}");
            }
        }
        let dark = value
            .get("mode")
            .and_then(|m| m.as_str())
            .is_none_or(|m| m != "light");
        Ok(Self::from_roles(&map, dark))
    }

    /// Noctalia v4's `colors.json` (`mPrimary`, `mSurface`, …).
    pub fn from_legacy_json(text: &str) -> Result<Self> {
        let value: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(text).context("not a JSON palette")?;
        let map: HashMap<String, u32> = value
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), parse_hex(v.as_str()?)?)))
            .collect();
        if !map.contains_key("mPrimary") || !map.contains_key("mSurface") {
            bail!("colors.json has no mPrimary or mSurface");
        }
        Ok(Self::from_legacy(&map))
    }

    fn from_legacy(map: &HashMap<String, u32>) -> Self {
        let roles = HashMap::from(
            [
                ("primary", "mPrimary"),
                ("on_primary", "mOnPrimary"),
                ("secondary", "mSecondary"),
                ("on_secondary", "mOnSecondary"),
                ("tertiary", "mTertiary"),
                ("on_tertiary", "mOnTertiary"),
                ("error", "mError"),
                ("on_error", "mOnError"),
                ("surface", "mSurface"),
                ("on_surface", "mOnSurface"),
                ("surface_variant", "mSurfaceVariant"),
                ("surface_container", "mSurfaceVariant"),
                ("on_surface_variant", "mOnSurfaceVariant"),
                ("outline", "mOutline"),
                ("shadow", "mShadow"),
                ("hover", "mHover"),
                ("on_hover", "mOnHover"),
            ]
            .map(|(role, key)| (role.to_owned(), map.get(key).copied().unwrap_or(0))),
        );
        let on_surface = roles["on_surface"];
        let surface = roles["surface"];
        let dark = luminance(surface) < luminance(on_surface);
        Self::from_roles(&roles, dark)
    }

    /// Fill every role, deriving the ones a source leaves out from those it has.
    fn from_roles(map: &HashMap<String, u32>, dark: bool) -> Self {
        let get = |key: &str| map.get(key).copied();
        let surface = get("surface").unwrap_or(if dark { 0x121212 } else { 0xfafafa });
        let on_surface = get("on_surface").unwrap_or(if dark { 0xe2e2e2 } else { 0x1a1a1a });
        let toward = |amount: f32| mix(surface, on_surface, amount);
        let primary = get("primary").unwrap_or(0x009fff);
        let surface_container = get("surface_container").unwrap_or_else(|| toward(0.06));
        let outline = get("outline").unwrap_or_else(|| toward(0.45));
        let terminal = (|| {
            let color = |name: &str| get(&format!("terminal_{name}"));
            let mut ansi = [0; 16];
            let names = [
                "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
            ];
            for (i, name) in names.iter().enumerate() {
                ansi[i] = color(&format!("normal_{name}"))?;
                ansi[i + 8] = color(&format!("bright_{name}"))?;
            }
            Some(TerminalColors {
                foreground: color("foreground")?,
                background: color("background")?,
                cursor: color("cursor").unwrap_or(on_surface),
                cursor_text: color("cursor_text").unwrap_or(surface),
                selection_background: color("selection_bg").unwrap_or_else(|| toward(0.25)),
                selection_foreground: color("selection_fg").unwrap_or(on_surface),
                ansi,
            })
        })();
        Self {
            dark,
            primary,
            on_primary: get("on_primary").unwrap_or(surface),
            primary_container: get("primary_container")
                .unwrap_or_else(|| mix(surface, primary, 0.3)),
            on_primary_container: get("on_primary_container").unwrap_or(on_surface),
            secondary: get("secondary").unwrap_or(primary),
            on_secondary: get("on_secondary").unwrap_or(surface),
            tertiary: get("tertiary").unwrap_or(primary),
            on_tertiary: get("on_tertiary").unwrap_or(surface),
            error: get("error").unwrap_or(0xff5449),
            on_error: get("on_error").unwrap_or(surface),
            surface,
            on_surface,
            surface_variant: get("surface_variant").unwrap_or_else(|| toward(0.12)),
            on_surface_variant: get("on_surface_variant").unwrap_or_else(|| toward(0.7)),
            surface_container_lowest: get("surface_container_lowest").unwrap_or(surface),
            surface_container_low: get("surface_container_low").unwrap_or_else(|| toward(0.03)),
            surface_container,
            surface_container_high: get("surface_container_high").unwrap_or_else(|| toward(0.09)),
            surface_container_highest: get("surface_container_highest")
                .unwrap_or_else(|| toward(0.13)),
            outline,
            outline_variant: get("outline_variant").unwrap_or_else(|| mix(surface, outline, 0.6)),
            shadow: get("shadow").unwrap_or(0),
            hover: get("hover").unwrap_or_else(|| get("tertiary").unwrap_or(primary)),
            on_hover: get("on_hover").unwrap_or(surface),
            terminal,
        }
    }

    /// A Material role by its template name, such as `outline_variant`.
    pub fn role(&self, name: &str) -> Option<u32> {
        Some(match name {
            "primary" => self.primary,
            "on_primary" => self.on_primary,
            "primary_container" => self.primary_container,
            "secondary" => self.secondary,
            "tertiary" => self.tertiary,
            "error" => self.error,
            "surface" => self.surface,
            "on_surface" => self.on_surface,
            "surface_variant" => self.surface_variant,
            "on_surface_variant" => self.on_surface_variant,
            "surface_container" => self.surface_container,
            "surface_container_high" => self.surface_container_high,
            "surface_container_highest" => self.surface_container_highest,
            "outline" => self.outline,
            "outline_variant" => self.outline_variant,
            "hover" => self.hover,
            _ => return None,
        })
    }

    /// The terminal palette, derived from the Material roles when the source has none.
    pub fn terminal_colors(&self) -> TerminalColors {
        if let Some(terminal) = &self.terminal {
            return terminal.clone();
        }
        let normal = [
            self.surface_container_highest,
            self.error,
            self.tertiary,
            self.secondary,
            self.primary,
            self.secondary,
            self.tertiary,
            self.on_surface_variant,
        ];
        let mut ansi = [0; 16];
        for (i, color) in normal.iter().enumerate() {
            ansi[i] = *color;
            ansi[i + 8] = mix(*color, self.on_surface, 0.25);
        }
        ansi[8] = self.outline;
        ansi[15] = self.on_surface;
        TerminalColors {
            foreground: self.on_surface,
            background: self.surface,
            cursor: self.on_surface,
            cursor_text: self.surface,
            selection_background: self.outline_variant,
            selection_foreground: self.on_surface,
            ansi,
        }
    }
}

fn luminance(color: u32) -> f32 {
    let r = ((color >> 16) & 0xff) as f32;
    let g = ((color >> 8) & 0xff) as f32;
    let b = (color & 0xff) as f32;
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = r##"# generated by Noctalia
mode = "dark"

[colors]
primary = "#b2d26b"
on_primary = "#263500"
surface = "#000000"
on_surface = "#e2e2e2"
outline_variant = "#474747"
surface_container = "#131313"
terminal_background = "#000000"
terminal_foreground = "#e2e2e2"
terminal_cursor = "#e2e2e2"
terminal_normal_black = "#474747"
terminal_normal_red = "#ffb4ab"
terminal_normal_green = "#b2d26b"
terminal_normal_yellow = "#c2caaa"
terminal_normal_blue = "#a0d0c8"
terminal_normal_magenta = "#b2d26b"
terminal_normal_cyan = "#c2caaa"
terminal_normal_white = "#e2e2e2"
terminal_bright_black = "#919191"
terminal_bright_red = "#ffb4ab"
terminal_bright_green = "#b2d26b"
terminal_bright_yellow = "#c2caaa"
terminal_bright_blue = "#a0d0c8"
terminal_bright_magenta = "#b2d26b"
terminal_bright_cyan = "#c2caaa"
terminal_bright_white = "#e2e2e2"
"##;

    #[test]
    fn template_rendering_reads_roles_and_terminal_colors() {
        let palette = Palette::from_template(TEMPLATE).unwrap();
        assert!(palette.dark);
        assert_eq!(palette.primary, 0xb2d26b);
        assert_eq!(palette.surface_container, 0x131313);
        assert_eq!(palette.role("outline_variant"), Some(0x474747));
        let terminal = palette.terminal.as_ref().unwrap();
        assert_eq!(terminal.ansi[1], 0xffb4ab);
        assert_eq!(terminal.ansi[8], 0x919191);
        // Roles the template left out are derived, not black.
        assert_ne!(palette.surface_container_high, 0);
        assert_ne!(palette.outline, 0);
    }

    #[test]
    fn partial_terminal_palettes_fall_back_to_roles() {
        let text = "mode = \"light\"\n[colors]\nprimary = \"#336699\"\non_primary = \"#ffffff\"\nsurface = \"#fafafa\"\non_surface = \"#111111\"\nterminal_background = \"#ffffff\"\n";
        let palette = Palette::from_template(text).unwrap();
        assert!(!palette.dark);
        assert!(palette.terminal.is_none());
        let derived = palette.terminal_colors();
        assert_eq!(
            (derived.background, derived.foreground),
            (0xfafafa, 0x111111)
        );
        assert_eq!(derived.ansi[4], 0x336699);
    }

    #[test]
    fn legacy_colors_json() {
        let palette = Palette::from_legacy_json(
            r##"{"mPrimary": "#009fff", "mOnPrimary": "#0a0a0a", "mSurface": "#171717",
                "mOnSurface": "#fafafa", "mSurfaceVariant": "#1d1d1d", "mOutline": "#404040"}"##,
        )
        .unwrap();
        assert!(palette.dark);
        assert_eq!(palette.primary, 0x009fff);
        assert_eq!(palette.surface_container, 0x1d1d1d);
        assert!(Palette::from_legacy_json("{}").is_err());
    }

    #[test]
    fn rejects_unrelated_toml() {
        assert!(Palette::from_template("[colors]\nfoo = \"#000000\"").is_err());
        assert!(Palette::from_template("not toml at all [").is_err());
    }
}
