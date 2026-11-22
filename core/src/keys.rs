//! Cross-platform key codes. Everything on the wire is a USB HID usage id
//! (keyboard page 0x07), so Windows scancodes and macOS virtual keycodes are
//! translated at the edges.

pub type Hid = u16;

#[allow(dead_code)]
pub mod hid {
    pub const A: u16 = 0x04;
    pub const C: u16 = 0x06;
    pub const H: u16 = 0x0B;
    pub const V: u16 = 0x19;
    pub const SPACE: u16 = 0x2C;
    pub const CAPS_LOCK: u16 = 0x39;
    pub const NUM_LOCK: u16 = 0x53;
    pub const PAUSE: u16 = 0x48;
    pub const LANG1: u16 = 0x90; // 한/영
    pub const LANG2: u16 = 0x91; // 한자
    pub const MUTE: u16 = 0x7F;
    pub const VOLUME_UP: u16 = 0x80;
    pub const VOLUME_DOWN: u16 = 0x81;
    // Media transport keys live on the HID consumer page; NoopFlow gives them
    // private codes above the keyboard page.
    pub const MEDIA_PLAY_PAUSE: u16 = 0x100;
    pub const MEDIA_NEXT: u16 = 0x101;
    pub const MEDIA_PREV: u16 = 0x102;
    pub const MEDIA_STOP: u16 = 0x103;
    pub const LCTRL: u16 = 0xE0;
    pub const LSHIFT: u16 = 0xE1;
    pub const LALT: u16 = 0xE2;
    pub const LGUI: u16 = 0xE3;
    pub const RCTRL: u16 = 0xE4;
    pub const RSHIFT: u16 = 0xE5;
    pub const RALT: u16 = 0xE6;
    pub const RGUI: u16 = 0xE7;
}

pub fn is_modifier(code: Hid) -> bool {
    (0xE0..=0xE7).contains(&code)
}

/// Map modifiers between a Windows and a Mac keyboard.
///
/// * `"position"` (default): keys in the same physical spot do the same job.
///   Alt (next to the space bar) acts as ⌘, the Windows key as ⌥, Ctrl stays Ctrl,
///   and the other way round for a Mac keyboard driving Windows.
/// * `"ctrl_cmd"`: Ctrl and ⌘/Win trade places (Ctrl+C on Windows = ⌘C on Mac).
/// * anything else: no remapping.
pub fn map_cross_os(code: Hid, layout: &str) -> Hid {
    match layout {
        "position" => swap_alt_gui(code),
        "ctrl_cmd" => swap_ctrl_gui(code),
        _ => code,
    }
}

pub fn swap_alt_gui(code: Hid) -> Hid {
    match code {
        hid::LALT => hid::LGUI,
        hid::LGUI => hid::LALT,
        hid::RALT => hid::RGUI,
        hid::RGUI => hid::RALT,
        other => other,
    }
}

/// Swap Ctrl <-> Cmd/Win so a Windows keyboard feels native on a Mac and vice versa.
pub fn swap_ctrl_gui(code: Hid) -> Hid {
    match code {
        hid::LCTRL => hid::LGUI,
        hid::LGUI => hid::LCTRL,
        hid::RCTRL => hid::RGUI,
        hid::RGUI => hid::RCTRL,
        other => other,
    }
}

