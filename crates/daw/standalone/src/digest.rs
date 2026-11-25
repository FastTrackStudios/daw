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

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Mutex, PoisonError, Weak};

use crate::sync::{FxChainKey, ProjectCell, ProjectState, Standalone};
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
    let (mut digest, fx_guids) = daw.read_project(project_guid, |p| (of_state(p), fx_of(p)))?;
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

/// Each track's FX, in order: guid, name, enabled.
type TrackFx = Vec<(String, Vec<(String, String, bool)>)>;

/// Each track's FX, as the `fx` section lists them.
fn fx_of(p: &ProjectState) -> TrackFx {
    p.tracks
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
        .collect()
}

/// The digest as one number: two projects with the same fingerprint have
/// the same digest (to a 64-bit hash). What "unsaved changes" compares —
/// the song now against the song as it was opened or last saved — cheap
/// to keep where a whole digest is not.
///
/// The project's own sections are hashed again only when its revision
/// moved (every edit moves it); what each FX holds is read from the
/// plugin every time, since a plugin's state changes without the
/// project's. Hashed as it is written, never built as text: the same
/// number as folding [`digest`]'s lines.
#[must_use]
pub fn fingerprint(daw: &Standalone, project_guid: &str) -> Option<u64> {
    let (hash, fx) = project_print(daw, project_guid)?;
    let mut w = Fnv(line(hash, "fx"));
    for (track, chain) in fx.iter() {
        let _ = write!(w, "{track}: ");
        for (i, (guid, name, enabled)) in chain.iter().enumerate() {
            let state = daw.save_plugin_state(guid).unwrap_or_default();
            let _ = write!(
                w,
                "{}{name}{} state={:016x}/{}",
                if i == 0 { "" } else { ", " },
                if *enabled { "" } else { " (off)" },
                fnv1a(&state),
                state.len()
            );
        }
        w.0 ^= 0x0a;
    }
    Some(w.0)
}

/// The fold of one line into a fingerprint.
fn line(h: u64, line: &str) -> u64 {
    fnv1a_from(h, line.as_bytes()) ^ 0x0a
}

/// FNV-1a as a [`std::fmt::Write`]: a line hashed as it is formatted.
struct Fnv(u64);

impl std::fmt::Write for Fnv {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0 = fnv1a_from(self.0, s.as_bytes());
        Ok(())
    }
}

/// A project's own sections, folded, and its FX, as of one revision of
/// one project cell.
struct ProjectPrint {
    /// The cell it was read from: a project replaced under the same guid
    /// is another cell (kept by a `Weak`, so its address is not reused
    /// while this is kept).
    cell: Weak<ProjectCell>,
    revision: u64,
    hash: u64,
    fx: std::sync::Arc<TrackFx>,
}

/// The last [`ProjectPrint`] of each project, by guid.
static PRINTS: Mutex<Option<HashMap<String, ProjectPrint>>> = Mutex::new(None);

/// How many times a project's own sections were hashed (not kept): what
/// the tests count.
static PROJECT_HASHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `project_guid`'s own sections folded (everything but what its FX
/// hold) and its FX: kept while its revision holds.
fn project_print(daw: &Standalone, project_guid: &str) -> Option<(u64, std::sync::Arc<TrackFx>)> {
    let cell = daw.cell(project_guid)?;
    let revision = cell.revision();
    {
        let prints = PRINTS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(kept) = prints.as_ref().and_then(|m| m.get(project_guid))
            && kept.revision == revision
            && std::ptr::eq(kept.cell.as_ptr(), std::sync::Arc::as_ptr(&cell))
        {
            return Some((kept.hash, std::sync::Arc::clone(&kept.fx)));
        }
    }
    let (revision, hash, fx) = {
        let slot = cell.read()?;
        let p = &slot.project;
        let digest = of_state(p);
        let hash = digest
            .sections
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, (name, lines)| {
                lines.iter().fold(line(h, name), |h, l| line(h, l))
            });
        (p.revision, hash, std::sync::Arc::new(fx_of(p)))
    };
    PROJECT_HASHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut prints = PRINTS.lock().unwrap_or_else(PoisonError::into_inner);
    let prints = prints.get_or_insert_with(HashMap::new);
    prints.retain(|_, p| p.cell.strong_count() > 0);
    prints.insert(
        project_guid.to_owned(),
        ProjectPrint {
            cell: std::sync::Arc::downgrade(&cell),
            revision,
            hash,
            fx: std::sync::Arc::clone(&fx),
        },
    );
    Some((hash, fx))
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
    fnv1a_from(0xcbf2_9ce4_8422_2325, bytes)
}

