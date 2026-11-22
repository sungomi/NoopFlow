//! File transfer: sending streams files as chunks over the bulk channel;
//! receiving writes them under the receive folder with safe, unique names.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

use crate::protocol::{FileMeta, Msg};

const CHUNK: usize = 48 * 1024;

#[derive(Debug, Clone)]
pub enum FileEvent {
    Progress { id: u64, peer: String, title: String, done: u64, total: u64, incoming: bool, finished: bool, error: Option<String> },
    Received { peer: String, paths: Vec<PathBuf>, reveal: bool },
}

fn walk(abs: &Path, rel: String, out: &mut Vec<(PathBuf, String)>) {
    if abs.is_dir() {
        if let Ok(rd) = std::fs::read_dir(abs) {
            let mut entries: Vec<_> = rd.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let name = e.file_name().to_string_lossy().into_owned();
                walk(&e.path(), format!("{rel}/{name}"), out);
            }
        }
    } else if abs.is_file() {
        out.push((abs.to_path_buf(), rel));
    }
}

pub fn collect(paths: &[PathBuf]) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        walk(p, name, &mut out);
    }
    out
}

pub fn title_for(paths: &[PathBuf]) -> String {
    match paths {
        [] => "파일".into(),
        [one] => one.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        [first, rest @ ..] => format!(
            "{} 외 {}개",
            first.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            rest.len()
        ),
    }
}

pub async fn send(
    bulk: mpsc::Sender<Msg>,
    id: u64,
    peer: String,
    paths: Vec<PathBuf>,
    reveal: bool,
    events: mpsc::UnboundedSender<FileEvent>,
) -> Result<()> {
    let title = title_for(&paths);
    let files = tokio::task::spawn_blocking(move || collect(&paths)).await?;
    let metas: Vec<FileMeta> = files
        .iter()
        .map(|(abs, rel)| FileMeta { path: rel.clone(), size: std::fs::metadata(abs).map(|m| m.len()).unwrap_or(0) })
        .collect();
    let total: u64 = metas.iter().map(|m| m.size).sum();
    let progress = |done: u64, finished: bool, error: Option<String>| {
        let _ = events.send(FileEvent::Progress { id, peer: peer.clone(), title: title.clone(), done, total, incoming: false, finished, error });
    };
    if files.is_empty() {
        progress(0, true, Some("보낼 파일이 없어요".into()));
        return Ok(());
    }
    bulk.send(Msg::FileOffer { id, files: metas, reveal }).await?;
    let mut done = 0u64;
    let mut last_report = 0u64;
    let mut buf = vec![0u8; CHUNK];
    for (index, (abs, _)) in files.iter().enumerate() {
        let mut f = match tokio::fs::File::open(abs).await {
            Ok(f) => f,
            Err(e) => {
                let _ = bulk.send(Msg::FileCancel { id }).await;
                progress(done, true, Some(format!("{}: {e}", abs.display())));
                return Ok(());
            }
        };
        loop {
            let n = f.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            bulk.send(Msg::FileChunk { id, index: index as u32, data: buf[..n].to_vec() }).await?;
            done += n as u64;
            if done - last_report > 1024 * 1024 {
                last_report = done;
                progress(done, false, None);
            }
        }
    }
    bulk.send(Msg::FileDone { id }).await?;
    progress(total, true, None);
    Ok(())
}

/// Turn an untrusted relative path into safe components.
fn sanitize(rel: &str) -> Vec<String> {
    rel.split(['/', '\\'])
        .filter(|c| !c.is_empty() && *c != "." && *c != "..")
        .map(|c| c.chars().map(|ch| if ":*?\"<>|".contains(ch) || ch.is_control() { '_' } else { ch }).collect::<String>())
        .filter(|c| !c.trim().is_empty())
        .collect()
}

fn unique(dir: &Path, name: &str) -> String {
    if !dir.join(name).exists() {
        return name.to_string();
    }
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| name.to_string());
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for i in 1.. {
        let cand = format!("{stem} ({i}){ext}");
        if !dir.join(&cand).exists() {
            return cand;
        }
    }
    unreachable!()
}

struct Incoming {
    peer_name: String,
    title: String,
    paths: Vec<PathBuf>,
    tops: Vec<PathBuf>,
    total: u64,
    done: u64,
    last_report: u64,
    current: Option<(u32, File)>,
    reveal: bool,
}

pub struct Receiver {
    dir: PathBuf,
    active: HashMap<(String, u64), Incoming>,
    events: mpsc::UnboundedSender<FileEvent>,
}

