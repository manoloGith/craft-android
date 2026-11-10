//! The part of [rfd](https://docs.rs/rfd)'s API PdfCraft's interface uses, for Android, where rfd
//! has no backend. A dialog is a description (title, filters, file name, folder); showing it asks
//! the [`Backend`] the app installed with [`set_backend`], which shows the platform's picker (the
//! Storage Access Framework) and answers with local paths.
#![forbid(unsafe_code)]

#[cfg(not(target_os = "android"))]
pub use rfd_real::*;

#[cfg(target_os = "android")]
mod shim;
#[cfg(target_os = "android")]
pub use shim::*;
