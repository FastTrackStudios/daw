//! A project, written down line by line: what two projects must share to
//! be the same project.
//!
//! What a save is checked against. Open a saved session in a fresh engine
//! and its digest should be the live project's, section for section — the
//! tracks and their mixer state, the routing, every item and take down to
//! its offset, its stretch markers and fixed lane, the MIDI, every FX in
//! order with **what it holds** (a hash of `save_plugin_state`: a sampler's
//! kit, a rig's capture, a reverb's time), the envelopes, markers, regions
//! and the tempo map. GUIDs are part of it: a save keeps them.
//!
//! [`differences`] names what drifted, a section and its first lines apart,
//! rather than "not equal".

use crate::sync::{FxChainKey, ProjectState, Standalone};
use daw_proto::{Marker, Position};

/// A project's lines, by section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Digest {
    pub sections: Vec<(&'static str, Vec<String>)>,
}

/// The digest of `project_guid` in `daw`; `None` if there is no such
/// project.
#[must_use]
pub fn digest(daw: &Standalone, project_guid: &str) -> Option<Digest> {
    let (mut digest, fx_guids) = daw.read_project(project_guid, |p| {
        let fx_guids: Vec<(String, Vec<(String, String, bool)>)> = p
            .tracks
            .iter()
            .map(|t| {
                let chain = p
                    .fx_chains
                    .get(&FxChainKey::Track(t.guid.clone()))
                    .into_iter()
                    .flatten()
                    .map(|e| (e.fx.guid.clone(), e.fx.name.clone(), e.fx.enabled))
                    .collect();
                (t.guid.clone(), chain)
            })
            .collect();
        (of_state(p), fx_guids)
    })?;
    // What each FX holds, read from the plugin itself — outside the
    // project's lock, as the plugins are.
    let fx = fx_guids
        .iter()
        .map(|(track, chain)| {
            let entries: Vec<String> = chain
                .iter()
                .map(|(guid, name, enabled)| {
                    let state = daw.save_plugin_state(guid).unwrap_or_default();
                    format!(
                        "{name}{} state={:016x}/{}",
                        if *enabled { "" } else { " (off)" },
                        fnv1a(&state),
                        state.len()
                    )
                })
                .collect();
            format!("{track}: {}", entries.join(", "))
        })
        .collect();
    digest.sections.push(("fx", fx));
    Some(digest)
}

/// What differs between `before` and `after`: per section that does, its
/// name, how many lines each side has, and the first few lines apart.
/// Empty when they are the same.
#[must_use]
pub fn differences(before: &Digest, after: &Digest) -> Vec<String> {
    const SHOWN: usize = 4;
    let mut out = Vec::new();
    for (name, a) in &before.sections {
        let b = after
            .sections
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, l)| l.as_slice())
            .unwrap_or(&[]);
        if a.as_slice() == b {
            continue;
        }
        out.push(format!(
            "{name}: {} lines before, {} after",
            a.len(),
            b.len()
        ));
        let gone = a.iter().filter(|l| !b.contains(l)).take(SHOWN);
        let new = b.iter().filter(|l| !a.contains(l)).take(SHOWN);
        out.extend(gone.map(|l| format!("  - {l}")));
        out.extend(new.map(|l| format!("  + {l}")));
    }
    out
}

