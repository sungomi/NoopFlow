//! macOS: a CGEventTap for capture (needs Accessibility permission) and
//! CGEventPost for injection. Input source (한/영) switching uses the TIS API on
//! the main thread.

#![allow(non_upper_case_globals, non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{edge_hit, emit, InputEvent, SHARED};
use crate::keys::{self, hid};
use crate::protocol::{Button, Rect, Side};

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFArrayRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CGEventRef = *mut c_void;
type CGEventSourceRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type TISInputSourceRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

type TapCallback = unsafe extern "C" fn(proxy: *mut c_void, etype: u32, event: CGEventRef, user: *mut c_void) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, events: u64, cb: TapCallback, user: *mut c_void) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetIntegerValueField(e: CGEventRef, field: u32) -> i64;
    fn CGEventSetIntegerValueField(e: CGEventRef, field: u32, value: i64);
    fn CGEventGetLocation(e: CGEventRef) -> CGPoint;
    fn CGEventGetFlags(e: CGEventRef) -> u64;
    fn CGEventSetFlags(e: CGEventRef, flags: u64);
    fn CGEventSetType(e: CGEventRef, t: u32);
    fn CGEventCreate(src: CGEventSourceRef) -> CGEventRef;
    fn CGEventCreateMouseEvent(src: CGEventSourceRef, t: u32, p: CGPoint, button: u32) -> CGEventRef;
    fn CGEventCreateKeyboardEvent(src: CGEventSourceRef, key: u16, down: bool) -> CGEventRef;
    fn CGEventCreateScrollWheelEvent2(src: CGEventSourceRef, units: u32, count: u32, w1: i32, w2: i32, w3: i32) -> CGEventRef;
    fn CGEventPost(tap: u32, e: CGEventRef);
    fn CGEventSourceCreate(state: i32) -> CGEventSourceRef;
    fn CGAssociateMouseAndMouseCursorPosition(connected: i32) -> i32;
    fn CGDisplayHideCursor(display: u32) -> i32;
    fn CGDisplayShowCursor(display: u32) -> i32;
    fn CGMainDisplayID() -> u32;
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFArrayGetCount(a: CFArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(a: CFArrayRef, i: isize) -> *const c_void;
    fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFDictionaryCreate(
        alloc: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        n: isize,
        key_cb: *const c_void,
        value_cb: *const c_void,
    ) -> CFDictionaryRef;
    fn CFMachPortCreateRunLoopSource(alloc: *const c_void, port: CFMachPortRef, order: isize) -> *mut c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, src: *mut c_void, mode: CFStringRef);
    fn CFRunLoopRun();
    static kCFTypeDictionaryKeyCallBacks: [usize; 6];
    static kCFTypeDictionaryValueCallBacks: [usize; 5];
    static kCFBooleanTrue: CFTypeRef;
    static kCFRunLoopCommonModes: CFStringRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
    fn TISCreateInputSourceList(properties: CFDictionaryRef, include_all: u8) -> CFArrayRef;
    fn TISGetInputSourceProperty(src: TISInputSourceRef, key: CFStringRef) -> *const c_void;
    fn TISSelectInputSource(src: TISInputSourceRef) -> i32;
    static kTISPropertyInputSourceID: CFStringRef;
    static kTISPropertyInputSourceCategory: CFStringRef;
    static kTISPropertyInputSourceIsSelectCapable: CFStringRef;
    static kTISCategoryKeyboardInputSource: CFStringRef;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDCheckAccess(request_type: u32) -> u32;
    fn IOHIDRequestAccess(request_type: u32) -> bool;
}
const IOHID_REQUEST_LISTEN: u32 = 1;
const IOHID_ACCESS_GRANTED: u32 = 0;

extern "C" {
    static _dispatch_main_q: u8;
    fn dispatch_async_f(queue: *const c_void, ctx: *mut c_void, work: extern "C" fn(*mut c_void));
}

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
}

// Event types
const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const RIGHT_DOWN: u32 = 3;
const RIGHT_UP: u32 = 4;
const MOUSE_MOVED: u32 = 5;
const LEFT_DRAGGED: u32 = 6;
const RIGHT_DRAGGED: u32 = 7;
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const SCROLL: u32 = 22;
const OTHER_DOWN: u32 = 25;
const OTHER_UP: u32 = 26;
const OTHER_DRAGGED: u32 = 27;
const TAP_DISABLED_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_USER: u32 = 0xFFFF_FFFF;

