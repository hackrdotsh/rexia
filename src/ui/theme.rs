//! The interface's design tokens, derived from the Noctalia theme. Every view reads colors,
//! corner radii, borders and fonts from here, so a Noctalia change restyles the whole app.
use crate::noctalia::{NoctaliaTheme, TerminalColors};
use gpui::{App, Global, Hsla, Pixels, Rgba, SharedString, px};

pub fn color(value: u32) -> Hsla {
    Rgba {
        r: ((value >> 16) & 0xff) as f32 / 255.,
        g: ((value >> 8) & 0xff) as f32 / 255.,
        b: (value & 0xff) as f32 / 255.,
        a: 1.,
    }
    .into()
}

/// Noctalia's base corner radii before its `corner_radius_scale`.
const RADIUS_SMALL: f32 = 4.;
const RADIUS_MEDIUM: f32 = 8.;
const RADIUS_LARGE: f32 = 12.;

#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    /// The window background (Noctalia's surface).
    pub background: Hsla,
    pub surface_low: Hsla,
    /// Cards and panels.
    pub surface: Hsla,
    pub surface_high: Hsla,
    pub surface_highest: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub primary: Hsla,
    pub on_primary: Hsla,
    pub primary_container: Hsla,
    pub on_primary_container: Hsla,
    pub secondary: Hsla,
    pub tertiary: Hsla,
    pub error: Hsla,
    pub on_error: Hsla,
    pub outline: Hsla,
    pub outline_variant: Hsla,
    /// The border Noctalia draws around panels.
    pub border: Hsla,
    pub hover: Hsla,
    pub on_hover: Hsla,
    pub shadow: Hsla,
    pub radius_scale: f32,
    pub border_width: Pixels,
    pub button_borders: bool,
    pub card_borders: bool,
    pub input_borders: bool,
    pub popup_borders: bool,
    pub popup_shadows: bool,
    pub ui_font: SharedString,
    pub ui_scale: f32,
    pub animation: bool,
    pub animation_speed: f32,
    pub terminal: TerminalColors,
}

impl Global for Theme {}

impl Theme {
    pub fn from_noctalia(theme: &NoctaliaTheme) -> Self {
        let p = &theme.palette;
        let s = &theme.shell;
        let mut shadow = color(p.shadow);
        shadow.a = s.shadow_alpha;
        Self {
            dark: p.dark,
            background: color(p.surface),
            surface_low: color(p.surface_container_low),
            surface: color(p.surface_container),
            surface_high: color(p.surface_container_high),
            surface_highest: color(p.surface_container_highest),
            text: color(p.on_surface),
            text_muted: color(p.on_surface_variant),
            primary: color(p.primary),
            on_primary: color(p.on_primary),
            primary_container: color(p.primary_container),
            on_primary_container: color(p.on_primary_container),
            secondary: color(p.secondary),
            tertiary: color(p.tertiary),
            error: color(p.error),
            on_error: color(p.on_error),
            outline: color(p.outline),
            outline_variant: color(p.outline_variant),
            border: color(p.role(&s.panel_border_role).unwrap_or(p.outline_variant)),
            hover: color(p.hover),
            on_hover: color(p.on_hover),
            shadow,
            radius_scale: s.corner_radius_scale,
            border_width: px(if s.panel_borders {
                s.panel_border_width.max(1.)
            } else {
                1.
            }),
            button_borders: s.button_borders,
            card_borders: s.card_borders,
            input_borders: s.input_borders,
            popup_borders: s.popup_borders,
            popup_shadows: s.popup_shadows,
            ui_font: s.font_family.clone().into(),
            ui_scale: s.ui_scale,
            animation: s.animation,
            animation_speed: s.animation_speed,
            terminal: p.terminal_colors(),
        }
    }

    pub fn get(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub fn radius_small(&self) -> Pixels {
        px(RADIUS_SMALL * self.radius_scale)
    }

    pub fn radius(&self) -> Pixels {
        px(RADIUS_MEDIUM * self.radius_scale)
    }

    pub fn radius_large(&self) -> Pixels {
        px(RADIUS_LARGE * self.radius_scale)
    }

    /// A hover or pressed tint over a surface, readable on both dark and light themes.
    pub fn overlay(&self, strength: f32) -> Hsla {
        let mut tint = self.text;
        tint.a = strength;
        tint
    }

    /// The terminal colors as a theme for the terminal engine.
    pub fn terminal_theme(&self) -> crate::rex::stream::TerminalTheme {
        let t = &self.terminal;
        crate::rex::stream::TerminalTheme {
            foreground: t.foreground,
            background: t.background,
            cursor: Some(t.cursor),
            palette: t
                .ansi
                .iter()
                .enumerate()
                .map(|(i, c)| (i as u8, *c))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::noctalia::{Palette, ShellStyle, Source};

    #[test]
    fn square_corners_and_panel_border_follow_noctalia() {
        let shell = ShellStyle {
            corner_radius_scale: 0.,
            panel_border_role: "outline".into(),
            font_family: "DM Sans".into(),
            ..Default::default()
        };
        let palette = Palette::default();
        let theme = Theme::from_noctalia(&NoctaliaTheme {
            palette: palette.clone(),
            shell,
            source: Source::Default,
            shell_live: true,
        });
        assert_eq!(theme.radius(), px(0.));
        assert_eq!(theme.border, color(palette.outline));
        assert_eq!(theme.ui_font.as_ref(), "DM Sans");
        let rounded = Theme::from_noctalia(&NoctaliaTheme {
            shell: ShellStyle {
                corner_radius_scale: 1.5,
                ..Default::default()
            },
            ..Default::default()
        });
        assert_eq!(rounded.radius(), px(12.));
    }

    #[test]
    fn hex_colors_convert_exactly() {
        let green: Rgba = color(0xb2d26b).into();
        assert!((green.r - 0xb2 as f32 / 255.).abs() < 0.002);
        assert!((green.g - 0xd2 as f32 / 255.).abs() < 0.002);
        assert!((green.b - 0x6b as f32 / 255.).abs() < 0.002);
    }
}