/// (scancode set 1, extended, hid)
const WIN_TABLE: &[(u16, bool, Hid)] = &[
    (0x01, false, 0x29), // Esc
    (0x02, false, 0x1E), (0x03, false, 0x1F), (0x04, false, 0x20), (0x05, false, 0x21),
    (0x06, false, 0x22), (0x07, false, 0x23), (0x08, false, 0x24), (0x09, false, 0x25),
    (0x0A, false, 0x26), (0x0B, false, 0x27),
    (0x0C, false, 0x2D), // -
    (0x0D, false, 0x2E), // =
    (0x0E, false, 0x2A), // Backspace
    (0x0F, false, 0x2B), // Tab
    (0x10, false, 0x14), (0x11, false, 0x1A), (0x12, false, 0x08), (0x13, false, 0x15),
    (0x14, false, 0x17), (0x15, false, 0x1C), (0x16, false, 0x18), (0x17, false, 0x0C),
    (0x18, false, 0x12), (0x19, false, 0x13), // Q..P
    (0x1A, false, 0x2F), (0x1B, false, 0x30), // [ ]
    (0x1C, false, 0x28), // Enter
    (0x1D, false, 0xE0), // LCtrl
    (0x1E, false, 0x04), (0x1F, false, 0x16), (0x20, false, 0x07), (0x21, false, 0x09),
    (0x22, false, 0x0A), (0x23, false, 0x0B), (0x24, false, 0x0D), (0x25, false, 0x0E),
    (0x26, false, 0x0F), // A..L
    (0x27, false, 0x33), (0x28, false, 0x34), (0x29, false, 0x35), // ; ' `
    (0x2A, false, 0xE1), // LShift
    (0x2B, false, 0x31), // \
    (0x2C, false, 0x1D), (0x2D, false, 0x1B), (0x2E, false, 0x06), (0x2F, false, 0x19),
    (0x30, false, 0x05), (0x31, false, 0x11), (0x32, false, 0x10), // Z..M
    (0x33, false, 0x36), (0x34, false, 0x37), (0x35, false, 0x38), // , . /
    (0x36, false, 0xE5), // RShift
    (0x37, false, 0x55), // KP *
    (0x38, false, 0xE2), // LAlt
    (0x39, false, 0x2C), // Space
    (0x3A, false, 0x39), // Caps
    (0x3B, false, 0x3A), (0x3C, false, 0x3B), (0x3D, false, 0x3C), (0x3E, false, 0x3D),
    (0x3F, false, 0x3E), (0x40, false, 0x3F), (0x41, false, 0x40), (0x42, false, 0x41),
    (0x43, false, 0x42), (0x44, false, 0x43), // F1..F10
    (0x46, false, 0x47), // Scroll lock
    (0x47, false, 0x5F), (0x48, false, 0x60), (0x49, false, 0x61), (0x4A, false, 0x56),
    (0x4B, false, 0x5C), (0x4C, false, 0x5D), (0x4D, false, 0x5E), (0x4E, false, 0x57),
    (0x4F, false, 0x59), (0x50, false, 0x5A), (0x51, false, 0x5B), (0x52, false, 0x62),
    (0x53, false, 0x63), // keypad
    (0x56, false, 0x64), // Intl backslash
    (0x59, false, 0x67), // KP =
    (0x57, false, 0x44), (0x58, false, 0x45), // F11 F12
    (0x64, false, 0x68), (0x65, false, 0x69), (0x66, false, 0x6A), (0x67, false, 0x6B), // F13..F16
    (0x68, false, 0x6C), (0x69, false, 0x6D), (0x6A, false, 0x6E), (0x6B, false, 0x6F), // F17..F20
    (0x71, false, 0x91), // Hanja
    (0x72, false, 0x90), // Hangul
    (0x73, false, 0x87), // Ro
    (0x7D, false, 0x89), // Yen
    (0x1C, true, 0x58), // KP Enter
    (0x1D, true, 0xE4), // RCtrl
    (0x35, true, 0x54), // KP /
    (0x37, true, 0x46), // PrintScreen
    (0x38, true, 0xE6), // RAlt
    (0x47, true, 0x4A), // Home
    (0x48, true, 0x52), // Up
    (0x49, true, 0x4B), // PgUp
    (0x4B, true, 0x50), // Left
    (0x4D, true, 0x4F), // Right
    (0x4F, true, 0x4D), // End
    (0x50, true, 0x51), // Down
    (0x51, true, 0x4E), // PgDn
    (0x52, true, 0x49), // Insert
    (0x53, true, 0x4C), // Delete
    (0x5B, true, 0xE3), // LWin
    (0x5C, true, 0xE7), // RWin
    (0x5D, true, 0x65), // Menu
];

const VK_PAUSE: u32 = 0x13;
const VK_NUMLOCK: u32 = 0x90;
const VK_HANGUL: u32 = 0x15;
const VK_HANJA: u32 = 0x19;

/// True for the shift Windows injects around navigation keys when NumLock is on
/// (scancode E0 2A). Real Shift keys never carry the extended flag with 0x2A;
/// note that a real *right* Shift may be reported as extended 0x36 on some
/// keyboards, so that one must not be dropped.
pub fn is_fake_shift(scan: u32, extended: bool) -> bool {
    extended && (scan & 0xFF) == 0x2A
}

