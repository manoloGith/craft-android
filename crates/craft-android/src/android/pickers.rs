//! The Storage Access Framework's pickers, through CraftActivity, as local paths.
//!
//! A picker request gets an id and a channel; the activity shows the system picker, copies what
//! was chosen to local mirrors (DocumentStore.kt, which writes saves back to the documents) and
//! answers with a `picked` event carrying the id and the local paths. The future [`pick`] returns
//! is meant to be awaited on a worker thread (rfd-style pickers are): it blocks until the answer.
//!
//! Apps whose interface uses `rfd` connect it with the rfd shim (`crates/rfd-android-shim`):
//! a `rfd::Backend` whose `pick` calls [`pick`] (see `docs/integrate.md`).

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, PoisonError};

pub use crate::protocol::PickKind;
use crate::protocol::pick_request;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static WAITING: Mutex<Option<HashMap<u64, Sender<Vec<PathBuf>>>>> = Mutex::new(None);

/// What a picker answers: the chosen documents or folders as local paths (empty: cancelled).
pub type Picked = Pin<Box<dyn Future<Output = Vec<PathBuf>> + Send>>;

/// Show the system picker. `filters`: (name, extensions without the dot); `file_name`: Save's
/// suggested name. Call it on the UI thread; await the answer elsewhere.
pub fn pick(kind: PickKind, title: Option<&str>, filters: &[(String, Vec<String>)], file_name: Option<&str>) -> Picked {
    if !super::bridge::available() {
        log::warn!("craft-android: no activity for the picker");
        return Box::pin(async { Vec::new() });
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = channel();
    WAITING.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_with(HashMap::new).insert(id, tx);
    super::bridge::call("pick", &pick_request(id, kind, title, filters, file_name), None);
    Box::pin(async move { rx.recv().unwrap_or_default() })
}

/// The activity's answer to request `id`.
pub(crate) fn complete(id: u64, paths: Vec<PathBuf>) {
    let sender = WAITING.lock().unwrap_or_else(PoisonError::into_inner).as_mut().and_then(|w| w.remove(&id));
    match sender {
        Some(tx) => {
            let _ = tx.send(paths);
        }
        None => log::warn!("craft-android: an answer for unknown picker {id}"),
    }
}

/// A new activity: pickers the old one was showing will never be answered, so cancel them (the
/// apps show one picker at a time and would otherwise wait for ever).
pub(crate) fn cancel_all() {
    let waiting = WAITING.lock().unwrap_or_else(PoisonError::into_inner).take().unwrap_or_default();
    for (_, tx) in waiting {
        let _ = tx.send(Vec::new());
    }
}
