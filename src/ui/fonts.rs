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
    /// Pixels added to each character's width.
    pub spacing: f32,
    pub ligatures: bool,
    /// Whether bold and italic text draws bold and italic.
    pub bold: bool,
    pub italic: bool,
    pub fallbacks: Vec<String>,
    /// The weight of ordinary text, 100 to 900; bold text draws 300 heavier.
    pub weight: f32,
}

/// One upright face of a font family, as fontconfig lists it: its name (such as Light or
/// Book) and its weight on the CSS scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub name: String,
    pub weight: u16,
}

/// Fontconfig's weight scale on the CSS one.
fn css_weight(fontconfig: u32) -> u16 {
    match fontconfig {
        0..=20 => 100,
        21..=45 => 200,
        46..=52 => 300,
        53..=62 => 350,
        63..=77 => 380,
        78..=90 => 400,
        91..=140 => 500,
        141..=190 => 600,
        191..=202 => 700,
        203..=207 => 800,
        _ => 900,
    }
}

/// The upright faces of `family` installed here, lightest first.
pub fn faces(family: &str) -> Vec<Face> {
    let Ok(output) = std::process::Command::new("fc-list")
        .args([&format!("{family}:slant=0"), "style", "weight"])
        .output()
    else {
        return Vec::new();
    };
    faces_from(&String::from_utf8_lossy(&output.stdout))
}

/// Faces from `fc-list FAMILY style weight` output, such as `:style=Light,Regular:weight=50`.
fn faces_from(listing: &str) -> Vec<Face> {
    let mut faces: Vec<Face> = Vec::new();
    for line in listing.lines() {
        let field = |name: &str| {
            line.split(':')
                .find_map(|part| part.strip_prefix(&format!("{name}=")))
                .map(str::to_owned)
        };
        let (Some(style), Some(weight)) = (field("style"), field("weight")) else {
            continue;
        };
        // The first style name is the face's own; later ones are translations.
        let name = style
            .split(',')
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let Ok(weight) = weight.trim().parse::<u32>() else {
            continue;
        };
        if name.is_empty() || faces.iter().any(|f| f.name == name) {
            continue;
        }
        faces.push(Face {
            name,
            weight: css_weight(weight),
        });
    }
    faces.sort_by_key(|f| f.weight);
    faces
}

/// A weight's name, for the settings.
pub fn weight_name(weight: u16) -> &'static str {
    match weight {
        0..=149 => "Thin",
        150..=249 => "Extra Light",
        250..=349 => "Light",
        350..=449 => "Regular",
        450..=549 => "Medium",
        550..=649 => "Semibold",
        650..=749 => "Bold",
        750..=849 => "Extra Bold",
        _ => "Black",
    }
}

impl TerminalFont {
    pub const DEFAULT_SIZE: f32 = 12.;
    pub const DEFAULT_WEIGHT: f32 = 400.;

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
        spacing: 0.,
        ligatures: false,
        bold: true,
        italic: true,
        fallbacks: Vec::new(),
        weight: TerminalFont::DEFAULT_WEIGHT,
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

    #[test]
    fn faces_come_from_fontconfig_lightest_first() {
        let listing = "DM Mono,DM Mono Medium:style=Medium,Regular:weight=100\n\
                       DM Mono:style=Regular:weight=80\n\
                       DM Mono,DM Mono Light:style=Light,Regular:weight=50\n";
        let faces = faces_from(listing);
        let names: Vec<_> = faces.iter().map(|f| (f.name.as_str(), f.weight)).collect();
        assert_eq!(names, [("Light", 300), ("Regular", 400), ("Medium", 500)]);
        assert_eq!(weight_name(400), "Regular");
    }
}
