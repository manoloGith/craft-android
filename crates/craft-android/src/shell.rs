//! [`Shell`]: a Crafting App's `eframe::App`, plus what a window on Android needs that the desktop
//! gets from its window system.
//!
//! - **Input from the activity:** the mouse ([`crate::pointer`]), the hardware keyboard
//!   ([`crate::keyboard`]), the on-screen keyboard ([`crate::ime`]) and the system's insets
//!   ([`crate::insets`]) are added to egui's input each frame. On other systems nothing is queued.
//! - **Back closes the window** the way its close button does on a desktop: the app sees egui's
//!   close request and may hold it back to ask about unsaved changes. Android finishes the
//!   activity only once the window has closed (`android::finish`).
//! - **Autosave when the window loses focus** ([`Shell::on_focus_lost`]): Android can end the app
//!   without asking (the close button of a DeX window, the system reclaiming memory), so the app
//!   should write its crash recovery as soon as the user turns away.
//! - **Cursor, title and links** go to the activity on Android, where winit ignores them.

use std::sync::atomic::{AtomicBool, Ordering};

/// Set by Android (Back); taken by the next frame.
static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Ask the window to close, as its close button would (from any thread).
pub fn request_close(ctx: Option<&egui::Context>) {
    CLOSE_REQUESTED.store(true, Ordering::SeqCst);
    if let Some(ctx) = ctx {
        ctx.request_repaint();
    }
}

type FocusLost<A> = Box<dyn FnMut(&mut A)>;
type Title<A> = Box<dyn Fn(&A) -> String>;

/// An app's `eframe::App`, made at home on Android. Build it with [`Shell::new`] and the
/// optional hooks, and return it from the app creator instead of the app.
pub struct Shell<A: eframe::App> {
    app: A,
    focused: bool,
    /// Pointer events still to log (diagnostics for new devices: the first few after start).
    log_pointer: u32,
    on_focus_lost: Option<FocusLost<A>>,
    title: Option<Title<A>>,
}

impl<A: eframe::App> Shell<A> {
    pub fn new(app: A) -> Self {
        Shell { app, focused: true, log_pointer: 40, on_focus_lost: None, title: None }
    }

    /// What to do when the window loses focus: the app's autosave or crash recovery (PdfCraft:
    /// `|app| app.autosave_now()`).
    pub fn on_focus_lost(mut self, f: impl FnMut(&mut A) + 'static) -> Self {
        self.on_focus_lost = Some(Box::new(f));
        self
    }

    /// The window's title, read after each frame (the task's label in DeX's taskbar). Android
    /// ignores egui's `ViewportCommand::Title`, so the app says it here.
    pub fn title(mut self, f: impl Fn(&A) -> String + 'static) -> Self {
        self.title = Some(Box::new(f));
        self
    }

    pub fn app(&self) -> &A {
        &self.app
    }

    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// Log the first pointer presses and wheel turns (positions only, no content), so a report
    /// from a device shows whether its mouse reaches the interface.
    fn log_pointer_events(&mut self, ctx: &egui::Context) {
        if self.log_pointer == 0 {
            return;
        }
        ctx.input(|i| {
            for e in &i.events {
                let line = match e {
                    egui::Event::PointerButton { button, pressed, pos, .. } => format!("button {button:?} pressed={pressed} at {pos:?}"),
                    egui::Event::MouseWheel { delta, unit, .. } => format!("wheel {delta:?} {unit:?}"),
                    egui::Event::Touch { phase, pos, .. } => format!("touch {phase:?} at {pos:?}"),
                    _ => continue,
                };
                log::info!("craft-android: input: {line}");
                self.log_pointer = self.log_pointer.saturating_sub(1);
            }
        });
    }
}

impl<A: eframe::App> eframe::App for Shell<A> {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if CLOSE_REQUESTED.swap(false, Ordering::SeqCst) {
            // egui turns this into the window's close request, which the app may hold back.
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let focused = ctx.input(|i| i.focused);
        if self.focused
            && !focused
            && let Some(f) = self.on_focus_lost.as_mut()
        {
            f(&mut self.app);
        }
        self.focused = focused;
        self.log_pointer_events(ctx);
        self.app.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
        // On Android, winit ignores the cursor and the title, and opening links needs an Intent.
        #[cfg(target_os = "android")]
        {
            crate::android::desktop::set_cursor(ui.ctx().output(|o| o.cursor_icon));
            if let Some(title) = &self.title {
                crate::android::desktop::set_title(&title(&self.app));
            }
            let urls = ui.ctx().output_mut(|o| {
                let mut urls = Vec::new();
                o.commands.retain(|c| match c {
                    egui::OutputCommand::OpenUrl(open) => {
                        urls.push(open.url.clone());
                        false
                    }
                    _ => true,
                });
                urls
            });
            for url in urls {
                crate::android::desktop::open_url(&url);
            }
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.app.save(storage);
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        self.app.on_exit(gl);
    }

    fn auto_save_interval(&self) -> std::time::Duration {
        self.app.auto_save_interval()
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        self.app.clear_color(visuals)
    }

    fn persist_egui_memory(&self) -> bool {
        self.app.persist_egui_memory()
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // The hardware keyboard, the on-screen keyboard, the insets and the mouse on Android
        // (CraftActivity hands them over; nothing is queued elsewhere).
        raw_input.events.extend(crate::keyboard::drain());
        raw_input.events.extend(crate::ime::drain());
        if let Some(safe_area) = crate::insets::safe_area(ctx.pixels_per_point()) {
            raw_input.safe_area_insets = Some(safe_area);
        }
        raw_input.events.extend(crate::pointer::drain(ctx.pixels_per_point()));
        self.app.raw_input_hook(ctx, raw_input);
    }
}
