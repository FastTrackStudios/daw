//! Drift correction — closes the loop on Phase B/C observation by
//! actuating REAPER's playrate when local position diverges from the
//! elected leader's projected position.
//!
//! # Control loop
//!
//! Runs as a tokio task at `CORRECTION_HZ` (~20Hz). Each tick:
//!
//! 1. Snapshot local position from the [`SnapshotCell`].
//! 2. Snapshot peer table from `PeerTable`; elect the lowest-UUID
//!    peer that's actively playing as leader. If we are the leader,
//!    no correction needed — just hold rate at 1.0.
//! 3. Project the leader's playhead into our clock domain using
//!    `RemotePosition::project_playhead` + the per-peer offset.
//! 4. drift_seconds = local_playhead − leader_projected_playhead
//! 5. If |drift| < deadband, target rate = 1.0 (let the natural clock
//!    take over). Otherwise compute a proportional correction:
//!
//!    ```text
//!    rate_delta = -drift_seconds / convergence_time_seconds
//!    rate       = clamp(1.0 + rate_delta, 1 − MAX_DEV, 1 + MAX_DEV)
//!    ```
//!
//!    Negative gain means: if we're AHEAD (drift > 0), we slow down
//!    (rate < 1.0). Convergence time controls aggressiveness — set
//!    so a millisecond of drift takes a second to bleed off (rate
//!    deviates by ~0.1%, well below audible threshold for most
//!    content).
//!
//! 6. Hand the new rate to the host-supplied actuator. The actuator
//!    is responsible for getting onto REAPER's main thread (via
//!    `TaskSupport` typically) and calling
//!    `CSurf_OnPlayRateChange(new_rate)`.
//!
//! # Why proportional + deadband instead of bang-bang
//!
//! ReaBlink uses a bang-bang controller (toggle to 0.94 / 1.06 via
//! `Main_OnCommand 40524/40525`) because Ableton Link only needs
//! beat-level alignment where ±6% is inaudible during a beat. For
//! sample-accurate sync, we need sub-percent corrections so the
//! pitch shift is below ~5 cents (imperceptible). Proportional with
//! ±1% cap fits that constraint and lets us land within a few
//! samples of the leader in steady state.

use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::{debug, trace};

use crate::SnapshotCell;
use crate::clock_sync::{ClockSync, PeerId};

/// How often the corrector runs. 20Hz matches the position-broadcast
/// rate so we always have a fresh leader sample to compare against.
const CORRECTION_HZ: u64 = 20;

/// Default control parameters. Tuned for sample-accurate convergence
/// without audible pitch artifacts.
#[derive(Clone, Copy, Debug)]
pub struct DriftConfig {
    /// Drift below this (in seconds) is ignored — return to rate 1.0
    /// and let the audio engine's natural clock take over. Below the
    /// deadband, sample-rate drift between machines is the dominant
    /// noise source; correction would just chase noise.
    pub deadband_seconds: f64,
    /// Target convergence time. Larger = smoother, less audible
    /// pitch wobble, but slower correction. 1.0s is a good default
    /// for live playback where peers should sound aligned but the
    /// listener can't hear sub-percent rate changes.
    pub convergence_seconds: f64,
    /// Maximum deviation from rate 1.0 in either direction. 0.01 =
    /// ±1% (≈ ±17 cents at worst). Anything larger risks audible
    /// pitch shift on sustained content.
    pub max_rate_deviation: f64,
    /// Per-peer minimum age — drop position frames older than this
    /// from consideration. Avoids correcting against stale data
    /// after a network hiccup or peer disconnect.
    pub max_position_age: Duration,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            deadband_seconds: 50e-6, // 50µs = ~2 samples at 48kHz
            convergence_seconds: 1.0,
            max_rate_deviation: 0.01,
            max_position_age: Duration::from_millis(500),
        }
    }
}

