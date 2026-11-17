//! Program identities and sourced logos. See assets/programs/SOURCES.md for provenance.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind {
    Brand,
    Shell,
    Generic,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProgramIcon {
    pub id: &'static str,
    pub name: &'static str,
    pub commands: &'static [&'static str],
    pub ansi: usize,
    pub background: u32,
    pub foreground: Option<u32>,
    pub kind: IconKind,
    pub icon: &'static str,
    pub svg: &'static str,
}

impl ProgramIcon {
    pub fn is_shell(&self) -> bool {
        self.kind == IconKind::Shell
    }
}

macro_rules! program {
    ($id:literal, $name:literal, [$($command:literal),*], $ansi:literal, $background:literal, $foreground:expr, $kind:expr, $asset:literal) => {
        ProgramIcon {
            id: $id,
            name: $name,
            commands: &[$($command),*],
            ansi: $ansi,
            background: $background,
            foreground: $foreground,
            kind: $kind,
            icon: concat!("programs/", $id),
            svg: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/programs/", $asset)),
        }
    };
}

mod catalog;
pub mod tile;
pub use catalog::PROGRAMS;

pub fn by_icon(icon: &str) -> Option<&'static ProgramIcon> {
    PROGRAMS.iter().find(|program| program.icon == icon)
}

pub fn pane_colors(model: &crate::rex::host::HostModel, block: &str) -> Option<(u32, u32)> {
    model
        .panes
        .get(block)?
        .snapshot()
        .frame
        .map(|frame| (frame.fg, frame.bg))
}

pub fn colors(
    name: &str,
    theme: &super::theme::Theme,
    terminal: Option<(u32, u32)>,
) -> Option<(u32, u32)> {
    let program = by_icon(name)?;
    Some(if program.is_shell() {
        terminal.unwrap_or((theme.terminal.foreground, theme.terminal.background))
    } else {
        (
            program
                .foreground
                .unwrap_or_else(|| tile::foreground(program.background)),
            program.background,
        )
    })
}

/// Program badges share flat brand tiles in tabs, headers, cards and lists.
pub fn render_icon(
    name: &'static str,
    side: f32,
    text: gpui::Hsla,
    theme: &super::theme::Theme,
    terminal_colors: Option<(u32, u32)>,
) -> gpui::AnyElement {
    use gpui::{IntoElement, ParentElement, Styled, div, px};
    if let Some((foreground, background)) = colors(name, theme, terminal_colors) {
        tile::render(name, side, background, Some(foreground))
    } else {
        div()
            .size(px(side))
            .child(super::icons::icon(name).size(px(side)).text_color(text))
            .into_any_element()
    }
}

/// Exact identity matching avoids branding arbitrary commands or terminal titles.
pub fn find(name: &str) -> Option<&'static ProgramIcon> {
    let name = name
        .trim()
        .rsplit(['/', '\\'])
        .next()?
        .trim_start_matches('-');
    let name = name.strip_suffix(".exe").unwrap_or(name);
    PROGRAMS.iter().find(|program| {
        program.id.eq_ignore_ascii_case(name)
            || program.name.eq_ignore_ascii_case(name)
            || program
                .commands
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_variants_follow_their_pane_while_brands_keep_their_colors() {
        let theme = super::super::theme::Theme::from_noctalia(&Default::default());
        let terminal = (0xaabbcc, 0x112233);
        for program in PROGRAMS {
            let actual = colors(program.icon, &theme, Some(terminal)).unwrap();
            if program.is_shell() {
                assert_eq!(actual, terminal, "{}", program.id);
            } else {
                assert_eq!(actual.1, program.background, "{}", program.id);
            }
        }
        for name in ["claude-code", "co", "buildkite"] {
            let program = find(name).unwrap();
            assert_eq!(colors(program.icon, &theme, None).unwrap().0, 0xffffff);
        }
    }

    #[test]
    fn requested_programs_have_svg_and_unambiguous_identities() {
        for program in PROGRAMS {
            assert!(program.ansi < 16);
            assert!(program.svg.contains("<svg"), "{}", program.id);
            assert!(program.svg.trim_end().ends_with("</svg>"), "{}", program.id);
            assert!(!program.svg.contains("<script"));
            for alias in program
                .commands
                .iter()
                .copied()
                .chain([program.id, program.name])
            {
                // A program may deliberately repeat its own ID as its command name.
                if let Some(other) = PROGRAMS.iter().find(|other| {
                    other.id != program.id
                        && (other.id.eq_ignore_ascii_case(alias)
                            || other.name.eq_ignore_ascii_case(alias)
                            || other.commands.iter().any(|a| a.eq_ignore_ascii_case(alias)))
                }) {
                    panic!("ambiguous alias {alias}: {} / {}", program.id, other.id);
                }
                assert_eq!(find(alias), Some(program));
            }
        }
        for requested in include_str!("../../tests/fixtures/program-icons.txt").lines() {
            assert!(
                find(requested).is_some(),
                "missing requested icon: {requested}"
            );
        }
        assert_eq!(find("/usr/bin/gojo").unwrap().id, "gojo");
        assert_eq!(find("co.codes").unwrap().name, "co");
        assert_eq!(find("co").unwrap().name, "co");
        assert_eq!(find("-bash").unwrap().id, "shell");
        assert!(find("my-docker-backup").is_none());
        assert!(find("echo gojo").is_none());
        assert!(find("deploy-to-vercel.sh").is_none());
    }
}
