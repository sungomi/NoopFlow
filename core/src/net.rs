//! Encrypted peer connections (Noise XX over TCP) and LAN discovery (UDP broadcast).
//!
//! Pairing: the responder shows a 6-digit code, the initiator types it in and the
//! code is mixed into the handshake as a pre-shared key (Noise XXpsk3). After a
//! successful pairing both sides remember each other's static public key, and
//! later connections use plain Noise XX and are accepted only for known keys.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snow::TransportState;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;

use crate::config::{hex, TrustedPeer};
use crate::protocol::{Msg, Os, Rect, MIN_PROTOCOL_VERSION, PROTOCOL_VERSION};

pub const NOISE_XX: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
pub const NOISE_XX_PSK: &str = "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s";
// Deliberately not Deskflow/Synergy's 24800 so both can run side by side.
pub const TCP_PORT: u16 = 47800;
pub const UDP_PORT: u16 = 47801;
const MAX_PLAIN: usize = 60_000;
const MAX_MESSAGE: usize = 96 * 1024 * 1024;
const APP_TAG: &str = "noopflow";
const PAIR_CODE_TTL: Duration = Duration::from_secs(120);

#[derive(Clone, Debug)]
pub struct HelloInfo {
    pub id: String,
    pub name: String,
    pub os: Os,
    pub screen: Rect,
    /// Peer's protocol version (ours for our own hello).
    pub version: u32,
}

pub struct NetCtx {
    pub private_key: Vec<u8>,
    pub hello: Mutex<HelloInfo>,
    pub trusted: RwLock<Vec<TrustedPeer>>,
    pair_code: Mutex<Option<(String, Instant)>>,
    pub events: mpsc::UnboundedSender<NetEvent>,
    next_conn: AtomicU64,
}

impl NetCtx {
    pub fn new(private_key: Vec<u8>, hello: HelloInfo, trusted: Vec<TrustedPeer>, events: mpsc::UnboundedSender<NetEvent>) -> Arc<NetCtx> {
        Arc::new(NetCtx {
            private_key,
            hello: Mutex::new(hello),
            trusted: RwLock::new(trusted),
            pair_code: Mutex::new(None),
            events,
            next_conn: AtomicU64::new(1),
        })
    }

    /// Opens a pairing window and returns the code to display.
    pub fn new_pair_code(&self) -> String {
        let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
        *self.pair_code.lock().unwrap() = Some((code.clone(), Instant::now()));
        code
    }

    pub fn clear_pair_code(&self) {
        *self.pair_code.lock().unwrap() = None;
    }

    fn take_pair_code(&self) -> Option<String> {
        // One attempt per code: a wrong guess burns it.
        let mut g = self.pair_code.lock().unwrap();
        match g.take() {
            Some((code, at)) if at.elapsed() < PAIR_CODE_TTL => Some(code),
            _ => None,
        }
    }
}

pub struct PeerLink {
    pub hi: mpsc::UnboundedSender<Msg>,
    pub bulk: mpsc::Sender<Msg>,
}

pub enum NetEvent {
    Connected {
        conn: u64,
        hello: HelloInfo,
        addr: SocketAddr,
        initiator_id: String,
        public_key: Vec<u8>,
        paired: bool,
        link: PeerLink,
    },
    Message { conn: u64, id: String, msg: Msg },
    Disconnected { conn: u64, id: String },
    DialFailed { id: Option<String>, pairing: bool, reason: String },
    Discovered { ann: Announce, addr: IpAddr },
    PairRequested { ann: Announce, addr: IpAddr },
    /// The peer runs a NoopFlow with a different protocol version.
    Incompatible { name: String, newer: bool },
}

fn psk_from_code(code: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"noopflow-pairing-v1:");
    h.update(code.trim().as_bytes());
    h.finalize().into()
}

fn builder<'a>(ctx: &'a NetCtx, psk: Option<&'a [u8; 32]>) -> snow::Builder<'a> {
    match psk {
        Some(p) => snow::Builder::new(NOISE_XX_PSK.parse().unwrap())
            .local_private_key(&ctx.private_key)
            .psk(3, p),
        None => snow::Builder::new(NOISE_XX.parse().unwrap()).local_private_key(&ctx.private_key),
    }
}

