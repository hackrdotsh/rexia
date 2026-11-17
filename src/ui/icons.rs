//! Bundled Tabler interface icons and sourced program artwork.
use gpui::{AssetSource, SharedString, Svg, prelude::*, svg};
use std::borrow::Cow;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../../assets/icons/", $name, ".svg")))),*
        ];
    };
}

icons![
    "alert-triangle",
    "arrow-back-up",
    "arrows-maximize",
    "arrows-minimize",
    "bell",
    "bell-off",
    "brand-git",
    "chevron-down",
    "chevron-right",
    "chevron-up",
    "circle-check",
    "clipboard",
    "command",
    "copy",
    "device-desktop",
    "dots",
    "eye",
    "file-code",
    "folder",
    "help-circle",
    "key",
    "keyboard",
    "layout-columns",
    "layout-grid",
    "layout-rows",
    "layout-sidebar",
    "layout-sidebar-left-collapse",
    "loader-2",
    "lock",
    "minus",
    "palette",
    "pencil",
    "picture-in-picture",
    "picture-in-picture-off",
    "player-play",
    "plug-connected",
    "plug-connected-x",
    "plus",
    "power",
    "refresh",
    "search",
    "server",
    "settings",
    "terminal-2",
    "trash",
    "volume-off",
    "world",
    "x",
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let name = path
            .strip_prefix("icons/")
            .and_then(|p| p.strip_suffix(".svg"));
        Ok(name.and_then(|name| {
            ICONS
                .iter()
                .find(|(icon, _)| *icon == name)
                .map(|(_, bytes)| Cow::Borrowed(*bytes))
                .or_else(|| {
                    crate::ui::program_icons::PROGRAMS
                        .iter()
                        .find(|program| program.icon == name)
                        .map(|program| Cow::Borrowed(program.svg.as_bytes()))
                })
        }))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .map(|(name, _)| SharedString::from(format!("icons/{name}.svg")))
            .chain(
                crate::ui::program_icons::PROGRAMS
                    .iter()
                    .map(|program| SharedString::from(format!("icons/{}.svg", program.icon))),
            )
            .filter(|p| p.starts_with(path))
            .collect())
    }
}

/// An interface or program icon, sized like text.
pub fn icon(name: &'static str) -> Svg {
    svg().path(format!("icons/{name}.svg")).flex_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_loads_and_unknown_names_do_not() {
        for (name, _) in ICONS {
            let bytes = Assets.load(&format!("icons/{name}.svg")).unwrap().unwrap();
            assert!(bytes.starts_with(b"<svg"), "{name}");
        }
        assert!(Assets.load("icons/nope.svg").unwrap().is_none());
        for program in crate::ui::program_icons::PROGRAMS {
            let bytes = Assets
                .load(&format!("icons/{}.svg", program.icon))
                .unwrap()
                .unwrap();
            assert_eq!(bytes.as_ref(), program.svg.as_bytes());
        }
        assert_eq!(
            Assets.list("icons/").unwrap().len(),
            ICONS.len() + crate::ui::program_icons::PROGRAMS.len()
        );
    }
}
