//! The terminal font: the user's choice, else Ghostty's `font-family`/`font-size`, else
//! fontconfig's monospace font.
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalFont {
    pub family: String,
    /// In points, as Ghostty and Rex for Mac measure it.
    pub size: f32,
    /// Extra line spacing, as a multiple of the font's natural line height.
    pub line_height: f32,
}

impl TerminalFont {
    pub const DEFAULT_SIZE: f32 = 12.;

    /// Pixels at 96 DPI; GPUI scales them for the display.
    pub fn pixels(&self) -> f32 {
        self.size * 4. / 3.
    }
}

/// `font-family` and `font-size` from a Ghostty config.
pub fn from_ghostty(config: &str) -> (Option<String>, Option<f32>) {
    let mut family = None;
    let mut size = None;
    for line in config.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim();
        match key.trim() {
            // The first family wins; later ones are fallbacks.
            "font-family" if family.is_none() && !value.is_empty() => {
                family = Some(value.to_owned())
            }
            "font-size" => size = value.parse::<f32>().ok().filter(|s| *s > 0.),
            _ => {}
        }
    }
    (family, size)
}

fn ghostty_config() -> Option<String> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    std::fs::read_to_string(config.join("ghostty/config")).ok()
}

fn fontconfig_monospace() -> Option<String> {
    let output = std::process::Command::new("fc-match")
        .args(["-f", "%{family[0]}", "monospace"])
        .output()
        .ok()?;
    let family = String::from_utf8(output.stdout).ok()?;
    let family = family.trim();
    (!family.is_empty()).then(|| family.to_owned())
}

pub fn default_terminal_font() -> TerminalFont {
    let (family, size) = ghostty_config()
        .map(|c| from_ghostty(&c))
        .unwrap_or_default();
    TerminalFont {
        family: family
            .or_else(fontconfig_monospace)
            .unwrap_or_else(|| "monospace".into()),
        size: size.unwrap_or(TerminalFont::DEFAULT_SIZE),
        line_height: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ghostty_font_settings() {
        let config = "theme = dark:pierre-dark\nfont-family = \"DepartureMono Nerd Font\"\nfont-family = Symbols\nfont-size = 11\n";
        assert_eq!(
            from_ghostty(config),
            (Some("DepartureMono Nerd Font".into()), Some(11.))
        );
        assert_eq!(from_ghostty("# nothing\n"), (None, None));
    }
}