async fn write_frame(w: &mut (impl AsyncWriteExt + Unpin), data: &[u8]) -> Result<()> {
    let mut buf = Vec::with_capacity(data.len() + 2);
    buf.extend_from_slice(&(data.len() as u16).to_be_bytes());
    buf.extend_from_slice(data);
    w.write_all(&buf).await?;
    Ok(())
}

async fn read_frame(r: &mut (impl AsyncReadExt + Unpin)) -> Result<Vec<u8>> {
    let len = r.read_u16().await? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

struct Crypto(Mutex<TransportState>);

impl Crypto {
    async fn send(&self, w: &mut OwnedWriteHalf, msg: &Msg) -> Result<()> {
        let bytes = bincode::serialize(msg)?;
        let mut out = vec![0u8; 65535];
        let mut plain = Vec::with_capacity(MAX_PLAIN + 1);
        let mut chunks = bytes.chunks(MAX_PLAIN).peekable();
        while let Some(c) = chunks.next() {
            plain.clear();
            plain.push(chunks.peek().is_some() as u8);
            plain.extend_from_slice(c);
            let n = self.0.lock().unwrap().write_message(&plain, &mut out)?;
            write_frame(w, &out[..n]).await?;
        }
        Ok(())
    }

    async fn recv(&self, r: &mut OwnedReadHalf) -> Result<Msg> {
        let mut acc = Vec::new();
        let mut out = vec![0u8; 65535];
        loop {
            let frame = read_frame(r).await?;
            let n = self.0.lock().unwrap().read_message(&frame, &mut out)?;
            if n == 0 {
                bail!("empty frame");
            }
            acc.extend_from_slice(&out[1..n]);
            if acc.len() > MAX_MESSAGE {
                bail!("message too large");
            }
            if out[0] == 0 {
                break;
            }
        }
        Ok(bincode::deserialize(&acc)?)
    }
}

async fn handshake(stream: &mut TcpStream, ctx: &NetCtx, initiator: bool, code: Option<&str>) -> Result<(TransportState, Vec<u8>, bool)> {
    let mut buf = vec![0u8; 65535];
    if initiator {
        let psk = code.map(psk_from_code);
        stream.write_u8(if psk.is_some() { 2 } else { 1 }).await?;
        let mut hs = builder(ctx, psk.as_ref()).build_initiator()?;
        let n = hs.write_message(&[], &mut buf)?;
        write_frame(stream, &buf[..n]).await?;
        let m = read_frame(stream).await?;
        hs.read_message(&m, &mut buf)?;
        let n = hs.write_message(&[], &mut buf)?;
        write_frame(stream, &buf[..n]).await?;
        let rs = hs.get_remote_static().ok_or_else(|| anyhow!("no remote key"))?.to_vec();
        Ok((hs.into_transport_mode()?, rs, psk.is_some()))
    } else {
        let mode = stream.read_u8().await?;
        let psk = match mode {
            1 => None,
            2 => Some(psk_from_code(&ctx.take_pair_code().ok_or_else(|| anyhow!("no pairing window open"))?)),
            _ => bail!("bad mode"),
        };
        let mut hs = builder(ctx, psk.as_ref()).build_responder()?;
        let m = read_frame(stream).await?;
        hs.read_message(&m, &mut buf)?;
        let n = hs.write_message(&[], &mut buf)?;
        write_frame(stream, &buf[..n]).await?;
        let m = read_frame(stream).await?;
        hs.read_message(&m, &mut buf).context("pairing code mismatch")?;
        let rs = hs.get_remote_static().ok_or_else(|| anyhow!("no remote key"))?.to_vec();
        Ok((hs.into_transport_mode()?, rs, psk.is_some()))
    }
}

async fn run_connection(ctx: Arc<NetCtx>, mut stream: TcpStream, initiator: bool, code: Option<String>, expect_id: Option<String>) -> Result<()> {
    stream.set_nodelay(true)?;
    let addr = stream.peer_addr()?;
    let (ts, remote_key, paired) = tokio::time::timeout(Duration::from_secs(10), handshake(&mut stream, &ctx, initiator, code.as_deref()))
        .await
        .map_err(|_| anyhow!("handshake timeout"))??;

    let known_id = ctx.trusted.read().unwrap().iter().find(|t| t.public_key == hex(&remote_key)).map(|t| t.id.clone());
    if !paired && known_id.is_none() {
        bail!("untrusted device");
    }

    let crypto = Arc::new(Crypto(Mutex::new(ts)));
    let (mut rd, mut wr) = stream.into_split();
    let me = ctx.hello.lock().unwrap().clone();
    crypto
        .send(&mut wr, &Msg::Hello { id: me.id.clone(), name: me.name.clone(), os: me.os, screen: me.screen, version: PROTOCOL_VERSION })
        .await?;
    let hello = match tokio::time::timeout(Duration::from_secs(10), crypto.recv(&mut rd)).await.map_err(|_| anyhow!("hello timeout"))?? {
        Msg::Hello { id, name, os, screen, version } => {
            if version < MIN_PROTOCOL_VERSION {
                ctx.events.send(NetEvent::Incompatible { name: name.clone(), newer: false }).ok();
                bail!("protocol version {version} < {MIN_PROTOCOL_VERSION}");
            }
            HelloInfo { id, name, os, screen, version }
        }
        _ => bail!("expected hello"),
    };
    if let Some(k) = &known_id {
        if !paired && *k != hello.id {
            bail!("device id mismatch");
        }
    }
    if let Some(e) = &expect_id {
        if *e != hello.id {
            bail!("unexpected device");
        }
    }
    if hello.id == me.id {
        bail!("connected to self");
    }

    let conn = ctx.next_conn.fetch_add(1, Ordering::Relaxed);
    let (hi_tx, mut hi_rx) = mpsc::unbounded_channel::<Msg>();
    let (bulk_tx, mut bulk_rx) = mpsc::channel::<Msg>(32);
    let initiator_id = if initiator { me.id.clone() } else { hello.id.clone() };
    let id = hello.id.clone();
    ctx.events
        .send(NetEvent::Connected {
            conn,
            hello,
            addr,
            initiator_id,
            public_key: remote_key,
            paired,
            link: PeerLink { hi: hi_tx, bulk: bulk_tx },
        })
        .ok();

    let wcrypto = crypto.clone();
    let writer = tokio::spawn(async move {
        let mut ping = tokio::time::interval(Duration::from_secs(3));
        loop {
            let msg = tokio::select! {
                biased;
                m = hi_rx.recv() => match m { Some(m) => m, None => break },
                m = bulk_rx.recv() => match m { Some(m) => m, None => break },
                _ = ping.tick() => Msg::Ping,
            };
            if wcrypto.send(&mut wr, &msg).await.is_err() {
                break;
            }
        }
        let _ = wr.shutdown().await;
    });

    loop {
        match tokio::time::timeout(Duration::from_secs(12), crypto.recv(&mut rd)).await {
            Ok(Ok(Msg::Ping)) => {}
            Ok(Ok(msg)) => {
                if ctx.events.send(NetEvent::Message { conn, id: id.clone(), msg }).is_err() {
                    break;
                }
            }
            _ => break,
        }
        if writer.is_finished() {
            break;
        }
    }
    writer.abort();
    ctx.events.send(NetEvent::Disconnected { conn, id }).ok();
    Ok(())
}

pub async fn listen(ctx: Arc<NetCtx>) -> Result<()> {
    listen_on(ctx, TCP_PORT).await
}

pub async fn listen_on(ctx: Arc<NetCtx>, port: u16) -> Result<()> {
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await.context("TCP port in use")?;
    loop {
        let (stream, _) = listener.accept().await?;
        let ctx = ctx.clone();
        tokio::spawn(async move {
            if let Err(e) = run_connection(ctx, stream, false, None, None).await {
                log::info!("incoming connection ended: {e:#}");
            }
        });
    }
}

pub fn dial(ctx: Arc<NetCtx>, addr: SocketAddr, code: Option<String>, expect_id: Option<String>) {
    tokio::spawn(async move {
        let pairing = code.is_some();
        let res = async {
            let stream = tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(addr))
                .await
                .map_err(|_| anyhow!("timeout"))??;
            run_connection(ctx.clone(), stream, true, code, expect_id.clone()).await
        }
        .await;
        if let Err(e) = res {
            log::info!("dial {addr} failed: {e:#}");
            ctx.events.send(NetEvent::DialFailed { id: expect_id, pairing, reason: format!("{e:#}") }).ok();
        }
    });
}

