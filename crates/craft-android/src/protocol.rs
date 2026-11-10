//! What crosses between the library and the Android activity (`MainActivity.bridge` and
//! `nativeEvent`): JSON text and byte arrays. Kept free of JNI and of Android types so it is
//! tested on any machine.

use std::path::PathBuf;

use serde_json::{Value, json};

/// The picker kinds, as `MainActivity` numbers them (the rfd shim's `Kind`, in order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    OpenFile = 0,
    OpenFiles = 1,
    Folder = 2,
    Folders = 3,
    Save = 4,
}

/// The `pick` request: `{id, kind, title, filters: [[name, [ext…]]…], file_name}`.
pub fn pick_request(id: u64, kind: PickKind, title: Option<&str>, filters: &[(String, Vec<String>)], file_name: Option<&str>) -> String {
    json!({
        "id": id,
        "kind": kind as i32,
        "title": title.unwrap_or_default(),
        "filters": filters.iter().map(|(name, exts)| json!([name, exts])).collect::<Vec<_>>(),
        "file_name": file_name.unwrap_or_default(),
    })
    .to_string()
}

/// A `picked` event: the request's id and the local paths chosen (empty when cancelled).
pub fn parse_picked(event: &str) -> Option<(u64, Vec<PathBuf>)> {
    let v: Value = serde_json::from_str(event).ok()?;
    let id = v.get("id")?.as_u64()?;
    Some((id, paths(&v).into_iter().map(PathBuf::from).collect()))
}

/// An `open` event ("Open with", "Share"): the local paths to open.
pub fn parse_open(event: &str) -> Vec<String> {
    serde_json::from_str::<Value>(event).map(|v| paths(&v)).unwrap_or_default()
}

fn paths(v: &Value) -> Vec<String> {
    v.get("paths")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter(|p| !p.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// What `clip_get` says is on the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clip {
    None,
    Text(String),
    Html { html: String, text: String },
    /// An image: fetch it with `clip_image`.
    Image,
}

pub fn parse_clip(bytes: &[u8]) -> Clip {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else { return Clip::None };
    let text = || v.get("text").and_then(Value::as_str).unwrap_or_default().to_owned();
    match v.get("kind").and_then(Value::as_str) {
        Some("text") => Clip::Text(text()),
        Some("html") => Clip::Html { html: v.get("html").and_then(Value::as_str).unwrap_or_default().to_owned(), text: text() },
        Some("image") => Clip::Image,
        _ => Clip::None,
    }
}

/// The largest clipboard image accepted (PdfCraft's own limit is 16384 px a side, 64 MiB).
const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

/// `clip_image`'s answer: width and height as big-endian `i32`, then RGBA8 pixels.
pub fn decode_image(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let header = bytes.get(..8)?;
    let w = usize::try_from(i32::from_be_bytes(header.get(..4)?.try_into().ok()?)).ok()?;
    let h = usize::try_from(i32::from_be_bytes(header.get(4..8)?.try_into().ok()?)).ok()?;
    let pixels = bytes.get(8..)?;
    let expected = w.checked_mul(h)?.checked_mul(4)?;
    (w > 0 && h > 0 && expected == pixels.len() && expected <= MAX_IMAGE_BYTES).then(|| (w, h, pixels.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_request_carries_everything_the_activity_reads() {
        let filters = vec![("PDF".to_owned(), vec!["pdf".to_owned()])];
        let v: Value = serde_json::from_str(&pick_request(7, PickKind::Save, Some("Save"), &filters, Some("a.pdf"))).unwrap_or_default();
        assert_eq!(v["id"], 7);
        assert_eq!(v["kind"], 4);
        assert_eq!(v["filters"][0][0], "PDF");
        assert_eq!(v["filters"][0][1][0], "pdf");
        assert_eq!(v["file_name"], "a.pdf");
    }

    #[test]
    fn picked_and_open_events_are_read_leniently() {
        assert_eq!(parse_picked(r#"{"id":3,"paths":["/a.pdf",""]}"#), Some((3, vec![PathBuf::from("/a.pdf")])));
        assert_eq!(parse_picked(r#"{"id":3}"#), Some((3, vec![])));
        assert_eq!(parse_picked("not json"), None);
        assert_eq!(parse_open(r#"{"paths":["/x.pdf",4]}"#), vec!["/x.pdf".to_owned()]);
        assert!(parse_open("{").is_empty());
    }

    #[test]
    fn clipboard_kinds() {
        assert_eq!(parse_clip(br#"{"kind":"text","text":"hola"}"#), Clip::Text("hola".into()));
        assert_eq!(parse_clip(br#"{"kind":"html","html":"<b>x</b>","text":"x"}"#), Clip::Html { html: "<b>x</b>".into(), text: "x".into() });
        assert_eq!(parse_clip(br#"{"kind":"image"}"#), Clip::Image);
        assert_eq!(parse_clip(b"garbage"), Clip::None);
    }

    #[test]
    fn images_are_checked_against_their_header() {
        let mut ok = vec![0, 0, 0, 1, 0, 0, 0, 2];
        ok.extend([1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(decode_image(&ok), Some((1, 2, vec![1, 2, 3, 4, 5, 6, 7, 8])));
        assert_eq!(decode_image(&ok[..12]), None, "short pixels");
        assert_eq!(decode_image(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 1]), None, "negative width");
        assert_eq!(decode_image(&[0, 0, 0, 0, 0, 0, 0, 0]), None, "empty");
        assert_eq!(decode_image(&[0x7f, 0xff, 0xff, 0xff, 0x7f, 0xff, 0xff, 0xff]), None, "overflow");
        assert_eq!(decode_image(&[0, 0]), None);
    }
}
