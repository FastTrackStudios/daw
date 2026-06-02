//! PTP-style peer clock synchronization for sample-accurate multi-machine
//! playback.
//!
//! # Protocol
//!
//! Each peer runs three tasks over a single UDP socket bound to a fixed
//! port + administratively-scoped multicast group:
//!
//! - **Announce** (1 Hz, multicast): broadcasts `{peer_id, listen_addr}`
//!   so newcomers learn who's on the wire. Peers expire from the table
//!   after 5 seconds without an announce.
//! - **Ping** (10 Hz per peer, unicast): NTP-style 4-timestamp round-trip
//!   carrying audio host-clock samples. Each successful exchange updates a
//!   rolling offset + one-way delay estimate.
//! - **Receive** (continuous): demuxes inbound messages by type, replies
//!   to pings inline, and updates the `PeerTable`.
//!
//! All timestamps are REAPER audio-clock microseconds (`host_micros` from
//! the latest [`crate::AudioSnapshot`]) so the computed offsets directly
//! drive sample-position alignment in Phase C. When no snapshot is
//! available yet (audio engine hasn't started), the protocol falls back
//! to `Instant`-derived microseconds so peer discovery still works
//! during start-up.
//!
//! # Offset & delay math
//!
//! Given timestamps from a single round trip:
//!   t1 — local send
//!   t2 — peer receive
//!   t3 — peer send
//!   t4 — local receive
//!
//! ```text
//! offset = ((t2 - t1) + (t3 - t4)) / 2     // remote clock − local clock
//! delay  = ((t4 - t1) - (t3 - t2)) / 2     // one-way network delay
//! ```
//!
//! Smoothed via an interquartile-mean rolling window (drops top + bottom
//! quartile before averaging) to reject scheduler / network jitter.

use core::sync::atomic::{AtomicU64, Ordering};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::SnapshotCell;

/// Default multicast group + port — administratively-scoped IPv4
/// multicast (239.x.x.x is reserved for site-local use, not routed
/// across the public internet). `7777` is unused per IANA; collisions
/// are tolerable because peer-id filtering rejects unrelated traffic.
pub const DEFAULT_MULTICAST: Ipv4Addr = Ipv4Addr::new(239, 255, 42, 42);
pub const DEFAULT_PORT: u16 = 7777;

/// How often each peer announces itself (multicast).
const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(1);
/// How often each peer pings every other peer (unicast).
const PING_INTERVAL: Duration = Duration::from_millis(100);
/// How often each peer broadcasts its sample position (unicast to
/// every known peer). Higher than ping rate because position is what
/// the drift corrector consumes directly. 20Hz = every audio buffer
/// at typical 256/48k settings.
const POSITION_INTERVAL: Duration = Duration::from_millis(50);
/// Drop a peer from the table after this long without an announce.
const PEER_TTL: Duration = Duration::from_secs(5);
/// Rolling window size for offset / delay smoothing. ~3s @ 10Hz.
const SMOOTHING_WINDOW: usize = 32;

/// Stable peer identifier. Random on first start, persists in memory
/// for the process lifetime. (Future: read from a config file so the
/// same physical machine keeps the same id across restarts.)
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct PeerId(pub Uuid);

impl PeerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for PeerId {
    fn default() -> Self {
        Self::new()
    }
}

/// Wire frame. `Announce` is multicast; `Ping`/`Pong`/`Position` are unicast.
#[derive(Debug, Serialize, Deserialize)]
enum Message {
    Announce {
        from: PeerId,
        port: u16,
    },
    Ping {
        from: PeerId,
        t1: i64,
    },
    Pong {
        from: PeerId,
        t1: i64,
        t2: i64,
        t3: i64,
    },
    /// Sample position broadcast — sent at audio-buffer rate per
    /// project. Multi-project peers send one frame per active
    /// project per tick; receivers index by `project_id`.
    Position {
        from: PeerId,
        /// Project this frame describes. `[0; 16]` is the sentinel
        /// "current project" for backward-compat with single-project
        /// senders.
        project_id: crate::ProjectId,
        /// Local audio host clock when the snapshot was taken (µs).
        host_micros: i64,
        /// Playhead in seconds at that moment.
        playhead_seconds: f64,
        /// REAPER reported sample rate (Hz).
        sample_rate: f64,
        /// REAPER reported playrate (1.0 = nominal).
        playrate: f64,
        /// Was transport playing at that moment.
        is_playing: bool,
    },
}