/// Latest correction decision. Exposed via [`DriftCorrector::last_decision`]
/// for diagnostics + tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct DriftDecision {
    pub sequence: u64,
    /// Leader currently tracked, or `None` if we're the leader or
    /// no peer is playing.
    pub leader: Option<PeerId>,
    /// Local minus leader projected, in seconds. `None` when
    /// `leader` is `None`.
    pub drift_seconds: Option<f64>,
    /// Rate we asked the actuator to apply. `1.0` when no
    /// correction is in flight.
    pub target_rate: f64,
}

/// Drift corrector. Holds the spawned task; drop to stop.
pub struct DriftCorrector {
    task: Option<JoinHandle<()>>,
    last_decision_bits: Arc<DecisionCell>,
}

impl DriftCorrector {
    /// Spawn the corrector loop on the current tokio runtime. The
    /// `actuator` closure is called whenever the controller decides
    /// to change the playrate; it's expected to dispatch the change
    /// to REAPER's main thread (typically via `TaskSupport`) and
    /// call `MediumReaper::csurf_on_play_rate_change`.
    ///
    /// The controller call rate is bounded by `CORRECTION_HZ`, so
    /// the actuator runs at most 20× per second under normal
    /// operation — but it MAY be called every tick when the system
    /// is actively converging, so the actuator should be cheap
    /// (just enqueue a main-thread task).
    pub fn spawn<F>(
        cell: Arc<SnapshotCell>,
        clock_sync: Arc<ClockSync>,
        config: DriftConfig,
        actuator: F,
    ) -> Self
    where
        F: Fn(f64) + Send + Sync + 'static,
    {
        let last_decision_bits = Arc::new(DecisionCell::default());
        let decision_cell = last_decision_bits.clone();
        let local_peer_id = clock_sync.peer_id;

        let task = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(1000 / CORRECTION_HZ));
            let mut last_rate = 1.0f64;
            let mut seq = 0u64;

            loop {
                tick.tick().await;
                seq = seq.wrapping_add(1);

                let Some(local) = cell.load() else { continue };
                if !local.is_playing {
                    // Reset rate when transport stops so we don't
                    // inherit stale corrections on next play.
                    if (last_rate - 1.0).abs() > 1e-9 {
                        actuator(1.0);
                        last_rate = 1.0;
                    }
                    decision_cell.store(DriftDecision {
                        sequence: seq,
                        leader: None,
                        drift_seconds: None,
                        target_rate: 1.0,
                    });
                    continue;
                }

                let peers = clock_sync.peers.peers_snapshot().await;
                let Some(leader) = elect_leader(&peers, local_peer_id, config.max_position_age)
                else {
                    // We're the leader (or no playing peer) — hold
                    // rate at 1.0 so we don't accumulate drift from
                    // a previous correction cycle.
                    if (last_rate - 1.0).abs() > 1e-9 {
                        actuator(1.0);
                        last_rate = 1.0;
                    }
                    decision_cell.store(DriftDecision {
                        sequence: seq,
                        leader: None,
                        drift_seconds: None,
                        target_rate: 1.0,
                    });
                    continue;
                };

                // Use the leader's playing position; the elector
                // already guaranteed at least one playing entry.
                // Multi-project drift (one corrector per project)
                // can be wired by passing a project-id filter into
                // config; for now we lock to the leader's first
                // playing project, which matches single-project
                // behavior.
                let position = leader
                    .positions
                    .iter()
                    .find(|p| p.is_playing)
                    .copied()
                    .expect("elected leader has a playing position");
                let offset_us = leader.offset_us;

                // Project leader's playhead into OUR clock domain:
                //   project_playhead(now_in_peer_clock_micros)
                //   peer_clock_now = local_clock_now + offset_us
                let now_in_peer_clock = local.host_micros as i64 + offset_us;
                let leader_projected = position.project_playhead(now_in_peer_clock);

                let drift = local.playhead_seconds - leader_projected;

                let target_rate = if drift.abs() < config.deadband_seconds {
                    1.0
                } else {
                    let rate_delta = -drift / config.convergence_seconds;
                    let clamped = rate_delta
                        .max(-config.max_rate_deviation)
                        .min(config.max_rate_deviation);
                    1.0 + clamped
                };

                // Only call the actuator when the rate actually
                // changes (within an epsilon). Avoids spamming
                // REAPER's main thread with redundant work.
                if (target_rate - last_rate).abs() > 1e-6 {
                    actuator(target_rate);
                    last_rate = target_rate;
                    trace!(
                        drift_us = (drift * 1e6) as i64,
                        target_rate,
                        leader = ?leader.id,
                        "drift correction applied",
                    );
                }

                decision_cell.store(DriftDecision {
                    sequence: seq,
                    leader: Some(leader.id),
                    drift_seconds: Some(drift),
                    target_rate,
                });
            }
        });

        Self {
            task: Some(task),
            last_decision_bits,
        }
    }

    /// Latest controller decision. `None` until the loop has run at
    /// least once.
    pub fn last_decision(&self) -> Option<DriftDecision> {
        self.last_decision_bits.load()
    }
}

