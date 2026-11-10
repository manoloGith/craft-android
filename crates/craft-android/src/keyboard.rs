//! A hardware keyboard on Android, from MainActivity (`nativeKey`), into egui.
//!
//! winit's Android backend maps keys it knows by code (arrows, function keys, Page Up…) but
//! gets no character for letters, digits and symbols, so typing and shortcuts like Ctrl+O did
//! nothing. The activity reads physical keyboards itself (Android's KeyEvent gives the character
//! for the user's layout, dead keys included) and hands each key here. Platform independent so it
//! is tested anywhere; on other systems nothing is queued.

use std::sync::{Mutex, PoisonError};

/// One key from the activity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPress {
    /// Android `KeyEvent.getKeyCode()`.
    pub keycode: i32,
    /// The key's character without modifiers (`getUnicodeChar(0)`), 0 if none: the logical key.
    pub base: u32,
    /// Text the key types (layout, Shift, AltGr and dead keys applied), empty if none.
    pub text: String,
    pub pressed: bool,
    pub repeat: bool,
    /// Android meta state.
    pub meta: u32,
}

static QUEUE: Mutex<Vec<KeyPress>> = Mutex::new(Vec::new());
/// The modifiers egui was last told are held.
static MODIFIERS: Mutex<egui::Modifiers> = Mutex::new(egui::Modifiers::NONE);

pub fn push(k: KeyPress) {
    QUEUE.lock().unwrap_or_else(PoisonError::into_inner).push(k);
}

/// The queued keys as egui events, with `ModifiersChanged` whenever the held modifiers change
/// (egui reads the held modifiers from those events).
pub fn drain() -> Vec<egui::Event> {
    let queued = std::mem::take(&mut *QUEUE.lock().unwrap_or_else(PoisonError::into_inner));
    let mut held = MODIFIERS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut out = Vec::new();
    for k in &queued {
        out.extend(with_modifiers(k, &mut held));
    }
    out
}

/// One key's events, preceded by `ModifiersChanged` if it changes the modifiers in `held`.
pub fn with_modifiers(k: &KeyPress, held: &mut egui::Modifiers) -> Vec<egui::Event> {
    let mut out = Vec::new();
    let now = modifiers(k.meta);
    if now != *held {
        *held = now;
        out.push(egui::Event::ModifiersChanged(now));
    }
    out.extend(to_egui(k));
    out
}

/// Android's meta state as egui modifiers (Ctrl is the command key off Apple systems).
pub fn modifiers(meta: u32) -> egui::Modifiers {
    // KeyEvent.META_SHIFT_ON, META_ALT_ON, META_CTRL_ON
    let ctrl = meta & 0x1000 != 0;
    egui::Modifiers { alt: meta & 0x02 != 0, ctrl, shift: meta & 0x01 != 0, mac_cmd: false, command: ctrl }
}

pub fn to_egui(k: &KeyPress) -> Vec<egui::Event> {
    let modifiers = modifiers(k.meta);
    let mut out = Vec::new();
    let physical_key = physical_key(k.keycode);
    if let Some(key) = char::from_u32(k.base).and_then(key_for_char).or(physical_key) {
        out.push(egui::Event::Key { key, physical_key, pressed: k.pressed, repeat: k.repeat, modifiers });
    }
    // Text is typed on press, not with Ctrl (shortcuts); control characters are keys, not text.
    if k.pressed && !modifiers.ctrl && !k.text.is_empty() && !k.text.chars().any(char::is_control) {
        out.push(egui::Event::Text(k.text.clone()));
    }
    out
}

/// The logical key a character is on (the user's layout decides which key types it).
pub fn key_for_char(c: char) -> Option<egui::Key> {
    use egui::Key;
    Some(match c.to_ascii_uppercase() {
        'A' => Key::A,
        'B' => Key::B,
        'C' => Key::C,
        'D' => Key::D,
        'E' => Key::E,
        'F' => Key::F,
        'G' => Key::G,
        'H' => Key::H,
        'I' => Key::I,
        'J' => Key::J,
        'K' => Key::K,
        'L' => Key::L,
        'M' => Key::M,
        'N' => Key::N,
        'O' => Key::O,
        'P' => Key::P,
        'Q' => Key::Q,
        'R' => Key::R,
        'S' => Key::S,
        'T' => Key::T,
        'U' => Key::U,
        'V' => Key::V,
        'W' => Key::W,
        'X' => Key::X,
        'Y' => Key::Y,
        'Z' => Key::Z,
        '0' => Key::Num0,
        '1' => Key::Num1,
        '2' => Key::Num2,
        '3' => Key::Num3,
        '4' => Key::Num4,
        '5' => Key::Num5,
        '6' => Key::Num6,
        '7' => Key::Num7,
        '8' => Key::Num8,
        '9' => Key::Num9,
        ' ' => Key::Space,
        '-' => Key::Minus,
        '+' => Key::Plus,
        '=' => Key::Equals,
        ',' => Key::Comma,
        '.' => Key::Period,
        '/' => Key::Slash,
        '\\' => Key::Backslash,
        ';' => Key::Semicolon,
        ':' => Key::Colon,
        '\'' => Key::Quote,
        '`' => Key::Backtick,
        '[' => Key::OpenBracket,
        ']' => Key::CloseBracket,
        '{' => Key::OpenCurlyBracket,
        '}' => Key::CloseCurlyBracket,
        '|' => Key::Pipe,
        '?' => Key::Questionmark,
        '!' => Key::Exclamationmark,
        _ => return None,
    })
}

