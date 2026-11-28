//! Windows: low-level keyboard/mouse hooks for capture, SendInput for injection.

use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use windows_sys::Win32::Foundation::{BOOL, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HDC, HMONITOR,
    MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::{edge_hit, emit, InputEvent, SHARED};
use crate::keys::{self, WinKey};
use crate::protocol::{Button, Rect, Side};

const MAGIC: usize = 0x4E4F_4F50; // "NOOP"

static FULLSCREEN_ACTIVE: AtomicBool = AtomicBool::new(false);

struct HookState {
    last: (i32, i32),
    park: (i32, i32),
}

static STATE: Mutex<HookState> = Mutex::new(HookState { last: (0, 0), park: (0, 0) });

fn any_button_down() -> bool {
    unsafe {
        [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON, VK_XBUTTON1, VK_XBUTTON2]
            .iter()
            .any(|vk| (GetAsyncKeyState(*vk as i32) as u16) & 0x8000 != 0)
    }
}

/// Holding Ctrl or Alt while pushing the edge lets the user leave a fullscreen app on purpose.
fn bypass_key_down() -> bool {
    unsafe {
        [VK_CONTROL, VK_MENU]
            .iter()
            .any(|vk| (GetAsyncKeyState(*vk as i32) as u16) & 0x8000 != 0)
    }
}

/// Detect whether a game or fullscreen app currently owns the screen or confines the cursor.
unsafe fn detect_fullscreen_or_game() -> bool {
    // 1. Official Windows notification state (DirectX/Vulkan exclusive fullscreen or busy fullscreen)
    let mut state = 0i32;
    if SHQueryUserNotificationState(&mut state) == 0
        && (state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_BUSY)
    {
        return true;
    }

    // 2. Cursor actively clipped to a sub-region (FPS / RTS / MOBA games)
    let mut clip = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    if GetClipCursor(&mut clip) != 0 {
        let vs = virtual_screen();
        let cw = clip.right - clip.left;
        let ch = clip.bottom - clip.top;
        if cw > 0 && ch > 0 && (cw < vs.w - 4 || ch < vs.h - 4) {
            return true;
        }
    }

    // 3. Borderless fullscreen foreground window covering its entire monitor
    let hwnd = GetForegroundWindow();
    if hwnd.is_null() {
        return false;
    }
    let mut cls = [0u16; 64];
    let len = GetClassNameW(hwnd, cls.as_mut_ptr(), cls.len() as i32) as usize;
    if len > 0 {
        let name = String::from_utf16_lossy(&cls[..len]);
        if matches!(
            name.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return false;
        }
    }
    let mut wr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    if GetWindowRect(hwnd, &mut wr) == 0 {
        return false;
    }
    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if mon.is_null() {
        return false;
    }
    let mut mi: MONITORINFO = std::mem::zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(mon, &mut mi) == 0 {
        return false;
    }
    let m = mi.rcMonitor;
    wr.left <= m.left && wr.top <= m.top && wr.right >= m.right && wr.bottom >= m.bottom
}

/// Test hook: enter/leave the "controlling another computer" state without
/// moving the cursor (used by examples/hook_check.rs).
#[doc(hidden)]
pub fn debug_set_grabbing(on: bool) {
    SHARED.grabbing.store(on, Ordering::Relaxed);
}

fn virtual_screen() -> Rect {
    unsafe {
        Rect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            w: GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            h: GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        }
    }
}

unsafe extern "system" fn monitor_cb(_m: HMONITOR, _dc: HDC, rc: *mut RECT, data: LPARAM) -> BOOL {
    let v = &mut *(data as *mut Vec<Rect>);
    let r = &*rc;
    v.push(Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top });
    1
}

pub fn screens() -> Vec<Rect> {
    let mut v: Vec<Rect> = Vec::new();
    unsafe {
        EnumDisplayMonitors(null_mut(), null(), Some(monitor_cb), &mut v as *mut Vec<Rect> as LPARAM);
    }
    if v.is_empty() {
        v.push(virtual_screen());
    }
    v
}

