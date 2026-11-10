//! Events from the activity, and what the interface polls for them.

use std::sync::{Mutex, OnceLock, PoisonError};

/// The interface's context, to wake it when something arrives.
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();
/// Documents opened with the app ("Open with", "Share") not yet handed to the interface.
static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Called once the interface exists.
pub fn set_context(ctx: egui::Context) {
    let _ = CONTEXT.set(ctx);
}

/// The documents opened with the app ("Open with", "Share", a drop) since the last call, as
/// local paths: hand them to the app's "open files" (PdfCraft: `OsEvent::Open` from `os_events`).
pub fn opened() -> Vec<String> {
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Wake the interface (something was queued for its next frame).
pub fn wake() {
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

pub fn dispatch(op: &str, json: &str) {
    match op {
        "picked" => match crate::protocol::parse_picked(json) {
            Some((id, paths)) => super::pickers::complete(id, paths),
            None => log::warn!("craft-android: malformed picked event"),
        },
        "open" => {
            let paths = crate::protocol::parse_open(json);
            log::info!("craft-android: {} document(s) opened with the app", paths.len());
            OPENED.lock().unwrap_or_else(PoisonError::into_inner).extend(paths);
            if let Some(ctx) = CONTEXT.get() {
                ctx.request_repaint();
            }
        }
        // Back: the app decides (it may ask about unsaved changes first).
        "close" => crate::shell::request_close(CONTEXT.get()),
        _ => log::warn!("craft-android: unknown event {op}"),
    }
}
