//! Terminal text color corrections: lift text that's too faint against its background, and
//! map arbitrary colors onto the theme's palette.

/// The least contrast "Improve text contrast" allows, as a WCAG ratio.
pub const MIN_CONTRAST: f32 = 3.0;

fn channels(rgb: u32) -> [f32; 3] {
    [(rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff].map(|c| c as f32 / 255.)
}

fn pack([r, g, b]: [f32; 3]) -> u32 {
    let byte = |c: f32| (c.clamp(0., 1.) * 255.).round() as u32;
    (byte(r) << 16) | (byte(g) << 8) | byte(b)
}

/// WCAG relative luminance.
pub fn luminance(rgb: u32) -> f32 {
    let linear = |c: f32| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b] = channels(rgb).map(linear);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG contrast ratio, from 1 (none) to 21 (black on white).
pub fn contrast(a: u32, b: u32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `fg`, moved toward white or black (whichever `bg` allows more of) just far enough to
/// reach `min` contrast against `bg`. Readable text is left as it is.
pub fn ensure_contrast(fg: u32, bg: u32, min: f32) -> u32 {
    if contrast(fg, bg) >= min {
        return fg;
    }
    let target = if contrast(0xffffff, bg) >= contrast(0x000000, bg) {
        [1., 1., 1.]
    } else {
        [0., 0., 0.]
    };
    let from = channels(fg);
    let mix = |t: f32| pack([0, 1, 2].map(|i| from[i] + (target[i] - from[i]) * t));
    // The smallest blend that's readable, found by bisection.
    let (mut low, mut high) = (0f32, 1f32);
    for _ in 0..16 {
        let mid = (low + high) / 2.;
        if contrast(mix(mid), bg) >= min {
            high = mid;
        } else {
            low = mid;
        }
    }
    mix(high)
}

/// CIELAB coordinates (D65), where distance follows how different colors look.
fn lab(rgb: u32) -> [f32; 3] {
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b] = channels(rgb).map(linear);
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16. / 116.
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116. * fy - 16., 500. * (fx - fy), 200. * (fy - fz)]
}

/// The theme color that looks nearest `rgb`, among `palette`.
pub fn nearest(rgb: u32, palette: &[u32]) -> u32 {
    let target = lab(rgb);
    let distance = |c: u32| {
        let other = lab(c);
        (0..3).map(|i| (other[i] - target[i]).powi(2)).sum::<f32>()
    };
    palette
        .iter()
        .copied()
        .min_by(|x, y| distance(*x).total_cmp(&distance(*y)))
        .unwrap_or(rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_matches_wcag() {
        assert!((contrast(0x000000, 0xffffff) - 21.).abs() < 0.01);
        assert!((contrast(0x777777, 0x777777) - 1.).abs() < 0.001);
    }

    #[test]
    fn faint_text_is_lifted_just_enough() {
        let bg = 0x000000;
        let dim = 0x202020;
        let fixed = ensure_contrast(dim, bg, MIN_CONTRAST);
        assert!(contrast(fixed, bg) >= MIN_CONTRAST);
        assert!(
            contrast(fixed, bg) < MIN_CONTRAST + 0.2,
            "barely past the minimum"
        );
        assert!(
            luminance(fixed) > luminance(dim),
            "lighter on a dark background"
        );
        // On a light background, it darkens; readable text doesn't change.
        assert!(luminance(ensure_contrast(0xe0e0e0, 0xffffff, MIN_CONTRAST)) < luminance(0xe0e0e0));
        assert_eq!(ensure_contrast(0xb2d26b, 0x000000, MIN_CONTRAST), 0xb2d26b);
    }

    #[test]
    fn colors_map_to_the_nearest_theme_color() {
        let palette = [0x000000, 0xcc3333, 0x33cc33, 0x3333cc, 0xeeeeee];
        assert_eq!(nearest(0xff5f5f, &palette), 0xcc3333);
        assert_eq!(nearest(0x00af00, &palette), 0x33cc33);
        assert_eq!(nearest(0xfafafa, &palette), 0xeeeeee);
        assert_eq!(nearest(0x33cc33, &palette), 0x33cc33);
        // A pastel theme's red is nearer a red than its grey is, though RGB says otherwise.
        assert_eq!(
            nearest(0xff5f5f, &[0x474747, 0xffb4ab, 0xb2d26b, 0x919191]),
            0xffb4ab
        );
    }
}