unsafe extern "system" fn mouse_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(null_mut(), code, wp, lp);
    }
    let info = &*(lp as *const MSLLHOOKSTRUCT);
    if info.dwExtraInfo == MAGIC {
        return CallNextHookEx(null_mut(), code, wp, lp);
    }
    let grabbing = SHARED.grabbing.load(Ordering::Relaxed);
    let msg = wp as u32;
    if msg == WM_MOUSEMOVE {
        let (x, y) = (info.pt.x, info.pt.y);
        let mut st = STATE.lock().unwrap();
        if grabbing {
            let mut cur = POINT { x: 0, y: 0 };
            GetCursorPos(&mut cur);
            // If UAC / Secure Desktop / another elevated window moved the real cursor
            // away from `st.park`, release the grab cleanly instead of sending huge
            // repeated deltas that pin the remote cursor into a corner.
            if (cur.x - st.park.0).abs() > 8 || (cur.y - st.park.1).abs() > 8 {
                SHARED.grabbing.store(false, Ordering::Relaxed);
                st.last = (cur.x, cur.y);
                emit(InputEvent::GrabLost);
                return CallNextHookEx(null_mut(), code, wp, lp);
            }
            st.park = (cur.x, cur.y);
            let (dx, dy) = (x - cur.x, y - cur.y);
            if dx != 0 || dy != 0 {
                emit(InputEvent::MouseMove { dx, dy });
            }
            return 1;
        }
        let (lx, ly) = st.last;
        st.last = (x, y);
        let mask = SHARED.neighbors.load(Ordering::Relaxed);
        // Never switch mid-drag, and block edge-switching during games/fullscreen unless Ctrl/Alt is held.
        let fs_blocked = SHARED.lock_fullscreen.load(Ordering::Relaxed)
            && FULLSCREEN_ACTIVE.load(Ordering::Relaxed)
            && !bypass_key_down();
        if mask != 0 && !any_button_down() && !fs_blocked {
            if let Some((side, t)) = edge_hit(virtual_screen(), x as f64, y as f64, (x - lx) as f64, (y - ly) as f64, mask, false) {
                SHARED.grabbing.store(true, Ordering::Relaxed);
                let mut cur = POINT { x: 0, y: 0 };
                GetCursorPos(&mut cur);
                st.park = (cur.x, cur.y);
                emit(InputEvent::EdgeExit { side, t });
                return 1;
            }
        }
        return CallNextHookEx(null_mut(), code, wp, lp);
    }
    if !grabbing {
        return CallNextHookEx(null_mut(), code, wp, lp);
    }
    let hi = ((info.mouseData >> 16) & 0xFFFF) as u16;
    let xb = if hi == 1 { Button::Back } else { Button::Forward };
    let ev = match msg {
        WM_LBUTTONDOWN => Some(InputEvent::Button { button: Button::Left, down: true }),
        WM_LBUTTONUP => Some(InputEvent::Button { button: Button::Left, down: false }),
        WM_RBUTTONDOWN => Some(InputEvent::Button { button: Button::Right, down: true }),
        WM_RBUTTONUP => Some(InputEvent::Button { button: Button::Right, down: false }),
        WM_MBUTTONDOWN => Some(InputEvent::Button { button: Button::Middle, down: true }),
        WM_MBUTTONUP => Some(InputEvent::Button { button: Button::Middle, down: false }),
        WM_XBUTTONDOWN => Some(InputEvent::Button { button: xb, down: true }),
        WM_XBUTTONUP => Some(InputEvent::Button { button: xb, down: false }),
        WM_MOUSEWHEEL => Some(InputEvent::Wheel { dx: 0, dy: hi as i16 as i32 }),
        WM_MOUSEHWHEEL => Some(InputEvent::Wheel { dx: hi as i16 as i32, dy: 0 }),
        _ => None,
    };
    if let Some(ev) = ev {
        emit(ev);
    }
    1
}

unsafe extern "system" fn keyboard_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code < 0 || !SHARED.grabbing.load(Ordering::Relaxed) {
        return CallNextHookEx(null_mut(), code, wp, lp);
    }
    {
        let mut cur = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cur);
        let mut st = STATE.lock().unwrap();
        if (cur.x - st.park.0).abs() > 8 || (cur.y - st.park.1).abs() > 8 {
            SHARED.grabbing.store(false, Ordering::Relaxed);
            st.last = (cur.x, cur.y);
            emit(InputEvent::GrabLost);
            return CallNextHookEx(null_mut(), code, wp, lp);
        }
    }
    let info = &*(lp as *const KBDLLHOOKSTRUCT);
    if info.dwExtraInfo == MAGIC {
        return CallNextHookEx(null_mut(), code, wp, lp);
    }
    let msg = wp as u32;
    let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let ext = info.flags & LLKHF_EXTENDED != 0;
    // AltGr's fake LCtrl and the fake shifts around extended keys.
    if info.scanCode & 0x200 != 0 || keys::is_fake_shift(info.scanCode, ext) || info.vkCode == 0xFF {
        return 1;
    }
    if let Some(h) = keys::from_win(info.scanCode, ext, info.vkCode) {
        emit(InputEvent::Key { code: h, down });
    }
    1
}

