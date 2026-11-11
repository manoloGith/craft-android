//! Copy, cut and paste with the system clipboard, which egui-winit doesn't reach on Android.
//!
//! On the desktop egui-winit turns Ctrl+C, X and V into `Event::Copy`, `Cut` and `Paste` (with the
//! clipboard's text) instead of key events, and puts the text egui copies on the clipboard. On
//! Android its clipboard is simulated and the keys come from `CraftActivity` ([`crate::keyboard`]),
//! so [`crate::Shell`] does both with these functions and `android::clipboard`. Platform
//! independent, tested on any machine.

/// Replace each press of Ctrl+C, X and V (or the Copy, Cut and Paste keys) with the event
/// egui-winit makes of it. `clipboard_text`: the system clipboard's text, read only for a paste.
pub fn keys_to_clipboard_events(
    events: &mut Vec<egui::Event>,
    mut clipboard_text: impl FnMut() -> Option<String>,
) {
    let mut out = Vec::with_capacity(events.len());
    for event in events.drain(..) {
        let egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = &event
        else {
            out.push(event);
            continue;
        };
        let command = modifiers.command_only();
        match key {
            egui::Key::Cut => out.push(egui::Event::Cut),
            egui::Key::X if command => out.push(egui::Event::Cut),
            egui::Key::Copy => out.push(egui::Event::Copy),
            egui::Key::C if command => out.push(egui::Event::Copy),
            egui::Key::Paste => out.push(egui::Event::Paste(clipboard_text().unwrap_or_default())),
            egui::Key::V if command => {
                out.push(egui::Event::Paste(clipboard_text().unwrap_or_default()))
            }
            _ => out.push(event),
        }
    }
    *events = out;
}

/// The text egui wants on the clipboard this frame (`OutputCommand::CopyText`), taken out of
/// its output so nothing else handles it.
pub fn take_copied_text(ctx: &egui::Context) -> Vec<String> {
    ctx.output_mut(|o| {
        let mut texts = Vec::new();
        o.commands.retain(|c| match c {
            egui::OutputCommand::CopyText(text) => {
                texts.push(text.clone());
                false
            }
            _ => true,
        });
        texts
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn ctrl_c_x_v_become_copy_cut_paste() {
        let mut events = vec![
            key(egui::Key::C, egui::Modifiers::COMMAND),
            key(egui::Key::X, egui::Modifiers::COMMAND),
            key(egui::Key::V, egui::Modifiers::COMMAND),
        ];
        keys_to_clipboard_events(&mut events, || Some("a\tb".into()));
        assert_eq!(
            events,
            vec![
                egui::Event::Copy,
                egui::Event::Cut,
                egui::Event::Paste("a\tb".into())
            ]
        );
    }

    #[test]
    fn other_keys_and_shortcuts_stay() {
        let plain = key(egui::Key::C, egui::Modifiers::NONE);
        let shifted = key(
            egui::Key::V,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        );
        let mut events = vec![
            plain.clone(),
            shifted.clone(),
            egui::Event::Text("c".into()),
        ];
        let mut asked = false;
        keys_to_clipboard_events(&mut events, || {
            asked = true;
            None
        });
        assert_eq!(events, vec![plain, shifted, egui::Event::Text("c".into())]);
        assert!(!asked, "the clipboard is read only for a paste");
    }

    #[test]
    fn an_empty_clipboard_pastes_nothing() {
        let mut events = vec![key(egui::Key::Paste, egui::Modifiers::NONE)];
        keys_to_clipboard_events(&mut events, || None);
        assert_eq!(events, vec![egui::Event::Paste(String::new())]);
    }
}
