//! Clipboard watcher: notices local changes (via the OS change counter), keeps
//! the full current content for on-demand hand-over, and applies clipboard
//! contents received from peers.

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;

use crate::platform;
use crate::protocol::{ClipData, ClipKind, ClipSummary, FileMeta};

const MAX_TEXT: usize = 8 * 1024 * 1024;
const MAX_IMAGE_RGBA: usize = 64 * 1024 * 1024;
const PREVIEW_CHARS: usize = 2000;
const THUMB_MAX: u32 = 240;

pub type CopiedFiles = Arc<Mutex<Option<(Vec<PathBuf>, Instant)>>>;

/// Full content of this computer's clipboard.
#[derive(Clone, Debug, Default)]
pub enum LocalClip {
    #[default]
    Empty,
    Text(String),
    Image { width: u32, height: u32, png: Vec<u8>, thumb: Vec<u8> },
    Files(Vec<PathBuf>),
}

impl LocalClip {
    /// What can be handed to another computer as clipboard data (files go as a transfer).
    pub fn as_data(&self) -> Option<ClipData> {
        match self {
            LocalClip::Text(t) => Some(ClipData::Text(t.clone())),
            LocalClip::Image { width, height, png, .. } => Some(ClipData::Image { width: *width, height: *height, png: png.clone() }),
            _ => None,
        }
    }

    pub fn summary(&self, at: u64) -> ClipSummary {
        match self {
            LocalClip::Empty => ClipSummary { at, ..Default::default() },
            LocalClip::Text(t) => ClipSummary {
                kind: ClipKind::Text,
                text: Some(t.chars().take(PREVIEW_CHARS).collect()),
                chars: t.chars().count(),
                at,
                ..Default::default()
            },
            LocalClip::Image { width, height, thumb, .. } => ClipSummary {
                kind: ClipKind::Image,
                width: *width,
                height: *height,
                thumb: thumb.clone(),
                at,
                ..Default::default()
            },
            LocalClip::Files(paths) => ClipSummary {
                kind: ClipKind::Files,
                files: paths
                    .iter()
                    .take(50)
                    .map(|p| FileMeta {
                        path: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        size: size_of_path(p),
                    })
                    .collect(),
                chars: paths.len(),
                at,
                ..Default::default()
            },
        }
    }
}

fn size_of_path(p: &std::path::Path) -> u64 {
    if p.is_dir() {
        crate::files::collect(&[p.to_path_buf()]).iter().filter_map(|(abs, _)| std::fs::metadata(abs).ok()).map(|m| m.len()).sum()
    } else {
        std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
    }
}

/// A clipboard change. `from_remote` is true when we applied data from a peer.
pub struct ClipChange {
    pub clip: LocalClip,
    pub from_remote: bool,
}

pub struct ClipWatcher {
    set_tx: mpsc::Sender<ClipData>,
    pub copied_files: CopiedFiles,
}

impl ClipWatcher {
    pub fn set(&self, data: ClipData) {
        let _ = self.set_tx.send(data);
    }
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut wr = enc.write_header().ok()?;
        wr.write_image_data(rgba).ok()?;
    }
    Some(out)
}

fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let dec = png::Decoder::new(Cursor::new(bytes));
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some((info.width, info.height, buf))
}