/// Per-peer state exposed to readers. Clone-only (positions is a
/// small Vec, ≤16 entries).
#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub id: PeerId,
    pub addr: SocketAddr,
    /// Remote clock − local clock, in microseconds. Positive means
    /// the remote peer's audio clock reads later than ours.
    pub offset_us: i64,
    /// One-way network delay estimate (microseconds).
    pub delay_us: i64,
    /// Last successful round-trip (for staleness checks).
    pub last_rtt_at: Instant,
    /// Last announce (for TTL expiry).
    pub last_announce_at: Instant,
    /// Latest per-project sample-position broadcasts. Empty until
    /// the first Position frame arrives.
    pub positions: Vec<RemotePosition>,
}

impl PeerInfo {
    /// Convenience: return the position frame for a specific
    /// project, or `None` if no recent frame.
    pub fn position_for(&self, project_id: crate::ProjectId) -> Option<&RemotePosition> {
        self.positions.iter().find(|p| p.project_id == project_id)
    }
}

/// Snapshot of a peer's transport at a known instant in their clock
/// domain. Combine with `PeerInfo::offset_us` to project onto our
/// clock.
#[derive(Clone, Copy, Debug)]
pub struct RemotePosition {
    /// Project this position describes.
    pub project_id: crate::ProjectId,
    /// Their audio host clock when they sent the broadcast (µs).
    pub host_micros: i64,
    /// Playhead in seconds at that moment.
    pub playhead_seconds: f64,
    pub sample_rate: f64,
    pub playrate: f64,
    pub is_playing: bool,
    /// Local time we received the broadcast (for staleness checks).
    pub received_at: Instant,
}

impl RemotePosition {
    /// Extrapolate "where is the peer playing *right now*" by adding
    /// elapsed local time to their last broadcast position. Doesn't
    /// account for clock drift between the two machines — Phase B's
    /// `offset_us` carries that — but does account for the time
    /// between their broadcast and our query.
    ///
    /// `now_local_micros` is the current LOCAL audio host clock; the
    /// caller is expected to subtract `offset_us` first to project
    /// into the peer's clock domain.
    pub fn project_playhead(&self, now_in_peer_clock_micros: i64) -> f64 {
        if !self.is_playing {
            return self.playhead_seconds;
        }
        let elapsed_secs = (now_in_peer_clock_micros - self.host_micros) as f64 * 1e-6;
        self.playhead_seconds + elapsed_secs * self.playrate
    }
}

/// Per-peer mutable state held inside the table. Owns the rolling
/// window of recent offsets / delays.
struct Peer {
    id: PeerId,
    addr: SocketAddr,
    offset_window: RollingWindow,
    delay_window: RollingWindow,
    last_rtt_at: Instant,
    last_announce_at: Instant,
    /// Map of project_id → latest position. Vec instead of HashMap
    /// since N is small (≤16) and linear scan beats hashing at this
    /// size.
    positions: Vec<RemotePosition>,
}

impl Peer {
    fn snapshot(&self) -> PeerInfo {
        PeerInfo {
            id: self.id,
            addr: self.addr,
            offset_us: self.offset_window.average() as i64,
            delay_us: self.delay_window.average() as i64,
            last_rtt_at: self.last_rtt_at,
            last_announce_at: self.last_announce_at,
            positions: self.positions.clone(),
        }
    }

    fn upsert_position(&mut self, pos: RemotePosition) {
        if let Some(slot) = self
            .positions
            .iter_mut()
            .find(|p| p.project_id == pos.project_id)
        {
            *slot = pos;
        } else {
            self.positions.push(pos);
        }
    }
}

/// Fixed-size interquartile-mean window. Same trick as
/// `daw-link::RollingAverage`: trim the top + bottom quartile before
/// averaging so a single jittery sample can't swing the estimate.
struct RollingWindow {
    values: Vec<f64>,
    cap: usize,
    cursor: usize,
    len: usize,
}