/// FNV-1a, 64-bit: a stable hash of a state blob (std's is seeded).
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Everything but the FX's held state, from the project itself.
fn of_state(p: &ProjectState) -> Digest {
    let secs = |pos: &Position| pos.time.map_or(-1.0, |t| t.as_seconds());
    let tracks = p
        .tracks
        .iter()
        .map(|t| {
            let ext = p.track_ext.get(&t.guid).cloned().unwrap_or_default();
            format!(
                "{} {:?} parent={:?} folder={} color={:?} vol={} pan={} mute={} solo={} \
                 phase={} width={:?} height={:?} tcp={} mcp={} mainsend={} nch={} lanes={}",
                t.guid,
                t.name,
                t.parent_guid,
                t.is_folder,
                t.color,
                t.volume,
                t.pan,
                t.muted,
                t.soloed,
                t.phase_inverted,
                t.width,
                t.height,
                t.visible_in_tcp,
                t.visible_in_mixer,
                ext.parent_send_enabled,
                ext.num_channels,
                t.lane_count,
            )
        })
        .collect();
    let mut sends: Vec<String> = p
        .sends
        .iter()
        .flat_map(|(src, rs)| {
            rs.iter().map(move |r| {
                // 0 channels is the main pair, as 2 is: the file writes
                // them alike (`channels_to_rpp`), so they are one value.
                let channels = r.dest_channels.num_channels.max(2);
                format!(
                    "{src} -> {:?} mode={:?} vol={} pan={} mute={} into={}+{channels}",
                    r.dest_track_guid,
                    r.send_mode,
                    r.volume,
                    r.pan,
                    r.muted,
                    r.dest_channels.start_channel
                )
            })
        })
        .collect();
    sends.sort();
    let mut items = Vec::new();
    let mut midi = Vec::new();
    let mut stretch = Vec::new();
    for t in &p.tracks {
        for ig in p.items_by_track.get(&t.guid).into_iter().flatten() {
            let i = &p.items[ig].item;
            items.push(format!(
                "{} {} pos={} len={} mute={} vol={} fin={} fout={} loop={} lane={:?}",
                t.guid,
                i.guid,
                i.position.as_seconds(),
                i.length.as_seconds(),
                i.muted,
                i.volume,
                i.fade_in_length.as_seconds(),
                i.fade_out_length.as_seconds(),
                i.loop_source,
                i.fixed_lane,
            ));
            let Some(tl) = p.takes.get(ig) else { continue };
            items.push(format!("  active={}", tl.active_idx));
            for tk in &tl.takes {
                items.push(format!(
                    "  take {} vol={} rate={} offs={} src={:?} midi={}",
                    tk.guid,
                    tk.volume,
                    tk.play_rate,
                    tk.start_offset.as_seconds(),
                    tk.source_file_path,
                    tk.is_midi,
                ));
                for m in p.stretch_markers.get(&tk.guid).into_iter().flatten() {
                    stretch.push(format!(
                        "{} at={} src={} slope={}",
                        tk.guid, m.position, m.source_position, m.slope
                    ));
                }
                for n in p.midi_notes.get(&tk.guid).into_iter().flatten() {
                    midi.push(format!(
                        "{} note ch={} p={} v={} at={} len={}",
                        tk.guid, n.channel, n.pitch, n.velocity, n.start_ppq, n.length_ppq
                    ));
                }
            }
        }
    }
    let mut envelopes: Vec<String> = p
        .envelopes
        .iter()
        .map(|((g, k), d)| {
            let pts: Vec<String> = d
                .points
                .iter()
                .map(|pt| format!("{:.6}={:.6}", pt.time.as_seconds(), pt.value))
                .collect();
            format!("{g} {k:?} {}", pts.join(" "))
        })
        .collect();
    envelopes.sort();
    let mut markers: Vec<String> = p
        .markers
        .values()
        .map(|m: &Marker| format!("{} {:?} lane={:?}", secs(&m.position), m.name, m.lane))
        .collect();
    markers.sort();
    let mut regions: Vec<String> = p
        .regions
        .values()
        .map(|r| {
            format!(
                "{}..{} {:?} lane={:?}",
                r.time_range.start_seconds(),
                r.time_range.end_seconds(),
                r.name,
                r.lane
            )
        })
        .collect();
    regions.sort();
    let tempo = p
        .tempo_points
        .iter()
        .map(|tp| format!("{} {}", secs(&tp.position), tp.bpm))
        .collect();
    Digest {
        sections: vec![
            ("tracks", tracks),
            ("sends", sends),
            ("items", items),
            ("stretch markers", stretch),
            ("midi", midi),
            ("envelopes", envelopes),
            ("markers", markers),
            ("regions", regions),
            ("tempo", tempo),
        ],
    }
}
