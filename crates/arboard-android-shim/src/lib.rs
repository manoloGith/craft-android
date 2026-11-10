//! The part of [arboard](https://docs.rs/arboard)'s API PdfCraft's interface uses, for Android,
//! where arboard has no backend. Reads and writes go to the [`Backend`] the app installed with
//! [`set_backend`] (Android's `ClipboardManager`, over JNI); without one, to a clipboard inside
//! the process, so copy and paste still work within the app.
#![forbid(unsafe_code)]

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub use arboard_real::*;

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
mod shim;
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub use shim::*;