impl Drop for DriftCorrector {
    fn drop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

/// Pick the leader from a peer table. Strategy: smallest-UUID peer
/// that's currently playing and has a recent enough position frame.
/// Returns `None` when no other peer qualifies OR when we (the local
/// peer) win the election — caller treats both as "no correction".
fn elect_leader(
    peers: &[crate::clock_sync::PeerInfo],
    local_peer_id: PeerId,
    max_age: Duration,
) -> Option<crate::clock_sync::PeerInfo> {
    let mut best: Option<&crate::clock_sync::PeerInfo> = None;
    let mut best_id: Option<uuid::Uuid> = None;
    for peer in peers {
        // A peer is eligible if it has at least one playing,
        // recently-broadcast position frame.
        let has_active = peer
            .positions
            .iter()
            .any(|pos| pos.is_playing && pos.received_at.elapsed() <= max_age);
        if !has_active {
            continue;
        }
        if peer.id == local_peer_id {
            continue;
        }
        match best_id {
            None => {
                best = Some(peer);
                best_id = Some(peer.id.0);
            }
            Some(b) if peer.id.0 < b => {
                best = Some(peer);
                best_id = Some(peer.id.0);
            }
            _ => {}
        }
    }
    // If our id is smaller than the best remote we found, WE are the
    // leader — return None so the controller holds rate at 1.0.
    if let Some(b) = best_id
        && local_peer_id.0 < b
    {
        return None;
    }
    best.cloned()
}

/// Lock-free single-writer / multi-reader cell for the latest
/// [`DriftDecision`]. Same seqlock pattern as
/// [`crate::SnapshotCell`].
#[derive(Default)]
struct DecisionCell {
    seq: AtomicU64,
    sequence: AtomicU64,
    leader_hi: AtomicU64,
    leader_lo: AtomicU64,
    has_leader: AtomicU64,
    drift_bits: AtomicU64,
    has_drift: AtomicU64,
    rate_bits: AtomicU64,
}

impl DecisionCell {
    fn store(&self, d: DriftDecision) {
        let prev = self.seq.load(Ordering::Relaxed);
        self.seq.store(prev.wrapping_add(1), Ordering::Release);
        self.sequence.store(d.sequence, Ordering::Relaxed);
        match d.leader {
            Some(id) => {
                let bytes = id.0.as_u128();
                self.leader_hi
                    .store((bytes >> 64) as u64, Ordering::Relaxed);
                self.leader_lo.store(bytes as u64, Ordering::Relaxed);
                self.has_leader.store(1, Ordering::Relaxed);
            }
            None => self.has_leader.store(0, Ordering::Relaxed),
        }
        match d.drift_seconds {
            Some(v) => {
                self.drift_bits.store(v.to_bits(), Ordering::Relaxed);
                self.has_drift.store(1, Ordering::Relaxed);
            }
            None => self.has_drift.store(0, Ordering::Relaxed),
        }
        self.rate_bits
            .store(d.target_rate.to_bits(), Ordering::Relaxed);
        self.seq.store(prev.wrapping_add(2), Ordering::Release);
    }

