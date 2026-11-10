//! Guards for the pointer fixes this branch carries on top of winit 0.30.13.
//!
//! Each test checks that a fix is still in the source, or runs its policy where that is plain
//! Rust, and fails if a rebase drops it. When a winit release carries an equivalent fix, drop the
//! patch together with its test.

use std::path::Path;

fn source(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/platform_impl").join(path);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

/// The text after `start`, up to the first `end`.
fn section<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let (_, rest) = text.split_once(start).unwrap_or_else(|| panic!("`{start}` not found"));
    rest.split_once(end).map_or(rest, |(body, _)| body)
}

#[test]
fn macos_title_bar_drags_reach_the_app() {
    // NSView answers YES to `mouseDownCanMoveWindow` for a view that isn't opaque. With a
    // transparent title bar over a full-size content view, AppKit then moved the window for any
    // drag that began in the title-bar band, before the app saw it, so tabs drawn there could not
    // be dragged. The view answers with the window's `movable_by_window_background` (NO unless
    // asked); apps move the window from their own title bar with `drag_window`
    // (storytold/pdfcraft#2347).
    let view = source("macos/view.rs");
    let method = section(&view, "#[method(mouseDownCanMoveWindow)]", "\n        }\n");
    assert!(
        method.contains("self.ivars().movable_by_window_background"),
        "mouseDownCanMoveWindow doesn't follow the window's setting"
    );
    let delegate = source("macos/window_delegate.rs");
    assert!(
        delegate.contains("attrs.platform_specific.movable_by_window_background,"),
        "the view isn't given the window's setting"
    );
}

// The scroll-phase policy is plain Rust over winit's own `TouchPhase`, so its tests run here.
#[cfg(wayland_platform)]
mod event {
    pub use winit::event::TouchPhase;
}
#[cfg(wayland_platform)]
#[path = "../src/platform_impl/linux/wayland/seat/pointer/scroll.rs"]
mod scroll;

#[test]
fn wayland_pixel_wheels_report_moved() {
    // winit 0.30.13 starts a finger gesture (`TouchPhase::Started`) for any axis event without
    // discrete steps, and only `axis_stop` ends it, which wheels need not send. A wheel, wheel tilt
    // or continuous source reporting only pixel deltas then left a gesture open, and egui
    // kept the modifiers it started with: Ctrl zoom stayed on after Ctrl was released. Such events
    // report `Moved`, under the rules `scroll::phase` tests (storytold/pdfcraft#1569).
    let pointer = source("linux/wayland/seat/pointer/mod.rs");
    let axis = section(&pointer, "PointerEventKind::Axis {", "pointer_data.phase = phase;");
    assert!(
        axis.contains("let phase = scroll::phase("),
        "axis events don't take their phase from the scroll source"
    );
}

#[test]
fn android_mice_are_mice_not_fingers() {
    // DexPdfCraft (W-DEX1): winit reported every Android motion event as a touch, so a mouse on
    // Samsung DeX or in desktop windowing had no hover, no wheel and no right or middle button.
    // Mouse events now move the cursor, press and release buttons from the button-state bits, and
    // scroll with the wheel's axes.
    let android = source("android/mod.rs");
    let mouse = section(&android, "if motion_event.source() == Source::Mouse", "InputEvent::MotionEvent(motion_event) => {");
    for needed in [
        "WindowEvent::CursorMoved",
        "WindowEvent::CursorEntered",
        "WindowEvent::CursorLeft",
        "WindowEvent::MouseWheel",
        "MouseScrollDelta::LineDelta",
        "Axis::Vscroll",
        "Axis::Hscroll",
        "WindowEvent::MouseInput",
        "mouse_button_bits(motion_event.button_state())",
        "mouse_button_bit(motion_event.action_button())",
        "MotionAction::Down | MotionAction::PointerDown => 1",
    ] {
        assert!(mouse.contains(needed), "the Android mouse handling lost `{needed}`");
    }
    assert!(!mouse.contains("WindowEvent::Touch"), "a mouse must not be reported as a touch");
    let buttons = section(&android, "const MOUSE_BUTTONS", "];");
    for button in ["Left", "Right", "Middle", "Back", "Forward"] {
        assert!(buttons.contains(button), "MOUSE_BUTTONS lost {button}");
    }
}

#[test]
fn android_destroy_ends_the_event_loop() {
    // DexPdfCraft (W-DEX2): GameActivity's onDestroy waits for android_main to return. winit
    // ignored MainEvent::Destroy, so the loop kept running, the activity's UI thread hung on
    // close and the next start failed.
    let android = source("android/mod.rs");
    let destroy = section(&android, "MainEvent::Destroy => {", "},");
    assert!(destroy.contains("self.window_target.p.exit()"), "Destroy must end the event loop");
}

#[test]
fn android_window_events_wait_for_the_window() {
    // DexPdfCraft (W-DEX3): Android reported a configuration change (DeX resizing the window as
    // it opens) as ScaleFactorChanged before the native window existed; eframe treats a window
    // event before its window as the end of the program, so the app closed as it opened.
    let android = source("android/mod.rs");
    let iteration = section(&android, "fn single_iteration<F>", "let cause = self.cause;");
    assert!(iteration.contains("!WINDOW_CREATED.load(Ordering::Relaxed)"), "window events must wait for the window");
    let init = section(&android, "MainEvent::InitWindow { .. } => {", "},");
    assert!(init.contains("WINDOW_CREATED.store(true"), "InitWindow must mark the window as created");
}