// Fields
const F_CLICK_STATE: u32 = 1;
const F_BUTTON_NUMBER: u32 = 3;
const F_DELTA_X: u32 = 4;
const F_DELTA_Y: u32 = 5;
const F_AUTOREPEAT: u32 = 8;
const F_KEYCODE: u32 = 9;
const F_SCROLL_POINT_1: u32 = 96;
const F_SCROLL_POINT_2: u32 = 97;
const F_USER_DATA: u32 = 42;

// Flags
const FLAG_CAPS: u64 = 0x0001_0000;
const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CTRL: u64 = 0x0004_0000;
const FLAG_ALT: u64 = 0x0008_0000;
const FLAG_CMD: u64 = 0x0010_0000;
const FLAG_NUMPAD: u64 = 0x0020_0000;
const FLAG_FN: u64 = 0x0080_0000;
const FLAG_NONCOALESCED: u64 = 0x0000_0100;

const HID_TAP: u32 = 0;
const MAGIC: i64 = 0x4E4F_4F50;

static TAP: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

struct TapState {
    park: (f64, f64),
    hidden: bool,
    bbox: Rect,
    bbox_at: Option<Instant>,
}

static STATE: Mutex<TapState> = Mutex::new(TapState { park: (0.0, 0.0), hidden: false, bbox: Rect { x: 0, y: 0, w: 1, h: 1 }, bbox_at: None });

pub fn screens() -> Vec<Rect> {
    let mut ids = [0u32; 16];
    let mut n = 0u32;
    let mut v = Vec::new();
    unsafe {
        if CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut n) == 0 {
            for id in &ids[..n as usize] {
                let r = CGDisplayBounds(*id);
                v.push(Rect { x: r.origin.x as i32, y: r.origin.y as i32, w: r.size.width as i32, h: r.size.height as i32 });
            }
        }
    }
    if v.is_empty() {
        v.push(Rect { x: 0, y: 0, w: 1440, h: 900 });
    }
    v
}

fn bbox(st: &mut TapState) -> Rect {
    if st.bbox_at.map_or(true, |t| t.elapsed() > Duration::from_secs(2)) {
        st.bbox = Rect::union(&screens());
        st.bbox_at = Some(Instant::now());
    }
    st.bbox
}

/// Device-dependent modifier bit for a modifier keycode.
fn modifier_bit(keycode: u16) -> Option<u64> {
    Some(match keycode {
        0x3B => 0x0001,   // LCtrl
        0x38 => 0x0002,   // LShift
        0x3C => 0x0004,   // RShift
        0x37 => 0x0008,   // LCmd
        0x36 => 0x0010,   // RCmd
        0x3A => 0x0020,   // LAlt
        0x3D => 0x0040,   // RAlt
        0x3E => 0x2000,   // RCtrl
        _ => return None,
    })
}

unsafe fn begin_grab(st: &mut TapState, loc: CGPoint) {
    SHARED.grabbing.store(true, Ordering::Relaxed);
    st.park = (loc.x, loc.y);
    CGAssociateMouseAndMouseCursorPosition(0);
    if !st.hidden {
        CGDisplayHideCursor(CGMainDisplayID());
        st.hidden = true;
    }
}

