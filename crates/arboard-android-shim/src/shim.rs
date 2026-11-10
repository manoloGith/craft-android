//! The Android implementation (see the crate documentation).

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

/// What can be on the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Text(String),
    /// HTML with its plain-text alternative.
    Html { html: String, alt: Option<String> },
    /// RGBA8 pixels.
    Image { width: usize, height: usize, bytes: Vec<u8> },
    Files(Vec<PathBuf>),
}

/// What the app installs: the system clipboard.
pub trait Backend: Send + Sync {
    fn get(&self) -> Option<Content>;
    fn set(&self, content: Content) -> Result<(), String>;
}

static BACKEND: OnceLock<Box<dyn Backend>> = OnceLock::new();
static LOCAL: Mutex<Option<Content>> = Mutex::new(None);

/// Install the system clipboard (once, at start-up). Returns false if one was already installed.
pub fn set_backend(backend: Box<dyn Backend>) -> bool {
    BACKEND.set(backend).is_ok()
}

fn get() -> Option<Content> {
    match BACKEND.get() {
        Some(b) => b.get(),
        None => LOCAL.lock().unwrap_or_else(PoisonError::into_inner).clone(),
    }
}

fn set(content: Content) -> Result<(), Error> {
    match BACKEND.get() {
        Some(b) => b.set(content).map_err(Error::Unknown),
        None => {
            *LOCAL.lock().unwrap_or_else(PoisonError::into_inner) = Some(content);
            Ok(())
        }
    }
}

/// arboard's error type (the variants PdfCraft can meet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    ContentNotAvailable,
    ClipboardNotSupported,
    ConversionFailure,
    Unknown(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ContentNotAvailable => f.write_str("the clipboard contents were not available in the requested format"),
            Error::ClipboardNotSupported => f.write_str("the clipboard is not supported here"),
            Error::ConversionFailure => f.write_str("the clipboard contents could not be converted"),
            Error::Unknown(e) => write!(f, "clipboard error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// arboard's image: RGBA8, row-major.
#[derive(Debug, Clone)]
pub struct ImageData<'a> {
    pub width: usize,
    pub height: usize,
    pub bytes: Cow<'a, [u8]>,
}

impl ImageData<'_> {
    pub fn to_owned_img(&self) -> ImageData<'static> {
        ImageData { width: self.width, height: self.height, bytes: Cow::Owned(self.bytes.to_vec()) }
    }
}

/// arboard's `Clipboard`.
#[derive(Debug)]
pub struct Clipboard(());

impl Clipboard {
    pub fn new() -> Result<Self, Error> {
        Ok(Clipboard(()))
    }

    pub fn get_text(&mut self) -> Result<String, Error> {
        match get() {
            Some(Content::Text(t)) => Ok(t),
            Some(Content::Html { alt: Some(t), .. }) => Ok(t),
            _ => Err(Error::ContentNotAvailable),
        }
    }

    pub fn set_text<'a, T: Into<Cow<'a, str>>>(&mut self, text: T) -> Result<(), Error> {
        set(Content::Text(text.into().into_owned()))
    }

    pub fn get_image(&mut self) -> Result<ImageData<'static>, Error> {
        match get() {
            Some(Content::Image { width, height, bytes }) => Ok(ImageData { width, height, bytes: Cow::Owned(bytes) }),
            _ => Err(Error::ContentNotAvailable),
        }
    }

    pub fn set_image(&mut self, image: ImageData<'_>) -> Result<(), Error> {
        set(Content::Image { width: image.width, height: image.height, bytes: image.bytes.into_owned() })
    }

    pub fn clear(&mut self) -> Result<(), Error> {
        set(Content::Text(String::new()))
    }

    pub fn get(&mut self) -> Get<'_> {
        Get { _clipboard: self }
    }

    pub fn set(&mut self) -> Set<'_> {
        Set { _clipboard: self }
    }
}

/// arboard's `Get` builder.
pub struct Get<'c> {
    _clipboard: &'c mut Clipboard,
}

impl Get<'_> {
    pub fn text(self) -> Result<String, Error> {
        Clipboard(()).get_text()
    }

    pub fn image(self) -> Result<ImageData<'static>, Error> {
        Clipboard(()).get_image()
    }

    pub fn file_list(self) -> Result<Vec<PathBuf>, Error> {
        match get() {
            Some(Content::Files(f)) => Ok(f),
            _ => Err(Error::ContentNotAvailable),
        }
    }
}

/// arboard's `Set` builder.
pub struct Set<'c> {
    _clipboard: &'c mut Clipboard,
}

impl Set<'_> {
    pub fn text<'a, T: Into<Cow<'a, str>>>(self, text: T) -> Result<(), Error> {
        set(Content::Text(text.into().into_owned()))
    }

    pub fn html<'a, T: Into<Cow<'a, str>>>(self, html: T, alt_text: Option<T>) -> Result<(), Error> {
        set(Content::Html { html: html.into().into_owned(), alt: alt_text.map(|t| t.into().into_owned()) })
    }

    pub fn image(self, image: ImageData<'_>) -> Result<(), Error> {
        Clipboard(()).set_image(image)
    }

    pub fn file_list(self, files: &[impl AsRef<Path>]) -> Result<(), Error> {
        set(Content::Files(files.iter().map(|p| p.as_ref().to_path_buf()).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_backend_copy_and_paste_stay_in_the_process() {
        let mut c = Clipboard::new().unwrap_or_else(|_| unreachable!());
        assert!(c.set().html("<b>x</b>", Some("x")).is_ok());
        assert_eq!(c.get_text().ok().as_deref(), Some("x"));
        assert!(c.set_image(ImageData { width: 1, height: 1, bytes: Cow::Borrowed(&[1, 2, 3, 4]) }).is_ok());
        assert_eq!(c.get_image().map(|i| i.bytes.into_owned()).ok(), Some(vec![1, 2, 3, 4]));
        assert!(c.get_text().is_err());
    }
}
