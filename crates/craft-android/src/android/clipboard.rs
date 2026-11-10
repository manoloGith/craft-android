//! Android's clipboard, through CraftActivity: text, HTML with its plain alternative, RGBA8
//! images (shared as PNG through the app's FileProvider) and file lists.
//!
//! Apps whose interface uses `arboard` connect it with the arboard shim
//! (`crates/arboard-android-shim`): a `arboard::Backend` calling [`get`] and [`set`].

use std::path::PathBuf;

use serde_json::json;

use crate::protocol::{Clip as Answer, decode_image, parse_clip};

/// What is (or goes) on the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clip {
    Text(String),
    Html { html: String, text: String },
    /// RGBA8, row-major.
    Image { width: usize, height: usize, rgba: Vec<u8> },
    Files(Vec<PathBuf>),
}

/// The clipboard's content, if it holds something the apps use.
pub fn get() -> Option<Clip> {
    let answer = super::bridge::call("clip_get", "", None)?;
    match parse_clip(&answer) {
        Answer::None => None,
        Answer::Text(text) => Some(Clip::Text(text)),
        Answer::Html { html, text } => Some(Clip::Html { html, text }),
        Answer::Image => {
            let (width, height, rgba) = decode_image(&super::bridge::call("clip_image", "", None)?)?;
            Some(Clip::Image { width, height, rgba })
        }
    }
}

/// Put `clip` on the clipboard.
pub fn set(clip: Clip) -> Result<(), String> {
    if !super::bridge::available() {
        return Err("the clipboard is not available yet".into());
    }
    match clip {
        Clip::Text(text) => super::bridge::call("clip_text", &json!({ "text": text }).to_string(), None),
        Clip::Html { html, text } => super::bridge::call("clip_html", &json!({ "html": html, "text": text }).to_string(), None),
        Clip::Image { width, height, rgba } => {
            super::bridge::call("clip_set_image", &json!({ "width": width, "height": height }).to_string(), Some(&rgba))
        }
        Clip::Files(paths) => {
            let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            super::bridge::call("clip_files", &json!({ "paths": paths }).to_string(), None)
        }
    };
    Ok(())
}
