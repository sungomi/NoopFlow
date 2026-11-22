//! Windows: low-level keyboard/mouse hooks for capture, SendInput for injection.

use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::{edge_hit, emit, InputEvent, SHARED};
use crate::keys::{self, WinKey};
use crate::protocol::{Button, Rect, Side};

const MAGIC: usize = 0x4E4F_4F50; // "NOOP"

struct HookState {
    last: (i32, i32),
    park: (i32, i32),
    /// Where to put the cursor when the grab ends (applied on the hook thread,
    /// after any pending take_focus, so the two can't race).
    ret: Option<(i32, i32)>,
}

static STATE: Mutex<HookState> = Mutex::new(HookState { last: (0, 0), park: (0, 0), ret: None });

/// Modifiers that were physically held when a grab started: their key-up goes to
/// Windows (so they don't stay stuck there) and their auto-repeat is swallowed.
///
/// Only Shift/Ctrl/Alt/Win, read live from the OS at switch time. Tracking every
/// key ourselves went wrong: Korean 한/영 and 한자 keys send no key-up, and other
/// hooks or elevated windows can hide key-ups, which left keys "held" forever and
/// made their presses vanish on the other computer.
static PASS_UP: Mutex<[bool; 256]> = Mutex::new([false; 256]);
const MODIFIER_VKS: [u16; 8] = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C];

/// Test hook: enter/leave the "controlling another computer" state without
/// moving the cursor or touching window focus (used by examples/hook_check.rs).
#[doc(hidden)]
pub fn debug_set_grabbing(on: bool) {
    if on {
        *PASS_UP.lock().unwrap() = snapshot_held_modifiers();
    }
    SHARED.grabbing.store(on, Ordering::Relaxed);
}

fn snapshot_held_modifiers() -> [bool; 256] {
    let mut held = [false; 256];
    for vk in MODIFIER_VKS {
        held[vk as usize] = unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 };
    }
    held
}

/// Invisible window that takes keyboard focus and hides the cursor while another
/// computer is being controlled, and the window that had focus before.
static CAPTURE: AtomicIsize = AtomicIsize::new(0);
static PREV_FOREGROUND: AtomicIsize = AtomicIsize::new(0);
const WM_NF_GRAB: u32 = WM_APP + 1;
const WM_NF_UNGRAB: u32 = WM_APP + 2;
const CAPTURE_SIZE: i32 = 32;

fn any_button_down() -> bool {
    unsafe {
        [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON, VK_XBUTTON1, VK_XBUTTON2]
            .iter()
            .any(|vk| (GetAsyncKeyState(*vk as i32) as u16) & 0x8000 != 0)
    }
}

fn monitor_center(x: i32, y: i32) -> (i32, i32) {
    unsafe {
        let m = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(m, &mut mi) == 0 {
            return (x, y);
        }
        let r = mi.rcMonitor;
        ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
    }
}

/// SetForegroundWindow is refused for background processes unless we briefly
/// attach to the current foreground thread's input.
unsafe fn force_foreground(target: HWND) {
    let fg = GetForegroundWindow();
    let me = GetCurrentThreadId();
    let fg_thread = if fg.is_null() { 0 } else { GetWindowThreadProcessId(fg, null_mut()) };
    let attach = fg_thread != 0 && fg_thread != me;
    if attach {
        AttachThreadInput(me, fg_thread, 1);
    }
    SetForegroundWindow(target);
    if attach {
        AttachThreadInput(me, fg_thread, 0);
    }
}

/// Runs on the hook thread right after a grab starts.
unsafe fn take_focus(h: HWND) {
    if !SHARED.grabbing.load(Ordering::Relaxed) {
        return; // already released again
    }
    let (cx, cy) = {
        let st = STATE.lock().unwrap();
        monitor_center(st.park.0, st.park.1)
    };
    PREV_FOREGROUND.store(GetForegroundWindow() as isize, Ordering::Relaxed);
    SetWindowPos(h, HWND_TOPMOST, cx - CAPTURE_SIZE / 2, cy - CAPTURE_SIZE / 2, CAPTURE_SIZE, CAPTURE_SIZE, SWP_SHOWWINDOW);
    // Park in the middle of the monitor: deltas never get clipped at a screen
    // edge, and the cursor sits over the capture window, which hides it.
    STATE.lock().unwrap().park = (cx, cy);
    SetCursorPos(cx, cy);
    force_foreground(h);
    SetCursor(null_mut());
}

