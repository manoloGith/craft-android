//! **craft-android**: the Android layer for storytold's Crafting Apps (Rust + egui/eframe), for
//! Samsung DeX, Android's desktop windows, tablets and phones.
//!
//! Extracted from DexPdfCraft (PdfCraft on Android). It holds what every such app needs and what
//! winit, eframe and GameActivity don't do on Android:
//!
//! - [`pointer`], [`keyboard`], [`ime`], [`insets`]: the mouse, the hardware keyboard, the
//!   on-screen keyboard and the system's insets, handed over by `CraftActivity` (Kotlin) and turned
//!   into egui input; [`clipboard_keys`] makes Ctrl+C, X and V copy, cut and paste with the
//!   system clipboard, as egui-winit does on the desktop. Platform independent, tested on any
//!   machine.
//! - [`tabs`]: a strip of document tabs for apps that show several documents in one window
//!   (the name, an unsaved dot, close and new buttons); the app does what was clicked.
//! - [`shell::Shell`]: wraps the app's `eframe::App`: injects that input, closes on Back the way a
//!   window's close button does (the app may ask about unsaved changes), autosaves when the window
//!   loses focus, and on Android hands the cursor, title and links to the activity.
//! - `android` (Android only): `start` (environment, diagnostics, panics, assets), the JNI bridge
//!   to `CraftActivity`, the Storage Access Framework pickers, the clipboard, printing, "Open with".
//!
//! See `docs/integrate.md` in the repository.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub mod clipboard_keys;
pub mod ime;
pub mod insets;
pub mod keyboard;
pub mod pointer;
pub mod protocol;
pub mod shell;
pub mod tabs;

#[cfg(target_os = "android")]
pub mod android;

pub use shell::Shell;

/// The system language as a POSIX locale (`es_ES.UTF-8`), which the Crafting Apps' Auto
/// language reads from `LANG` as on Linux. `None` parts give `C.UTF-8` (English).
pub fn posix_locale(language: Option<&str>, country: Option<&str>) -> String {
    let clean = |s: &str| s.chars().filter(char::is_ascii_alphabetic).collect::<String>();
    match (language.map(clean).filter(|l| !l.is_empty()), country.map(clean).filter(|c| !c.is_empty())) {
        (Some(l), Some(c)) => format!("{}_{}.UTF-8", l.to_ascii_lowercase(), c.to_ascii_uppercase()),
        (Some(l), None) => format!("{}.UTF-8", l.to_ascii_lowercase()),
        _ => "C.UTF-8".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::posix_locale;

    #[test]
    fn the_android_locale_becomes_a_posix_one() {
        assert_eq!(posix_locale(Some("es"), Some("ES")), "es_ES.UTF-8");
        assert_eq!(posix_locale(Some("pt"), Some("br")), "pt_BR.UTF-8");
        assert_eq!(posix_locale(Some("de"), None), "de.UTF-8");
        assert_eq!(posix_locale(None, Some("US")), "C.UTF-8");
        assert_eq!(posix_locale(Some("e;s"), Some("")), "es.UTF-8");
    }
}
