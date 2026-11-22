use serde::{Deserialize, Serialize};

/// v2: MouseMove carries DPI-independent logical points as f32.
/// v3: clipboard previews (ClipInfo / ClipRequest / ClipPut).
pub const PROTOCOL_VERSION: u32 = 3;
/// Oldest peer we still talk to; newer messages are only sent to peers that know them.
pub const MIN_PROTOCOL_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x as f64 && x < self.right() as f64 && y >= self.y as f64 && y < self.bottom() as f64
    }
    pub fn union(rects: &[Rect]) -> Rect {
        if rects.is_empty() {
            return Rect { x: 0, y: 0, w: 1920, h: 1080 };
        }
        let l = rects.iter().map(|r| r.x).min().unwrap();
        let t = rects.iter().map(|r| r.y).min().unwrap();
        let r = rects.iter().map(|r| r.right()).max().unwrap();
        let b = rects.iter().map(|r| r.bottom()).max().unwrap();
        Rect { x: l, y: t, w: r - l, h: b - t }
    }
    /// Point just inside `side` of this rect at fraction `t` along that side.
    pub fn point_on_side(&self, side: Side, t: f32, inset: f64) -> (f64, f64) {
        let t = t.clamp(0.0, 1.0) as f64;
        let (x, y, w, h) = (self.x as f64, self.y as f64, self.w as f64, self.h as f64);
        match side {
            Side::Left => (x + inset, y + t * (h - 1.0)),
            Side::Right => (x + w - 1.0 - inset, y + t * (h - 1.0)),
            Side::Top => (x + t * (w - 1.0), y + inset),
            Side::Bottom => (x + t * (w - 1.0), y + h - 1.0 - inset),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];
    pub fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
    pub fn bit(self) -> u8 {
        match self {
            Side::Left => 1,
            Side::Right => 2,
            Side::Top => 4,
            Side::Bottom => 8,
        }
    }
    pub fn offset(self) -> (i32, i32) {
        match self {
            Side::Left => (-1, 0),
            Side::Right => (1, 0),
            Side::Top => (0, -1),
            Side::Bottom => (0, 1),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl Button {
    pub fn bit(self) -> u8 {
        match self {
            Button::Left => 1,
            Button::Right => 2,
            Button::Middle => 4,
            Button::Back => 8,
            Button::Forward => 16,
        }
    }
    pub const ALL: [Button; 5] = [Button::Left, Button::Right, Button::Middle, Button::Back, Button::Forward];
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Cell {
    pub id: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub version: u64,
    pub cells: Vec<Cell>,
}

impl Layout {
    pub fn cell(&self, id: &str) -> Option<&Cell> {
        self.cells.iter().find(|c| c.id == id)
    }
    pub fn at(&self, x: i32, y: i32) -> Option<&Cell> {
        self.cells.iter().find(|c| c.x == x && c.y == y)
    }
    pub fn neighbor(&self, id: &str, side: Side) -> Option<&str> {
        let c = self.cell(id)?;
        let (dx, dy) = side.offset();
        self.at(c.x + dx, c.y + dy).map(|c| c.id.as_str())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ClipData {
    Text(String),
    Image { width: u32, height: u32, png: Vec<u8> },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileMeta {
    /// Relative path using '/' separators.
    pub path: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Msg {
    Hello { id: String, name: String, os: Os, screen: Rect, version: u32 },
    Layout(Layout),
    /// Cursor enters the receiver's screen on `side` at fraction `t`.
    Enter { side: Side, t: f32 },
    Leave,
    /// Relative move in logical points (Windows pixels divided by the display scale).
    MouseMove { dx: f32, dy: f32 },
    MouseButton { button: Button, down: bool },
    /// Wheel units: 120 = one notch (Windows WHEEL_DELTA).
    Wheel { dx: i32, dy: i32 },
    Key { code: u16, down: bool },
    /// Receiver's virtual cursor crossed its `side` at fraction `t`.
    EdgeHit { side: Side, t: f32 },
    Clipboard(ClipData),
    FileOffer { id: u64, files: Vec<FileMeta>, reveal: bool },
    FileChunk { id: u64, index: u32, data: Vec<u8> },
    FileDone { id: u64 },
    FileCancel { id: u64 },
    Ping,
    // ---- protocol v3 (only sent to peers that said version >= 3) ----
    /// What is on the sender's clipboard right now (preview only).
    ClipInfo(ClipSummary),
    /// "Send me your clipboard": text/images come back as ClipPut, files as a transfer.
    ClipRequest,
    /// Explicit clipboard hand-over: always applied, unlike the automatic `Clipboard` sync.
    ClipPut(ClipData),
}

impl Msg {
    pub fn is_bulk(&self) -> bool {
        matches!(
            self,
            Msg::Clipboard(_) | Msg::FileOffer { .. } | Msg::FileChunk { .. } | Msg::FileDone { .. } | Msg::ClipInfo(_) | Msg::ClipPut(_)
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ClipKind {
    #[default]
    Empty,
    Text,
    Image,
    Files,
}

/// A small preview of a clipboard, cheap enough to send on every change.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ClipSummary {
    pub kind: ClipKind,
    /// First part of the text (whole text is fetched on request).
    pub text: Option<String>,
    pub chars: usize,
    pub width: u32,
    pub height: u32,
    /// PNG thumbnail (max ~240 px) for images.
    pub thumb: Vec<u8>,
    pub files: Vec<FileMeta>,
    /// Unix ms when this content was copied.
    pub at: u64,
}
