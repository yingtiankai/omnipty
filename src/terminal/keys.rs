use alacritty_terminal::term::TermMode;
use gpui::Keystroke;

use crate::config::schema::OptionAsMeta;

pub fn uses_text_input(ks: &Keystroke, option_as_meta: OptionAsMeta) -> bool {
    cfg!(target_os = "macos")
        && crate::line_edit::is_text_key(ks)
        && !(ks.modifiers.alt && option_as_meta != OptionAsMeta::None)
}

/// Encode a GPUI keystroke into the byte sequence the PTY expects.
/// Returns None for keys that must not reach the shell (anything with cmd,
/// bare modifiers, unrecognized named keys).
pub fn to_bytes(ks: &Keystroke, mode: &TermMode, option_as_meta: OptionAsMeta) -> Option<Vec<u8>> {
    let mods = &ks.modifiers;
    // cmd never reaches the PTY; it belongs to the app.
    if mods.platform || mods.function {
        return None;
    }

    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    // GPUI reports a single `alt` flag, so left/right can't be distinguished;
    // any setting other than "none" treats Option as Meta.
    let alt_is_meta = option_as_meta != OptionAsMeta::None;

    // CSI 1;<m> modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
    let mod_param = 1 + (mods.shift as u8) + ((mods.alt as u8) << 1) + ((mods.control as u8) << 2);
    let modified = mods.shift || mods.alt || mods.control;

    let csi_cursor = |letter: char| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{mod_param}{letter}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        }
    };
    let csi_tilde = |num: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{num};{mod_param}~").into_bytes()
        } else {
            format!("\x1b[{num}~").into_bytes()
        }
    };
    let function_key = |letter: char| -> Vec<u8> {
        if modified {
            csi_cursor(letter)
        } else {
            format!("\x1bO{letter}").into_bytes()
        }
    };

    match ks.key.as_str() {
        // shortcut: modified Enter always uses CSI-u, add negotiation if legacy apps need old bindings.
        "enter" if modified => {
            return Some(format!("\x1b[13;{mod_param}u").into_bytes());
        }
        "enter" => return Some(b"\r".to_vec()), // \r, not \n — the PTY expects CR
        "backspace" => {
            let byte = if mods.control { 0x08 } else { 0x7f };
            return Some(if mods.alt {
                vec![0x1b, byte]
            } else {
                vec![byte]
            });
        }
        "tab" => {
            return Some(if mods.shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            });
        }
        "escape" => return Some(vec![0x1b]),
        "up" => return Some(csi_cursor('A')),
        "down" => return Some(csi_cursor('B')),
        // macOS only: Terminal.app and Ghostty translate Option+arrows to ESC b / ESC f,
        // which zsh and bash bind to word movement. Linux terminals send the CSI
        // form and tmux, vim and fish rely on it, so leave it alone there.
        "right" => {
            if cfg!(target_os = "macos") && mods.alt && !mods.shift && !mods.control {
                return Some(b"\x1bf".to_vec());
            }
            return Some(csi_cursor('C'));
        }
        "left" => {
            if cfg!(target_os = "macos") && mods.alt && !mods.shift && !mods.control {
                return Some(b"\x1bb".to_vec());
            }
            return Some(csi_cursor('D'));
        }
        "home" => return Some(csi_cursor('H')),
        "end" => return Some(csi_cursor('F')),
        "pageup" => return Some(csi_tilde(5)),
        "pagedown" => return Some(csi_tilde(6)),
        "insert" => return Some(csi_tilde(2)),
        "delete" => return Some(csi_tilde(3)),
        "f1" => return Some(function_key('P')),
        "f2" => return Some(function_key('Q')),
        "f3" => return Some(function_key('R')),
        "f4" => return Some(function_key('S')),
        "f5" => return Some(csi_tilde(15)),
        "f6" => return Some(csi_tilde(17)),
        "f7" => return Some(csi_tilde(18)),
        "f8" => return Some(csi_tilde(19)),
        "f9" => return Some(csi_tilde(20)),
        "f10" => return Some(csi_tilde(21)),
        "f11" => return Some(csi_tilde(23)),
        "f12" => return Some(csi_tilde(24)),
        "space" => {
            if mods.control {
                return Some(vec![0x00]);
            }
            if mods.alt && alt_is_meta {
                return Some(vec![0x1b, b' ']);
            }
            return Some(b" ".to_vec());
        }
        _ => {}
    }

    // Control characters from the base key.
    if mods.control {
        let ch = single_char(&ks.key)?;
        let byte = match ch {
            'a'..='z' => (ch as u8).to_ascii_uppercase() & 0x1f,
            '[' => 0x1b,
            '\\' => 0x1c,
            ']' => 0x1d,
            '^' | '6' => 0x1e,
            '_' | '-' | '/' => 0x1f,
            '@' | '2' => 0x00,
            '?' => 0x7f,
            _ => return None,
        };
        return Some(if mods.alt && alt_is_meta {
            vec![0x1b, byte]
        } else {
            vec![byte]
        });
    }

    // Option-as-Meta: ESC prefix + the base key, not the composed character.
    if mods.alt && alt_is_meta {
        let ch = single_char(&ks.key)?;
        let ch = if mods.shift { shifted(ch) } else { ch };
        let mut bytes = vec![0x1b];
        bytes.extend(ch.to_string().into_bytes());
        return Some(bytes);
    }

    // Plain printable input: prefer the composed character (handles shift,
    // and Option-composed characters like é when Option is not Meta).
    if let Some(key_char) = &ks.key_char {
        return Some(key_char.clone().into_bytes());
    }
    let ch = single_char(&ks.key)?;
    let ch = if mods.shift { shifted(ch) } else { ch };
    Some(ch.to_string().into_bytes())
}

