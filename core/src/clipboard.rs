//! Clipboard watcher: notices local changes (via the OS change counter) and
//! applies clipboard contents received from peers.

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;

use crate::platform;
use crate::protocol::ClipData;

const MAX_TEXT: usize = 8 * 1024 * 1024;
const MAX_IMAGE_RGBA: usize = 64 * 1024 * 1024;

pub type CopiedFiles = Arc<Mutex<Option<(Vec<PathBuf>, Instant)>>>;

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

pub fn start(out: UnboundedSender<ClipData>) -> ClipWatcher {
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
            let mut last_seq = platform::clipboard_seq();
            let mut last_text = String::new();
            loop {
                match set_rx.recv_timeout(Duration::from_millis(350)) {
                    Ok(ClipData::Text(t)) => {
                        let _ = cb.set_text(t.clone());
                        last_text = t;
                        *files.lock().unwrap() = None;
                        last_seq = platform::clipboard_seq();
                    }
                    Ok(ClipData::Image { png, .. }) => {
                        if let Some((w, h, rgba)) = decode_png(&png) {
                            let _ = cb.set_image(arboard::ImageData { width: w as usize, height: h as usize, bytes: rgba.into() });
                            *files.lock().unwrap() = None;
                        }
                        last_seq = platform::clipboard_seq();
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        let seq = platform::clipboard_seq();
                        if seq == last_seq {
                            continue;
                        }
                        last_seq = seq;
                        // Files first: Finder also puts the file name on the pasteboard as text.
                        if let Ok(list) = cb.get().file_list() {
                            if !list.is_empty() {
                                *files.lock().unwrap() = Some((list, Instant::now()));
                                continue;
                            }
                        }
                        *files.lock().unwrap() = None;
                        if let Ok(t) = cb.get_text() {
                            if !t.is_empty() && t.len() <= MAX_TEXT && t != last_text {
                                last_text = t.clone();
                                let _ = out.send(ClipData::Text(t));
                            }
                            continue;
                        }
                        if let Ok(img) = cb.get_image() {
                            if img.bytes.len() <= MAX_IMAGE_RGBA {
                                if let Some(png) = encode_png(img.width as u32, img.height as u32, &img.bytes) {
                                    let _ = out.send(ClipData::Image { width: img.width as u32, height: img.height as u32, png });
                                }
                            }
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .expect("clipboard thread");
    ClipWatcher { set_tx, copied_files }
}