impl RollingWindow {
    fn new(cap: usize) -> Self {
        Self {
            values: vec![0.0; cap],
            cap,
            cursor: 0,
            len: 0,
        }
    }

    fn push(&mut self, v: f64) {
        self.values[self.cursor] = v;
        self.cursor = (self.cursor + 1) % self.cap;
        if self.len < self.cap {
            self.len += 1;
        }
    }

    fn average(&self) -> f64 {
        if self.len == 0 {
            return 0.0;
        }
        let mut sorted: Vec<f64> = self.values[..self.len].to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if self.len < 4 {
            return sorted.iter().sum::<f64>() / self.len as f64;
        }
        let q = self.len / 4;
        let trimmed = &sorted[q..self.len - q];
        trimmed.iter().sum::<f64>() / trimmed.len() as f64
    }
}

/// Shared peer table. Single writer (the receive task) + multiple
/// readers (audio thread via `peers_snapshot()`, diagnostics, etc.).
pub struct PeerTable {
    inner: RwLock<HashMap<PeerId, Peer>>,
}

impl PeerTable {
    fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Snapshot every currently-known peer. Returns owned values so
    /// the caller can drop the lock immediately.
    pub async fn peers_snapshot(&self) -> Vec<PeerInfo> {
        let guard = self.inner.read().await;
        let now = Instant::now();
        guard
            .values()
            .filter(|p| now.duration_since(p.last_announce_at) < PEER_TTL)
            .map(|p| p.snapshot())
            .collect()
    }
}

/// Live clock-sync session. Holds the spawned tasks; drop to stop.
pub struct ClockSync {
    pub peer_id: PeerId,
    pub peers: Arc<PeerTable>,
    /// Tokio runtime handle the session was bound on. Stored so
    /// `seed_peer_sync` can be called from a non-async context
    /// (architect-dispatched main-thread closures).
    pub runtime: tokio::runtime::Handle,
    /// Most recent local "audio host clock" reading. Updated by the
    /// announce + ping tasks before sending so the wire timestamps
    /// reflect a consistent clock view. Microseconds.
    local_clock: Arc<AtomicU64>,
    /// Audio snapshot source for sample-position broadcasts. None
    /// disables position broadcasting (useful for tests / standalone
    /// peers that don't host a DAW).
    cell: Option<Arc<SnapshotCell>>,
    /// Bound socket — kept alive for the lifetime of the session.
    _socket: Arc<UdpSocket>,
    tasks: Vec<JoinHandle<()>>,
}

impl ClockSync {
    /// Insert (or refresh) a peer manually. Useful for tests, for
    /// crossing multicast-blocked network boundaries, or for joining
    /// a known peer at startup before multicast discovery has had a
    /// chance to fire. Peer still has to be reachable on its
    /// announced port; this just bypasses the multicast discovery
    /// step so pings start flowing immediately.
    pub async fn seed_peer(&self, id: PeerId, addr: SocketAddr) {
        let mut guard = self.peers.inner.write().await;
        let now = Instant::now();
        guard
            .entry(id)
            .and_modify(|p| {
                p.addr = addr;
                p.last_announce_at = now;
            })
            .or_insert_with(|| Peer {
                id,
                addr,
                offset_window: RollingWindow::new(SMOOTHING_WINDOW),
                delay_window: RollingWindow::new(SMOOTHING_WINDOW),
                last_rtt_at: now,
                last_announce_at: now,
                positions: Vec::new(),
            });
    }