pub(crate) fn start_impl() -> anyhow::Result<()> {
    let _ = std::thread::Builder::new().name("noopflow-fs-watch".into()).spawn(|| {
        loop {
            let active = if SHARED.lock_fullscreen.load(Ordering::Relaxed) {
                unsafe { detect_fullscreen_or_game() }
            } else {
                false
            };
            FULLSCREEN_ACTIVE.store(active, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(250));
        }
    });
    let (ok_tx, ok_rx) = std::sync::mpsc::channel::<bool>();
    std::thread::Builder::new().name("noopflow-hooks".into()).spawn(move || unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let hmod = GetModuleHandleW(null());
        let m = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), hmod, 0);
        let k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), hmod, 0);
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        STATE.lock().unwrap().last = (p.x, p.y);
        let _ = ok_tx.send(!m.is_null() && !k.is_null());
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    })?;
    if !ok_rx.recv().unwrap_or(false) {
        anyhow::bail!("could not install input hooks");
    }
    emit(InputEvent::Permission(true));
    Ok(())
}

pub fn ungrab(entry: Option<(Side, f32)>) {
    let (x, y) = {
        let mut st = STATE.lock().unwrap();
        let p = match entry {
            Some((side, t)) => {
                let (x, y) = virtual_screen().point_on_side(side, t, 2.0);
                (x.round() as i32, y.round() as i32)
            }
            None => st.park,
        };
        st.last = p;
        p
    };
    // Don't hold the lock here: the hook thread takes it on every mouse event.
    unsafe {
        SetCursorPos(x, y);
    }
    SHARED.grabbing.store(false, Ordering::Relaxed);
}

fn send(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
    }
}

fn mouse(dx: i32, dy: i32, data: i32, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data as _, dwFlags: flags, time: 0, dwExtraInfo: MAGIC } },
    }
}

fn key(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: MAGIC } },
    }
}

pub fn on_enter(x: f64, y: f64) {
    inject_move(x, y, 0, 0);
}

pub fn inject_move(x: f64, y: f64, _dx: i32, _dy: i32) {
    let v = virtual_screen();
    let nx = (((x - v.x as f64) * 65535.0) / (v.w - 1).max(1) as f64).round() as i32;
    let ny = (((y - v.y as f64) * 65535.0) / (v.h - 1).max(1) as f64).round() as i32;
    send(&[mouse(nx, ny, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK)]);
}

pub fn inject_button(b: Button, down: bool, _x: f64, _y: f64) {
    let (flags, data) = match (b, down) {
        (Button::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (Button::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
        (Button::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (Button::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (Button::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (Button::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (Button::Back, true) => (MOUSEEVENTF_XDOWN, 1),
        (Button::Back, false) => (MOUSEEVENTF_XUP, 1),
        (Button::Forward, true) => (MOUSEEVENTF_XDOWN, 2),
        (Button::Forward, false) => (MOUSEEVENTF_XUP, 2),
    };
    send(&[mouse(0, 0, data, flags)]);
}

pub fn inject_wheel(dx: i32, dy: i32) {
    if dy != 0 {
        send(&[mouse(0, 0, dy, MOUSEEVENTF_WHEEL)]);
    }
    if dx != 0 {
        send(&[mouse(0, 0, dx, MOUSEEVENTF_HWHEEL)]);
    }
}

pub fn inject_key(code: u16, down: bool) {
    let up = if down { 0 } else { KEYEVENTF_KEYUP };
    match keys::to_win(code) {
        Some(WinKey::Scan { scan, extended }) => {
            let ext = if extended { KEYEVENTF_EXTENDEDKEY } else { 0 };
            send(&[key(0, scan, KEYEVENTF_SCANCODE | ext | up)]);
        }
        Some(WinKey::Vk { vk, extended }) => {
            let ext = if extended { KEYEVENTF_EXTENDEDKEY } else { 0 };
            let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
            send(&[key(vk, scan, ext | up)]);
        }
        None => {}
    }
}

/// On Windows the IME listens to VK_HANGUL directly.
pub fn toggle_input_source(_method: &str) {
    inject_key(keys::hid::LANG1, true);
    inject_key(keys::hid::LANG1, false);
}

pub fn cursor_pos() -> (f64, f64) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut p);
    }
    (p.x as f64, p.y as f64)
}

pub fn clipboard_seq() -> u64 {
    unsafe { GetClipboardSequenceNumber() as u64 }
}

pub fn permission_ok() -> bool {
    true
}

pub fn request_permission() {}

/// Display scale (1.0 = 100%) of the monitor at a point, for pixel <-> logical conversion.
pub fn scale_at(x: f64, y: f64) -> f64 {
    unsafe {
        let m = MonitorFromPoint(POINT { x: x as i32, y: y as i32 }, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) == 0 && dx > 0 {
            dx as f64 / 96.0
        } else {
            1.0
        }
    }
}

pub fn permission_status() -> super::PermStatus {
    super::PermStatus { accessibility: true, input_monitoring: true, installed: true }
}

pub fn request_input_monitoring() {}