/// Windows low-level hook -> HID.
pub fn from_win(scan: u32, extended: bool, vk: u32) -> Option<Hid> {
    match vk {
        VK_HANGUL => return Some(hid::LANG1),
        VK_HANJA => return Some(hid::LANG2),
        VK_PAUSE => return Some(hid::PAUSE),
        VK_NUMLOCK => return Some(hid::NUM_LOCK),
        // Modifiers: the hook's left/right virtual keys are reliable, scancode flags are not.
        0xA0 => return Some(hid::LSHIFT),
        0xA1 => return Some(hid::RSHIFT),
        0xA2 => return Some(hid::LCTRL),
        0xA3 => return Some(hid::RCTRL),
        0xA4 => return Some(hid::LALT),
        0xA5 => return Some(hid::RALT),
        0x5B => return Some(hid::LGUI),
        0x5C => return Some(hid::RGUI),
        // Media keys (volume, playback).
        0xAD => return Some(hid::MUTE),
        0xAE => return Some(hid::VOLUME_DOWN),
        0xAF => return Some(hid::VOLUME_UP),
        0xB0 => return Some(hid::MEDIA_NEXT),
        0xB1 => return Some(hid::MEDIA_PREV),
        0xB2 => return Some(hid::MEDIA_STOP),
        0xB3 => return Some(hid::MEDIA_PLAY_PAUSE),
        _ => {}
    }
    let scan = (scan & 0xFF) as u16;
    WIN_TABLE
        .iter()
        .find(|(s, e, _)| *s == scan && *e == extended)
        .map(|(_, _, h)| *h)
}

pub enum WinKey {
    Scan { scan: u16, extended: bool },
    Vk { vk: u16, extended: bool },
}

/// HID -> what to inject on Windows.
pub fn to_win(code: Hid) -> Option<WinKey> {
    match code {
        hid::LANG1 => return Some(WinKey::Vk { vk: VK_HANGUL as u16, extended: false }),
        hid::LANG2 => return Some(WinKey::Vk { vk: VK_HANJA as u16, extended: false }),
        hid::PAUSE => return Some(WinKey::Vk { vk: VK_PAUSE as u16, extended: false }),
        hid::NUM_LOCK => return Some(WinKey::Vk { vk: VK_NUMLOCK as u16, extended: true }),
        hid::MUTE => return Some(WinKey::Vk { vk: 0xAD, extended: true }),
        hid::VOLUME_DOWN => return Some(WinKey::Vk { vk: 0xAE, extended: true }),
        hid::VOLUME_UP => return Some(WinKey::Vk { vk: 0xAF, extended: true }),
        hid::MEDIA_NEXT => return Some(WinKey::Vk { vk: 0xB0, extended: true }),
        hid::MEDIA_PREV => return Some(WinKey::Vk { vk: 0xB1, extended: true }),
        hid::MEDIA_STOP => return Some(WinKey::Vk { vk: 0xB2, extended: true }),
        hid::MEDIA_PLAY_PAUSE => return Some(WinKey::Vk { vk: 0xB3, extended: true }),
        _ => {}
    }
    WIN_TABLE
        .iter()
        .find(|(_, _, h)| *h == code)
        .map(|(s, e, _)| WinKey::Scan { scan: *s, extended: *e })
}