unsafe fn give_back_focus(h: HWND) {
    let ret = STATE.lock().unwrap().ret.take();
    if let Some((x, y)) = ret {
        SetCursorPos(x, y);
    }
    ShowWindow(h, SW_HIDE);
    let prev = PREV_FOREGROUND.swap(0, Ordering::Relaxed) as HWND;
    if !prev.is_null() && IsWindow(prev) != 0 {
        force_foreground(prev);
    }
}

unsafe extern "system" fn capture_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SETCURSOR => {
            SetCursor(null_mut());
            1
        }
        WM_NF_GRAB => {
            take_focus(h);
            0
        }
        WM_NF_UNGRAB => {
            give_back_focus(h);
            0
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

unsafe fn create_capture_window() -> HWND {
    let hinst = GetModuleHandleW(null());
    let class: Vec<u16> = "NoopFlowCapture\0".encode_utf16().collect();
    let wc = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(capture_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: class.as_ptr(),
    };
    RegisterClassW(&wc);
    let h = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED,
        class.as_ptr(),
        class.as_ptr(),
        WS_POPUP,
        0,
        0,
        CAPTURE_SIZE,
        CAPTURE_SIZE,
        null_mut(),
        null_mut(),
        hinst,
        null(),
    );
    if !h.is_null() {
        // Alpha 1/255: invisible but still hit-testable, so it owns the cursor shape.
        SetLayeredWindowAttributes(h, 0, 1, LWA_ALPHA);
    }
    h
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
            let (dx, dy) = (x - st.park.0, y - st.park.1);
            if dx == 0 && dy == 0 {
                // Our own SetCursorPos to the park point: let it through.
                return CallNextHookEx(null_mut(), code, wp, lp);
            }
            emit(InputEvent::MouseMove { dx, dy });
            return 1;
        }
        let (lx, ly) = st.last;
        st.last = (x, y);
        let mask = SHARED.neighbors.load(Ordering::Relaxed);
        // Never switch mid-drag: the button would stay pressed on this computer.
        if mask != 0 && !any_button_down() {
            if let Some((side, t)) = edge_hit(virtual_screen(), x as f64, y as f64, (x - lx) as f64, (y - ly) as f64, mask, false) {
                SHARED.grabbing.store(true, Ordering::Relaxed);
                let mut cur = POINT { x: 0, y: 0 };
                GetCursorPos(&mut cur);
                st.park = (cur.x, cur.y);
                *PASS_UP.lock().unwrap() = snapshot_held_modifiers();
                emit(InputEvent::EdgeExit { side, t });
                let cap = CAPTURE.load(Ordering::Relaxed) as HWND;
                if !cap.is_null() {
                    // Do the focus/cursor work after this hook returns.
                    PostMessageW(cap, WM_NF_GRAB, 0, 0);
                }
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
    if code < 0 {
        return CallNextHookEx(null_mut(), code, wp, lp);
    }
    let info = &*(lp as *const KBDLLHOOKSTRUCT);
    if info.dwExtraInfo == MAGIC {
        return CallNextHookEx(null_mut(), code, wp, lp);
    }
    let msg = wp as u32;
    let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    if !SHARED.grabbing.load(Ordering::Relaxed) {
        return CallNextHookEx(null_mut(), code, wp, lp);
    }
    let vk = (info.vkCode & 0xFF) as usize;
    {
        let mut pass = PASS_UP.lock().unwrap();
        if pass[vk] {
            // Modifier held since before the switch: release it here, swallow its auto-repeat.
            if down {
                return 1;
            }
            pass[vk] = false;
            return CallNextHookEx(null_mut(), code, wp, lp);
        }
    }
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
    let (ok_tx, ok_rx) = std::sync::mpsc::channel::<bool>();
    std::thread::Builder::new().name("noopflow-hooks".into()).spawn(move || unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let hmod = GetModuleHandleW(null());
        let m = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), hmod, 0);
        let k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), hmod, 0);
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        STATE.lock().unwrap().last = (p.x, p.y);
        CAPTURE.store(create_capture_window() as isize, Ordering::Relaxed);
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
        st.ret = Some(p);
        p
    };
    SHARED.grabbing.store(false, Ordering::Relaxed);
    // Don't hold the lock here: the hook thread takes it on every mouse event.
    unsafe {
        let cap = CAPTURE.load(Ordering::Relaxed) as HWND;
        if cap.is_null() || PostMessageW(cap, WM_NF_UNGRAB, 0, 0) == 0 {
            STATE.lock().unwrap().ret = None;
            SetCursorPos(x, y);
        }
    }
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
