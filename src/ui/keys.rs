//! Translate GPUI keystrokes into the key events Rex's `client.key` takes: a physical key
//! named after its W3C code, the modifiers, and the text the key typed.
use gpui::{Keystroke, Modifiers};
use serde_json::{Value, json};

/// A US-layout symbol, the physical key that types it, and whether it needs shift.
const SYMBOLS: &[(char, &str, bool)] = &[
    ('`', "backquote", false),
    ('~', "backquote", true),
    ('-', "minus", false),
    ('_', "minus", true),
    ('=', "equal", false),
    ('+', "equal", true),
    ('[', "bracket_left", false),
    ('{', "bracket_left", true),
    (']', "bracket_right", false),
    ('}', "bracket_right", true),
    ('\\', "backslash", false),
    ('|', "backslash", true),
    (';', "semicolon", false),
    (':', "semicolon", true),
    ('\'', "quote", false),
    ('"', "quote", true),
    (',', "comma", false),
    ('<', "comma", true),
    ('.', "period", false),
    ('>', "period", true),
    ('/', "slash", false),
    ('?', "slash", true),
    ('!', "digit_1", true),
    ('@', "digit_2", true),
    ('#', "digit_3", true),
    ('$', "digit_4", true),
    ('%', "digit_5", true),
    ('^', "digit_6", true),
    ('&', "digit_7", true),
    ('*', "digit_8", true),
    ('(', "digit_9", true),
    (')', "digit_0", true),
];

const DIGIT_BASE: &str = "1234567890";
const SHIFTED_DIGITS: &str = "!@#$%^&*()";

/// The physical key, whether the layout needed shift to type `key`, and the character the
/// key types without shift.
pub fn physical_key(key: &str) -> Option<(String, bool, Option<char>)> {
    let named = match key {
        "enter" => "enter",
        "escape" => "escape",
        "backspace" => "backspace",
        "tab" => "tab",
        "space" => "space",
        "delete" => "delete",
        "insert" => "insert",
        "home" => "home",
        "end" => "end",
        "pageup" => "page_up",
        "pagedown" => "page_down",
        "left" => "arrow_left",
        "right" => "arrow_right",
        "up" => "arrow_up",
        "down" => "arrow_down",
        "copy" => "copy",
        "cut" => "cut",
        "paste" => "paste",
        _ => "",
    };
    if !named.is_empty() {
        let unshifted = match named {
            "space" => Some(' '),
            "tab" => Some('\t'),
            "enter" => Some('\r'),
            _ => None,
        };
        return Some((named.into(), false, unshifted));
    }
    if let Some(number) = key.strip_prefix('f')
        && let Ok(n) = number.parse::<u8>()
        && (1..=25).contains(&n)
    {
        return Some((format!("f{n}"), false, None));
    }
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    if c.is_ascii_lowercase() {
        return Some((format!("key_{c}"), false, Some(c)));
    }
    if c.is_ascii_digit() {
        return Some((format!("digit_{c}"), false, Some(c)));
    }
    let (_, code, shifted) = SYMBOLS.iter().find(|(symbol, _, _)| *symbol == c)?;
    let base = if *shifted {
        if let Some(i) = SHIFTED_DIGITS.find(c) {
            DIGIT_BASE.chars().nth(i)
        } else {
            SYMBOLS
                .iter()
                .find(|(_, other, needs_shift)| other == code && !needs_shift)
                .map(|(symbol, _, _)| *symbol)
        }
    } else {
        Some(c)
    };
    Some(((*code).into(), *shifted, base))
}

/// Rex's modifier spelling: names joined with + in the order shift, ctrl, alt, super.
pub fn mods(modifiers: &Modifiers, shift: bool) -> String {
    let mut names = Vec::new();
    if shift || modifiers.shift {
        names.push("shift");
    }
    if modifiers.control {
        names.push("ctrl");
    }
    if modifiers.alt {
        names.push("alt");
    }
    if modifiers.platform {
        names.push("super");
    }
    names.join("+")
}