    /// Bind to `0.0.0.0:port` + join the multicast group, spawn
    /// announce / ping / receive tasks. The `cell` is the audio
    /// snapshot source — when present, its `host_micros` is used as
    /// the wire clock; otherwise we fall back to a process-local
    /// monotonic clock so peer discovery still functions during
    /// REAPER's audio-engine warm-up.
    pub async fn bind(
        port: u16,
        multicast: Ipv4Addr,
        cell: Option<Arc<SnapshotCell>>,
    ) -> std::io::Result<Self> {
        // Build the socket via socket2 so we can set SO_REUSEADDR +
        // SO_REUSEPORT *before* binding. Required for multiple peers
        // on the same host (CI, dev workstations) — otherwise the
        // second bind hits AddrInUse. Loopback multicast also needs
        // it on macOS / Linux for reliable join.
        let socket = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )?;
        socket.set_reuse_address(true)?;
        #[cfg(unix)]
        socket.set_reuse_port(true)?;
        socket.set_nonblocking(true)?;
        socket.bind(&SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)).into())?;
        let std_socket: std::net::UdpSocket = socket.into();
        let socket = UdpSocket::from_std(std_socket)?;
        socket.join_multicast_v4(multicast, Ipv4Addr::UNSPECIFIED)?;
        // Enable loopback so two peers on the same host can discover
        // each other (default off on some platforms; on for others).
        socket.set_multicast_loop_v4(true)?;

        let peer_id = PeerId::new();
        let peers = Arc::new(PeerTable::new());
        let local_clock = Arc::new(AtomicU64::new(0));
        let socket = Arc::new(socket);

        let multicast_addr = SocketAddr::V4(SocketAddrV4::new(multicast, port));

        let mut tasks = Vec::with_capacity(5);
        tasks.push(spawn_clock_sampler(local_clock.clone(), cell.clone()));
        tasks.push(spawn_announcer(
            socket.clone(),
            peer_id,
            port,
            multicast_addr,
        ));
        tasks.push(spawn_pinger(
            socket.clone(),
            peer_id,
            local_clock.clone(),
            peers.clone(),
        ));
        tasks.push(spawn_receiver(
            socket.clone(),
            peer_id,
            local_clock.clone(),
            peers.clone(),
        ));
        if let Some(c) = cell.clone() {
            tasks.push(spawn_position_broadcaster(
                socket.clone(),
                peer_id,
                c,
                peers.clone(),
            ));
        }

        Ok(Self {
            peer_id,
            peers,
            runtime: tokio::runtime::Handle::current(),
            local_clock,
            cell,
            _socket: socket,
            tasks,
        })
    }

    /// Blocking sibling of [`Self::seed_peer`] — schedules the table
    /// update on the session's runtime and waits for it. Safe to
    /// call from non-async contexts (architect-dispatched main-thread
    /// closures, REAPER actions, etc.).
    pub fn seed_peer_sync(&self, id: PeerId, addr: SocketAddr) {
        let peers = self.peers.clone();
        self.runtime.block_on(async move {
            let mut guard = peers.inner.write().await;
            let now = Instant::now();
            guard
                .entry(id)
                .and_modify(|p| {
                    p.addr = addr;
                    p.last_announce_at = now;
                })
                .or_insert_with(|| Peer {
                    id,
                    addr,
                    offset_window: RollingWindow::new(SMOOTHING_WINDOW),
                    delay_window: RollingWindow::new(SMOOTHING_WINDOW),
                    last_rtt_at: now,
                    last_announce_at: now,
                    positions: Vec::new(),
                });
        });
    }

    pub fn local_clock_micros(&self) -> u64 {
        self.local_clock.load(Ordering::Relaxed)
    }
}

impl Drop for ClockSync {
    fn drop(&mut self) {
        for t in self.tasks.drain(..) {
            t.abort();
        }
    }
}

// ── Background tasks ────────────────────────────────────────────────

/// Continuously refresh `local_clock` from the audio snapshot (or
/// fallback to `Instant`). Other tasks read the AtomicU64 — no per-
/// send lock contention.
fn spawn_clock_sampler(out: Arc<AtomicU64>, cell: Option<Arc<SnapshotCell>>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let start = Instant::now();
        let mut tick = tokio::time::interval(Duration::from_millis(1));
        loop {
            tick.tick().await;
            let micros = cell
                .as_ref()
                .and_then(|c| c.load())
                .map(|s| s.host_micros)
                .unwrap_or_else(|| start.elapsed().as_micros() as u64);
            out.store(micros, Ordering::Relaxed);
        }
    })
}

