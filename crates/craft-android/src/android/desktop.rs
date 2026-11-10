//! What a desktop window shows that egui asks winit for, and winit ignores on Android: the
//! mouse cursor's shape and the window's title. Sent to CraftActivity only when they change.

use std::sync::{Mutex, PoisonError};

use serde_json::json;

static CURSOR: Mutex<Option<i32>> = Mutex::new(None);
static TITLE: Mutex<Option<String>> = Mutex::new(None);

/// The cursor egui wants this frame.
pub fn set_cursor(icon: egui::CursorIcon) {
    let kind = crate::pointer::pointer_icon_type(icon);
    let mut last = CURSOR.lock().unwrap_or_else(PoisonError::into_inner);
    if *last != Some(kind) && super::bridge::available() {
        *last = Some(kind);
        super::bridge::call("cursor", &json!({ "type": kind }).to_string(), None);
    }
}

/// The window's title: the activity's title and the task's label (DeX's taskbar shows it).
pub fn set_title(title: &str) {
    let mut last = TITLE.lock().unwrap_or_else(PoisonError::into_inner);
    if last.as_deref() != Some(title) && super::bridge::available() {
        *last = Some(title.to_owned());
        super::bridge::call("title", &json!({ "title": title }).to_string(), None);
    }
}

/// Open a link with the app that handles it (only web, mail and phone links: a document's link
/// to anything else, such as a file on the device, is not followed).
pub fn open_url(url: &str) {
    let lower = url.trim().to_ascii_lowercase();
    if ["https://", "http://", "mailto:", "tel:"].iter().any(|scheme| lower.starts_with(scheme)) {
        super::bridge::call("open_url", &json!({ "url": url.trim() }).to_string(), None);
    } else {
        super::diag("a link with an unsupported scheme was not opened");
    }
}
