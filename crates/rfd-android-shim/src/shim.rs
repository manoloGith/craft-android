//! The Android implementation (see the crate documentation).

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::OnceLock;

/// A future the backend answers with.
pub type Answer<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Which picker to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    OpenFile,
    OpenFiles,
    Folder,
    Folders,
    Save,
}

/// Everything a dialog was given.
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub title: Option<String>,
    /// (name, extensions without the dot)
    pub filters: Vec<(String, Vec<String>)>,
    pub file_name: Option<String>,
    pub directory: Option<PathBuf>,
    pub can_create_directories: bool,
}

/// A message box (rfd's `MessageDialog`).
#[derive(Clone, Debug, Default)]
pub struct Message {
    pub title: String,
    pub description: String,
    pub level: MessageLevel,
    pub buttons: MessageButtons,
}

/// What the app installs: the platform's pickers and message boxes.
pub trait Backend: Send + Sync {
    /// Show the picker and answer with the chosen files as local paths (empty when cancelled).
    fn pick(&self, kind: Kind, request: Request) -> Answer<Vec<PathBuf>>;
    /// Show a message box; `true` when the user accepted it.
    fn message(&self, message: Message) -> Answer<MessageDialogResult> {
        log::warn!("rfd shim: no message box on this platform: {}: {}", message.title, message.description);
        Box::pin(async { MessageDialogResult::Cancel })
    }
}

static BACKEND: OnceLock<Box<dyn Backend>> = OnceLock::new();

/// Install the platform's pickers (once, at start-up). Returns false if one was already installed.
pub fn set_backend(backend: Box<dyn Backend>) -> bool {
    BACKEND.set(backend).is_ok()
}

fn pick(kind: Kind, request: Request) -> Answer<Vec<PathBuf>> {
    match BACKEND.get() {
        Some(b) => b.pick(kind, request),
        None => {
            log::warn!("rfd shim: no file picker installed; {kind:?} is cancelled");
            Box::pin(async { Vec::new() })
        }
    }
}

/// A chosen file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHandle(PathBuf);

impl FileHandle {
    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn file_name(&self) -> String {
        self.0.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    pub async fn read(&self) -> Vec<u8> {
        std::fs::read(&self.0).unwrap_or_default()
    }

    pub async fn write(&self, data: &[u8]) -> std::io::Result<()> {
        std::fs::write(&self.0, data)
    }

    pub fn inner(&self) -> &Path {
        &self.0
    }
}

impl From<PathBuf> for FileHandle {
    fn from(p: PathBuf) -> Self {
        FileHandle(p)
    }
}

impl From<&Path> for FileHandle {
    fn from(p: &Path) -> Self {
        FileHandle(p.to_path_buf())
    }
}

/// rfd's `AsyncFileDialog`.
#[derive(Clone, Debug, Default)]
pub struct AsyncFileDialog {
    request: Request,
}

impl AsyncFileDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_filter(mut self, name: impl Into<String>, extensions: &[impl ToString]) -> Self {
        self.request.filters.push((name.into(), extensions.iter().map(ToString::to_string).collect()));
        self
    }

    pub fn set_directory<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.request.directory = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn set_file_name(mut self, file_name: impl Into<String>) -> Self {
        self.request.file_name = Some(file_name.into());
        self
    }

    pub fn set_title(mut self, title: impl Into<String>) -> Self {
        self.request.title = Some(title.into());
        self
    }

    pub fn set_can_create_directories(mut self, can: bool) -> Self {
        self.request.can_create_directories = can;
        self
    }

    /// Accepted and ignored: Android pickers have no parent window.
    pub fn set_parent<W>(self, _parent: &W) -> Self {
        self
    }

    /// The request this dialog describes.
    pub fn request(&self) -> &Request {
        &self.request
    }

    pub fn pick_file(self) -> impl Future<Output = Option<FileHandle>> {
        let answer = pick(Kind::OpenFile, self.request);
        async move { answer.await.into_iter().next().map(FileHandle) }
    }

    pub fn pick_files(self) -> impl Future<Output = Option<Vec<FileHandle>>> {
        let answer = pick(Kind::OpenFiles, self.request);
        async move { Some(answer.await.into_iter().map(FileHandle).collect::<Vec<_>>()).filter(|v| !v.is_empty()) }
    }

    pub fn pick_folder(self) -> impl Future<Output = Option<FileHandle>> {
        let answer = pick(Kind::Folder, self.request);
        async move { answer.await.into_iter().next().map(FileHandle) }
    }

    pub fn pick_folders(self) -> impl Future<Output = Option<Vec<FileHandle>>> {
        let answer = pick(Kind::Folders, self.request);
        async move { Some(answer.await.into_iter().map(FileHandle).collect::<Vec<_>>()).filter(|v| !v.is_empty()) }
    }

    pub fn save_file(self) -> impl Future<Output = Option<FileHandle>> {
        let answer = pick(Kind::Save, self.request);
        async move { answer.await.into_iter().next().map(FileHandle) }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MessageLevel {
    #[default]
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MessageButtons {
    #[default]
    Ok,
    OkCancel,
    YesNo,
    YesNoCancel,
    OkCustom(String),
    OkCancelCustom(String, String),
    YesNoCancelCustom(String, String, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageDialogResult {
    Yes,
    No,
    Ok,
    Cancel,
    Custom(String),
}

/// rfd's `AsyncMessageDialog`.
#[derive(Clone, Debug, Default)]
pub struct AsyncMessageDialog {
    message: Message,
}

impl AsyncMessageDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_level(mut self, level: MessageLevel) -> Self {
        self.message.level = level;
        self
    }

    pub fn set_title(mut self, title: impl Into<String>) -> Self {
        self.message.title = title.into();
        self
    }

    pub fn set_description(mut self, text: impl Into<String>) -> Self {
        self.message.description = text.into();
        self
    }

    pub fn set_buttons(mut self, buttons: MessageButtons) -> Self {
        self.message.buttons = buttons;
        self
    }

    pub fn set_parent<W>(self, _parent: &W) -> Self {
        self
    }

    pub fn show(self) -> impl Future<Output = MessageDialogResult> {
        let answer = match BACKEND.get() {
            Some(b) => b.message(self.message),
            None => Box::pin(async { MessageDialogResult::Cancel }) as Answer<MessageDialogResult>,
        };
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_backend_every_picker_is_cancelled() {
        let dialog = AsyncFileDialog::new().set_title("t").add_filter("PDF", &["pdf"]).set_file_name("a.pdf");
        assert_eq!(dialog.request().filters, vec![("PDF".to_owned(), vec!["pdf".to_owned()])]);
        let none = futures_lite_block_on(dialog.save_file());
        assert!(none.is_none());
    }

    /// A minimal executor for the test (the futures here never wait).
    fn futures_lite_block_on<F: Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        let mut f = std::pin::pin!(f);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }
}