fn spawn_announcer(
    socket: Arc<UdpSocket>,
    peer_id: PeerId,
    port: u16,
    multicast_addr: SocketAddr,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let msg = Message::Announce {
            from: peer_id,
            port,
        };
        let buf = match bincode::serialize(&msg) {
            Ok(b) => b,
            Err(e) => {
                warn!(?e, "announce serialize failed");
                return;
            }
        };
        let mut tick = tokio::time::interval(ANNOUNCE_INTERVAL);
        loop {
            tick.tick().await;
            if let Err(e) = socket.send_to(&buf, multicast_addr).await {
                debug!(?e, "announce send failed");
            }
        }
    })
}

fn spawn_pinger(
    socket: Arc<UdpSocket>,
    peer_id: PeerId,
    local_clock: Arc<AtomicU64>,
    peers: Arc<PeerTable>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(PING_INTERVAL);
        loop {
            tick.tick().await;
            let snapshot = peers.peers_snapshot().await;
            for peer in snapshot {
                let t1 = local_clock.load(Ordering::Relaxed) as i64;
                let msg = Message::Ping { from: peer_id, t1 };
                let buf = match bincode::serialize(&msg) {
                    Ok(b) => b,
                    Err(e) => {
                        warn!(?e, "ping serialize failed");
                        continue;
                    }
                };
                if let Err(e) = socket.send_to(&buf, peer.addr).await {
                    debug!(?e, peer = ?peer.id, "ping send failed");
                }
            }
        }
    })
}

fn spawn_position_broadcaster(
    socket: Arc<UdpSocket>,
    peer_id: PeerId,
    cell: Arc<SnapshotCell>,
    peers: Arc<PeerTable>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(POSITION_INTERVAL);
        loop {
            tick.tick().await;
            // Collect every project's latest snapshot. When the
            // single-cell hook is in use, this is one frame for the
            // sentinel project_id [0; 16]. When the multi-project
            // hook is in use, also pull from the global registry.
            let mut frames: Vec<crate::AudioSnapshot> = Vec::new();
            if let Some(snap) = cell.load() {
                frames.push(snap);
            }
            if let Some(reg) = crate::registry::global_registry() {
                for (_idx, snap) in reg.snapshots() {
                    if frames.iter().any(|f| f.project_id == snap.project_id) {
                        continue;
                    }
                    frames.push(snap);
                }
            }
            if frames.is_empty() {
                continue;
            }
            let snapshot = peers.peers_snapshot().await;
            for snap in &frames {
                let msg = Message::Position {
                    from: peer_id,
                    project_id: snap.project_id,
                    host_micros: snap.host_micros as i64,
                    playhead_seconds: snap.playhead_seconds,
                    sample_rate: snap.sample_rate,
                    playrate: 1.0, // future: read REAPER playrate
                    is_playing: snap.is_playing,
                };
                let buf = match bincode::serialize(&msg) {
                    Ok(b) => b,
                    Err(e) => {
                        warn!(?e, "position serialize failed");
                        continue;
                    }
                };
                for peer in &snapshot {
                    if let Err(e) = socket.send_to(&buf, peer.addr).await {
                        debug!(?e, peer = ?peer.id, "position send failed");
                    }
                }
            }
        }
    })
}