/// The `key` object for `client.key`, or None for keys Rex has no name for.
pub fn key_event(keystroke: &Keystroke, repeat: bool) -> Option<Value> {
    let (code, layout_shift, unshifted) = physical_key(&keystroke.key)?;
    let modifiers = &keystroke.modifiers;
    let text = keystroke
        .key_char
        .as_deref()
        .filter(|_| !modifiers.control && !modifiers.platform)
        .unwrap_or_default();
    let mut event = json!({
        "key": code,
        "mods": mods(modifiers, layout_shift),
        "text": text,
        "unshifted_codepoint": unshifted.map_or(0, |c| c as u32),
        "action": if repeat { "repeat" } else { "press" },
    });
    let shift_typed = layout_shift || (modifiers.shift && !text.is_empty());
    if shift_typed {
        event["consumed_mods"] = json!("shift");
    }
    Some(event)
}

/// `key_a` → `KeyA`, `arrow_left` → `ArrowLeft`: the W3C spelling Rex writes in brackets.
pub fn w3c_code(code: &str) -> String {
    code.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect()
}

/// GPUI key names that Rex spells differently.
fn rex_key_name(key: &str) -> &str {
    match key {
        "escape" => "esc",
        other => other,
    }
}

/// A keystroke as the keymap matches it.
pub fn stroke(keystroke: &Keystroke) -> crate::keymap::Stroke {
    let physical = physical_key(&keystroke.key);
    let modifiers = &keystroke.modifiers;
    let unshifted = physical
        .as_ref()
        .filter(|(_, layout_shift, _)| *layout_shift)
        .and_then(|(_, _, base)| base.map(|c| c.to_string()));
    crate::keymap::Stroke {
        ctrl: modifiers.control,
        alt: modifiers.alt,
        shift: modifiers.shift,
        cmd: modifiers.platform,
        key: rex_key_name(&keystroke.key).to_owned(),
        unshifted,
        code: physical.map(|(code, _, _)| w3c_code(&code)),
    }
}

/// True for a press of a modifier alone, which never runs a binding.
pub fn is_modifier(keystroke: &Keystroke) -> bool {
    matches!(
        keystroke.key.as_str(),
        "shift" | "control" | "alt" | "platform" | "function" | "super" | "capslock"
    )
}

