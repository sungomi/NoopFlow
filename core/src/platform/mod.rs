//! OS integration: capturing local input (with the ability to swallow it while
//! another computer is being controlled) and injecting input received from peers.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};
use std::sync::OnceLock;

use tokio::sync::mpsc::UnboundedSender;

use crate::protocol::{Button, Rect, Side};

#[derive(Debug, Clone)]
pub enum InputEvent {
    /// The local cursor was pushed against `side`; local input is now grabbed.
    EdgeExit { side: Side, t: f32 },
    MouseMove { dx: i32, dy: i32 },
    Button { button: Button, down: bool },
    /// 120 units = one wheel notch.
    Wheel { dx: i32, dy: i32 },
    Key { code: u16, down: bool },
    /// Key observed while not grabbing (only when observing is enabled).
    LocalKey { code: u16, down: bool },
    /// Mouse click observed while not grabbing (only when observing is enabled).
    LocalClick,
    Permission(bool),
}

/// What the OS currently allows this app to do (macOS privacy settings).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct PermStatus {
    /// Accessibility: needed to control the mouse/keyboard and to capture input.
    pub accessibility: bool,
    /// Input Monitoring: needed on some macOS versions to read the keyboard.
    pub input_monitoring: bool,
    /// Running from a real install location (not from the .dmg or a translocated copy).
    pub installed: bool,
}

pub(crate) struct Shared {
    pub grabbing: AtomicBool,
    pub neighbors: AtomicU8,
    pub observe_local: AtomicBool,
    pub corner_guard: AtomicI32,
    pub tx: OnceLock<UnboundedSender<InputEvent>>,
}

pub(crate) static SHARED: Shared = Shared {
    grabbing: AtomicBool::new(false),
    neighbors: AtomicU8::new(0),
    observe_local: AtomicBool::new(false),
    corner_guard: AtomicI32::new(6),
    tx: OnceLock::new(),
};

pub(crate) fn emit(ev: InputEvent) {
    if let Some(tx) = SHARED.tx.get() {
        let _ = tx.send(ev);
    }
}

pub fn set_neighbors(mask: u8) {
    SHARED.neighbors.store(mask, Ordering::Relaxed);
}
pub fn set_observe_local(on: bool) {
    SHARED.observe_local.store(on, Ordering::Relaxed);
}
pub fn set_corner_guard(px: i32) {
    SHARED.corner_guard.store(px.max(0), Ordering::Relaxed);
}
pub fn is_grabbing() -> bool {
    SHARED.grabbing.load(Ordering::Relaxed)
}

/// Decide whether a cursor move at (x, y) with delta (dx, dy) leaves the desktop
/// through a side that has a neighbour. `strict` requires the delta to point
/// outward (macOS gives real deltas; Windows hook points may already be clamped).
pub(crate) fn edge_hit(b: Rect, x: f64, y: f64, dx: f64, dy: f64, mask: u8, strict: bool) -> Option<(Side, f32)> {
    let guard = SHARED.corner_guard.load(Ordering::Relaxed) as f64;
    let (l, t, r, btm) = (b.x as f64, b.y as f64, (b.right() - 1) as f64, (b.bottom() - 1) as f64);
    let out = |d: f64, neg: bool| if strict { if neg { d < 0.0 } else { d > 0.0 } } else if neg { d <= 0.0 } else { d >= 0.0 };
    let along_y = || if y < t + guard || y > btm - guard { None } else { Some(((y - t) / (btm - t).max(1.0)) as f32) };
    let along_x = || if x < l + guard || x > r - guard { None } else { Some(((x - l) / (r - l).max(1.0)) as f32) };
    if mask & Side::Left.bit() != 0 && x <= l && out(dx, true) {
        if let Some(f) = along_y() {
            return Some((Side::Left, f));
        }
    }
    if mask & Side::Right.bit() != 0 && x >= r && out(dx, false) {
        if let Some(f) = along_y() {
            return Some((Side::Right, f));
        }
    }
    if mask & Side::Top.bit() != 0 && y <= t && out(dy, true) {
        if let Some(f) = along_x() {
            return Some((Side::Top, f));
        }
    }
    if mask & Side::Bottom.bit() != 0 && y >= btm && out(dy, false) {
        if let Some(f) = along_x() {
            return Some((Side::Bottom, f));
        }
    }
    None
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

#[cfg(not(any(windows, target_os = "macos")))]
mod stub;
#[cfg(not(any(windows, target_os = "macos")))]
pub use self::stub::*;

pub fn start(tx: UnboundedSender<InputEvent>) -> anyhow::Result<()> {
    let _ = SHARED.tx.set(tx);
    start_impl()
}