impl Receiver {
    pub fn new(dir: PathBuf, events: mpsc::UnboundedSender<FileEvent>) -> Receiver {
        Receiver { dir, active: HashMap::new(), events }
    }

    pub fn set_dir(&mut self, dir: PathBuf) {
        self.dir = dir;
    }

    fn report(&self, key: &(String, u64), inc: &Incoming, finished: bool, error: Option<String>) {
        let _ = self.events.send(FileEvent::Progress {
            id: key.1,
            peer: inc.peer_name.clone(),
            title: inc.title.clone(),
            done: inc.done,
            total: inc.total,
            incoming: true,
            finished,
            error,
        });
    }

    pub fn handle(&mut self, peer: &str, peer_name: &str, msg: Msg) {
        match msg {
            Msg::FileOffer { id, files, reveal } => {
                if let Err(e) = std::fs::create_dir_all(&self.dir) {
                    log::warn!("cannot create receive dir: {e}");
                    return;
                }
                let mut renames: HashMap<String, String> = HashMap::new();
                let mut paths = Vec::new();
                let mut tops = Vec::new();
                for m in &files {
                    let mut parts = sanitize(&m.path);
                    if parts.is_empty() {
                        parts.push("file".into());
                    }
                    let top = renames.entry(parts[0].clone()).or_insert_with(|| unique(&self.dir, &parts[0])).clone();
                    let top_path = self.dir.join(&top);
                    if !tops.contains(&top_path) {
                        tops.push(top_path);
                    }
                    parts[0] = top;
                    let mut p = self.dir.clone();
                    for c in &parts {
                        p.push(c);
                    }
                    if let Some(parent) = p.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = File::create(&p);
                    paths.push(p);
                }
                let title = title_for(&tops);
                let inc = Incoming {
                    peer_name: peer_name.to_string(),
                    title,
                    paths,
                    tops,
                    total: files.iter().map(|f| f.size).sum(),
                    done: 0,
                    last_report: 0,
                    current: None,
                    reveal,
                };
                let key = (peer.to_string(), id);
                self.report(&key, &inc, false, None);
                self.active.insert(key, inc);
            }
            Msg::FileChunk { id, index, data } => {
                let key = (peer.to_string(), id);
                let Some(mut inc) = self.active.remove(&key) else { return };
                let need_open = !matches!(&inc.current, Some((i, _)) if *i == index);
                if need_open {
                    inc.current = inc
                        .paths
                        .get(index as usize)
                        .and_then(|p| OpenOptions::new().append(true).open(p).ok())
                        .map(|f| (index, f));
                }
                let ok = inc.current.as_mut().map(|(_, f)| f.write_all(&data).is_ok()).unwrap_or(false);
                if !ok {
                    self.report(&key, &inc, true, Some("파일을 저장하지 못했어요".into()));
                    return;
                }
                inc.done += data.len() as u64;
                if inc.done - inc.last_report > 1024 * 1024 {
                    inc.last_report = inc.done;
                    self.report(&key, &inc, false, None);
                }
                self.active.insert(key, inc);
            }
            Msg::FileDone { id } => {
                let key = (peer.to_string(), id);
                if let Some(mut inc) = self.active.remove(&key) {
                    inc.current = None;
                    inc.done = inc.total;
                    self.report(&key, &inc, true, None);
                    let _ = self.events.send(FileEvent::Received { peer: inc.peer_name.clone(), paths: inc.tops.clone(), reveal: inc.reveal });
                }
            }
            Msg::FileCancel { id } => {
                let key = (peer.to_string(), id);
                if let Some(inc) = self.active.remove(&key) {
                    self.report(&key, &inc, true, Some("보내는 쪽에서 취소했어요".into()));
                }
            }
            _ => {}
        }
    }

    pub fn drop_peer(&mut self, peer: &str) {
        let keys: Vec<_> = self.active.keys().filter(|k| k.0 == peer).cloned().collect();
        for k in keys {
            if let Some(inc) = self.active.remove(&k) {
                self.report(&k, &inc, true, Some("연결이 끊겼어요".into()));
            }
        }
    }
}

pub fn reveal(path: &Path) {
    #[cfg(windows)]
    {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path);
        let _ = std::process::Command::new("explorer").arg(arg).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
}

pub fn open_dir(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_paths() {
        assert_eq!(sanitize("../../etc/passwd"), vec!["etc", "passwd"]);
        assert_eq!(sanitize("C:\\Windows\\x"), vec!["C_", "Windows", "x"]);
        assert_eq!(sanitize("a/./b"), vec!["a", "b"]);
    }
}
