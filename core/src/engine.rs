//! The engine ties everything together. Every device runs the same engine and
//! can play two roles:
//!
//! * controller – its own mouse hit a screen edge that has a neighbour, so its
//!   local input is grabbed and forwarded to the neighbour;
//! * target – a peer sent `Enter`, so it injects the input it receives and
//!   reports back when the virtual cursor crosses one of its edges.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

use crate::clipboard::{self, ClipChange, ClipWatcher, LocalClip};
use crate::config::{hex, Config, Settings, TrustedPeer};
use crate::files::{self, FileEvent};
use crate::keys::{self, hid};
use crate::net::{self, Announce, Discovery, HelloInfo, NetCtx, NetEvent, PeerLink};
use crate::platform::{self, InputEvent};
use crate::protocol::{Button, Cell, ClipData, ClipKind, ClipSummary, Layout, Msg, Os, Rect, Side};

const TAP_WINDOW: Duration = Duration::from_millis(600);
/// ~250 Hz: smooth on any display while cutting the message rate of 1000 Hz mice.
const MOVE_INTERVAL: Duration = Duration::from_millis(4);

// ----------------------------------------------------------------- public API

#[derive(Serialize, Clone, Debug)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub os: Os,
    pub trusted: bool,
    pub online: bool,
    pub nearby: bool,
    pub addr: Option<String>,
    pub screen: Option<Rect>,
    /// Peer's protocol version (0 when not connected).
    pub version: u32,
}

/// Live counters shown in Settings → 진단, to tell "nothing arrives" from "arrives but can't move".
#[derive(Serialize, Clone, Debug, Default)]
pub struct Diag {
    /// Times another computer handed the cursor to this one.
    pub enters: u64,
    /// Cursor moves received from another computer.
    pub moves_in: u64,
    /// Cursor moves sent while controlling another computer.
    pub moves_out: u64,
    pub last_controller: Option<String>,
}

