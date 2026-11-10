//! Printing through Android's print service: a PDF the app has composed (its own print dialog's
//! pages, scaling, n-up…) goes to the system print dialog, where the user picks the printer
//! (Wi-Fi, Mopria, the manufacturer's service, Save as PDF), copies and two-sided printing.
//!
//! PdfCraft hands it its spooler's PDF (see DexPdfCraft's `android/print.rs`); an app that prints
//! by drawing pages can write them to a PDF first.

use std::path::Path;

use serde_json::json;

/// Open Android's print dialog with `pdf`, written to a file in `dir` (removed once the system
/// has read it). `title`: the job's name.
pub fn print_pdf(pdf: &[u8], title: &str, dir: &Path) -> Result<String, String> {
    if !super::bridge::available() {
        return Err("printing is not available yet".to_owned());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let path = dir.join(format!("print-{stamp}.pdf"));
    std::fs::write(&path, pdf).map_err(|e| e.to_string())?;
    let title = if title.trim().is_empty() { "Document" } else { title };
    super::diag(&format!("print: {} bytes", pdf.len()));
    super::bridge::call("print", &json!({ "path": path.to_string_lossy(), "title": title }).to_string(), None);
    Ok("Sent to Android's print dialog".to_owned())
}