    fn load(&self) -> Option<DriftDecision> {
        for _ in 0..4 {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 == 0 {
                return None;
            }
            if s1 & 1 != 0 {
                core::hint::spin_loop();
                continue;
            }
            let sequence = self.sequence.load(Ordering::Relaxed);
            let leader = if self.has_leader.load(Ordering::Relaxed) != 0 {
                let hi = self.leader_hi.load(Ordering::Relaxed) as u128;
                let lo = self.leader_lo.load(Ordering::Relaxed) as u128;
                Some(PeerId(uuid::Uuid::from_u128((hi << 64) | lo)))
            } else {
                None
            };
            let drift = if self.has_drift.load(Ordering::Relaxed) != 0 {
                Some(f64::from_bits(self.drift_bits.load(Ordering::Relaxed)))
            } else {
                None
            };
            let target_rate = f64::from_bits(self.rate_bits.load(Ordering::Relaxed));
            let s2 = self.seq.load(Ordering::Acquire);
            if s1 == s2 {
                return Some(DriftDecision {
                    sequence,
                    leader,
                    drift_seconds: drift,
                    target_rate,
                });
            }
        }
        None
    }
}

/// Mark the decision-cell as unused-on-Drop-only to silence the
/// "field never read" lint when only the spawn caller reads it.
#[allow(dead_code)]
fn _force_decision_cell_use(_: &DecisionCell) {}

// Suppress warning about unused `debug` import on builds where
// tracing macros are stripped.
fn _force_debug_use() {
    debug!("noop");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock_sync::{PeerInfo, RemotePosition};
    use std::net::SocketAddr;
    use std::time::Instant;
    use uuid::Uuid;

    fn make_peer(id: u128, playing: bool, playhead: f64, host_us: i64) -> PeerInfo {
        PeerInfo {
            id: PeerId(Uuid::from_u128(id)),
            addr: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
            offset_us: 0,
            delay_us: 0,
            last_rtt_at: Instant::now(),
            last_announce_at: Instant::now(),
            positions: vec![RemotePosition {
                project_id: [0u8; 16],
                host_micros: host_us,
                playhead_seconds: playhead,
                sample_rate: 48_000.0,
                playrate: 1.0,
                is_playing: playing,
                received_at: Instant::now(),
            }],
        }
    }

    #[test]
    fn election_picks_smallest_uuid_when_we_are_larger() {
        let me = PeerId(Uuid::from_u128(200));
        let peers = vec![make_peer(100, true, 1.0, 0), make_peer(150, true, 1.0, 0)];
        let leader = elect_leader(&peers, me, Duration::from_secs(1)).unwrap();
        assert_eq!(leader.id.0.as_u128(), 100);
    }

    #[test]
    fn election_returns_none_when_we_are_smallest() {
        let me = PeerId(Uuid::from_u128(50));
        let peers = vec![make_peer(100, true, 1.0, 0)];
        assert!(elect_leader(&peers, me, Duration::from_secs(1)).is_none());
    }

    #[test]
    fn election_skips_stopped_peers() {
        let me = PeerId(Uuid::from_u128(200));
        let peers = vec![make_peer(50, false, 0.0, 0), make_peer(100, true, 1.0, 0)];
        let leader = elect_leader(&peers, me, Duration::from_secs(1)).unwrap();
        assert_eq!(leader.id.0.as_u128(), 100);
    }

    #[test]
    fn decision_cell_round_trip() {
        let cell = DecisionCell::default();
        assert!(cell.load().is_none());
        let d = DriftDecision {
            sequence: 42,
            leader: Some(PeerId(Uuid::from_u128(0xabc))),
            drift_seconds: Some(0.0012),
            target_rate: 0.9985,
        };
        cell.store(d);
        let got = cell.load().unwrap();
        assert_eq!(got.sequence, 42);
        assert_eq!(got.leader.unwrap().0.as_u128(), 0xabc);
        assert!((got.drift_seconds.unwrap() - 0.0012).abs() < 1e-12);
        assert!((got.target_rate - 0.9985).abs() < 1e-12);
    }
}