/// FNV-1a over `bytes`, carrying on from `h`.
fn fnv1a_from(h: u64, bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(h, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
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
                    "{src} -> {:?} mode={:?} vol={} pan={} mute={} from={} into={}+{channels}",
                    r.dest_track_guid,
                    r.send_mode,
                    r.volume,
                    r.pan,
                    r.muted,
                    r.source_channels.start_channel,
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
    // An FX by its place in its chain: a reopened built-in FX is the same
    // FX under a new guid.
    let fx_place = |guid: &str| {
        p.fx_chains.iter().find_map(|(chain, entries)| {
            entries
                .iter()
                .position(|e| e.fx.guid == guid)
                .map(|i| format!("{chain:?}#{i}"))
        })
    };
    let mut envelopes: Vec<String> = p
        .envelopes
        .iter()
        .map(|((g, k), d)| {
            let pts: Vec<String> = d
                .points
                .iter()
                .map(|pt| format!("{:.6}={:.6}:{:?}", pt.time.as_seconds(), pt.value, pt.shape))
                .collect();
            let key = match k {
                crate::sync::EnvelopeKey::FxParam {
                    fx_guid,
                    param_index,
                } => format!(
                    "FxParam {} {param_index}",
                    fx_place(fx_guid).unwrap_or_else(|| fx_guid.clone())
                ),
                k => format!("{k:?}"),
            };
            // Its automation items, each by its window and its curve (a
            // pool's number is the file's to choose, so not that).
            let items: Vec<String> = d
                .automation_items
                .iter()
                .map(|(ai, curve)| {
                    let curve: Vec<String> = curve
                        .iter()
                        .map(|pt| {
                            format!("{:.6}={:.6}:{:?}", pt.time.as_seconds(), pt.value, pt.shape)
                        })
                        .collect();
                    format!(
                        "[{:.6}+{:.6} x{} {:.3}/{:.3}{} {}]",
                        ai.position.as_seconds(),
                        ai.length.as_seconds(),
                        ai.play_rate,
                        ai.baseline,
                        ai.amplitude,
                        if ai.loop_source { " loop" } else { "" },
                        curve.join(" ")
                    )
                })
                .collect();
            format!(
                "{g} {key} {:?} {} {}",
                d.automation_mode,
                pts.join(" "),
                items.join(" ")
            )
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

#[cfg(test)]
mod tests {
    use daw_proto::ProjectInfo;

    use super::*;

    /// The fingerprint as [`digest`]'s lines fold to: what
    /// [`fingerprint`] must equal.
    fn folded(daw: &Standalone, guid: &str) -> u64 {
        digest(daw, guid)
            .expect("the project")
            .sections
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, (name, lines)| {
                lines.iter().fold(line(h, name), |h, l| line(h, l))
            })
    }

    fn song(guid: &str, tracks: usize) -> Standalone {
        let daw = Standalone::new();
        daw.seed_project(ProjectInfo {
            guid: guid.into(),
            name: guid.into(),
            path: String::new(),
        });
        daw.write_project(guid, |p| {
            for i in 0..tracks {
                p.tracks.push(daw_proto::Track {
                    guid: format!("t{i}"),
                    name: format!("Track {i}"),
                    volume: 0.5,
                    ..Default::default()
                });
                p.fx_chains.insert(
                    FxChainKey::Track(format!("t{i}")),
                    vec![crate::sync::FxEntry {
                        fx: daw_proto::Fx {
                            guid: format!("fx{i}"),
                            name: format!("EQ {i}"),
                            enabled: i % 2 == 0,
                            ..Default::default()
                        },
                        state_chunk: String::new(),
                        params: HashMap::new(),
                        ranges: Vec::new(),
                    }],
                );
            }
        });
        daw
    }

    #[test]
    fn the_fingerprint_is_the_digest_folded_and_moves_with_an_edit() {
        let daw = song("print-a", 12);
        let before = fingerprint(&daw, "print-a").unwrap();
        assert_eq!(before, folded(&daw, "print-a"));
        daw.write_project("print-a", |p| p.tracks[3].volume = 0.25);
        let after = fingerprint(&daw, "print-a").unwrap();
        assert_ne!(after, before, "an edit moves it");
        assert_eq!(after, folded(&daw, "print-a"));
        // An FX switched off is in it too.
        daw.write_project("print-a", |p| {
            for e in p.fx_chains.values_mut().flatten() {
                e.fx.enabled = !e.fx.enabled;
            }
        });
        assert_eq!(
            fingerprint(&daw, "print-a").unwrap(),
            folded(&daw, "print-a")
        );
    }

    #[test]
    fn a_song_that_holds_still_is_not_hashed_again() {
        let daw = song("print-b", 8);
        let first = fingerprint(&daw, "print-b").unwrap();
        let (_, kept) = project_print(&daw, "print-b").unwrap();
        for _ in 0..5 {
            assert_eq!(fingerprint(&daw, "print-b").unwrap(), first);
            let (_, again) = project_print(&daw, "print-b").unwrap();
            assert!(
                std::sync::Arc::ptr_eq(&kept, &again),
                "kept, not read again"
            );
        }
        daw.write_project("print-b", |p| p.tracks[0].muted = true);
        let (_, after) = project_print(&daw, "print-b").unwrap();
        assert!(
            !std::sync::Arc::ptr_eq(&kept, &after),
            "read again after an edit"
        );
    }

    #[test]
    fn a_song_replaced_under_its_guid_is_hashed_afresh() {
        let a = song("print-c", 4);
        let print_a = fingerprint(&a, "print-c").unwrap();
        // Another engine, the same guid and revision, other tracks.
        let b = song("print-c", 5);
        assert_eq!(
            a.cell("print-c").unwrap().revision(),
            b.cell("print-c").unwrap().revision()
        );
        let print_b = fingerprint(&b, "print-c").unwrap();
        assert_ne!(print_a, print_b);
        assert_eq!(print_b, folded(&b, "print-c"));
        assert_eq!(fingerprint(&a, "print-c").unwrap(), folded(&a, "print-c"));
    }

    /// What a look costs on a song of 120 tracks with 40 items each,
    /// while it holds still: hashed afresh every time (before) against
    /// kept by revision (now). `--ignored --nocapture` to see it.
    #[test]
    #[ignore = "a measurement"]
    fn what_a_look_costs() {
        let daw = song("print-bench", 120);
        for t in 0..120 {
            for i in 0..40 {
                let at = f64::from(i) * 2.0;
                daw_proto::Midi::create_midi_item(
                    &daw,
                    daw_proto::ProjectContext::Project("print-bench".into()),
                    daw_proto::TrackRef::Guid(format!("t{t}")),
                    at,
                    at + 1.5,
                )
                .expect("an item");
            }
        }
        let n = 200;
        let start = std::time::Instant::now();
        for _ in 0..n {
            std::hint::black_box(folded(&daw, "print-bench"));
        }
        let before = start.elapsed() / n;
        assert_eq!(
            fingerprint(&daw, "print-bench"),
            Some(folded(&daw, "print-bench"))
        );
        let start = std::time::Instant::now();
        for _ in 0..n {
            std::hint::black_box(fingerprint(&daw, "print-bench"));
        }
        let after = start.elapsed() / n;
        eprintln!("fingerprint of a still song: {before:?} before, {after:?} now");
        assert!(after < before);
    }
}