/// (mac virtual keycode, hid)
const MAC_TABLE: &[(u16, Hid)] = &[
    (0x00, 0x04), (0x0B, 0x05), (0x08, 0x06), (0x02, 0x07), (0x0E, 0x08), (0x03, 0x09),
    (0x05, 0x0A), (0x04, 0x0B), (0x22, 0x0C), (0x26, 0x0D), (0x28, 0x0E), (0x25, 0x0F),
    (0x2E, 0x10), (0x2D, 0x11), (0x1F, 0x12), (0x23, 0x13), (0x0C, 0x14), (0x0F, 0x15),
    (0x01, 0x16), (0x11, 0x17), (0x20, 0x18), (0x09, 0x19), (0x0D, 0x1A), (0x07, 0x1B),
    (0x10, 0x1C), (0x06, 0x1D), // A..Z
    (0x12, 0x1E), (0x13, 0x1F), (0x14, 0x20), (0x15, 0x21), (0x17, 0x22), (0x16, 0x23),
    (0x1A, 0x24), (0x1C, 0x25), (0x19, 0x26), (0x1D, 0x27), // 1..0
    (0x24, 0x28), // Return
    (0x35, 0x29), // Esc
    (0x33, 0x2A), // Backspace
    (0x30, 0x2B), // Tab
    (0x31, 0x2C), // Space
    (0x1B, 0x2D), (0x18, 0x2E), (0x21, 0x2F), (0x1E, 0x30), (0x2A, 0x31),
    (0x29, 0x33), (0x27, 0x34), (0x32, 0x35), (0x2B, 0x36), (0x2F, 0x37), (0x2C, 0x38),
    (0x39, 0x39), // Caps
    (0x7A, 0x3A), (0x78, 0x3B), (0x63, 0x3C), (0x76, 0x3D), (0x60, 0x3E), (0x61, 0x3F),
    (0x62, 0x40), (0x64, 0x41), (0x65, 0x42), (0x6D, 0x43), (0x67, 0x44), (0x6F, 0x45), // F1..F12
    (0x69, 0x46), // F13 ~ PrintScreen
    (0x6B, 0x47), // F14 ~ ScrollLock
    (0x71, 0x48), // F15 ~ Pause
    (0x72, 0x49), // Help/Insert
    (0x73, 0x4A), (0x74, 0x4B), (0x75, 0x4C), (0x77, 0x4D), (0x79, 0x4E),
    (0x7C, 0x4F), (0x7B, 0x50), (0x7D, 0x51), (0x7E, 0x52), // arrows
    (0x47, 0x53), // Clear ~ NumLock
    (0x4B, 0x54), (0x43, 0x55), (0x4E, 0x56), (0x45, 0x57), (0x4C, 0x58),
    (0x53, 0x59), (0x54, 0x5A), (0x55, 0x5B), (0x56, 0x5C), (0x57, 0x5D), (0x58, 0x5E),
    (0x59, 0x5F), (0x5B, 0x60), (0x5C, 0x61), (0x52, 0x62), (0x41, 0x63), (0x51, 0x67), // keypad
    (0x0A, 0x64), // ISO section
    (0x6E, 0x65), // context menu
    (0x6A, 0x6B), (0x40, 0x6C), (0x4F, 0x6D), (0x50, 0x6E), (0x5A, 0x6F), // F16..F20
    (0x5E, 0x87), (0x5D, 0x89), // JIS
    (0x68, 0x90), (0x66, 0x91), // Kana/Eisu ~ LANG1/LANG2
    (0x3B, 0xE0), (0x38, 0xE1), (0x3A, 0xE2), (0x37, 0xE3),
    (0x3E, 0xE4), (0x3C, 0xE5), (0x3D, 0xE6), (0x36, 0xE7), // modifiers
];

pub fn is_media(code: Hid) -> bool {
    matches!(code, hid::MUTE | hid::VOLUME_UP | hid::VOLUME_DOWN) || (0x100..=0x103).contains(&code)
}

/// macOS "NX" auxiliary key types (sent as system-defined events, not key events).
pub fn to_mac_media(code: Hid) -> Option<i64> {
    Some(match code {
        hid::VOLUME_UP => 0,
        hid::VOLUME_DOWN => 1,
        hid::MUTE => 7,
        hid::MEDIA_PLAY_PAUSE | hid::MEDIA_STOP => 16,
        hid::MEDIA_NEXT => 17,
        hid::MEDIA_PREV => 18,
        _ => return None,
    })
}

pub fn from_mac_media(keytype: i64) -> Option<Hid> {
    Some(match keytype {
        0 => hid::VOLUME_UP,
        1 => hid::VOLUME_DOWN,
        7 => hid::MUTE,
        16 => hid::MEDIA_PLAY_PAUSE,
        17 | 19 => hid::MEDIA_NEXT,
        18 | 20 => hid::MEDIA_PREV,
        _ => return None,
    })
}

pub fn from_mac(keycode: u16) -> Option<Hid> {
    MAC_TABLE.iter().find(|(k, _)| *k == keycode).map(|(_, h)| *h)
}