fn single_char(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        None
    } else {
        Some(ch)
    }
}

/// Best-effort US-layout shift mapping, used only when the platform did not
/// hand us a composed key_char.
fn shifted(ch: char) -> char {
    match ch {
        'a'..='z' => ch.to_ascii_uppercase(),
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        '`' => '~',
        _ => ch,
    }
}

/// Prepare pasted text: normalize newlines, strip escape bytes (pasting raw
/// escape sequences into a shell is a real security issue), and bracket when
/// the application asked for bracketed paste.
pub fn prepare_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let cleaned: String = text
        .replace("\r\n", "\r")
        .replace('\n', "\r")
        .chars()
        .filter(|&c| c != '\x1b')
        .collect();
    if bracketed {
        let mut out = b"\x1b[200~".to_vec();
        out.extend(cleaned.into_bytes());
        out.extend(b"\x1b[201~");
        out
    } else {
        cleaned.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ks(s: &str) -> Keystroke {
        Keystroke::parse(s).unwrap()
    }

    #[test]
    fn native_text_input_preserves_control_keys_and_option_as_meta() {
        let mut text = ks("a");
        text.key_char = Some("a".into());
        assert_eq!(
            uses_text_input(&text, OptionAsMeta::None),
            cfg!(target_os = "macos")
        );
        text.modifiers.alt = true;
        assert_eq!(
            uses_text_input(&text, OptionAsMeta::None),
            cfg!(target_os = "macos")
        );
        assert!(!uses_text_input(&text, OptionAsMeta::Both));
        for key in [
            "enter",
            "left",
            "backspace",
            "escape",
            "ctrl-c",
            "cmd-v",
            "f1",
        ] {
            assert!(!uses_text_input(&ks(key), OptionAsMeta::None), "{key}");
        }
    }

    #[test]
    fn ctrl_letters() {
        let mode = TermMode::empty();
        assert_eq!(
            to_bytes(&ks("ctrl-c"), &mode, OptionAsMeta::None),
            Some(vec![0x03])
        );
        assert_eq!(
            to_bytes(&ks("ctrl-a"), &mode, OptionAsMeta::None),
            Some(vec![0x01])
        );
        assert_eq!(
            to_bytes(&ks("ctrl-["), &mode, OptionAsMeta::None),
            Some(vec![0x1b])
        );
    }

    #[test]
    fn ctrl_slash_sends_unit_separator_instead_of_delete() {
        for mode in [
            TermMode::empty(),
            TermMode::APP_CURSOR,
            TermMode::ALT_SCREEN,
        ] {
            for meta in [OptionAsMeta::None, OptionAsMeta::Both] {
                for key_char in [None, Some("/"), Some("\x1f"), Some("\x7f")] {
                    let mut key = ks("ctrl-/");
                    key.key_char = key_char.map(str::to_owned);
                    assert!(!uses_text_input(&key, meta));
                    assert_eq!(to_bytes(&key, &mode, meta), Some(vec![0x1f]));
                }
            }
        }
    }

    #[test]
    fn ctrl_slash_preserves_meta_and_other_punctuation_encodings() {
        for (key, meta, expected) in [
            ("ctrl-alt-/", OptionAsMeta::Both, b"\x1b\x1f".as_slice()),
            ("ctrl-alt-/", OptionAsMeta::None, b"\x1f".as_slice()),
            ("ctrl-_", OptionAsMeta::None, b"\x1f".as_slice()),
            ("ctrl--", OptionAsMeta::None, b"\x1f".as_slice()),
            ("ctrl-?", OptionAsMeta::None, b"\x7f".as_slice()),
            ("backspace", OptionAsMeta::None, b"\x7f".as_slice()),
            ("/", OptionAsMeta::None, b"/".as_slice()),
            ("?", OptionAsMeta::None, b"?".as_slice()),
        ] {
            assert_eq!(
                to_bytes(&ks(key), &TermMode::empty(), meta),
                Some(expected.to_vec()),
                "{key}"
            );
        }
        for key in ["cmd-ctrl-/", "fn-ctrl-/"] {
            assert_eq!(
                to_bytes(&ks(key), &TermMode::empty(), OptionAsMeta::None),
                None
            );
        }
    }

    #[test]
    fn enter_is_cr() {
        assert_eq!(
            to_bytes(&ks("enter"), &TermMode::empty(), OptionAsMeta::None),
            Some(vec![b'\r'])
        );
    }

    #[test]
    fn enter_chords_preserve_modifiers_without_changing_plain_enter() {
        for mode in [
            TermMode::empty(),
            TermMode::APP_CURSOR,
            TermMode::ALT_SCREEN,
        ] {
            for meta in [OptionAsMeta::None, OptionAsMeta::Both] {
                for (key, expected) in [
                    ("enter", "\r"),
                    ("shift-enter", "\x1b[13;2u"),
                    ("alt-enter", "\x1b[13;3u"),
                    ("alt-shift-enter", "\x1b[13;4u"),
                    ("ctrl-enter", "\x1b[13;5u"),
                    ("ctrl-shift-enter", "\x1b[13;6u"),
                    ("ctrl-alt-enter", "\x1b[13;7u"),
                    ("ctrl-alt-shift-enter", "\x1b[13;8u"),
                ] {
                    let mut keystroke = ks(key);
                    // AppKit supplies a newline key_char even for Enter chords.
                    keystroke.key_char = Some("\n".into());
                    assert!(!uses_text_input(&keystroke, meta), "{key}");
                    assert_eq!(
                        to_bytes(&keystroke, &mode, meta),
                        Some(expected.as_bytes().to_vec()),
                        "{key}"
                    );
                }
            }
        }
    }

    #[test]
    fn function_keys_preserve_modifiers_and_legacy_plain_sequences() {
        for mode in [TermMode::empty(), TermMode::APP_CURSOR] {
            for (prefix, parameter) in [
                ("", 1),
                ("shift-", 2),
                ("alt-", 3),
                ("alt-shift-", 4),
                ("ctrl-", 5),
                ("ctrl-shift-", 6),
                ("ctrl-alt-", 7),
                ("ctrl-alt-shift-", 8),
            ] {
                for (key, letter) in [("f1", 'P'), ("f2", 'Q'), ("f3", 'R'), ("f4", 'S')] {
                    let expected = if parameter == 1 {
                        format!("\x1bO{letter}")
                    } else {
                        format!("\x1b[1;{parameter}{letter}")
                    };
                    assert_eq!(
                        to_bytes(&ks(&format!("{prefix}{key}")), &mode, OptionAsMeta::None),
                        Some(expected.into_bytes()),
                        "{prefix}{key}"
                    );
                }
                for (key, number) in [
                    ("f5", 15),
                    ("f6", 17),
                    ("f7", 18),
                    ("f8", 19),
                    ("f9", 20),
                    ("f10", 21),
                    ("f11", 23),
                    ("f12", 24),
                ] {
                    let expected = if parameter == 1 {
                        format!("\x1b[{number}~")
                    } else {
                        format!("\x1b[{number};{parameter}~")
                    };
                    assert_eq!(
                        to_bytes(&ks(&format!("{prefix}{key}")), &mode, OptionAsMeta::None),
                        Some(expected.into_bytes()),
                        "{prefix}{key}"
                    );
                }
            }
        }
    }

    #[test]
    fn legacy_control_and_tab_sequences_stay_unchanged() {
        for (key, expected) in [
            ("ctrl-j", b"\n".as_slice()),
            ("ctrl-c", b"\x03".as_slice()),
            ("ctrl-d", b"\x04".as_slice()),
            ("tab", b"\t".as_slice()),
            ("shift-tab", b"\x1b[Z".as_slice()),
        ] {
            assert_eq!(
                to_bytes(&ks(key), &TermMode::empty(), OptionAsMeta::None),
                Some(expected.to_vec()),
                "{key}"
            );
        }
    }

    #[test]
    fn arrows_respect_app_cursor_mode() {
        assert_eq!(
            to_bytes(&ks("up"), &TermMode::empty(), OptionAsMeta::None),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            to_bytes(&ks("up"), &TermMode::APP_CURSOR, OptionAsMeta::None),
            Some(b"\x1bOA".to_vec())
        );
        // ctrl-shift-right = CSI 1;6C.
        assert_eq!(
            to_bytes(
                &ks("ctrl-shift-right"),
                &TermMode::empty(),
                OptionAsMeta::None
            ),
            Some(b"\x1b[1;6C".to_vec())
        );
    }

    #[test]
    fn cmd_never_reaches_pty() {
        for key in [
            "cmd-a",
            "cmd-enter",
            "cmd-shift-enter",
            "cmd-f1",
            "cmd-shift-f12",
        ] {
            assert_eq!(
                to_bytes(&ks(key), &TermMode::empty(), OptionAsMeta::None),
                None,
                "{key}"
            );
        }
    }

    #[test]
    fn alt_meta_prefixes_escape() {
        assert_eq!(
            to_bytes(&ks("alt-b"), &TermMode::empty(), OptionAsMeta::Both),
            Some(vec![0x1b, b'b'])
        );
    }

    #[test]
    fn paste_is_sanitized_and_bracketed() {
        let out = prepare_paste("a\nb\x1b[31m", true);
        assert_eq!(out, b"\x1b[200~a\rb[31m\x1b[201~".to_vec());
    }

    #[test]
    fn multiline_paste_stays_inside_a_single_bracketed_payload() {
        assert_eq!(
            prepare_paste("第一行\r\n第二行\nthird", true),
            "\x1b[200~第一行\r第二行\rthird\x1b[201~".as_bytes(),
        );
    }

    #[test]
    fn option_backspace_deletes_word() {
        let m = TermMode::empty();
        assert_eq!(
            to_bytes(&ks("alt-backspace"), &m, OptionAsMeta::None),
            Some(vec![0x1b, 0x7f])
        );
        assert_eq!(
            to_bytes(&ks("alt-backspace"), &m, OptionAsMeta::Both),
            Some(vec![0x1b, 0x7f])
        );
        assert_eq!(
            to_bytes(&ks("backspace"), &m, OptionAsMeta::None),
            Some(vec![0x7f])
        );
    }

    #[test]
    fn option_arrows_jump_words() {
        let m = TermMode::empty();
        let (left, right): (&[u8], &[u8]) = if cfg!(target_os = "macos") {
            (b"\x1bb", b"\x1bf")
        } else {
            (b"\x1b[1;3D", b"\x1b[1;3C")
        };
        assert_eq!(
            to_bytes(&ks("alt-left"), &m, OptionAsMeta::None),
            Some(left.to_vec())
        );
        assert_eq!(
            to_bytes(&ks("alt-right"), &m, OptionAsMeta::None),
            Some(right.to_vec())
        );
        assert_eq!(
            to_bytes(&ks("alt-shift-left"), &m, OptionAsMeta::None),
            Some(b"\x1b[1;4D".to_vec())
        );
        assert_eq!(
            to_bytes(&ks("left"), &m, OptionAsMeta::None),
            Some(b"\x1b[D".to_vec())
        );
    }
}
