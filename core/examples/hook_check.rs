//! Windows keyboard-hook check: installs the real low-level hooks, simulates
//! "controlling another computer", injects keystrokes and verifies every key is
//! captured (and that a modifier held across the switch is released locally).
//!
//!   cargo run -p noopflow-core --example hook_check
//!
//! Keys injected while "controlling" are swallowed by the hook, so they don't
//! reach the apps on this PC.

#[cfg(windows)]
fn main() {
    use std::time::{Duration, Instant};

    use noopflow_core::keys::hid;
    use noopflow_core::platform::{self, InputEvent};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

    fn key(vk: u16, scan: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        }
    }
    fn send(inputs: &[INPUT]) {
        unsafe {
            SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }
    fn tap(vk: u16, scan: u16, ext: bool) -> Vec<INPUT> {
        let e = if ext { KEYEVENTF_EXTENDEDKEY } else { 0 };
        vec![key(vk, scan, e), key(vk, scan, e | KEYEVENTF_KEYUP)]
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    platform::start(tx).expect("hooks");
    std::thread::sleep(Duration::from_millis(300));
    while rx.try_recv().is_ok() {}

    let collect = |rx: &mut tokio::sync::mpsc::UnboundedReceiver<InputEvent>| {
        let mut got = Vec::new();
        let end = Instant::now() + Duration::from_millis(400);
        while Instant::now() < end {
            while let Ok(ev) = rx.try_recv() {
                if let InputEvent::Key { code, down } = ev {
                    got.push((code, down));
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        got
    };

    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name} {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };

    // 1) While controlling: letters, digits, Enter, Space, Backspace, both Shifts,
    //    Ctrl, Alt, Win, arrows, F-keys, Caps Lock, 한/영 and 한자 all arrive.
    platform::debug_set_grabbing(true);
    let mut inputs = Vec::new();
    let cases: &[(u16, u16, bool, u16, &str)] = &[
        (0x41, 0x1E, false, hid::A, "A"),
        (0x31, 0x02, false, 0x1E, "1"),
        (0x0D, 0x1C, false, 0x28, "Enter"),
        (0x20, 0x39, false, hid::SPACE, "Space"),
        (0x08, 0x0E, false, 0x2A, "Backspace"),
        (0xA0, 0x2A, false, hid::LSHIFT, "LShift"),
        (0xA1, 0x36, false, hid::RSHIFT, "RShift"),
        (0xA2, 0x1D, false, hid::LCTRL, "LCtrl"),
        (0xA4, 0x38, false, hid::LALT, "LAlt"),
        (0x5B, 0x5B, true, hid::LGUI, "Win"),
        (0x25, 0x4B, true, 0x50, "Left"),
        (0x74, 0x3F, false, 0x3E, "F5"),
        (0x14, 0x3A, false, hid::CAPS_LOCK, "CapsLock"),
        (0x15, 0x72, false, hid::LANG1, "한/영"),
        (0x19, 0x71, false, hid::LANG2, "한자"),
        (0x09, 0x0F, false, 0x2B, "Tab"),
    ];
    for (vk, scan, ext, _, _) in cases {
        inputs.extend(tap(*vk, *scan, *ext));
    }
    send(&inputs);
    let got = collect(&mut rx);
    for (_, _, _, code, name) in cases {
        let ok = got.contains(&(*code, true)) && got.contains(&(*code, false));
        check(&format!("key {name}"), ok, String::new());
    }

    // 2) Alt+Tab while controlling: both forwarded (so Windows' switcher never sees them).
    send(&[key(0xA4, 0x38, 0), key(0x09, 0x0F, 0), key(0x09, 0x0F, KEYEVENTF_KEYUP), key(0xA4, 0x38, KEYEVENTF_KEYUP)]);
    let got = collect(&mut rx);
    check(
        "Alt+Tab forwarded",
        got == vec![(hid::LALT, true), (0x2B, true), (0x2B, false), (hid::LALT, false)],
        format!("{got:?}"),
    );

    // 3) 한/영 pressed with no key-up (as Korean keyboards do) must not break later presses.
    platform::debug_set_grabbing(false);
    send(&[key(0x7C, 0x64, 0)]); // F13 down only, harmless stand-in for a key-up that never comes
    std::thread::sleep(Duration::from_millis(100));
    platform::debug_set_grabbing(true);
    send(&tap(0x7C, 0x64, false));
    let got = collect(&mut rx);
    check("key with missing key-up still forwarded later", got.contains(&(0x68, true)), format!("{got:?}"));

    // 4) Shift held before switching: its key-up goes to Windows, the next key is forwarded.
    platform::debug_set_grabbing(false);
    send(&[key(0xA0, 0x2A, 0)]);
    std::thread::sleep(Duration::from_millis(100));
    platform::debug_set_grabbing(true);
    send(&[key(0xA0, 0x2A, KEYEVENTF_KEYUP)]);
    send(&tap(0x41, 0x1E, false));
    let got = collect(&mut rx);
    let shift_released_locally = unsafe { (GetAsyncKeyState(0xA0) as u16) & 0x8000 == 0 };
    check(
        "held Shift released on Windows, A forwarded",
        shift_released_locally && got == vec![(hid::A, true), (hid::A, false)],
        format!("{got:?} shift_up_on_windows={shift_released_locally}"),
    );

    // 5) This PC as the *target* (a Mac controls it): keys injected by NoopFlow must
    //    reach Windows and must not be re-captured by our own hook.
    platform::debug_set_grabbing(false);
    while rx.try_recv().is_ok() {}
    platform::inject_key(hid::RSHIFT, true);
    std::thread::sleep(Duration::from_millis(80));
    let down_seen = unsafe { (GetAsyncKeyState(0xA1) as u16) & 0x8000 != 0 };
    platform::inject_key(hid::RSHIFT, false);
    std::thread::sleep(Duration::from_millis(80));
    let up_seen = unsafe { (GetAsyncKeyState(0xA1) as u16) & 0x8000 == 0 };
    platform::inject_key(hid::LCTRL, true);
    std::thread::sleep(Duration::from_millis(80));
    let ctrl_seen = unsafe { (GetAsyncKeyState(0xA2) as u16) & 0x8000 != 0 };
    platform::inject_key(hid::LCTRL, false);
    let recaptured = collect(&mut rx);
    check(
        "injected keys reach Windows (target side)",
        down_seen && up_seen && ctrl_seen && recaptured.is_empty(),
        format!("rshift_down={down_seen} rshift_up={up_seen} ctrl_down={ctrl_seen} recaptured={recaptured:?}"),
    );

    platform::debug_set_grabbing(false);
    // Clean up the F13 that was left "down" on purpose in test 3.
    send(&[key(0x7C, 0x64, KEYEVENTF_KEYUP)]);
    println!();
    if failures == 0 {
        println!("ALL PASSED");
    } else {
        println!("{failures} FAILED");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    println!("Windows only");
}