/// Box-filtered thumbnail no larger than THUMB_MAX on either side, as PNG.
pub fn thumbnail(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
        return Vec::new();
    }
    let scale = (w.max(h) as f64 / THUMB_MAX as f64).max(1.0);
    let (tw, th) = (((w as f64 / scale).round() as u32).max(1), ((h as f64 / scale).round() as u32).max(1));
    let mut out = vec![0u8; (tw * th * 4) as usize];
    for ty in 0..th {
        let (y0, y1) = ((ty as f64 * scale) as u32, (((ty + 1) as f64 * scale) as u32).min(h).max((ty as f64 * scale) as u32 + 1));
        for tx in 0..tw {
            let (x0, x1) = ((tx as f64 * scale) as u32, (((tx + 1) as f64 * scale) as u32).min(w).max((tx as f64 * scale) as u32 + 1));
            let mut acc = [0u64; 4];
            let mut n = 0u64;
            for y in y0..y1.min(h) {
                for x in x0..x1.min(w) {
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += rgba[i + c] as u64;
                    }
                    n += 1;
                }
            }
            let o = ((ty * tw + tx) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    encode_png(tw, th, &out).unwrap_or_default()
}

fn image_clip(w: u32, h: u32, rgba: &[u8], png: Option<Vec<u8>>) -> Option<LocalClip> {
    let png = match png {
        Some(p) => p,
        None => encode_png(w, h, rgba)?,
    };
    Some(LocalClip::Image { width: w, height: h, png, thumb: thumbnail(w, h, rgba) })
}

fn read_clipboard(cb: &mut arboard::Clipboard) -> LocalClip {
    // Files first: Finder also puts the file name on the pasteboard as text.
    if let Ok(list) = cb.get().file_list() {
        if !list.is_empty() {
            return LocalClip::Files(list);
        }
    }
    if let Ok(t) = cb.get_text() {
        if !t.is_empty() && t.len() <= MAX_TEXT {
            return LocalClip::Text(t);
        }
    }
    if let Ok(img) = cb.get_image() {
        if img.bytes.len() <= MAX_IMAGE_RGBA {
            if let Some(c) = image_clip(img.width as u32, img.height as u32, &img.bytes, None) {
                return c;
            }
        }
    }
    LocalClip::Empty
}

pub fn start(out: UnboundedSender<ClipChange>) -> ClipWatcher {
    let (set_tx, set_rx) = mpsc::channel::<ClipData>();
    let copied_files: CopiedFiles = Arc::new(Mutex::new(None));
    let files = copied_files.clone();
    std::thread::Builder::new()
        .name("noopflow-clipboard".into())
        .spawn(move || {
            let mut cb = loop {
                match arboard::Clipboard::new() {
                    Ok(cb) => break cb,
                    Err(_) => std::thread::sleep(Duration::from_secs(2)),
                }
            };
            // Sentinel so the current clipboard is read once at startup.
            let mut last_seq = u64::MAX;
            loop {
                match set_rx.recv_timeout(Duration::from_millis(350)) {
                    Ok(ClipData::Text(t)) => {
                        let _ = cb.set_text(t.clone());
                        *files.lock().unwrap() = None;
                        last_seq = platform::clipboard_seq();
                        let _ = out.send(ClipChange { clip: LocalClip::Text(t), from_remote: true });
                    }
                    Ok(ClipData::Image { png, .. }) => {
                        if let Some((w, h, rgba)) = decode_png(&png) {
                            let _ = cb.set_image(arboard::ImageData { width: w as usize, height: h as usize, bytes: rgba.clone().into() });
                            *files.lock().unwrap() = None;
                            if let Some(c) = image_clip(w, h, &rgba, Some(png)) {
                                let _ = out.send(ClipChange { clip: c, from_remote: true });
                            }
                        }
                        last_seq = platform::clipboard_seq();
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        let seq = platform::clipboard_seq();
                        if seq == last_seq {
                            continue;
                        }
                        last_seq = seq;
                        let clip = read_clipboard(&mut cb);
                        *files.lock().unwrap() = match &clip {
                            LocalClip::Files(list) => Some((list.clone(), Instant::now())),
                            _ => None,
                        };
                        let _ = out.send(ClipChange { clip, from_remote: false });
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .expect("clipboard thread");
    ClipWatcher { set_tx, copied_files }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_is_small_png() {
        let (w, h) = (1000u32, 500u32);
        let rgba = vec![200u8; (w * h * 4) as usize];
        let t = thumbnail(w, h, &rgba);
        let (tw, th, _) = decode_png(&t).expect("png");
        assert_eq!((tw, th), (240, 120));
    }

    #[test]
    fn text_summary_is_truncated() {
        let s = LocalClip::Text("가".repeat(5000)).summary(1);
        assert_eq!(s.kind, ClipKind::Text);
        assert_eq!(s.chars, 5000);
        assert_eq!(s.text.unwrap().chars().count(), PREVIEW_CHARS);
    }
}