unsafe extern "C" fn tap_cb(_proxy: *mut c_void, etype: u32, ev: CGEventRef, _user: *mut c_void) -> CGEventRef {
    if etype == TAP_DISABLED_TIMEOUT || etype == TAP_DISABLED_USER {
        let t = TAP.load(Ordering::Relaxed);
        if !t.is_null() {
            CGEventTapEnable(t, true);
        }
        return ev;
    }
    if CGEventGetIntegerValueField(ev, F_USER_DATA) == MAGIC {
        return ev;
    }
    let grabbing = SHARED.grabbing.load(Ordering::Relaxed);
    let observe = SHARED.observe_local.load(Ordering::Relaxed);
    match etype {
        MOUSE_MOVED | LEFT_DRAGGED | RIGHT_DRAGGED | OTHER_DRAGGED => {
            let dx = CGEventGetIntegerValueField(ev, F_DELTA_X) as i32;
            let dy = CGEventGetIntegerValueField(ev, F_DELTA_Y) as i32;
            if grabbing {
                if dx != 0 || dy != 0 {
                    emit(InputEvent::MouseMove { dx, dy });
                }
                return null_mut();
            }
            let mask = SHARED.neighbors.load(Ordering::Relaxed);
            if mask != 0 {
                let loc = CGEventGetLocation(ev);
                let mut st = STATE.lock().unwrap();
                let b = bbox(&mut st);
                if let Some((side, t)) = edge_hit(b, loc.x, loc.y, dx as f64, dy as f64, mask, true) {
                    begin_grab(&mut st, loc);
                    emit(InputEvent::EdgeExit { side, t });
                    return null_mut();
                }
            }
            ev
        }
        LEFT_DOWN | LEFT_UP | RIGHT_DOWN | RIGHT_UP | OTHER_DOWN | OTHER_UP => {
            if !grabbing {
                if observe && matches!(etype, LEFT_DOWN | RIGHT_DOWN | OTHER_DOWN) {
                    emit(InputEvent::LocalClick);
                }
                return ev;
            }
            let button = match etype {
                LEFT_DOWN | LEFT_UP => Button::Left,
                RIGHT_DOWN | RIGHT_UP => Button::Right,
                _ => match CGEventGetIntegerValueField(ev, F_BUTTON_NUMBER) {
                    3 => Button::Back,
                    4 => Button::Forward,
                    _ => Button::Middle,
                },
            };
            let down = matches!(etype, LEFT_DOWN | RIGHT_DOWN | OTHER_DOWN);
            emit(InputEvent::Button { button, down });
            null_mut()
        }
        SCROLL => {
            if !grabbing {
                return ev;
            }
            // Point deltas: ~40pt per notch -> 120 units.
            let py = CGEventGetIntegerValueField(ev, F_SCROLL_POINT_1) as i32;
            let px = CGEventGetIntegerValueField(ev, F_SCROLL_POINT_2) as i32;
            if px != 0 || py != 0 {
                emit(InputEvent::Wheel { dx: px * 3, dy: py * 3 });
            }
            null_mut()
        }
        KEY_DOWN | KEY_UP => {
            let kc = CGEventGetIntegerValueField(ev, F_KEYCODE) as u16;
            let Some(code) = keys::from_mac(kc) else {
                return if grabbing { null_mut() } else { ev };
            };
            let down = etype == KEY_DOWN;
            if grabbing {
                emit(InputEvent::Key { code, down });
                return null_mut();
            }
            if observe {
                emit(InputEvent::LocalKey { code, down });
            }
            ev
        }
        FLAGS_CHANGED => {
            let kc = CGEventGetIntegerValueField(ev, F_KEYCODE) as u16;
            let flags = CGEventGetFlags(ev);
            let Some(code) = keys::from_mac(kc) else { return if grabbing { null_mut() } else { ev } };
            let events: Vec<(u16, bool)> = if kc == 0x39 {
                vec![(code, true), (code, false)]
            } else if let Some(bit) = modifier_bit(kc) {
                vec![(code, flags & bit != 0)]
            } else {
                vec![]
            };
            for (code, down) in events {
                if grabbing {
                    emit(InputEvent::Key { code, down });
                } else if observe {
                    emit(InputEvent::LocalKey { code, down });
                }
            }
            if grabbing {
                null_mut()
            } else {
                ev
            }
        }
        _ => ev,
    }
}

const TAP_MASK: u64 = (1 << LEFT_DOWN)
    | (1 << LEFT_UP)
    | (1 << RIGHT_DOWN)
    | (1 << RIGHT_UP)
    | (1 << MOUSE_MOVED)
    | (1 << LEFT_DRAGGED)
    | (1 << RIGHT_DRAGGED)
    | (1 << KEY_DOWN)
    | (1 << KEY_UP)
    | (1 << FLAGS_CHANGED)
    | (1 << SCROLL)
    | (1 << OTHER_DOWN)
    | (1 << OTHER_UP)
    | (1 << OTHER_DRAGGED);

