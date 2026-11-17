//! The stack of pane icons on each tab: which panes show, and how each is colored.
use super::Workspace;
use crate::{
    rex::{host::HostModel, view::Window},
    settings::{InterfaceSettings, ShellIconTheme, TabIconColor},
    ui::{
        icons::icon,
        theme::{Theme, color},
        titles::Program,
    },
};
use gpui::{AnyElement, Hsla, IntoElement, ParentElement, Styled, div, px};

/// Vertical tabs show up to three icons; horizontal tabs up to five.
pub const VERTICAL_MAX: u8 = 3;
pub const HORIZONTAL_MAX: u8 = 5;

/// One icon on a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabIcon {
    pub program: Program,
    /// Whether it draws in its program's colors.
    pub colored: bool,
    /// Current terminal foreground/background, including OSC color changes.
    pub terminal_colors: Option<(u32, u32)>,
}

/// How an icon draws: its glyph's color, and the tile behind it when colored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconLook {
    pub glyph: Hsla,
    pub tile: Option<Hsla>,
}

/// The icons a tab shows: its focused pane first, then the others in order, up to `limit`.
/// Whether each draws in color follows `mode`.
pub fn tab_icons(
    window: &Window,
    model: &HostModel,
    selected: bool,
    mode: TabIconColor,
    limit: u8,
) -> Vec<TabIcon> {
    let focused = window.focused_block_id.as_deref();
    let mut blocks: Vec<&str> = window
        .blocks()
        .filter_map(|b| b.block_id.as_deref())
        .collect();
    if let Some(index) = blocks.iter().position(|b| Some(*b) == focused) {
        let first = blocks.remove(index);
        blocks.insert(0, first);
    }
    blocks
        .into_iter()
        .take(usize::from(limit))
        .map(|block| {
            let pane_focused = Some(block) == focused;
            TabIcon {
                program: Program::of(model.blocks.get(block)),
                colored: match mode {
                    TabIconColor::Always => true,
                    TabIconColor::ActivePanes => pane_focused,
                    TabIconColor::ActiveTabs => selected,
                    TabIconColor::Never => false,
                },
                terminal_colors: crate::ui::program_icons::pane_colors(model, block),
            }
        })
        .collect()
}

/// How `item` draws on a tab whose text is `text`. Colored icons sit on a tile of their
/// program's color, so they read on any tab background; plain ones take the tab's text color.
pub fn icon_look(item: TabIcon, _shell: ShellIconTheme, text: Hsla, theme: &Theme) -> IconLook {
    if !item.colored {
        return IconLook {
            glyph: text,
            tile: None,
        };
    }
    let (fg, bg) =
        crate::ui::program_icons::colors(item.program.icon(), theme, item.terminal_colors)
            .unwrap_or_else(|| {
                let bg = theme.terminal.ansi[item.program.ansi()];
                (crate::ui::program_icons::tile::foreground(bg), bg)
            });
    IconLook {
        glyph: color(fg),
        tile: Some(color(bg)),
    }
}

/// The icon stack itself: tiles overlap a little, front to back from the focused pane.
pub fn render_icons(
    items: &[TabIcon],
    shell: ShellIconTheme,
    text: Hsla,
    size: f32,
    theme: &Theme,
) -> Option<AnyElement> {
    if items.is_empty() {
        return None;
    }
    let tile = size + 4.;
    let icons = items.iter().enumerate().map(|(index, item)| {
        let look = icon_look(*item, shell, text, theme);
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(tile))
            .rounded(px(tile * 0.24))
            .when_some(look.tile, |d, bg| d.bg(bg))
            .when(index > 0, |d| {
                d.ml(px(if look.tile.is_some() { -3. } else { 1. }))
            })
            .child(
                icon(item.program.icon())
                    .size(px(if item.colored { tile * 0.72 } else { size }))
                    .text_color(look.glyph),
            )
    });
    Some(
        div()
            .flex()
            .flex_none()
            .items_center()
            .children(icons)
            .into_any_element(),
    )
}

impl Workspace {
    /// The icons tab `window_id` of the current session shows, horizontal or vertical.
    pub fn tab_icon_list(
        &self,
        window_id: &str,
        vertical: bool,
        model: &HostModel,
    ) -> Vec<TabIcon> {
        let Some(view) = model.view() else {
            return Vec::new();
        };
        let Some(window) = view.windows.iter().find(|w| w.id() == window_id) else {
            return Vec::new();
        };
        let selected = view.active_window_id.as_deref() == Some(window_id);
        let interface = &self.settings.interface;
        tab_icons(
            window,
            model,
            selected,
            interface.tab_icon_color,
            icon_limit(interface, vertical),
        )
    }
}

/// How many icons a tab shows in the given orientation.
pub fn icon_limit(interface: &InterfaceSettings, vertical: bool) -> u8 {
    interface.tab_icons.min(if vertical {
        VERTICAL_MAX
    } else {
        HORIZONTAL_MAX
    })
}

use gpui::prelude::FluentBuilder;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colored_icons_take_program_and_shell_colors() {
        let theme = Theme::from_noctalia(&Default::default());
        let text = theme.text;
        let shell = |colored| TabIcon {
            program: Program::Shell,
            colored,
            terminal_colors: Some((0xaabbcc, 0x112233)),
        };
        assert_eq!(
            icon_look(shell(false), ShellIconTheme::Accent, text, &theme),
            IconLook {
                glyph: text,
                tile: None
            }
        );
        let accent = icon_look(shell(true), ShellIconTheme::Accent, text, &theme);
        assert_eq!(accent.tile, Some(color(0x112233)));
        assert_eq!(accent.glyph, color(0xaabbcc));
        let themed = icon_look(shell(true), ShellIconTheme::Theme, text, &theme);
        assert_eq!(themed.tile, Some(color(0x112233)));
        assert_eq!(themed.glyph, color(0xaabbcc));
        let artwork = icon_look(shell(true), ShellIconTheme::Default, text, &theme);
        assert_eq!(artwork.tile, Some(color(0x112233)));
        let git = TabIcon {
            program: Program::Git,
            colored: true,
            terminal_colors: None,
        };
        let git = icon_look(git, ShellIconTheme::Accent, text, &theme);
        assert_eq!(git.tile, Some(color(0xc03d2c)));
    }

    #[test]
    fn limits_follow_orientation() {
        let mut interface = InterfaceSettings {
            tab_icons: 5,
            ..Default::default()
        };
        assert_eq!(icon_limit(&interface, true), 3);
        assert_eq!(icon_limit(&interface, false), 5);
        interface.tab_icons = 0;
        assert_eq!(icon_limit(&interface, false), 0);
    }
}