/// A `client.key` event for a key written as Rex spells it, such as `ctrl+l` or `enter`.
pub fn event_from_spelling(spelling: &str) -> Option<Value> {
    let mut rest = spelling.trim();
    let mut modifiers = Modifiers::default();
    loop {
        if let Some(after) = rest.strip_prefix("ctrl+") {
            modifiers.control = true;
            rest = after;
        } else if let Some(after) = rest.strip_prefix("alt+") {
            modifiers.alt = true;
            rest = after;
        } else if let Some(after) = rest.strip_prefix("shift+") {
            modifiers.shift = true;
            rest = after;
        } else if let Some(after) = rest
            .strip_prefix("cmd+")
            .or_else(|| rest.strip_prefix("super+"))
        {
            modifiers.platform = true;
            rest = after;
        } else {
            break;
        }
    }
    let key = match rest {
        "esc" => "escape",
        other => other,
    };
    let typed =
        (key.chars().count() == 1 && !modifiers.control && !modifiers.platform).then(|| {
            if modifiers.shift {
                key.to_uppercase()
            } else {
                key.to_owned()
            }
        });
    let keystroke = Keystroke {
        modifiers,
        key: key.to_lowercase(),
        key_char: typed,
    };
    key_event(&keystroke, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(text: &'static str) -> Keystroke {
        Keystroke::parse(text).unwrap()
    }

    fn with_char(text: &'static str, key_char: &str) -> Keystroke {
        Keystroke {
            key_char: Some(key_char.into()),
            ..stroke(text)
        }
    }

    #[test]
    fn letters_digits_and_named_keys() {
        let a = key_event(&with_char("a", "a"), false).unwrap();
        assert_eq!(
            (
                a["key"].as_str(),
                a["mods"].as_str(),
                a["text"].as_str(),
                a["unshifted_codepoint"].as_u64()
            ),
            (Some("key_a"), Some(""), Some("a"), Some('a' as u64))
        );
        let upper = key_event(&with_char("shift-a", "A"), false).unwrap();
        assert_eq!(
            (upper["mods"].as_str(), upper["text"].as_str()),
            (Some("shift"), Some("A"))
        );
        assert_eq!(upper["consumed_mods"], "shift");
        let enter = key_event(&stroke("enter"), true).unwrap();
        assert_eq!(
            (enter["key"].as_str(), enter["action"].as_str()),
            (Some("enter"), Some("repeat"))
        );
        assert_eq!(
            key_event(&stroke("pageup"), false).unwrap()["key"],
            "page_up"
        );
        assert_eq!(
            key_event(&stroke("left"), false).unwrap()["key"],
            "arrow_left"
        );
        assert_eq!(key_event(&stroke("f12"), false).unwrap()["key"], "f12");
        assert_eq!(
            key_event(&with_char("7", "7"), false).unwrap()["key"],
            "digit_7"
        );
    }

    #[test]
    fn shifted_symbols_name_their_physical_key() {
        // GPUI reports shift+1 as "!" without shift; Rex wants digit_1 with shift.
        let bang = key_event(&with_char("!", "!"), false).unwrap();
        assert_eq!(
            (
                bang["key"].as_str(),
                bang["mods"].as_str(),
                bang["text"].as_str()
            ),
            (Some("digit_1"), Some("shift"), Some("!"))
        );
        assert_eq!(bang["unshifted_codepoint"], '1' as u32);
        let colon = key_event(&with_char(":", ":"), false).unwrap();
        assert_eq!(
            (colon["key"].as_str(), colon["unshifted_codepoint"].as_u64()),
            (Some("semicolon"), Some(';' as u64))
        );
    }

    #[test]
    fn control_combinations_send_no_text() {
        let ctrl_c = key_event(&stroke("ctrl-c"), false).unwrap();
        assert_eq!(
            (
                ctrl_c["key"].as_str(),
                ctrl_c["mods"].as_str(),
                ctrl_c["text"].as_str()
            ),
            (Some("key_c"), Some("ctrl"), Some(""))
        );
        let alt_b = key_event(&with_char("alt-b", "b"), false).unwrap();
        assert_eq!(
            (alt_b["mods"].as_str(), alt_b["text"].as_str()),
            (Some("alt"), Some("b"))
        );
        let all = mods(
            &Modifiers {
                control: true,
                alt: true,
                shift: true,
                platform: true,
                function: false,
            },
            false,
        );
        assert_eq!(all, "shift+ctrl+alt+super");
    }

    #[test]
    fn strokes_and_spellings_meet_the_keymap() {
        let brace = with_char("}", "}");
        let stroke = super::stroke(&Keystroke {
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
            ..brace
        });
        assert_eq!(
            stroke.spellings(),
            vec!["ctrl+}", "ctrl+shift+]", "ctrl+shift+[BracketRight]"]
        );
        let escape = super::stroke(&stroke_of("escape"));
        assert!(escape.is_escape());
        assert_eq!(w3c_code("arrow_left"), "ArrowLeft");
        let ctrl_l = event_from_spelling("ctrl+l").unwrap();
        assert_eq!(
            (
                ctrl_l["key"].as_str(),
                ctrl_l["mods"].as_str(),
                ctrl_l["text"].as_str()
            ),
            (Some("key_l"), Some("ctrl"), Some(""))
        );
        let enter = event_from_spelling("enter").unwrap();
        assert_eq!(enter["key"], "enter");
        let upper = event_from_spelling("shift+a").unwrap();
        assert_eq!(upper["text"], "A");
    }

    fn stroke_of(text: &'static str) -> Keystroke {
        Keystroke::parse(text).unwrap()
    }

    #[test]
    fn unknown_keys_are_skipped() {
        assert!(key_event(&stroke("back"), false).is_none());
    }
}