pub(crate) fn start_impl() -> anyhow::Result<()> {
    std::thread::Builder::new().name("noopflow-tap".into()).spawn(|| unsafe {
        loop {
            // HID-level first; fall back to the session tap (1), which some setups allow when HID doesn't.
            let mut tap = CGEventTapCreate(HID_TAP, 0, 0, TAP_MASK, tap_cb, null_mut());
            if tap.is_null() {
                tap = CGEventTapCreate(1, 0, 0, TAP_MASK, tap_cb, null_mut());
            }
            if tap.is_null() {
                emit(InputEvent::Permission(false));
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            emit(InputEvent::Permission(true));
            TAP.store(tap, Ordering::Relaxed);
            let src = CFMachPortCreateRunLoopSource(null(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            CFRunLoopRun();
            break;
        }
    })?;
    Ok(())
}

// ------------------------------------------------------------------ injection

struct Injector {
    src: usize,
    mods: u64,
    caps: bool,
    buttons: u8,
    last_down: Option<(Instant, f64, f64, Button)>,
    clicks: i64,
    held: Vec<u16>,
}

static INJ: Mutex<Injector> = Mutex::new(Injector { src: 0, mods: 0, caps: false, buttons: 0, last_down: None, clicks: 1, held: Vec::new() });

impl Injector {
    fn source(&mut self) -> CGEventSourceRef {
        if self.src == 0 {
            self.src = unsafe { CGEventSourceCreate(1) } as usize; // HID system state
        }
        self.src as CGEventSourceRef
    }
    fn flags(&self) -> u64 {
        self.mods | if self.caps { FLAG_CAPS } else { 0 } | FLAG_NONCOALESCED
    }
}

unsafe fn post(ev: CGEventRef) {
    if ev.is_null() {
        return;
    }
    CGEventSetIntegerValueField(ev, F_USER_DATA, MAGIC);
    CGEventPost(HID_TAP, ev);
    CFRelease(ev as CFTypeRef);
}

pub fn ungrab(entry: Option<(Side, f32)>) {
    let mut st = STATE.lock().unwrap();
    let (x, y) = match entry {
        Some((side, t)) => bbox(&mut st).point_on_side(side, t, 2.0),
        None => st.park,
    };
    unsafe {
        CGAssociateMouseAndMouseCursorPosition(1);
        let mut inj = INJ.lock().unwrap();
        let src = inj.source();
        let ev = CGEventCreateMouseEvent(src, MOUSE_MOVED, CGPoint { x, y }, 0);
        post(ev);
        if st.hidden {
            CGDisplayShowCursor(CGMainDisplayID());
            st.hidden = false;
        }
    }
    SHARED.grabbing.store(false, Ordering::Relaxed);
}

pub fn inject_move(x: f64, y: f64, dx: i32, dy: i32) {
    let mut inj = INJ.lock().unwrap();
    let (t, b) = if inj.buttons & Button::Left.bit() != 0 {
        (LEFT_DRAGGED, 0)
    } else if inj.buttons & Button::Right.bit() != 0 {
        (RIGHT_DRAGGED, 1)
    } else if inj.buttons != 0 {
        (OTHER_DRAGGED, 2)
    } else {
        (MOUSE_MOVED, 0)
    };
    unsafe {
        let ev = CGEventCreateMouseEvent(inj.source(), t, CGPoint { x, y }, b);
        if ev.is_null() {
            return;
        }
        CGEventSetIntegerValueField(ev, F_DELTA_X, dx as i64);
        CGEventSetIntegerValueField(ev, F_DELTA_Y, dy as i64);
        CGEventSetFlags(ev, inj.flags());
        post(ev);
    }
}

pub fn inject_button(b: Button, down: bool, x: f64, y: f64) {
    let mut inj = INJ.lock().unwrap();
    if down {
        let now = Instant::now();
        let again = matches!(inj.last_down, Some((t, lx, ly, lb)) if lb == b && now.duration_since(t) < Duration::from_millis(450) && (lx - x).abs() < 6.0 && (ly - y).abs() < 6.0);
        inj.clicks = if again { (inj.clicks + 1).min(3) } else { 1 };
        inj.last_down = Some((now, x, y, b));
        inj.buttons |= b.bit();
    } else {
        inj.buttons &= !b.bit();
    }
    let (t, num) = match (b, down) {
        (Button::Left, true) => (LEFT_DOWN, 0),
        (Button::Left, false) => (LEFT_UP, 0),
        (Button::Right, true) => (RIGHT_DOWN, 1),
        (Button::Right, false) => (RIGHT_UP, 1),
        (Button::Middle, d) => (if d { OTHER_DOWN } else { OTHER_UP }, 2),
        (Button::Back, d) => (if d { OTHER_DOWN } else { OTHER_UP }, 3),
        (Button::Forward, d) => (if d { OTHER_DOWN } else { OTHER_UP }, 4),
    };
    unsafe {
        let ev = CGEventCreateMouseEvent(inj.source(), t, CGPoint { x, y }, num.min(2));
        if ev.is_null() {
            return;
        }
        CGEventSetIntegerValueField(ev, F_BUTTON_NUMBER, num as i64);
        CGEventSetIntegerValueField(ev, F_CLICK_STATE, inj.clicks);
        CGEventSetFlags(ev, inj.flags());
        post(ev);
    }
}

pub fn inject_wheel(dx: i32, dy: i32) {
    let mut inj = INJ.lock().unwrap();
    unsafe {
        let ev = CGEventCreateScrollWheelEvent2(inj.source(), 0, 2, dy / 3, dx / 3, 0);
        if ev.is_null() {
            return;
        }
        CGEventSetFlags(ev, inj.flags());
        post(ev);
    }
}

fn mod_flags(code: u16) -> u64 {
    match code {
        hid::LCTRL => FLAG_CTRL | 0x0001,
        hid::LSHIFT => FLAG_SHIFT | 0x0002,
        hid::RSHIFT => FLAG_SHIFT | 0x0004,
        hid::LGUI => FLAG_CMD | 0x0008,
        hid::RGUI => FLAG_CMD | 0x0010,
        hid::LALT => FLAG_ALT | 0x0020,
        hid::RALT => FLAG_ALT | 0x0040,
        hid::RCTRL => FLAG_CTRL | 0x2000,
        _ => 0,
    }
}

fn recompute_mods(held: &[u16]) -> u64 {
    held.iter().fold(0, |acc, c| acc | mod_flags(*c))
}

pub fn inject_key(code: u16, down: bool) {
    let Some(kc) = keys::to_mac(code) else { return };
    let mut inj = INJ.lock().unwrap();
    let repeat = down && inj.held.contains(&code);
    if down && !repeat {
        inj.held.push(code);
    } else if !down {
        inj.held.retain(|c| *c != code);
    }
    unsafe {
        if keys::is_modifier(code) {
            inj.mods = recompute_mods(&inj.held);
            let ev = CGEventCreateKeyboardEvent(inj.source(), kc, down);
            if ev.is_null() {
                return;
            }
            CGEventSetType(ev, FLAGS_CHANGED);
            CGEventSetFlags(ev, inj.flags());
            post(ev);
            return;
        }
        if code == hid::CAPS_LOCK {
            if down {
                inj.caps = !inj.caps;
                let ev = CGEventCreateKeyboardEvent(inj.source(), kc, true);
                if !ev.is_null() {
                    CGEventSetType(ev, FLAGS_CHANGED);
                    CGEventSetFlags(ev, inj.flags());
                    post(ev);
                }
            }
            return;
        }
        let extra = match code {
            0x4F..=0x52 => FLAG_NUMPAD | FLAG_FN,     // arrows
            0x49..=0x4E | 0x3A..=0x45 => FLAG_FN,      // nav cluster, F-keys
            0x54..=0x63 | 0x67 => FLAG_NUMPAD,          // keypad
            _ => 0,
        };
        let ev = CGEventCreateKeyboardEvent(inj.source(), kc, down);
        if ev.is_null() {
            return;
        }
        CGEventSetFlags(ev, inj.flags() | extra);
        if repeat {
            CGEventSetIntegerValueField(ev, F_AUTOREPEAT, 1);
        }
        post(ev);
    }
}

// ------------------------------------------------------------- input source

unsafe fn cfstring_to_string(s: CFStringRef) -> String {
    if s.is_null() {
        return String::new();
    }
    let mut buf = [0 as c_char; 256];
    if CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as isize, 0x0800_0100) == 0 {
        return String::new();
    }
    std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
}

unsafe fn source_id(src: TISInputSourceRef) -> String {
    cfstring_to_string(TISGetInputSourceProperty(src, kTISPropertyInputSourceID) as CFStringRef)
}

fn is_korean(id: &str) -> bool {
    let l = id.to_ascii_lowercase();
    l.contains("korean") || l.contains("gureum") || l.contains(".han")
}

extern "C" fn toggle_on_main(_ctx: *mut c_void) {
    unsafe {
        let cur = TISCopyCurrentKeyboardInputSource();
        let cur_id = if cur.is_null() { String::new() } else { source_id(cur) };
        if !cur.is_null() {
            CFRelease(cur as CFTypeRef);
        }
        let want_korean = !is_korean(&cur_id);

        let keys = [kTISPropertyInputSourceCategory, kTISPropertyInputSourceIsSelectCapable];
        let vals = [kTISCategoryKeyboardInputSource, kCFBooleanTrue];
        let filter = CFDictionaryCreate(
            null(),
            keys.as_ptr(),
            vals.as_ptr(),
            2,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        );
        let list = TISCreateInputSourceList(filter, 0);
        if !filter.is_null() {
            CFRelease(filter);
        }
        if list.is_null() {
            return;
        }
        let mut best: Option<(i32, TISInputSourceRef)> = None;
        for i in 0..CFArrayGetCount(list) {
            let src = CFArrayGetValueAtIndex(list, i) as TISInputSourceRef;
            let id = source_id(src);
            if id == cur_id {
                continue;
            }
            let score = if want_korean {
                if !is_korean(&id) {
                    continue;
                }
                if id.starts_with("com.apple.inputmethod.Korean.") { 3 } else { 2 }
            } else {
                if is_korean(&id) {
                    continue;
                }
                if id == "com.apple.keylayout.ABC" || id == "com.apple.keylayout.US" {
                    3
                } else if id.starts_with("com.apple.keylayout.") {
                    2
                } else {
                    1
                }
            };
            if best.map_or(true, |(s, _)| score > s) {
                best = Some((score, src));
            }
        }
        if let Some((_, src)) = best {
            TISSelectInputSource(src);
        }
        CFRelease(list);
    }
}

fn send_ctrl_space() {
    inject_key(hid::LCTRL, true);
    inject_key(hid::SPACE, true);
    inject_key(hid::SPACE, false);
    inject_key(hid::LCTRL, false);
}

/// Switch between 한글 and ABC. TIS must run on the main thread on recent macOS.
pub fn toggle_input_source(method: &str) {
    if method == "ctrl_space" {
        send_ctrl_space();
        return;
    }
    unsafe {
        dispatch_async_f(&_dispatch_main_q as *const u8 as *const c_void, null_mut(), toggle_on_main);
    }
}

pub fn cursor_pos() -> (f64, f64) {
    unsafe {
        let ev = CGEventCreate(null_mut());
        if ev.is_null() {
            return (0.0, 0.0);
        }
        let p = CGEventGetLocation(ev);
        CFRelease(ev as CFTypeRef);
        (p.x, p.y)
    }
}

pub fn clipboard_seq() -> u64 {
    unsafe {
        let cls = objc_getClass(c"NSPasteboard".as_ptr());
        if cls.is_null() {
            return 0;
        }
        let send_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send_int: unsafe extern "C" fn(*mut c_void, *mut c_void) -> isize = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let pb = send_obj(cls, sel_registerName(c"generalPasteboard".as_ptr()));
        if pb.is_null() {
            return 0;
        }
        send_int(pb, sel_registerName(c"changeCount".as_ptr())) as u64
    }
}

pub fn permission_ok() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

pub fn permission_status() -> super::PermStatus {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    super::PermStatus {
        accessibility: unsafe { AXIsProcessTrusted() != 0 },
        input_monitoring: unsafe { IOHIDCheckAccess(IOHID_REQUEST_LISTEN) == IOHID_ACCESS_GRANTED },
        // Running from the mounted .dmg or a Gatekeeper-translocated copy: permissions
        // granted there don't stick, so the app must be moved to /Applications first.
        installed: !(exe.starts_with("/Volumes/") || exe.contains("/AppTranslocation/")),
    }
}

pub fn request_input_monitoring() {
    unsafe {
        IOHIDRequestAccess(IOHID_REQUEST_LISTEN);
    }
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
        .spawn();
}

pub fn request_permission() {
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let vals = [kCFBooleanTrue];
        let opts = CFDictionaryCreate(
            null(),
            keys.as_ptr(),
            vals.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        );
        AXIsProcessTrustedWithOptions(opts);
        if !opts.is_null() {
            CFRelease(opts);
        }
    }
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}