// ---------------------------------------------------------------- discovery

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Announce {
    pub id: String,
    pub name: String,
    pub os: Os,
    pub port: u16,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t")]
enum Datagram {
    Announce { app: String, a: Announce },
    Probe { app: String, a: Announce },
    PairRequest { app: String, a: Announce },
}

pub struct Discovery {
    sock: Arc<UdpSocket>,
    ctx: Arc<NetCtx>,
}

impl Discovery {
    pub fn bind(ctx: Arc<NetCtx>) -> Result<Arc<Discovery>> {
        let std = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, UDP_PORT)).context("UDP port in use")?;
        std.set_broadcast(true)?;
        std.set_nonblocking(true)?;
        Ok(Arc::new(Discovery { sock: Arc::new(UdpSocket::from_std(std)?), ctx }))
    }

    fn me(&self) -> Announce {
        let h = self.ctx.hello.lock().unwrap();
        Announce { id: h.id.clone(), name: h.name.clone(), os: h.os, port: TCP_PORT }
    }

    async fn send(&self, dg: &Datagram, to: SocketAddr) {
        if let Ok(bytes) = serde_json::to_vec(dg) {
            let _ = self.sock.send_to(&bytes, to).await;
        }
    }

    fn broadcast_targets() -> Vec<SocketAddr> {
        let mut v: Vec<SocketAddr> = vec![SocketAddr::from((Ipv4Addr::BROADCAST, UDP_PORT))];
        if let Ok(ifs) = if_addrs::get_if_addrs() {
            for i in ifs {
                if let if_addrs::IfAddr::V4(a) = i.addr {
                    if a.ip.is_loopback() {
                        continue;
                    }
                    if let Some(b) = a.broadcast {
                        let sa = SocketAddr::from((b, UDP_PORT));
                        if !v.contains(&sa) {
                            v.push(sa);
                        }
                    }
                }
            }
        }
        v
    }

    pub async fn pair_request(&self, ip: IpAddr) {
        self.send(&Datagram::PairRequest { app: APP_TAG.into(), a: self.me() }, SocketAddr::new(ip, UDP_PORT)).await;
    }

    pub async fn probe(&self, ip: IpAddr) {
        self.send(&Datagram::Probe { app: APP_TAG.into(), a: self.me() }, SocketAddr::new(ip, UDP_PORT)).await;
    }

    pub async fn run(self: Arc<Self>) {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        let mut buf = vec![0u8; 4096];
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let dg = Datagram::Announce { app: APP_TAG.into(), a: self.me() };
                    for t in Self::broadcast_targets() {
                        self.send(&dg, t).await;
                    }
                }
                r = self.sock.recv_from(&mut buf) => {
                    let Ok((n, from)) = r else { continue };
                    let Ok(dg) = serde_json::from_slice::<Datagram>(&buf[..n]) else { continue };
                    let my_id = self.me().id;
                    match dg {
                        Datagram::Announce { app, a } if app == APP_TAG && a.id != my_id => {
                            self.ctx.events.send(NetEvent::Discovered { ann: a, addr: from.ip() }).ok();
                        }
                        Datagram::Probe { app, a } if app == APP_TAG && a.id != my_id => {
                            self.send(&Datagram::Announce { app: APP_TAG.into(), a: self.me() }, from).await;
                            self.ctx.events.send(NetEvent::Discovered { ann: a, addr: from.ip() }).ok();
                        }
                        Datagram::PairRequest { app, a } if app == APP_TAG && a.id != my_id => {
                            self.ctx.events.send(NetEvent::Discovered { ann: a.clone(), addr: from.ip() }).ok();
                            self.ctx.events.send(NetEvent::PairRequested { ann: a, addr: from.ip() }).ok();
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(id: &str) -> (Arc<NetCtx>, mpsc::UnboundedReceiver<NetEvent>, Vec<u8>) {
        let kp = snow::Builder::new(NOISE_XX.parse().unwrap()).generate_keypair().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let hello = HelloInfo { id: id.into(), name: id.to_uppercase(), os: Os::current(), screen: Rect::default(), version: PROTOCOL_VERSION };
        (NetCtx::new(kp.private, hello, vec![], tx), rx, kp.public)
    }

    async fn next(rx: &mut mpsc::UnboundedReceiver<NetEvent>) -> NetEvent {
        tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.expect("event timeout").expect("closed")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pair_then_reconnect_and_reject_strangers() {
        let (a, mut ae, a_pub) = make("aaa");
        let (b, mut be, b_pub) = make("bbb");
        let port = 34871;
        tokio::spawn(listen_on(b.clone(), port));
        tokio::time::sleep(Duration::from_millis(150)).await;
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

        // Wrong code fails and burns the code.
        let code = b.new_pair_code();
        let wrong = if code == "123456" { "654321" } else { "123456" };
        dial(a.clone(), addr, Some(wrong.into()), Some("bbb".into()));
        assert!(matches!(next(&mut ae).await, NetEvent::DialFailed { pairing: true, .. }));
        dial(a.clone(), addr, Some(code.clone()), Some("bbb".into()));
        assert!(matches!(next(&mut ae).await, NetEvent::DialFailed { pairing: true, .. }), "code must be single-use");

        // Right code pairs.
        let code = b.new_pair_code();
        dial(a.clone(), addr, Some(code), Some("bbb".into()));
        let ea = next(&mut ae).await;
        let eb = next(&mut be).await;
        match (ea, eb) {
            (NetEvent::Connected { hello: ha, paired: pa, public_key: ka, link, .. }, NetEvent::Connected { hello: hb, paired: pb, public_key: kb, .. }) => {
                assert!(pa && pb);
                assert_eq!(ha.id, "bbb");
                assert_eq!(hb.id, "aaa");
                assert_eq!(ka, b_pub);
                assert_eq!(kb, a_pub);
                // A message makes it across the encrypted channel, including a large one.
                link.hi.send(Msg::Key { code: 4, down: true }).unwrap();
                link.bulk.send(Msg::FileChunk { id: 1, index: 0, data: vec![7u8; 200_000] }).await.unwrap();
                match next(&mut be).await {
                    NetEvent::Message { msg: Msg::Key { code: 4, down: true }, .. } => {}
                    _ => panic!("expected key"),
                }
                match next(&mut be).await {
                    NetEvent::Message { msg: Msg::FileChunk { data, .. }, .. } => assert_eq!(data.len(), 200_000),
                    _ => panic!("expected chunk"),
                }
            }
            _ => panic!("expected connected on both sides"),
        }

        // A stranger without pairing is rejected.
        let (c, mut ce, _) = make("ccc");
        dial(c.clone(), addr, None, None);
        assert!(matches!(next(&mut ce).await, NetEvent::DialFailed { pairing: false, .. }));

        // After trusting each other, a normal (no code) connection works.
        a.trusted.write().unwrap().push(crate::config::TrustedPeer { id: "bbb".into(), name: "B".into(), os: Os::current(), public_key: hex(&b_pub), last_addr: None });
        b.trusted.write().unwrap().push(crate::config::TrustedPeer { id: "aaa".into(), name: "A".into(), os: Os::current(), public_key: hex(&a_pub), last_addr: None });
        dial(a.clone(), addr, None, Some("bbb".into()));
        loop {
            match next(&mut ae).await {
                NetEvent::Connected { paired, hello, .. } => {
                    assert!(!paired);
                    assert_eq!(hello.id, "bbb");
                    break;
                }
                NetEvent::Disconnected { .. } => continue,
                _ => panic!("unexpected event"),
            }
        }
    }
}