fn spawn_receiver(
    socket: Arc<UdpSocket>,
    peer_id: PeerId,
    local_clock: Arc<AtomicU64>,
    peers: Arc<PeerTable>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = vec![0u8; 1500];
        loop {
            let (n, from_addr) = match socket.recv_from(&mut buf).await {
                Ok(v) => v,
                Err(e) => {
                    debug!(?e, "recv failed");
                    continue;
                }
            };
            let msg: Message = match bincode::deserialize(&buf[..n]) {
                Ok(m) => m,
                Err(e) => {
                    debug!(?e, "decode failed");
                    continue;
                }
            };
            match msg {
                Message::Announce { from, port } => {
                    if from == peer_id {
                        continue;
                    }
                    let addr = SocketAddr::new(from_addr.ip(), port);
                    let mut guard = peers.inner.write().await;
                    let now = Instant::now();
                    guard
                        .entry(from)
                        .and_modify(|p| {
                            p.last_announce_at = now;
                            p.addr = addr;
                        })
                        .or_insert_with(|| Peer {
                            id: from,
                            addr,
                            offset_window: RollingWindow::new(SMOOTHING_WINDOW),
                            delay_window: RollingWindow::new(SMOOTHING_WINDOW),
                            last_rtt_at: now,
                            last_announce_at: now,
                            positions: Vec::new(),
                        });
                }
                Message::Ping { from, t1 } => {
                    if from == peer_id {
                        continue;
                    }
                    let t2 = local_clock.load(Ordering::Relaxed) as i64;
                    let t3 = t2; // Inline reply — no perceptible gap.
                    let reply = Message::Pong {
                        from: peer_id,
                        t1,
                        t2,
                        t3,
                    };
                    let buf = match bincode::serialize(&reply) {
                        Ok(b) => b,
                        Err(e) => {
                            warn!(?e, "pong serialize failed");
                            continue;
                        }
                    };
                    if let Err(e) = socket.send_to(&buf, from_addr).await {
                        debug!(?e, "pong send failed");
                    }
                }
                Message::Position {
                    from,
                    project_id,
                    host_micros,
                    playhead_seconds,
                    sample_rate,
                    playrate,
                    is_playing,
                } => {
                    if from == peer_id {
                        continue;
                    }
                    let pos = RemotePosition {
                        project_id,
                        host_micros,
                        playhead_seconds,
                        sample_rate,
                        playrate,
                        is_playing,
                        received_at: Instant::now(),
                    };
                    let mut guard = peers.inner.write().await;
                    if let Some(peer) = guard.get_mut(&from) {
                        peer.upsert_position(pos);
                    }
                }
                Message::Pong { from, t1, t2, t3 } => {
                    if from == peer_id {
                        continue;
                    }
                    let t4 = local_clock.load(Ordering::Relaxed) as i64;
                    let offset = ((t2 - t1) + (t3 - t4)) / 2;
                    let delay = ((t4 - t1) - (t3 - t2)) / 2;
                    let mut guard = peers.inner.write().await;
                    if let Some(peer) = guard.get_mut(&from) {
                        peer.offset_window.push(offset as f64);
                        peer.delay_window.push(delay as f64);
                        peer.last_rtt_at = Instant::now();
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_window_trims_outliers() {
        let mut w = RollingWindow::new(8);
        // Push 8 values: six clustered at 100, two outliers at -1000 / 9000.
        for v in [100.0, 100.0, 100.0, 100.0, 100.0, 100.0, -1000.0, 9000.0] {
            w.push(v);
        }
        // Trims 2 from each end → 4 left, all 100 → avg 100.
        assert!((w.average() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn rolling_window_short_window_uses_plain_mean() {
        let mut w = RollingWindow::new(16);
        w.push(10.0);
        w.push(20.0);
        w.push(30.0);
        assert!((w.average() - 20.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn two_peers_converge_via_seed() {
        // Multicast on loopback varies wildly by platform / network
        // stack (Linux often filters it; macOS routes it; CI hosts
        // disable it). Skip discovery; explicitly seed peer
        // addresses. Validates the ping/pong + offset math path.
        let a = ClockSync::bind(17778, DEFAULT_MULTICAST, None)
            .await
            .expect("bind a");
        let b = ClockSync::bind(17779, DEFAULT_MULTICAST, None)
            .await
            .expect("bind b");

        let a_addr: SocketAddr = "127.0.0.1:17778".parse().unwrap();
        let b_addr: SocketAddr = "127.0.0.1:17779".parse().unwrap();
        a.seed_peer(b.peer_id, b_addr).await;
        b.seed_peer(a.peer_id, a_addr).await;

        // Wait up to 5s for at least 8 RTTs (~800ms at 10Hz ping
        // rate). 8 samples = enough for the IQM window to stabilise.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let pa = a.peers.peers_snapshot().await;
            let pb = b.peers.peers_snapshot().await;
            let a_rtt = pa
                .iter()
                .any(|p| p.id == b.peer_id && p.last_rtt_at.elapsed() < Duration::from_millis(500));
            let b_rtt = pb
                .iter()
                .any(|p| p.id == a.peer_id && p.last_rtt_at.elapsed() < Duration::from_millis(500));
            if a_rtt && b_rtt {
                let pa = a.peers.peers_snapshot().await;
                let pb = b.peers.peers_snapshot().await;
                let a_peer = pa.iter().find(|p| p.id == b.peer_id).unwrap();
                let b_peer = pb.iter().find(|p| p.id == a.peer_id).unwrap();
                eprintln!(
                    "a→b offset {}µs delay {}µs   b→a offset {}µs delay {}µs",
                    a_peer.offset_us, a_peer.delay_us, b_peer.offset_us, b_peer.delay_us
                );
                // Loopback delay should be sub-ms; offsets should
                // sum to ~0 (clock symmetry).
                assert!(
                    a_peer.delay_us.unsigned_abs() < 5_000,
                    "loopback delay too large: {}µs",
                    a_peer.delay_us,
                );
                assert!(
                    (a_peer.offset_us + b_peer.offset_us).unsigned_abs() < 5_000,
                    "offset symmetry broken: a→b={} b→a={}",
                    a_peer.offset_us,
                    b_peer.offset_us,
                );
                return;
            }
            if std::time::Instant::now() > deadline {
                panic!("peers never exchanged RTT (a_rtt={a_rtt} b_rtt={b_rtt})");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    #[tokio::test]
    async fn position_broadcast_round_trip() {
        // Wire two peers, each with its own audio snapshot cell.
        // Stuff a known position into A's cell; B should observe it
        // via the Position message within ~200ms (2× POSITION_INTERVAL
        // plus tolerance).
        let cell_a = Arc::new(SnapshotCell::new());
        let cell_b = Arc::new(SnapshotCell::new());

        let a = ClockSync::bind(17780, DEFAULT_MULTICAST, Some(cell_a.clone()))
            .await
            .expect("bind a");
        let b = ClockSync::bind(17781, DEFAULT_MULTICAST, Some(cell_b.clone()))
            .await
            .expect("bind b");

        a.seed_peer(b.peer_id, "127.0.0.1:17781".parse().unwrap())
            .await;
        b.seed_peer(a.peer_id, "127.0.0.1:17780".parse().unwrap())
            .await;

        // A reports playing at 12.345s, 48kHz.
        cell_a.store(&crate::AudioSnapshot {
            sequence: 1,
            project_id: [0u8; 16],
            host_micros: 1_000_000,
            playhead_seconds: 12.345,
            sample_rate: 48_000.0,
            buffer_len: 256,
            is_playing: true,
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let pb = b.peers.peers_snapshot().await;
            let from_a = pb.iter().find(|p| p.id == a.peer_id);
            if let Some(p) = from_a
                && let Some(pos) = p.positions.first().copied()
                && (pos.playhead_seconds - 12.345).abs() < 1e-6
            {
                assert!(pos.is_playing, "expected is_playing=true");
                assert_eq!(pos.sample_rate, 48_000.0);
                eprintln!(
                    "B observed A's position: playhead={:.6}s sr={} host_us={}",
                    pos.playhead_seconds, pos.sample_rate, pos.host_micros
                );
                return;
            }
            if std::time::Instant::now() > deadline {
                panic!("B never observed A's position");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[test]
    fn project_playhead_extrapolates_during_playback() {
        let pos = RemotePosition {
            project_id: [0u8; 16],
            host_micros: 1_000_000,
            playhead_seconds: 10.0,
            sample_rate: 48_000.0,
            playrate: 1.0,
            is_playing: true,
            received_at: Instant::now(),
        };
        // 500ms later in peer's clock → playhead should be 10.5s.
        let projected = pos.project_playhead(1_500_000);
        assert!((projected - 10.5).abs() < 1e-9);

        // Playrate 0.5 → 10.25s.
        let pos_slow = RemotePosition {
            playrate: 0.5,
            ..pos
        };
        assert!((pos_slow.project_playhead(1_500_000) - 10.25).abs() < 1e-9);

        // Stopped → no extrapolation.
        let pos_stopped = RemotePosition {
            is_playing: false,
            ..pos
        };
        assert!((pos_stopped.project_playhead(1_500_000) - 10.0).abs() < 1e-9);
    }
}