/// One computer's clipboard as shown in the Clipboard tab.
#[derive(Serialize, Clone, Debug)]
pub struct ClipView {
    pub device_id: String,
    pub kind: ClipKind,
    pub text: Option<String>,
    /// Characters of text, or number of files.
    pub count: usize,
    pub width: u32,
    pub height: u32,
    /// data: URL of the image thumbnail.
    pub thumb: Option<String>,
    pub files: Vec<crate::protocol::FileMeta>,
    pub at: u64,
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity((data.len() + 2) / 3 * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

impl ClipView {
    fn from_summary(device_id: &str, s: &ClipSummary) -> ClipView {
        ClipView {
            device_id: device_id.to_string(),
            kind: s.kind,
            text: s.text.clone(),
            count: if s.kind == ClipKind::Files { s.files.len().max(s.chars) } else { s.chars },
            width: s.width,
            height: s.height,
            thumb: (!s.thumb.is_empty()).then(|| format!("data:image/png;base64,{}", base64(&s.thumb))),
            files: s.files.clone(),
            at: s.at,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Snapshot {
    pub me: DeviceView,
    pub devices: Vec<DeviceView>,
    pub layout: Layout,
    pub active: String,
    pub controlled_by: Option<String>,
    pub locked: bool,
    pub settings: Settings,
    /// Input capture is running (macOS: the event tap could be created).
    pub permission: bool,
    pub perm: platform::PermStatus,
    pub diag: Diag,
    pub clipboards: Vec<ClipView>,
    pub receive_dir: String,
    pub pairing_with: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CoreEvent {
    State(Snapshot),
    Toast { level: String, text: String },
    PairCode { id: String, name: String, code: String },
    PairCodeClosed,
    PairResult { ok: bool, name: String, error: Option<String> },
    Transfer { id: u64, peer: String, title: String, done: u64, total: u64, incoming: bool, finished: bool, error: Option<String> },
    Received { peer: String, paths: Vec<String> },
}

pub enum Cmd {
    Snapshot(oneshot::Sender<Snapshot>),
    SetLayout(Vec<Cell>),
    SetSettings(Settings),
    SetName(String),
    SetLocked(bool),
    ToggleLocked,
    PairRequest(String),
    PairSubmit { id: String, code: String },
    PairCancel,
    Unpair(String),
    SendFiles { id: String, paths: Vec<String> },
    Probe(String),
    OpenReceiveDir,
    Reveal(String),
    RequestPermission,
    RequestInputMonitoring,
    /// Copy another computer's clipboard here (files are downloaded).
    ClipPull(String),
    /// Hand this computer's clipboard to another computer.
    ClipPush(String),
}

#[derive(Clone)]
pub struct CoreHandle {
    tx: mpsc::UnboundedSender<Cmd>,
}

impl CoreHandle {
    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
    pub async fn snapshot(&self) -> Option<Snapshot> {
        let (tx, rx) = oneshot::channel();
        self.send(Cmd::Snapshot(tx));
        rx.await.ok()
    }
}

pub type Sink = Arc<dyn Fn(CoreEvent) + Send + Sync>;

pub fn start(config_dir: PathBuf, sink: Sink) -> Result<CoreHandle> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::Builder::new().name("noopflow-core".into()).spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("runtime");
        rt.block_on(async move {
            match Engine::new(config_dir, sink.clone()) {
                Ok((engine, chans)) => engine.run(rx, chans).await,
                Err(e) => sink(CoreEvent::Toast { level: "error".into(), text: format!("시작하지 못했어요: {e:#}") }),
            }
        });
    })?;
    Ok(CoreHandle { tx })
}

// ------------------------------------------------------------------ hold-tap

#[derive(Debug, PartialEq)]
enum Act {
    Pass(u16, bool),
    Tap,
}

/// Turns a lone tap of `key` into a `Tap` while keeping it usable as a modifier.
struct HoldTap {
    key: u16,
    pending: Option<Instant>,
    forwarded: bool,
}

impl HoldTap {
    fn new(key: u16) -> HoldTap {
        HoldTap { key, pending: None, forwarded: false }
    }
    fn key(&mut self, code: u16, down: bool) -> Vec<Act> {
        if code == self.key {
            if down {
                if self.forwarded {
                    return vec![Act::Pass(code, true)];
                }
                if self.pending.is_none() {
                    self.pending = Some(Instant::now());
                }
                return vec![];
            }
            if let Some(t) = self.pending.take() {
                return if t.elapsed() < TAP_WINDOW { vec![Act::Tap] } else { vec![] };
            }
            if self.forwarded {
                self.forwarded = false;
                return vec![Act::Pass(code, false)];
            }
            return vec![];
        }
        let mut v = self.other_input();
        v.push(Act::Pass(code, down));
        v
    }
    fn other_input(&mut self) -> Vec<Act> {
        if self.pending.take().is_some() {
            self.forwarded = true;
            vec![Act::Pass(self.key, true)]
        } else {
            vec![]
        }
    }
    fn reset(&mut self) {
        self.pending = None;
        self.forwarded = false;
    }
}

// -------------------------------------------------------------------- engine

struct Peer {
    conn: u64,
    hello: HelloInfo,
    addr: SocketAddr,
    initiator_id: String,
    link: PeerLink,
}

impl Peer {
    fn send(&self, msg: Msg) {
        if msg.is_bulk() {
            let bulk = self.link.bulk.clone();
            tokio::spawn(async move {
                let _ = bulk.send(msg).await;
            });
        } else {
            let _ = self.link.hi.send(msg);
        }
    }
}

struct Seen {
    ann: Announce,
    addr: IpAddr,
    at: Instant,
}

enum Mode {
    Local,
    Controlling(String),
}

struct Chans {
    net_rx: mpsc::UnboundedReceiver<NetEvent>,
    input_rx: mpsc::UnboundedReceiver<InputEvent>,
    clip_rx: mpsc::UnboundedReceiver<ClipChange>,
    file_rx: mpsc::UnboundedReceiver<FileEvent>,
}

struct Engine {
    dir: PathBuf,
    cfg: Config,
    os: Os,
    sink: Sink,
    net: Arc<NetCtx>,
    disc: Option<Arc<Discovery>>,
    peers: HashMap<String, Peer>,
    seen: HashMap<String, Seen>,
    dialing: HashMap<String, Instant>,
    mode: Mode,
    permission: bool,
    perm: platform::PermStatus,
    diag: Diag,
    warned_no_access: bool,
    // target role
    controlled_by: Option<String>,
    vpos: (f64, f64),
    frac: (f64, f64),
    monitors: Vec<Rect>,
    held_keys: Vec<u16>,
    held_buttons: u8,
    ralt: HoldTap,
    /// Logical points -> this screen's units at the virtual cursor.
    target_scale: f64,
    /// Modifier layout for the current controller ("position", "ctrl_cmd", "none").
    key_map: String,
    // controller-side mouse smoothing
    ctrl_scale: f64,
    pending_move: (f32, f32),
    last_move_sent: Instant,
    last_incompat: Option<(String, Instant)>,
    // controller role
    fwd_down: Vec<u16>,
    rcmd: HoldTap,
    local_rcmd: Option<Instant>,
    // pairing
    pairing_with: Option<String>,
    // clipboard & files
    clip: ClipWatcher,
    local_clip: LocalClip,
    local_clip_summary: ClipSummary,
    peer_clips: HashMap<String, ClipSummary>,
    last_paste_copy: Option<Instant>,
    files_tx: mpsc::UnboundedSender<FileEvent>,
    receiver: files::Receiver,
    next_transfer: u64,
    last_tick: Instant,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Remove stale offline duplicate entries representing the same device (e.g.
/// after a reinstall/re-pair changed its id/keypair) and inherit its layout cell.
fn dedupe_trusted_cfg(
    cfg: &mut Config,
    is_live: impl Fn(&str) -> bool,
    id: &str,
    name: &str,
    os: Os,
    ip: &str,
    paired: bool,
) -> Vec<String> {
    let stale: Vec<String> = cfg
        .trusted
        .iter()
        .filter(|t| t.id != id && !is_live(&t.id) && t.os == os)
        .filter(|t| {
            let same_name = t.name == name;
            let same_ip = t.last_addr.as_deref() == Some(ip);
            if paired {
                same_name || same_ip
            } else {
                same_name && same_ip
            }
        })
        .map(|t| t.id.clone())
        .collect();
    if stale.is_empty() {
        return stale;
    }
    cfg.trusted.retain(|t| !stale.contains(&t.id));
    if cfg.layout.cell(id).is_none() {
        if let Some((x, y)) = cfg.layout.cells.iter().find(|c| stale.contains(&c.id)).map(|c| (c.x, c.y)) {
            cfg.layout.cells.push(Cell { id: id.to_string(), x, y });
        }
    }
    cfg.layout.cells.retain(|c| !stale.contains(&c.id));
    cfg.layout.version = now_ms().max(cfg.layout.version + 1);
    stale
}

impl Engine {
    fn new(dir: PathBuf, sink: Sink) -> Result<(Engine, Chans)> {
        let cfg = Config::load_or_create(&dir);
        let (net_tx, net_rx) = mpsc::unbounded_channel();
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (clip_tx, clip_rx) = mpsc::unbounded_channel();
        let (file_tx, file_rx) = mpsc::unbounded_channel();

        let monitors = platform::screens();
        let hello = HelloInfo { id: cfg.id.clone(), name: cfg.name.clone(), os: Os::current(), screen: Rect::union(&monitors), version: crate::protocol::PROTOCOL_VERSION };
        let private = crate::config::unhex(&cfg.private_key).unwrap_or_default();
        let net = NetCtx::new(private, hello, cfg.trusted.clone(), net_tx);

        let ctx = net.clone();
        tokio::spawn(async move {
            if let Err(e) = net::listen(ctx).await {
                log::error!("listen failed: {e:#}");
            }
        });
        let disc = match Discovery::bind(net.clone()) {
            Ok(d) => {
                tokio::spawn(d.clone().run());
                Some(d)
            }
            Err(e) => {
                log::error!("discovery failed: {e:#}");
                None
            }
        };

        if let Err(e) = platform::start(input_tx) {
            sink(CoreEvent::Toast { level: "error".into(), text: format!("입력 장치를 연결하지 못했어요: {e:#}") });
        }
        let clip = clipboard::start(clip_tx);
        let receiver = files::Receiver::new(cfg.receive_dir(), file_tx.clone());

        let engine = Engine {
            dir,
            os: Os::current(),
            sink,
            net,
            disc,
            peers: HashMap::new(),
            seen: HashMap::new(),
            dialing: HashMap::new(),
            mode: Mode::Local,
            permission: platform::permission_ok(),
            perm: platform::permission_status(),
            diag: Diag::default(),
            warned_no_access: false,
            controlled_by: None,
            vpos: (0.0, 0.0),
            frac: (0.0, 0.0),
            monitors,
            held_keys: vec![],
            held_buttons: 0,
            ralt: HoldTap::new(hid::RALT),
            target_scale: 1.0,
            key_map: "none".into(),
            ctrl_scale: 1.0,
            pending_move: (0.0, 0.0),
            last_move_sent: Instant::now(),
            last_incompat: None,
            fwd_down: vec![],
            rcmd: HoldTap::new(hid::RGUI),
            local_rcmd: None,
            pairing_with: None,
            clip,
            local_clip: LocalClip::Empty,
            local_clip_summary: ClipSummary::default(),
            peer_clips: HashMap::new(),
            last_paste_copy: None,
            files_tx: file_tx,
            receiver,
            next_transfer: 1,
            last_tick: Instant::now(),
            cfg,
        };
        Ok((engine, Chans { net_rx, input_rx, clip_rx, file_rx }))
    }

    async fn run(mut self, mut cmd_rx: mpsc::UnboundedReceiver<Cmd>, mut ch: Chans) {
        self.apply_settings();
        self.refresh_neighbors();
        self.emit_state();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        // Mouse moves are batched into at most one message per MOVE_INTERVAL.
        let mut move_tick = tokio::time::interval(MOVE_INTERVAL);
        move_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                Some(i) = ch.input_rx.recv() => self.on_input(i),
                _ = move_tick.tick(), if self.pending_move != (0.0, 0.0) => self.flush_move(),
                Some(e) = ch.net_rx.recv() => self.on_net(e),
                Some(c) = cmd_rx.recv() => self.on_cmd(c).await,
                Some(c) = ch.clip_rx.recv() => self.on_clip(c),
                Some(f) = ch.file_rx.recv() => self.on_file(f),
                _ = tick.tick() => self.on_tick(),
                else => break,
            }
        }
    }

    // ----------------------------------------------------------- helpers

    fn me(&self) -> &str {
        &self.cfg.id
    }

    fn emit(&self, ev: CoreEvent) {
        (self.sink)(ev);
    }

    fn toast(&self, level: &str, text: impl Into<String>) {
        self.emit(CoreEvent::Toast { level: level.into(), text: text.into() });
    }

    fn save(&self) {
        self.cfg.save(&self.dir);
    }

    fn snapshot(&self) -> Snapshot {
        let me = DeviceView {
            id: self.cfg.id.clone(),
            name: self.cfg.name.clone(),
            os: self.os,
            trusted: true,
            online: true,
            nearby: true,
            addr: None,
            screen: Some(Rect::union(&self.monitors)),
            version: crate::protocol::PROTOCOL_VERSION,
        };
        let mut devices: Vec<DeviceView> = self
            .cfg
            .trusted
            .iter()
            .map(|t| {
                let peer = self.peers.get(&t.id);
                let seen = self.seen.get(&t.id);
                DeviceView {
                    id: t.id.clone(),
                    name: peer.map(|p| p.hello.name.clone()).unwrap_or_else(|| t.name.clone()),
                    os: t.os,
                    trusted: true,
                    online: peer.is_some(),
                    nearby: seen.is_some(),
                    addr: peer.map(|p| p.addr.ip().to_string()).or_else(|| seen.map(|s| s.addr.to_string())).or_else(|| t.last_addr.clone()),
                    screen: peer.map(|p| p.hello.screen),
                    version: peer.map(|p| p.hello.version).unwrap_or(0),
                }
            })
            .collect();
        for (id, s) in &self.seen {
            if self.cfg.trusted.iter().any(|t| &t.id == id) {
                continue;
            }
            devices.push(DeviceView {
                id: id.clone(),
                name: s.ann.name.clone(),
                os: s.ann.os,
                trusted: false,
                online: false,
                nearby: true,
                addr: Some(s.addr.to_string()),
                screen: None,
                version: 0,
            });
        }
        devices.sort_by(|a, b| b.trusted.cmp(&a.trusted).then(a.name.cmp(&b.name)));
        Snapshot {
            me,
            devices,
            layout: self.cfg.layout.clone(),
            active: match &self.mode {
                Mode::Controlling(t) => t.clone(),
                Mode::Local => self.cfg.id.clone(),
            },
            controlled_by: self.controlled_by.clone(),
            locked: self.cfg.locked,
            settings: self.cfg.settings.clone(),
            permission: self.permission,
            perm: self.perm,
            diag: self.diag.clone(),
            clipboards: self.clip_views(),
            receive_dir: self.cfg.receive_dir().to_string_lossy().into_owned(),
            pairing_with: self.pairing_with.clone(),
        }
    }

    fn emit_state(&self) {
        self.emit(CoreEvent::State(self.snapshot()));
    }

    fn apply_settings(&mut self) {
        platform::set_corner_guard(self.cfg.settings.corner_guard);
        platform::set_observe_local(self.os == Os::Mac && self.cfg.settings.right_cmd_hangul);
        self.receiver.set_dir(self.cfg.receive_dir());
    }

    fn refresh_neighbors(&self) {
        let mut mask = 0u8;
        if !self.cfg.locked && self.controlled_by.is_none() && self.permission {
            for side in Side::ALL {
                if let Some(n) = self.cfg.layout.neighbor(self.me(), side) {
                    if self.peers.contains_key(n) {
                        mask |= side.bit();
                    }
                }
            }
        }
        platform::set_neighbors(mask);
    }

    fn broadcast_layout(&self) {
        for p in self.peers.values() {
            p.send(Msg::Layout(self.cfg.layout.clone()));
        }
    }

    fn ensure_in_layout(&mut self, id: &str) -> bool {
        if self.cfg.layout.cell(id).is_some() {
            return false;
        }
        let me = self.cfg.layout.cell(&self.cfg.id).cloned().unwrap_or(Cell { id: self.cfg.id.clone(), x: 0, y: 0 });
        let mut x = me.x + 1;
        while self.cfg.layout.at(x, me.y).is_some() {
            x += 1;
        }
        self.cfg.layout.cells.push(Cell { id: id.to_string(), x, y: me.y });
        self.cfg.layout.version = now_ms().max(self.cfg.layout.version + 1);
        true
    }

    fn sync_trusted(&self) {
        *self.net.trusted.write().unwrap() = self.cfg.trusted.clone();
    }

    fn peer_name(&self, id: &str) -> String {
        self.peers
            .get(id)
            .map(|p| p.hello.name.clone())
            .or_else(|| self.cfg.trusted.iter().find(|t| t.id == id).map(|t| t.name.clone()))
            .or_else(|| self.seen.get(id).map(|s| s.ann.name.clone()))
            .unwrap_or_else(|| "다른 기기".into())
    }

    // ------------------------------------------------------------- ticks

    fn on_tick(&mut self) {
        let before = self.seen.len();
        self.seen.retain(|_, s| s.at.elapsed() < Duration::from_secs(8));
        let mut changed = before != self.seen.len();

        let perm = platform::permission_status();
        if perm != self.perm {
            self.perm = perm;
            changed = true;
        }

        if self.last_tick.elapsed() >= Duration::from_secs(5) {
            self.last_tick = Instant::now();
            let mons = platform::screens();
            if mons != self.monitors {
                self.monitors = mons;
                changed = true;
            }
            let mut h = self.net.hello.lock().unwrap();
            h.screen = Rect::union(&self.monitors);
            h.name = self.cfg.name.clone();
        }

        let targets: Vec<(String, SocketAddr)> = self
            .cfg
            .trusted
            .iter()
            .filter(|t| !self.peers.contains_key(&t.id))
            .filter(|t| self.dialing.get(&t.id).map_or(true, |at| at.elapsed() > Duration::from_secs(4)))
            .filter_map(|t| {
                let ip = self.seen.get(&t.id).map(|s| s.addr).or_else(|| t.last_addr.as_ref().and_then(|a| a.parse().ok()))?;
                Some((t.id.clone(), SocketAddr::new(ip, net::TCP_PORT)))
            })
            .collect();
        for (id, addr) in targets {
            self.dialing.insert(id.clone(), Instant::now());
            net::dial(self.net.clone(), addr, None, Some(id));
        }
        if changed {
            self.emit_state();
        }
    }

    // --------------------------------------------------------------- net

    fn on_net(&mut self, ev: NetEvent) {
        match ev {
            NetEvent::Discovered { ann, addr } => {
                let new = !self.seen.contains_key(&ann.id);
                let renamed = self.seen.get(&ann.id).map_or(false, |s| s.ann.name != ann.name);
                self.seen.insert(ann.id.clone(), Seen { ann, addr, at: Instant::now() });
                if new || renamed {
                    self.emit_state();
                }
            }
            NetEvent::Incompatible { name, newer } => {
                let text = if newer {
                    format!("{name}의 NoopFlow가 더 새 버전이에요. 이 컴퓨터의 NoopFlow를 업데이트해 주세요.")
                } else {
                    format!("{name}의 NoopFlow가 예전 버전이에요. 그 컴퓨터의 NoopFlow를 업데이트해 주세요.")
                };
                if self.last_incompat.as_ref().map_or(true, |(n, at)| *n != name || at.elapsed() > Duration::from_secs(60)) {
                    self.last_incompat = Some((name, Instant::now()));
                    self.toast("error", text);
                }
            }
            NetEvent::PairRequested { ann, .. } => {
                let code = self.net.new_pair_code();
                self.emit(CoreEvent::PairCode { id: ann.id, name: ann.name, code });
            }
            NetEvent::DialFailed { id, pairing, reason } => {
                if let Some(id) = &id {
                    self.dialing.insert(id.clone(), Instant::now());
                }
                if pairing {
                    let name = id.as_deref().map(|i| self.peer_name(i)).unwrap_or_default();
                    self.pairing_with = None;
                    let error = if reason.contains("timeout") || reason.contains("refused") {
                        "상대 기기에 연결할 수 없어요. 같은 와이파이인지, 방화벽이 막고 있지 않은지 확인해 주세요."
                    } else {
                        "코드가 맞지 않거나 시간이 지났어요. 다시 시도해 주세요."
                    };
                    self.emit(CoreEvent::PairResult { ok: false, name, error: Some(error.into()) });
                    self.emit_state();
                }
            }
            NetEvent::Connected { conn, hello, addr, initiator_id, public_key, paired, link } => {
                self.on_connected(conn, hello, addr, initiator_id, public_key, paired, link);
            }
            NetEvent::Disconnected { conn, id } => {
                if self.peers.get(&id).map_or(false, |p| p.conn == conn) {
                    self.peers.remove(&id);
                    self.peer_clips.remove(&id);
                    self.receiver.drop_peer(&id);
                    if matches!(&self.mode, Mode::Controlling(t) if *t == id) {
                        self.mode = Mode::Local;
                        platform::ungrab(None);
                    }
                    if self.controlled_by.as_deref() == Some(id.as_str()) {
                        self.release_all();
                        self.controlled_by = None;
                    }
                    self.refresh_neighbors();
                    self.emit_state();
                }
            }
            NetEvent::Message { conn, id, msg } => {
                if self.peers.get(&id).map_or(false, |p| p.conn == conn) {
                    self.on_msg(&id, msg);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_connected(&mut self, conn: u64, hello: HelloInfo, addr: SocketAddr, initiator_id: String, public_key: Vec<u8>, paired: bool, link: PeerLink) {
        let id = hello.id.clone();
        let ip_str = addr.ip().to_string();
        let key = hex(&public_key);
        let mut dirty = false;
        let mut layout_changed = false;
        match self.cfg.trusted.iter_mut().find(|t| t.id == id) {
            Some(t) => {
                if t.name != hello.name || t.last_addr.as_deref() != Some(ip_str.as_str()) || (paired && t.public_key != key) {
                    t.name = hello.name.clone();
                    t.os = hello.os;
                    t.last_addr = Some(ip_str.clone());
                    if paired {
                        t.public_key = key.clone();
                    }
                    dirty = true;
                }
            }
            None if paired => {
                self.cfg.trusted.push(TrustedPeer { id: id.clone(), name: hello.name.clone(), os: hello.os, public_key: key.clone(), last_addr: Some(ip_str.clone()) });
                dirty = true;
            }
            None => return, // not trusted (should not happen)
        }
        let stale = dedupe_trusted_cfg(
            &mut self.cfg,
            |other| self.peers.contains_key(other) || self.seen.contains_key(other),
            &id,
            &hello.name,
            hello.os,
            &ip_str,
            paired,
        );
        if !stale.is_empty() {
            for s in &stale {
                self.dialing.remove(s);
                self.peer_clips.remove(s);
            }
            dirty = true;
            layout_changed = true;
        }
        if paired {
            if self.ensure_in_layout(&id) {
                dirty = true;
                layout_changed = true;
            }
            self.net.clear_pair_code();
            self.emit(CoreEvent::PairCodeClosed);
            self.pairing_with = None;
            self.emit(CoreEvent::PairResult { ok: true, name: hello.name.clone(), error: None });
        }
        if dirty {
            self.sync_trusted();
            self.save();
        }

        if let Some(existing) = self.peers.get(&id) {
            let preferred = std::cmp::min(self.cfg.id.clone(), id.clone());
            if existing.initiator_id == preferred && initiator_id != preferred {
                return; // keep the existing connection; dropping `link` closes this one
            }
        }
        let fresh = !self.peers.contains_key(&id);
        let peer = Peer { conn, hello, addr, initiator_id, link };
        peer.send(Msg::Layout(self.cfg.layout.clone()));
        if peer.hello.version >= 3 {
            peer.send(Msg::ClipInfo(self.local_clip_summary.clone()));
        }
        if fresh && !paired {
            self.toast("ok", format!("{} 연결됨", peer.hello.name));
        }
        self.peers.insert(id, peer);
        if layout_changed {
            self.broadcast_layout();
        }
        self.refresh_neighbors();
        self.emit_state();
    }

    // ---------------------------------------------------------- messages

    fn on_msg(&mut self, from: &str, msg: Msg) {
        match msg {
            Msg::Layout(l) => {
                if l.version > self.cfg.layout.version {
                    self.cfg.layout = l;
                    let me = self.cfg.id.clone();
                    if self.ensure_in_layout(&me) {
                        self.broadcast_layout();
                    }
                    self.save();
                    self.refresh_neighbors();
                    self.emit_state();
                }
            }
            Msg::Enter { side, t } => {
                if let Mode::Controlling(target) = std::mem::replace(&mut self.mode, Mode::Local) {
                    if let Some(p) = self.peers.get(&target) {
                        p.send(Msg::Leave);
                    }
                    platform::ungrab(None);
                }
                self.release_all();
                self.controlled_by = Some(from.to_string());
                self.monitors = platform::screens();
                let b = Rect::union(&self.monitors);
                self.vpos = b.point_on_side(side, t, 1.0);
                self.target_scale = platform::scale_at(self.vpos.0, self.vpos.1);
                self.frac = (0.0, 0.0);
                self.ralt.reset();
                platform::inject_move(self.vpos.0, self.vpos.1, 0, 0);
                self.diag.enters += 1;
                self.diag.last_controller = Some(self.peer_name(from));
                // The OS silently drops injected input without Accessibility; say so instead.
                self.perm = platform::permission_status();
                if !self.perm.accessibility && !self.warned_no_access {
                    self.warned_no_access = true;
                    self.toast("error", "손쉬운 사용 권한이 없어서 이 맥의 마우스를 움직일 수 없어요. 설정 → 진단을 확인해 주세요.");
                }
                self.refresh_neighbors();
                self.emit_state();
            }
            Msg::Leave => {
                if self.controlled_by.as_deref() == Some(from) {
                    self.release_all();
                    self.controlled_by = None;
                    self.refresh_neighbors();
                    self.emit_state();
                }
            }
            Msg::MouseMove { dx, dy } if self.controlled_by.as_deref() == Some(from) => {
                self.diag.moves_in += 1;
                self.target_move(from, dx, dy);
            }
            Msg::MouseButton { button, down } if self.controlled_by.as_deref() == Some(from) => {
                for a in self.ralt.other_input() {
                    self.target_act(a);
                }
                if down {
                    self.held_buttons |= button.bit();
                } else {
                    self.held_buttons &= !button.bit();
                }
                platform::inject_button(button, down, self.vpos.0, self.vpos.1);
            }
            Msg::Wheel { dx, dy } if self.controlled_by.as_deref() == Some(from) => {
                let s = if self.cfg.settings.invert_scroll { -1 } else { 1 };
                platform::inject_wheel(dx * s, dy * s);
            }
            Msg::Key { code, down } if self.controlled_by.as_deref() == Some(from) => self.target_key(from, code, down),
            Msg::EdgeHit { side, t } => {
                if !matches!(&self.mode, Mode::Controlling(c) if c == from) {
                    return;
                }
                let Some(next) = self.cfg.layout.neighbor(from, side).map(str::to_string) else { return };
                if next == self.cfg.id {
                    self.leave_to_local(Some((side.opposite(), t)));
                } else if self.peers.contains_key(&next) {
                    self.switch_target(next, side.opposite(), t);
                }
            }
            Msg::Clipboard(data) => {
                if self.cfg.settings.clipboard_sync {
                    self.clip.set(data);
                }
            }
            Msg::ClipInfo(summary) => {
                self.peer_clips.insert(from.to_string(), summary);
                self.emit_state();
            }
            Msg::ClipRequest => {
                let name = self.peer_name(from);
                match self.local_clip.clone() {
                    LocalClip::Files(paths) => self.send_files(from, paths, true),
                    clip => {
                        if let (Some(data), Some(p)) = (clip.as_data(), self.peers.get(from)) {
                            p.send(Msg::ClipPut(data));
                        } else {
                            self.toast("info", format!("{name}에서 클립보드를 요청했지만 복사된 게 없어요"));
                        }
                    }
                }
            }
            Msg::ClipPut(data) => {
                let what = match &data {
                    ClipData::Text(_) => "글자",
                    ClipData::Image { .. } => "이미지",
                };
                self.clip.set(data);
                let key = if self.os == Os::Mac { "⌘V" } else { "Ctrl+V" };
                self.toast("ok", format!("{}의 {what}를 가져왔어요. {key}로 붙여넣으세요", self.peer_name(from)));
            }
            m @ (Msg::FileOffer { .. } | Msg::FileChunk { .. } | Msg::FileDone { .. } | Msg::FileCancel { .. }) => {
                let name = self.peer_name(from);
                self.receiver.handle(from, &name, m);
            }
            _ => {}
        }
    }

    // ------------------------------------------------------- target role

    /// `dx`/`dy` are logical points (DPI-independent); convert to this screen's units.
    fn target_move(&mut self, from: &str, dx: f32, dy: f32) {
        let speed = self.cfg.settings.mouse_speed.clamp(0.2, 4.0) as f64 * self.target_scale;
        self.frac.0 += dx as f64 * speed;
        self.frac.1 += dy as f64 * speed;
        let (mx, my) = (self.frac.0.trunc(), self.frac.1.trunc());
        self.frac.0 -= mx;
        self.frac.1 -= my;
        let b = Rect::union(&self.monitors);
        let (mut x, mut y) = (self.vpos.0 + mx, self.vpos.1 + my);
        let (l, t, r, btm) = (b.x as f64, b.y as f64, (b.right() - 1) as f64, (b.bottom() - 1) as f64);
        let crossed = if x < l {
            Some(Side::Left)
        } else if x > r {
            Some(Side::Right)
        } else if y < t {
            Some(Side::Top)
        } else if y > btm {
            Some(Side::Bottom)
        } else {
            None
        };
        x = x.clamp(l, r);
        y = y.clamp(t, btm);
        if let Some(side) = crossed {
            if self.cfg.layout.neighbor(self.me(), side).is_some() {
                let frac = match side {
                    Side::Left | Side::Right => (y - t) / (btm - t).max(1.0),
                    Side::Top | Side::Bottom => (x - l) / (r - l).max(1.0),
                };
                if let Some(p) = self.peers.get(from) {
                    p.send(Msg::EdgeHit { side, t: frac as f32 });
                }
            }
        }
        // Keep the cursor out of dead zones between monitors of different sizes.
        if !self.monitors.iter().any(|m| m.contains(x, y)) {
            let (ox, oy) = self.vpos;
            if let Some(m) = self.monitors.iter().find(|m| m.contains(ox, oy)).or(self.monitors.first()) {
                x = x.clamp(m.x as f64, (m.right() - 1) as f64);
                y = y.clamp(m.y as f64, (m.bottom() - 1) as f64);
            }
        }
        // Moving onto another monitor of this computer may change its scale.
        let moved_monitor = self.monitors.iter().position(|m| m.contains(x, y)) != self.monitors.iter().position(|m| m.contains(self.vpos.0, self.vpos.1));
        self.vpos = (x, y);
        if moved_monitor {
            self.target_scale = platform::scale_at(x, y);
        }
        platform::inject_move(x, y, mx as i32, my as i32);
    }

    fn target_key(&mut self, from: &str, code: u16, down: bool) {
        let ctrl_os = self.peers.get(from).map(|p| p.hello.os).unwrap_or(self.os);
        self.key_map = if ctrl_os != self.os && (ctrl_os == Os::Mac || self.os == Os::Mac) {
            self.cfg.settings.key_layout.clone()
        } else {
            "none".into()
        };
        if self.os == Os::Mac {
            if code == hid::LANG1 && self.cfg.settings.lang1_hangul {
                if down {
                    platform::toggle_input_source(&self.cfg.settings.hangul_method);
                }
                return;
            }
            // Tap detection works on the physical key (right Alt), before remapping.
            if self.cfg.settings.right_alt_hangul {
                for a in self.ralt.key(code, down) {
                    self.target_act(a);
                }
                return;
            }
        }
        self.inject_tracked(keys::map_cross_os(code, &self.key_map), down);
    }

    fn target_act(&mut self, a: Act) {
        match a {
            Act::Pass(c, d) => {
                let c = keys::map_cross_os(c, &self.key_map);
                self.inject_tracked(c, d);
            }
            Act::Tap => platform::toggle_input_source(&self.cfg.settings.hangul_method),
        }
    }

    fn inject_tracked(&mut self, code: u16, down: bool) {
        if down {
            if !self.held_keys.contains(&code) {
                self.held_keys.push(code);
            }
        } else {
            self.held_keys.retain(|c| *c != code);
        }
        platform::inject_key(code, down);
    }

    fn release_all(&mut self) {
        for code in std::mem::take(&mut self.held_keys).into_iter().rev() {
            platform::inject_key(code, false);
        }
        for b in Button::ALL {
            if self.held_buttons & b.bit() != 0 {
                platform::inject_button(b, false, self.vpos.0, self.vpos.1);
            }
        }
        self.held_buttons = 0;
        self.ralt.reset();
    }

    // --------------------------------------------------- controller role

    fn target(&self) -> Option<&Peer> {
        match &self.mode {
            Mode::Controlling(t) => self.peers.get(t),
            Mode::Local => None,
        }
    }

    /// Send to the current target. Anything other than a move first flushes the
    /// batched move, so clicks and keys land where the cursor actually is.
    fn forward(&mut self, msg: Msg) {
        if !matches!(msg, Msg::MouseMove { .. }) {
            self.flush_move();
        }
        if let Some(p) = self.target() {
            p.send(msg);
        }
    }

    fn flush_move(&mut self) {
        let (dx, dy) = std::mem::take(&mut self.pending_move);
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        self.last_move_sent = Instant::now();
        if let Some(p) = self.target() {
            p.send(Msg::MouseMove { dx, dy });
        }
    }

    fn start_controlling(&mut self, id: String, side: Side, t: f32) {
        self.fwd_down.clear();
        self.rcmd.reset();
        self.pending_move = (0.0, 0.0);
        let (cx, cy) = platform::cursor_pos();
        self.ctrl_scale = platform::scale_at(cx, cy).max(0.5);
        if let Some(p) = self.peers.get(&id) {
            p.send(Msg::Enter { side, t });
        }
        self.mode = Mode::Controlling(id);
        self.emit_state();
    }

    fn switch_target(&mut self, next: String, side: Side, t: f32) {
        self.forward(Msg::Leave);
        self.start_controlling(next, side, t);
    }

    fn leave_to_local(&mut self, entry: Option<(Side, f32)>) {
        self.forward(Msg::Leave);
        self.mode = Mode::Local;
        self.fwd_down.clear();
        self.rcmd.reset();
        platform::ungrab(entry);
        self.emit_state();
    }

    fn on_input(&mut self, ev: InputEvent) {
        match ev {
            InputEvent::Permission(ok) => {
                if ok != self.permission {
                    self.permission = ok;
                    self.refresh_neighbors();
                    self.emit_state();
                }
            }
            InputEvent::EdgeExit { side, t } => {
                let next = self.cfg.layout.neighbor(self.me(), side).map(str::to_string);
                match next {
                    Some(n) if self.peers.contains_key(&n) && !self.cfg.locked && self.controlled_by.is_none() => {
                        self.start_controlling(n, side.opposite(), t);
                    }
                    _ => platform::ungrab(None),
                }
            }
            InputEvent::MouseMove { dx, dy } => {
                if self.target().is_none() {
                    return;
                }
                self.diag.moves_out += 1;
                let s = self.ctrl_scale as f32;
                self.pending_move.0 += dx as f32 / s;
                self.pending_move.1 += dy as f32 / s;
                // First move after a pause goes out at once; bursts are batched by the move tick.
                if self.last_move_sent.elapsed() >= MOVE_INTERVAL {
                    self.flush_move();
                }
            }
            InputEvent::Button { button, down } => {
                for a in self.rcmd.other_input() {
                    self.controller_act(a);
                }
                self.forward(Msg::MouseButton { button, down });
            }
            InputEvent::Wheel { dx, dy } => self.forward(Msg::Wheel { dx, dy }),
            InputEvent::Key { code, down } => self.controller_key(code, down),
            InputEvent::LocalKey { code, down } => {
                if code == hid::RGUI {
                    if down {
                        self.local_rcmd = Some(Instant::now());
                    } else if let Some(t) = self.local_rcmd.take() {
                        if t.elapsed() < TAP_WINDOW && self.cfg.settings.right_cmd_hangul {
                            platform::toggle_input_source(&self.cfg.settings.hangul_method);
                        }
                    }
                } else if down {
                    self.local_rcmd = None;
                }
            }
            InputEvent::LocalClick => self.local_rcmd = None,
        }
    }

    fn controller_key(&mut self, code: u16, down: bool) {
        if self.target().is_none() {
            return;
        }
        if down {
            if !self.fwd_down.contains(&code) {
                self.fwd_down.push(code);
            }
        } else {
            self.fwd_down.retain(|c| *c != code);
        }
        let held = |a: u16, b: u16| self.fwd_down.contains(&a) || self.fwd_down.contains(&b);
        let ctrl = held(hid::LCTRL, hid::RCTRL);
        let alt = held(hid::LALT, hid::RALT);
        let shift = held(hid::LSHIFT, hid::RSHIFT);
        let gui = held(hid::LGUI, hid::RGUI);

        // Ctrl+Alt+Shift+H: come back to this computer.
        if down && code == hid::H && ctrl && alt && shift {
            self.leave_to_local(None);
            return;
        }

        // A paste on the other computer sends files copied here. Which physical
        // modifier means "paste" there depends on its key layout (with the default
        // layout a Windows keyboard pastes on a Mac with Alt+V), so accept any.
        if down && code == hid::V && (ctrl || gui || alt) && !shift && self.cfg.settings.paste_files {
            let copied = self.clip.copied_files.lock().unwrap().clone();
            if let Some((paths, at)) = copied {
                if self.last_paste_copy != Some(at) && at.elapsed() < Duration::from_secs(30 * 60) {
                    self.last_paste_copy = Some(at);
                    if let Mode::Controlling(t) = &self.mode {
                        let t = t.clone();
                        self.send_files(&t, paths, true);
                    }
                }
            }
        }

        if self.os == Os::Mac && self.cfg.settings.right_cmd_hangul {
            for a in self.rcmd.key(code, down) {
                self.controller_act(a);
            }
        } else {
            self.forward(Msg::Key { code, down });
        }
    }

    fn controller_act(&mut self, a: Act) {
        match a {
            Act::Pass(code, down) => self.forward(Msg::Key { code, down }),
            Act::Tap => {
                self.forward(Msg::Key { code: hid::LANG1, down: true });
                self.forward(Msg::Key { code: hid::LANG1, down: false });
            }
        }
    }

    // ------------------------------------------------- clipboard & files

    fn on_clip(&mut self, change: ClipChange) {
        self.local_clip = change.clip;
        self.local_clip_summary = self.local_clip.summary(now_ms());
        // Automatic sync of text/images (not echoed back when the data came from a peer).
        if self.cfg.settings.clipboard_sync && !change.from_remote {
            if let Some(data) = self.local_clip.as_data() {
                for p in self.peers.values() {
                    p.send(Msg::Clipboard(data.clone()));
                }
            }
        }
        for p in self.peers.values().filter(|p| p.hello.version >= 3) {
            p.send(Msg::ClipInfo(self.local_clip_summary.clone()));
        }
        self.emit_state();
    }

    fn clip_views(&self) -> Vec<ClipView> {
        let mut v = vec![ClipView::from_summary(&self.cfg.id, &self.local_clip_summary)];
        for (id, s) in &self.peer_clips {
            if self.peers.contains_key(id) {
                v.push(ClipView::from_summary(id, s));
            }
        }
        v
    }

    fn clip_pull(&mut self, id: &str) {
        match self.peers.get(id) {
            Some(p) if p.hello.version >= 3 => p.send(Msg::ClipRequest),
            Some(_) => self.toast("error", "상대 기기의 NoopFlow를 0.3.0 이상으로 업데이트해 주세요"),
            None => self.toast("error", "연결된 기기에서만 가져올 수 있어요"),
        }
    }

    fn clip_push(&mut self, id: &str) {
        let name = self.peer_name(id);
        match self.local_clip.clone() {
            LocalClip::Files(paths) => {
                self.send_files(id, paths, true);
            }
            clip => match (clip.as_data(), self.peers.get(id)) {
                (Some(data), Some(p)) if p.hello.version >= 3 => {
                    p.send(Msg::ClipPut(data));
                    self.toast("ok", format!("{name}로 클립보드를 보냈어요"));
                }
                (Some(_), Some(_)) => self.toast("error", "상대 기기의 NoopFlow를 0.3.0 이상으로 업데이트해 주세요"),
                (None, _) => self.toast("info", "이 컴퓨터에 복사된 게 없어요"),
                (_, None) => self.toast("error", "연결된 기기에만 보낼 수 있어요"),
            },
        }
    }

    fn send_files(&mut self, id: &str, paths: Vec<PathBuf>, reveal: bool) {
        let Some(p) = self.peers.get(id) else {
            self.toast("error", "연결된 기기에만 보낼 수 있어요");
            return;
        };
        let tid = self.next_transfer;
        self.next_transfer += 1;
        let bulk = p.link.bulk.clone();
        let name = p.hello.name.clone();
        let events = self.files_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = files::send(bulk, tid, name.clone(), paths, reveal, events.clone()).await {
                let _ = events.send(FileEvent::Progress { id: tid, peer: name, title: String::new(), done: 0, total: 0, incoming: false, finished: true, error: Some(format!("{e:#}")) });
            }
        });
    }

    fn on_file(&mut self, ev: FileEvent) {
        match ev {
            FileEvent::Progress { id, peer, title, done, total, incoming, finished, error } => {
                self.emit(CoreEvent::Transfer { id, peer, title, done, total, incoming, finished, error });
            }
            FileEvent::Received { peer, paths, reveal } => {
                if reveal {
                    if let Some(p) = paths.first() {
                        files::reveal(p);
                    }
                }
                self.emit(CoreEvent::Received { peer, paths: paths.iter().map(|p| p.to_string_lossy().into_owned()).collect() });
            }
        }
    }

    // ---------------------------------------------------------- commands

    async fn on_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Snapshot(tx) => {
                let _ = tx.send(self.snapshot());
            }
            Cmd::SetLayout(cells) => {
                let mut cells = cells;
                cells.dedup_by(|a, b| a.id == b.id);
                let mut seen_pos = std::collections::HashSet::new();
                if !cells.iter().all(|c| seen_pos.insert((c.x, c.y))) {
                    self.toast("error", "두 기기를 같은 자리에 둘 수 없어요");
                    self.emit_state();
                    return;
                }
                self.cfg.layout = Layout { version: now_ms().max(self.cfg.layout.version + 1), cells };
                let me = self.cfg.id.clone();
                self.ensure_in_layout(&me);
                self.save();
                self.broadcast_layout();
                self.refresh_neighbors();
                self.emit_state();
            }
            Cmd::SetSettings(s) => {
                self.cfg.settings = s;
                self.apply_settings();
                self.save();
                self.emit_state();
            }
            Cmd::SetName(name) => {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    self.cfg.name = name.clone();
                    self.net.hello.lock().unwrap().name = name;
                    self.save();
                    self.emit_state();
                }
            }
            Cmd::ToggleLocked => {
                let on = !self.cfg.locked;
                Box::pin(self.on_cmd(Cmd::SetLocked(on))).await;
            }
            Cmd::SetLocked(on) => {
                self.cfg.locked = on;
                if on && matches!(self.mode, Mode::Controlling(_)) {
                    self.leave_to_local(None);
                }
                self.save();
                self.refresh_neighbors();
                self.emit_state();
            }
            Cmd::PairRequest(id) => {
                let Some(s) = self.seen.get(&id) else {
                    self.toast("error", "기기를 찾을 수 없어요. 상대 기기에서 NoopFlow가 켜져 있는지 확인해 주세요.");
                    return;
                };
                let addr = s.addr;
                self.pairing_with = Some(id);
                if let Some(d) = &self.disc {
                    d.pair_request(addr).await;
                }
                self.emit_state();
            }
            Cmd::PairSubmit { id, code } => {
                let ip = self.seen.get(&id).map(|s| s.addr);
                match ip {
                    Some(ip) => net::dial(self.net.clone(), SocketAddr::new(ip, net::TCP_PORT), Some(code), Some(id)),
                    None => {
                        self.pairing_with = None;
                        self.emit(CoreEvent::PairResult { ok: false, name: String::new(), error: Some("기기가 사라졌어요. 다시 찾아볼게요.".into()) });
                        self.emit_state();
                    }
                }
            }
            Cmd::PairCancel => {
                self.net.clear_pair_code();
                self.pairing_with = None;
                self.emit(CoreEvent::PairCodeClosed);
                self.emit_state();
            }
            Cmd::Unpair(id) => {
                self.cfg.trusted.retain(|t| t.id != id);
                self.cfg.layout.cells.retain(|c| c.id != id);
                self.cfg.layout.version = now_ms().max(self.cfg.layout.version + 1);
                if matches!(&self.mode, Mode::Controlling(t) if *t == id) {
                    self.leave_to_local(None);
                }
                if self.controlled_by.as_deref() == Some(id.as_str()) {
                    self.release_all();
                    self.controlled_by = None;
                }
                self.peers.remove(&id);
                self.sync_trusted();
                self.save();
                self.broadcast_layout();
                self.refresh_neighbors();
                self.emit_state();
            }
            Cmd::SendFiles { id, paths } => {
                let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
                self.send_files(&id, paths, false);
            }
            Cmd::Probe(ip) => match ip.trim().parse::<IpAddr>() {
                Ok(ip) => {
                    if let Some(d) = &self.disc {
                        d.probe(ip).await;
                    }
                }
                Err(_) => self.toast("error", "IP 주소 형식이 올바르지 않아요 (예: 192.168.0.12)"),
            },
            Cmd::OpenReceiveDir => files::open_dir(&self.cfg.receive_dir()),
            Cmd::Reveal(p) => files::reveal(std::path::Path::new(&p)),
            Cmd::RequestPermission => platform::request_permission(),
            Cmd::RequestInputMonitoring => platform::request_input_monitoring(),
            Cmd::ClipPull(id) => self.clip_pull(&id),
            Cmd::ClipPush(id) => self.clip_push(&id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_tap_alone_is_tap() {
        let mut h = HoldTap::new(hid::RGUI);
        assert!(h.key(hid::RGUI, true).is_empty());
        assert_eq!(h.key(hid::RGUI, false), vec![Act::Tap]);
    }

    #[test]
    fn hold_tap_with_other_key_is_modifier() {
        let mut h = HoldTap::new(hid::RGUI);
        assert!(h.key(hid::RGUI, true).is_empty());
        assert_eq!(h.key(hid::C, true), vec![Act::Pass(hid::RGUI, true), Act::Pass(hid::C, true)]);
        assert_eq!(h.key(hid::C, false), vec![Act::Pass(hid::C, false)]);
        assert_eq!(h.key(hid::RGUI, false), vec![Act::Pass(hid::RGUI, false)]);
    }

    #[test]
    fn layout_neighbors() {
        let l = Layout {
            version: 1,
            cells: vec![Cell { id: "a".into(), x: 0, y: 0 }, Cell { id: "b".into(), x: 1, y: 0 }, Cell { id: "c".into(), x: 0, y: 1 }],
        };
        assert_eq!(l.neighbor("a", Side::Right), Some("b"));
        assert_eq!(l.neighbor("b", Side::Left), Some("a"));
        assert_eq!(l.neighbor("a", Side::Bottom), Some("c"));
        assert_eq!(l.neighbor("a", Side::Left), None);
    }

    fn test_cfg(trusted: Vec<TrustedPeer>, cells: Vec<Cell>) -> Config {
        Config {
            id: "me".into(),
            name: "Mac".into(),
            private_key: String::new(),
            public_key: String::new(),
            trusted,
            layout: Layout { version: 1, cells },
            settings: Settings::default(),
            locked: false,
        }
    }

    #[test]
    fn dedupe_replaces_stale_device_on_repair_and_inherits_cell() {
        let mut cfg = test_cfg(
            vec![
                TrustedPeer { id: "old".into(), name: "JEHEELY_".into(), os: Os::Windows, public_key: "k1".into(), last_addr: Some("192.168.0.10".into()) },
                TrustedPeer { id: "new".into(), name: "JEHEELY_".into(), os: Os::Windows, public_key: "k2".into(), last_addr: Some("192.168.0.10".into()) },
            ],
            vec![Cell { id: "me".into(), x: 1, y: 0 }, Cell { id: "old".into(), x: 0, y: 0 }],
        );
        let removed = dedupe_trusted_cfg(&mut cfg, |_| false, "new", "JEHEELY_", Os::Windows, "192.168.0.10", true);
        assert_eq!(removed, vec!["old".to_string()]);
        assert_eq!(cfg.trusted.len(), 1);
        assert_eq!(cfg.trusted[0].id, "new");
        assert_eq!(cfg.layout.cell("new"), Some(&Cell { id: "new".into(), x: 0, y: 0 }));
        assert!(cfg.layout.cell("old").is_none());
    }

    #[test]
    fn dedupe_auto_heals_existing_duplicate_on_reconnect() {
        let mut cfg = test_cfg(
            vec![
                TrustedPeer { id: "old".into(), name: "JEHEELY_".into(), os: Os::Windows, public_key: "k1".into(), last_addr: Some("192.168.0.10".into()) },
                TrustedPeer { id: "cur".into(), name: "JEHEELY_".into(), os: Os::Windows, public_key: "k2".into(), last_addr: Some("192.168.0.10".into()) },
                // Different IP with the same name must not be removed on plain reconnect.
                TrustedPeer { id: "other".into(), name: "JEHEELY_".into(), os: Os::Windows, public_key: "k3".into(), last_addr: Some("192.168.0.99".into()) },
            ],
            vec![
                Cell { id: "me".into(), x: 1, y: 1 },
                Cell { id: "old".into(), x: 1, y: 0 },
                Cell { id: "cur".into(), x: 0, y: 0 },
                Cell { id: "other".into(), x: 2, y: 0 },
            ],
        );
        let removed = dedupe_trusted_cfg(&mut cfg, |_| false, "cur", "JEHEELY_", Os::Windows, "192.168.0.10", false);
        assert_eq!(removed, vec!["old".to_string()]);
        assert_eq!(cfg.trusted.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["cur", "other"]);
        assert!(cfg.layout.cell("old").is_none());
        assert!(cfg.layout.cell("cur").is_some());
        assert!(cfg.layout.cell("other").is_some());
    }
}
