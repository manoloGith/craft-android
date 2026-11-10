//! The on-screen keyboard on Android, from MainActivity (`nativeIme`), into egui.
//!
//! GameActivity gives the on-screen keyboard an edit buffer of its own (GameTextInput) and reports
//! each change as the buffer's whole state: its text and the region being composed (the word a
//! predictive keyboard is still writing, underlined). winit drops these, so nothing typed on the
//! phone reached the interface. Each state is compared here with the last one: text that became
//! final is typed (after Backspaces for what was deleted), and the word being composed is shown
//! as egui's pre-edit. MainActivity empties the buffer once nothing is being composed, so the
//! keyboard never reaches back into words already typed. Platform independent so it is tested
//! anywhere; on other systems nothing is queued.

use std::sync::{Mutex, PoisonError};

/// What has been told to egui about the keyboard's buffer.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ime {
    /// The buffer's final text (outside the composing region), as typed so far.
    committed: String,
    /// The word being composed, as shown in egui's pre-edit.
    preedit: String,
}

/// UTF-16 offset (as Java reports positions) to a char index of `s`, clamped.
fn char_index(s: &str, utf16: i32) -> usize {
    let Ok(target) = usize::try_from(utf16) else { return 0 };
    let mut units = 0;
    for (i, c) in s.chars().enumerate() {
        if units >= target {
            return i;
        }
        units += c.len_utf16();
    }
    s.chars().count()
}

fn key(key: egui::Key, pressed: bool) -> egui::Event {
    egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: egui::Modifiers::NONE }
}

/// Typing `text`: characters as text, line breaks as Enter.
fn typed(text: &str, out: &mut Vec<egui::Event>) {
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push(key(egui::Key::Enter, true));
            out.push(key(egui::Key::Enter, false));
        }
        if !line.is_empty() {
            out.push(egui::Event::Text(line.to_owned()));
        }
    }
}

impl Ime {
    /// The keyboard's buffer is now `text`, composing `[start, end)` (UTF-16, negative: none).
    pub fn state(&mut self, text: &str, start: i32, end: i32) -> Vec<egui::Event> {
        let chars: Vec<char> = text.chars().collect();
        let (cs, ce) = if start >= 0 && end > start {
            let (a, b) = (char_index(text, start), char_index(text, end));
            (a.min(chars.len()), b.min(chars.len()))
        } else {
            (chars.len(), chars.len())
        };
        let committed: String = chars.iter().take(cs).chain(chars.iter().skip(ce)).collect();
        let preedit: String = chars.get(cs..ce).map(|c| c.iter().collect()).unwrap_or_default();

        let mut out = Vec::new();
        if committed != self.committed {
            // A word stops being composed (or the text before it changes): end the pre-edit
            // first, so typing goes where the cursor is.
            if !self.preedit.is_empty() {
                out.push(egui::Event::Ime(egui::ImeEvent::Preedit { text: String::new(), active_range_chars: None }));
                self.preedit.clear();
            }
            let common = self.committed.chars().zip(committed.chars()).take_while(|(a, b)| a == b).count();
            let deleted = self.committed.chars().count() - common;
            for _ in 0..deleted {
                out.push(key(egui::Key::Backspace, true));
                out.push(key(egui::Key::Backspace, false));
            }
            let inserted: String = committed.chars().skip(common).collect();
            typed(&inserted, &mut out);
            self.committed = committed;
        }
        if preedit != self.preedit {
            out.push(egui::Event::Ime(egui::ImeEvent::Preedit { text: preedit.clone(), active_range_chars: None }));
            self.preedit = preedit;
        }
        out
    }

    /// MainActivity emptied the buffer: nothing to tell egui.
    pub fn reset(&mut self) {
        *self = Ime::default();
    }
}

static IME: Mutex<Ime> = Mutex::new(Ime { committed: String::new(), preedit: String::new() });
static QUEUE: Mutex<Vec<egui::Event>> = Mutex::new(Vec::new());

/// A new buffer state from the activity (`reset`: it emptied the buffer itself).
pub fn push(text: &str, start: i32, end: i32, reset: bool) {
    let mut ime = IME.lock().unwrap_or_else(PoisonError::into_inner);
    if reset {
        ime.reset();
        return;
    }
    let events = ime.state(text, start, end);
    QUEUE.lock().unwrap_or_else(PoisonError::into_inner).extend(events);
}

pub fn drain() -> Vec<egui::Event> {
    std::mem::take(&mut *QUEUE.lock().unwrap_or_else(PoisonError::into_inner))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(events: &[egui::Event]) -> Vec<String> {
        events
            .iter()
            .map(|e| match e {
                egui::Event::Text(t) => format!("text {t}"),
                egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => format!("preedit {text}"),
                egui::Event::Key { key, pressed: true, .. } => format!("key {key:?}"),
                _ => String::new(),
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    #[test]
    fn a_predicted_word_is_shown_while_composed_then_typed() {
        let mut ime = Ime::default();
        assert_eq!(texts(&ime.state("h", 0, 1)), ["preedit h"]);
        assert_eq!(texts(&ime.state("hol", 0, 3)), ["preedit hol"]);
        assert_eq!(texts(&ime.state("hola ", -1, -1)), ["preedit ", "text hola "]);
        ime.reset();
        assert_eq!(texts(&ime.state("y", 0, 1)), ["preedit y"]);
    }

    #[test]
    fn plain_typing_and_deleting() {
        let mut ime = Ime::default();
        assert_eq!(texts(&ime.state("ab", -1, -1)), ["text ab"]);
        assert_eq!(texts(&ime.state("a", -1, -1)), ["key Backspace"]);
        assert_eq!(texts(&ime.state("añ", -1, -1)), ["text ñ"]);
    }

    #[test]
    fn line_breaks_are_enter() {
        let mut ime = Ime::default();
        assert_eq!(texts(&ime.state("a\nb", -1, -1)), ["text a", "key Enter", "text b"]);
    }

    #[test]
    fn positions_are_utf16() {
        // An emoji is two UTF-16 units: composing "x" after it is [2, 3).
        let mut ime = Ime::default();
        assert_eq!(texts(&ime.state("😀x", 2, 3)), ["text 😀", "preedit x"]);
        // The same state again tells egui nothing new.
        assert!(texts(&ime.state("😀x", 2, 3)).is_empty());
        // Out-of-range positions are clamped, never a panic.
        let mut fresh = Ime::default();
        assert_eq!(texts(&fresh.state("😀x", 2, 99)), ["text 😀", "preedit x"]);
        assert_eq!(texts(&Ime::default().state("ab", 5, 9)), ["text ab"]);
    }
}
