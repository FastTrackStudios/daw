//! Detection for a folded group: sum the mics, detect once, merge lanes.
//!
//! Transients for a kit lane are found on the lane's **summed** signal —
//! the mean of its member mics — not per mic. Per-mic detection is the
//! thing the group rule forbids downstream (two mics would get different
//! cut points), and the sum is also simply a better detector input: the
//! mics are phase-aligned takes of one source, so summing raises the
//! drum against the bleed.
//!
//! The kick and snare lanes each detect on their own sum, and the two
//! hit lists are merged: union, with near-duplicates inside the
//! retrigger window collapsing to the louder. That merged list is the
//! kit's edit points.
//!
//! Spec: `drum-mode.md` r[drums.group.detection-source].

use crate::detect::{DetectConfig, Transient, transients};

/// Mean of the members' mono samples, out to the longest member.
///
/// Mean rather than sum so the level (and so the absolute threshold in
/// the detector) does not depend on how many mics a lane has. A shorter
/// member contributes silence past its end — an item shorter than the
/// group is ordinary.
// r[impl drums.group.detection-source]
pub fn summed_signal(members: &[&[f64]]) -> Vec<f64> {
    let n = members.iter().map(|m| m.len()).max().unwrap_or(0);
    if n == 0 || members.is_empty() {
        return Vec::new();
    }
    let scale = 1.0 / members.len() as f64;
    let mut out = vec![0.0f64; n];
    for m in members {
        for (o, v) in out.iter_mut().zip(m.iter()) {
            *o += v * scale;
        }
    }
    out
}

/// Detect on a lane's summed signal.
// r[impl drums.group.detection-source]
pub fn detect_summed(members: &[&[f64]], sample_rate: f64, cfg: DetectConfig) -> Vec<Transient> {
    transients(&summed_signal(members), sample_rate, cfg)
}

/// Merge hit lists from several lanes into the kit's one edit list.
///
/// Union, sorted by time; two hits within `window_secs` of each other
/// are one hit, and the louder one is kept — the same contest the
/// spacing rule in detection runs, for the same reason: the ghost note
/// must not suppress the hit.
// r[impl drums.group.detection-source]
pub fn merge_hits(lists: &[&[Transient]], window_secs: f64) -> Vec<Transient> {
    let mut all: Vec<Transient> = lists.iter().flat_map(|l| l.iter().cloned()).collect();
    all.sort_by(|a, b| a.at.total_cmp(&b.at));
    let mut out: Vec<Transient> = Vec::with_capacity(all.len());
    for t in all {
        match out.last_mut() {
            Some(last) if t.at - last.at < window_secs => {
                if t.loudness > last.loudness {
                    *last = t;
                }
            }
            _ => out.push(t),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::Hit;

    fn t(at: f64, loudness: f64) -> Transient {
        Transient {
            at,
            loudness,
            crest_db: 12.0,
            hit: Hit {
                sample: (at * 48000.0) as usize,
                peak: loudness,
                rms: loudness * 0.7,
                crest_db: 12.0,
            },
        }
    }

    // r[verify drums.group.detection-source]
    #[test]
    fn summing_is_a_mean_and_pads_short_members() {
        let a = [1.0, 1.0, 1.0, 1.0];
        let b = [0.0, 1.0];
        let s = summed_signal(&[&a, &b]);
        assert_eq!(s, vec![0.5, 1.0, 0.5, 0.5]);
        assert!(summed_signal(&[]).is_empty());
    }

    // r[verify drums.group.detection-source]
    #[test]
    fn merging_collapses_near_duplicates_to_the_louder() {
        let kick = [t(1.000, 0.9), t(2.000, 0.8)];
        let snare = [t(1.004, 0.5), t(1.500, 0.7)];
        let merged = merge_hits(&[&kick, &snare], 0.010);
        let times: Vec<f64> = merged.iter().map(|t| t.at).collect();
        assert_eq!(times, vec![1.000, 1.500, 2.000]);
        assert!(
            (merged[0].loudness - 0.9).abs() < 1e-12,
            "the kick's louder hit won the window"
        );
    }
}