/// The key at a position, by Android keycode (US names, as egui's physical keys are).
pub fn physical_key(keycode: i32) -> Option<egui::Key> {
    use egui::Key;
    const LETTERS: [Key; 26] = [
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::H,
        Key::I,
        Key::J,
        Key::K,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
        Key::Z,
    ];
    const DIGITS: [Key; 10] =
        [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
    const FUNCTION: [Key; 12] = [Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12];
    let index = |first: i32| usize::try_from(keycode - first).ok();
    Some(match keycode {
        7..=16 => *DIGITS.get(index(7)?)?,
        29..=54 => *LETTERS.get(index(29)?)?,
        131..=142 => *FUNCTION.get(index(131)?)?,
        144..=153 => *DIGITS.get(index(144)?)?, // the numeric keypad
        19 => Key::ArrowUp,
        20 => Key::ArrowDown,
        21 => Key::ArrowLeft,
        22 => Key::ArrowRight,
        55 => Key::Comma,
        56 => Key::Period,
        61 => Key::Tab,
        62 => Key::Space,
        66 | 160 => Key::Enter,
        67 => Key::Backspace,
        68 => Key::Backtick,
        69 | 156 => Key::Minus,
        70 | 161 => Key::Equals,
        71 => Key::OpenBracket,
        72 => Key::CloseBracket,
        73 => Key::Backslash,
        74 => Key::Semicolon,
        75 => Key::Quote,
        76 | 154 => Key::Slash,
        81 | 157 => Key::Plus,
        92 => Key::PageUp,
        93 => Key::PageDown,
        111 => Key::Escape,
        112 => Key::Delete,
        122 => Key::Home,
        123 => Key::End,
        124 => Key::Insert,
        158 => Key::Period,
        277 => Key::Cut,
        278 => Key::Copy,
        279 => Key::Paste,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(keycode: i32, base: char, text: &str, meta: u32) -> KeyPress {
        KeyPress { keycode, base: base as u32, text: text.into(), pressed: true, repeat: false, meta }
    }

    #[test]
    fn a_letter_is_a_key_and_its_text() {
        let out = to_egui(&key(29, 'a', "a", 0));
        assert!(matches!(out[0], egui::Event::Key { key: egui::Key::A, physical_key: Some(egui::Key::A), pressed: true, .. }));
        assert_eq!(out[1], egui::Event::Text("a".into()));
    }

    #[test]
    fn ctrl_shortcuts_type_nothing() {
        let out = to_egui(&key(43, 'o', "o", 0x1000));
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], egui::Event::Key { key: egui::Key::O, modifiers, .. } if modifiers.command));
    }

    #[test]
    fn the_layout_decides_the_logical_key() {
        // AZERTY: the key in QWERTY's Q position types 'a'.
        let out = to_egui(&key(45, 'a', "a", 0));
        assert!(matches!(out[0], egui::Event::Key { key: egui::Key::A, physical_key: Some(egui::Key::Q), .. }));
    }

    #[test]
    fn accents_and_symbols_without_a_key_still_type() {
        // A dead key's result (´ then a) and ñ have no egui key, but are typed.
        let out = to_egui(&key(29, 'a', "á", 0));
        assert_eq!(out[1], egui::Event::Text("á".into()));
        let enye = to_egui(&key(74, 'ñ', "ñ", 0));
        assert!(matches!(enye[0], egui::Event::Key { key: egui::Key::Semicolon, .. }));
        assert_eq!(enye[1], egui::Event::Text("ñ".into()));
        // AltGr+2 on a Spanish keyboard
        let at = to_egui(&key(9, '2', "@", 0x02 | 0x20));
        assert_eq!(at[1], egui::Event::Text("@".into()));
    }

    #[test]
    fn pressing_ctrl_tells_egui_once() {
        let mut held = egui::Modifiers::NONE;
        let ctrl = with_modifiers(&key(113, '\0', "", 0x1000 | 0x2000), &mut held);
        assert!(matches!(ctrl[0], egui::Event::ModifiersChanged(m) if m.ctrl));
        let o = with_modifiers(&key(43, 'o', "o", 0x1000 | 0x2000), &mut held);
        assert!(matches!(o[0], egui::Event::Key { key: egui::Key::O, .. }), "no repeat of ModifiersChanged");
        let up = with_modifiers(&KeyPress { pressed: false, ..key(113, '\0', "", 0) }, &mut held);
        assert!(matches!(up[0], egui::Event::ModifiersChanged(m) if !m.ctrl));
    }

    #[test]
    fn special_keys_are_keys_not_text() {
        let enter = to_egui(&key(66, '\n', "\n", 0));
        assert_eq!(enter.len(), 1);
        assert!(matches!(enter[0], egui::Event::Key { key: egui::Key::Enter, .. }));
        let release = to_egui(&KeyPress { pressed: false, ..key(29, 'a', "a", 0) });
        assert_eq!(release.len(), 1);
        assert!(matches!(physical_key(131), Some(egui::Key::F1)));
        assert!(matches!(physical_key(93), Some(egui::Key::PageDown)));
        assert_eq!(physical_key(-5), None);
    }
}
