//! The Android side: [`start`] (the environment the apps expect, diagnostics, panics, assets),
//! the JNI bridge to `io.github.craftandroid.CraftActivity`, and the services the activity
//! provides: the Storage Access Framework pickers ([`pickers`]), the clipboard ([`clipboard`]),
//! printing ([`print`]), "Open with" ([`events`]), the cursor, title and links ([`desktop`]).

mod bridge;
pub mod clipboard;
pub mod desktop;
pub mod events;
pub mod pickers;
pub mod print;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub use winit::platform::android::activity::AndroidApp;
use winit::platform::android::activity::input::Axis;

/// `start` has run in this process. winit's event loop can be created once per process, so a
/// second activity in the same process can't start the window (CraftActivity ends the process
/// when its activity is destroyed, and restarts it in a fresh one when Android reuses it).
static STARTED: AtomicBool = AtomicBool::new(false);

/// What a panic leaves for the next start to show (CraftActivity reads it).
const CRASH_FILE: &str = "crash.txt";

/// The diagnostics log CraftActivity keeps (lifecycle, first mouse events and keys); the library
/// adds its own milestones, so a report from a device without adb tells what happened.
const DIAG_FILE: &str = "diag.log";
static DIAG: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// How an app starts on Android.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// The logcat tag.
    pub log_tag: &'static str,
    /// APK asset folders to copy to private storage (the first time, and when an update changed
    /// a file), each with the environment variable that names the copy for the app: OCR models,
    /// language data… e.g. `("models", "PDFCRAFT_MODELS")`.
    pub asset_dirs: Vec<(&'static str, &'static str)>,
}

/// What [`start`] set up.
#[derive(Clone, Debug)]
pub struct Started {
    /// The app's private storage (`HOME`).
    pub data_dir: PathBuf,
}

/// Append a line to the diagnostics log (and logcat).
pub fn diag(line: &str) {
    log::info!("craft-android: {line}");
    let Some(path) = DIAG.get() else { return };
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let _ = writeln!(f, "{now} rust: {line}");
    }
}

/// Call first thing in `android_main`, before anything starts a thread. Sets the environment the
/// Crafting Apps expect (HOME and XDG folders in private storage, LANG from the system), records
/// panics for the next start's report, copies the configured assets, and asks GameActivity for
/// the wheel's axes. `None`: `android_main` was already called in this process, which can't start
/// a window again; return at once (CraftActivity restarts the app in a fresh process).
pub fn start(app: &AndroidApp, config: &Config) -> Option<Started> {
    let tag = if config.log_tag.is_empty() { "craft-android" } else { config.log_tag };
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag(tag));
    let data = app.internal_data_path().unwrap_or_else(|| PathBuf::from("/data/local/tmp/craft-android"));
    let _ = DIAG.set(data.join(DIAG_FILE));
    record_panics(data.join(CRASH_FILE));
    if STARTED.swap(true, Ordering::SeqCst) {
        diag("android_main called again in the same process; not starting");
        return None;
    }
    diag(&format!("android_main starts ({tag})"));
    app.enable_motion_axis(Axis::Vscroll);
    app.enable_motion_axis(Axis::Hscroll);
    let configuration = app.config();
    let locale = crate::posix_locale(configuration.language().as_deref(), configuration.country().as_deref());
    set_environment(&data, &locale);
    for (asset_dir, variable) in &config.asset_dirs {
        if let Some(dir) = extract_asset_dir(app, asset_dir, &data.join(asset_dir)) {
            set_var(variable, dir.as_os_str());
        }
    }
    Some(Started { data_dir: data })
}

/// `options` for this activity's window (eframe needs the `AndroidApp`).
pub fn native_options(app: &AndroidApp, mut options: eframe::NativeOptions) -> eframe::NativeOptions {
    options.android_app = Some(app.clone());
    options
}

/// After eframe's loop has ended (the window closed, or Android destroyed the activity): finish
/// the activity. Its onDestroy ends the process.
pub fn finish() {
    diag("the event loop ended; finishing the activity");
    bridge::call("finish", "", None);
}

/// Show a short message from the system (a toast).
pub fn toast(text: &str) {
    bridge::call("toast", &serde_json::json!({ "text": text }).to_string(), None);
}

/// Every file of the APK's `assets/<asset_dir>`, copied to `dir` when missing or of another size
/// (an update changed it). The folder, if the APK has any such file.
fn extract_asset_dir(app: &AndroidApp, asset_dir: &str, dir: &Path) -> Option<PathBuf> {
    let assets = app.asset_manager();
    let name = std::ffi::CString::new(asset_dir).ok()?;
    let files: Vec<std::ffi::CString> = assets.open_dir(&name)?.collect();
    if files.is_empty() {
        return None;
    }
    std::fs::create_dir_all(dir).ok()?;
    for file in files {
        let Ok(file_name) = file.into_string() else { continue };
        let Ok(path) = std::ffi::CString::new(format!("{asset_dir}/{file_name}")) else { continue };
        let Some(mut asset) = assets.open(&path) else { continue };
        let target = dir.join(&file_name);
        if std::fs::metadata(&target).map(|m| m.len()).ok() == u64::try_from(asset.length()).ok() {
            continue;
        }
        let partial = dir.join(format!(".{file_name}.part"));
        let copied = std::fs::File::create(&partial)
            .and_then(|mut f| std::io::copy(&mut asset, &mut f))
            .and_then(|_| std::fs::rename(&partial, &target));
        match copied {
            Ok(()) => diag(&format!("{asset_dir}: {file_name} installed")),
            Err(e) => {
                let _ = std::fs::remove_file(&partial);
                diag(&format!("{asset_dir}: copying {file_name} failed: {e}"));
            }
        }
    }
    Some(dir.to_path_buf())
}

/// Write every panic (message, place, backtrace) to `file`, after the default hook has logged it,
/// so the next start can show it to the user (CraftActivity), who may have no adb.
fn record_panics(file: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let report = format!(
            "{info}\n\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        let _ = std::fs::write(&file, &report);
        diag(&format!("panic: {info}"));
    }));
}

/// The Crafting Apps keep settings, recovery data and its dictionary under `$HOME` and the XDG folders
/// on Linux-like systems, and takes its Auto language from `LANG`; an Android process has
/// none of them. Point the folders at the app's private storage, so autosave and crash
/// recovery work unchanged, and `LANG` at the system language.
fn set_environment(data: &Path, locale: &str) {
    let vars = [
        ("HOME", data.to_path_buf()),
        ("XDG_DATA_HOME", data.join("share")),
        ("XDG_CONFIG_HOME", data.join("config")),
        ("XDG_CACHE_HOME", data.join("cache")),
        ("XDG_STATE_HOME", data.join("state")),
        ("TMPDIR", data.join("tmp")),
    ];
    for (key, dir) in vars {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("craft-android: {}: {e}", dir.display());
        }
        set_var(key, dir.as_os_str());
    }
    log::info!("craft-android: locale {locale}");
    set_var("LANG", std::ffi::OsStr::new(locale));
}

fn set_var(key: &str, value: &std::ffi::OsStr) {
    // SAFETY: called first thing in `android_main`, before this library starts any thread that
    // reads the environment (eframe, the render pool and the app start later).
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var(key, value);
    }
}
