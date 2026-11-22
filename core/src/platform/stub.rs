//! Placeholder for unsupported platforms (lets the crate build and tests run).

use crate::protocol::{Button, Rect, Side};

pub(crate) fn start_impl() -> anyhow::Result<()> {
    anyhow::bail!("input capture is not supported on this OS yet")
}
pub fn screens() -> Vec<Rect> {
    vec![Rect { x: 0, y: 0, w: 1920, h: 1080 }]
}
pub fn ungrab(_entry: Option<(Side, f32)>) {
    super::SHARED.grabbing.store(false, std::sync::atomic::Ordering::Relaxed);
}
pub fn inject_move(_x: f64, _y: f64, _dx: i32, _dy: i32) {}
pub fn inject_button(_b: Button, _down: bool, _x: f64, _y: f64) {}
pub fn inject_wheel(_dx: i32, _dy: i32) {}
pub fn inject_key(_code: u16, _down: bool) {}
pub fn toggle_input_source(_method: &str) {}
pub fn cursor_pos() -> (f64, f64) {
    (0.0, 0.0)
}
pub fn clipboard_seq() -> u64 {
    0
}
pub fn permission_ok() -> bool {
    true
}
pub fn request_permission() {}
pub fn permission_status() -> super::PermStatus {
    super::PermStatus { accessibility: true, input_monitoring: true, installed: true }
}
pub fn request_input_monitoring() {}
