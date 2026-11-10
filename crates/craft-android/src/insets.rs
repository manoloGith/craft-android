//! The parts of the window the system covers on a phone or tablet (status and navigation bars,
//! a display cutout, the on-screen keyboard), from MainActivity (`nativeInsets`), as egui's safe
//! area: egui keeps panels and content out of it. Android 15 draws apps edge to edge; in a DeX
//! window the insets are usually zero.

use std::sync::{Mutex, PoisonError};

/// The insets in pixels: left, top, right, bottom.
static INSETS: Mutex<Option<[f32; 4]>> = Mutex::new(None);

pub fn set(left: f32, top: f32, right: f32, bottom: f32) {
    *INSETS.lock().unwrap_or_else(PoisonError::into_inner) = Some([left, top, right, bottom]);
}

/// The safe area in points, once the activity has reported insets.
pub fn safe_area(pixels_per_point: f32) -> Option<egui::SafeAreaInsets> {
    let px = (*INSETS.lock().unwrap_or_else(PoisonError::into_inner))?;
    Some(to_points(px, pixels_per_point))
}

fn to_points(px: [f32; 4], pixels_per_point: f32) -> egui::SafeAreaInsets {
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 { pixels_per_point } else { 1.0 };
    let p = |v: f32| if v.is_finite() { v.max(0.0) / ppp } else { 0.0 };
    egui::SafeAreaInsets(egui::epaint::MarginF32 { left: p(px[0]), right: p(px[2]), top: p(px[1]), bottom: p(px[3]) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_become_points_and_nonsense_becomes_zero() {
        let s = to_points([0.0, 96.0, 0.0, f32::NAN], 3.0);
        assert_eq!(s.0.top, 32.0);
        assert_eq!(s.0.bottom, 0.0);
        let s = to_points([-5.0, 10.0, 0.0, 0.0], 0.0);
        assert_eq!(s.0.left, 0.0);
        assert_eq!(s.0.top, 10.0);
    }
}
