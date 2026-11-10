//! The mouse on Android, from MainActivity (`nativePointer`), into egui. (Platform independent so
//! it is tested anywhere; on other systems nothing is queued.)
//!
//! The activity takes mouse events off GameActivity's surface view, where Android's own
//! MotionEvent API tells their buttons, wheel and hover reliably, and hands them here. Each frame
//! turns the ones that arrived into egui events (`Shell::raw_input_hook`). Touch input still goes
//! through GameActivity and winit.

use std::sync::{Mutex, PoisonError};

/// One mouse event from the activity, in the surface's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    /// The pointer left the window.
    pub gone: bool,
    pub x: f32,
    pub y: f32,
    /// Buttons down: 1 primary, 2 secondary, 4 middle, 8 back, 16 forward.
    pub buttons: u32,
    /// Wheel lines (AXIS_HSCROLL, AXIS_VSCROLL).
    pub wheel: (f32, f32),
    /// Android meta state (shift, alt, ctrl, meta).
    pub meta: u32,
}

static QUEUE: Mutex<Vec<Pointer>> = Mutex::new(Vec::new());
/// Buttons egui was last told are down.
static DOWN: Mutex<u32> = Mutex::new(0);

pub fn push(p: Pointer) {
    QUEUE.lock().unwrap_or_else(PoisonError::into_inner).push(p);
}

/// The queued events as egui events, `pixels_per_point` converting the surface's pixels.
pub fn drain(pixels_per_point: f32) -> Vec<egui::Event> {
    let queued = std::mem::take(&mut *QUEUE.lock().unwrap_or_else(PoisonError::into_inner));
    let mut down = DOWN.lock().unwrap_or_else(PoisonError::into_inner);
    let mut out = Vec::new();
    for p in queued {
        out.extend(to_egui(&p, pixels_per_point, &mut down));
    }
    out
}

const BUTTONS: [(u32, egui::PointerButton); 5] = [
    (1, egui::PointerButton::Primary),
    (2, egui::PointerButton::Secondary),
    (4, egui::PointerButton::Middle),
    (8, egui::PointerButton::Extra1),
    (16, egui::PointerButton::Extra2),
];

fn modifiers(meta: u32) -> egui::Modifiers {
    // KeyEvent.META_SHIFT_ON, META_ALT_ON, META_CTRL_ON, META_META_ON
    let ctrl = meta & 0x1000 != 0;
    egui::Modifiers { alt: meta & 0x02 != 0, ctrl, shift: meta & 0x01 != 0, mac_cmd: false, command: ctrl }
}

/// One activity event as egui events; `down` is updated to the buttons now down.
pub fn to_egui(p: &Pointer, pixels_per_point: f32, down: &mut u32) -> Vec<egui::Event> {
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 { pixels_per_point } else { 1.0 };
    let pos = egui::pos2(p.x / ppp, p.y / ppp);
    let modifiers = modifiers(p.meta);
    let mut out = Vec::new();
    if p.gone {
        // Leaving with a button down (a drag out of the window) releases it first.
        for (bit, button) in BUTTONS {
            if *down & bit != 0 {
                out.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers });
            }
        }
        *down = 0;
        out.push(egui::Event::PointerGone);
        return out;
    }
    out.push(egui::Event::PointerMoved(pos));
    let changed = *down ^ p.buttons;
    for (bit, button) in BUTTONS {
        if changed & bit != 0 {
            out.push(egui::Event::PointerButton { pos, button, pressed: p.buttons & bit != 0, modifiers });
        }
    }
    *down = p.buttons;
    let (h, v) = p.wheel;
    if (h != 0.0 || v != 0.0) && h.is_finite() && v.is_finite() {
        out.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(h, v),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers,
        });
    }
    out
}