pub fn to_mac(code: Hid) -> Option<u16> {
    // Mac F13-F15 double as PrintScreen/ScrollLock/Pause in MAC_TABLE; real F13-F15
    // from a Windows keyboard still need a key.
    match code {
        0x68 => return Some(0x69), // F13
        0x69 => return Some(0x6B), // F14
        0x6A => return Some(0x71), // F15
        _ => {}
    }
    MAC_TABLE.iter().find(|(_, h)| *h == code).map(|(k, _)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_round_trip() {
        for (s, e, h) in WIN_TABLE {
            match to_win(*h) {
                Some(WinKey::Scan { scan, extended }) => assert_eq!((scan, extended), (*s, *e), "hid {h:#x}"),
                Some(WinKey::Vk { .. }) => {}
                None => panic!("missing {h:#x}"),
            }
        }
        for (k, h) in MAC_TABLE {
            assert_eq!(to_mac(*h), Some(*k), "hid {h:#x}");
        }
    }

    #[test]
    fn position_layout() {
        // Windows keyboard on a Mac: Alt -> ⌘, Win -> ⌥, Ctrl unchanged.
        assert_eq!(map_cross_os(hid::LALT, "position"), hid::LGUI);
        assert_eq!(map_cross_os(hid::LGUI, "position"), hid::LALT);
        assert_eq!(map_cross_os(hid::LCTRL, "position"), hid::LCTRL);
        // Mac keyboard on Windows: ⌘ -> Alt, ⌥ -> Win.
        assert_eq!(map_cross_os(hid::RGUI, "position"), hid::RALT);
        assert_eq!(map_cross_os(hid::C, "position"), hid::C);
        assert_eq!(map_cross_os(hid::LCTRL, "ctrl_cmd"), hid::LGUI);
        assert_eq!(map_cross_os(hid::LALT, "none"), hid::LALT);
    }

    #[test]
    fn right_shift_even_when_flagged_extended() {
        // Plain and "extended" right Shift both map to RSHIFT.
        assert_eq!(from_win(0x36, false, 0xA1), Some(hid::RSHIFT));
        assert_eq!(from_win(0x36, true, 0xA1), Some(hid::RSHIFT));
        assert!(!is_fake_shift(0x36, true));
        // The NumLock fake shift (E0 2A) is dropped; a real left Shift is not.
        assert!(is_fake_shift(0x2A, true));
        assert!(!is_fake_shift(0x2A, false));
        assert_eq!(from_win(0x2A, false, 0xA0), Some(hid::LSHIFT));
        assert_eq!(from_win(0x1D, true, 0xA3), Some(hid::RCTRL));
    }

    /// Every key one OS can send must be typeable on the other.
    #[test]
    fn every_key_has_a_counterpart() {
        let mut missing = Vec::new();
        for (_, _, h) in WIN_TABLE {
            // 한/영 and 한자 are handled specially on the Mac (input source switch).
            if to_mac(*h).is_none() && !matches!(*h, hid::LANG1 | hid::LANG2) {
                missing.push(format!("win->mac {h:#04x}"));
            }
        }
        for (_, h) in MAC_TABLE {
            if to_win(*h).is_none() {
                missing.push(format!("mac->win {h:#04x}"));
            }
        }
        for code in [hid::MUTE, hid::VOLUME_UP, hid::VOLUME_DOWN, hid::MEDIA_PLAY_PAUSE, hid::MEDIA_NEXT, hid::MEDIA_PREV] {
            if to_mac_media(code).is_none() || to_win(code).is_none() {
                missing.push(format!("media {code:#05x}"));
            }
        }
        assert!(missing.is_empty(), "unmapped keys: {missing:?}");
    }

    #[test]
    fn media_keys_round_trip() {
        for vk in 0xADu32..=0xB3 {
            let h = from_win(0, true, vk).expect("media vk");
            assert!(is_media(h));
            assert!(matches!(to_win(h), Some(WinKey::Vk { vk: v, .. }) if v as u32 == vk));
        }
        assert_eq!(from_mac_media(to_mac_media(hid::VOLUME_UP).unwrap()), Some(hid::VOLUME_UP));
    }

    #[test]
    fn hangul_key() {
        assert_eq!(from_win(0x72, false, VK_HANGUL), Some(hid::LANG1));
        assert_eq!(from_win(0x38, true, VK_HANGUL), Some(hid::LANG1));
        assert_eq!(from_mac(0x36), Some(hid::RGUI));
    }
}