/// Android's `PointerIcon` type for an egui cursor (`PointerIcon.TYPE_*`), the closest one.
pub fn pointer_icon_type(icon: egui::CursorIcon) -> i32 {
    use egui::CursorIcon as C;
    match icon {
        C::None => 0,                                // TYPE_NULL
        C::ContextMenu => 1001,                      // TYPE_CONTEXT_MENU
        C::PointingHand => 1002,                     // TYPE_HAND
        C::Help => 1003,                             // TYPE_HELP
        C::Wait | C::Progress => 1004,               // TYPE_WAIT
        C::Cell => 1006,                             // TYPE_CELL
        C::Crosshair => 1007,                        // TYPE_CROSSHAIR
        C::Text => 1008,                             // TYPE_TEXT
        C::VerticalText => 1009,                     // TYPE_VERTICAL_TEXT
        C::Alias => 1010,                            // TYPE_ALIAS
        C::Copy => 1011,                             // TYPE_COPY
        C::NoDrop | C::NotAllowed => 1012,           // TYPE_NO_DROP
        C::Move | C::AllScroll => 1013,              // TYPE_ALL_SCROLL
        C::ResizeHorizontal | C::ResizeEast | C::ResizeWest | C::ResizeColumn => 1014, // TYPE_HORIZONTAL_DOUBLE_ARROW
        C::ResizeVertical | C::ResizeNorth | C::ResizeSouth | C::ResizeRow => 1015, // TYPE_VERTICAL_DOUBLE_ARROW
        C::ResizeNeSw | C::ResizeNorthEast | C::ResizeSouthWest => 1016, // TYPE_TOP_RIGHT_DIAGONAL_DOUBLE_ARROW
        C::ResizeNwSe | C::ResizeNorthWest | C::ResizeSouthEast => 1017, // TYPE_TOP_LEFT_DIAGONAL_DOUBLE_ARROW
        C::ZoomIn => 1018,                           // TYPE_ZOOM_IN
        C::ZoomOut => 1019,                          // TYPE_ZOOM_OUT
        C::Grab => 1020,                             // TYPE_GRAB
        C::Grabbing => 1021,                         // TYPE_GRABBING
        _ => 1000,                                   // TYPE_ARROW (Default, and any newer cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32, buttons: u32) -> Pointer {
        Pointer { gone: false, x, y, buttons, wheel: (0.0, 0.0), meta: 0 }
    }

    #[test]
    fn a_click_is_a_press_then_a_release_where_the_pointer_is() {
        let mut down = 0;
        let press = to_egui(&at(20.0, 40.0, 1), 2.0, &mut down);
        assert_eq!(press[0], egui::Event::PointerMoved(egui::pos2(10.0, 20.0)));
        assert!(matches!(press[1], egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: true, .. }));
        assert_eq!(down, 1);
        let release = to_egui(&at(20.0, 40.0, 0), 2.0, &mut down);
        assert!(matches!(release[1], egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: false, .. }));
        assert_eq!(down, 0);
    }

    #[test]
    fn right_click_and_ctrl_wheel() {
        let mut down = 0;
        let right = to_egui(&at(0.0, 0.0, 2), 1.0, &mut down);
        assert!(matches!(right[1], egui::Event::PointerButton { button: egui::PointerButton::Secondary, pressed: true, .. }));
        let mut wheel = at(0.0, 0.0, 2);
        wheel.wheel = (0.0, -1.0);
        wheel.meta = 0x1000;
        let events = to_egui(&wheel, 1.0, &mut down);
        assert!(matches!(events.last(), Some(egui::Event::MouseWheel { modifiers, .. }) if modifiers.ctrl && modifiers.command));
    }

    #[test]
    fn leaving_the_window_releases_held_buttons() {
        let mut down = 5;
        let out = to_egui(&Pointer { gone: true, ..at(1.0, 1.0, 0) }, 1.0, &mut down);
        assert_eq!(out.iter().filter(|e| matches!(e, egui::Event::PointerButton { pressed: false, .. })).count(), 2);
        assert_eq!(out.last(), Some(&egui::Event::PointerGone));
        assert_eq!(down, 0);
    }

    #[test]
    fn cursors_map_to_android_pointer_icons() {
        assert_eq!(pointer_icon_type(egui::CursorIcon::Default), 1000);
        assert_eq!(pointer_icon_type(egui::CursorIcon::PointingHand), 1002);
        assert_eq!(pointer_icon_type(egui::CursorIcon::Text), 1008);
        assert_eq!(pointer_icon_type(egui::CursorIcon::ResizeHorizontal), 1014);
        assert_eq!(pointer_icon_type(egui::CursorIcon::ResizeNwSe), 1017);
        assert_eq!(pointer_icon_type(egui::CursorIcon::Grabbing), 1021);
        assert_eq!(pointer_icon_type(egui::CursorIcon::None), 0);
    }

    #[test]
    fn a_bad_scale_does_not_poison_positions() {
        let mut down = 0;
        let out = to_egui(&at(5.0, 5.0, 0), 0.0, &mut down);
        assert_eq!(out[0], egui::Event::PointerMoved(egui::pos2(5.0, 5.0)));
    }
}
