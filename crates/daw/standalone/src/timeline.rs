//! A track's timeline as `com.fasttrackstudio.host-timeline/1` gives it
//! (daw-clap-ext `docs/host-timeline.md`): every note and controller of
//! the track's items, merged, in project quarter notes, as the renderer
//! plays them, and the song's tempo map.
//!
//! How an item's MIDI becomes the track's, the way the realtime render
//! plays it (`audio_engine::render::snapshot`):
//!
//! - a take's notes count from its item's start (`start_ppq` is quarter
//!   notes from the item's start: `midi.rs`'s `add_notes`), so a note's
//!   project QN is the item's start QN plus its own;
//! - the active take only; a muted item, a muted note and a zero-velocity
//!   note are left out;
//! - a note is cut at the item's end, and one starting outside the item
//!   is left out; a looped item (`loop_source`, with its take's source
//!   length known) repeats its notes for the item's length.

use daw_clap_ext::timeline::{Control, Ctrl, Note, Tempo, TempoShape, Timeline};

use crate::audio_engine::render::TempoMap;
use crate::sync::ProjectState;

/// Tolerance for "at the item's end" (QN).
const EDGE_QN: f64 = 1e-9;

/// `track`'s timeline in `p`, `None` if the project has no such track.
/// `revision` is stamped on it as given (the caller keeps the count).
#[must_use]
pub fn track_timeline(p: &ProjectState, track: &str, revision: u64) -> Option<Timeline> {
    if !p.tracks.iter().any(|t| t.guid == track) {
        return None;
    }
    let tempo = TempoMap::from_state(p);
    let mut out = Timeline {
        revision,
        tempo: tempo_points(p, &tempo),
        ..Timeline::default()
    };
    for item in p.items_by_track.get(track).into_iter().flatten() {
        if let Some(window) = ItemWindow::of(p, &tempo, item) {
            add_item(p, &window, &mut out);
        }
    }
    out.notes.sort_by(|a, b| {
        a.start_qn
            .total_cmp(&b.start_qn)
            .then(a.channel.cmp(&b.channel))
            .then(a.key.cmp(&b.key))
    });
    out.ctrls.sort_by(|a, b| a.qn.total_cmp(&b.qn));
    Some(out)
}

/// The tempo map as the timeline carries it: each segment's start QN and
/// seconds (the renderer's own), its bpm, the project's time signature.
fn tempo_points(p: &ProjectState, tempo: &TempoMap) -> Vec<Tempo> {
    let numerator = u16::try_from(p.transport.time_signature.numerator()).unwrap_or(4);
    let denominator = u16::try_from(p.transport.time_signature.denominator()).unwrap_or(4);
    tempo
        .segments()
        .map(|(seconds, qn, bpm)| Tempo {
            qn,
            seconds,
            bpm,
            numerator,
            denominator,
            shape: TempoShape::Step,
        })
        .collect()
}

/// Where one item's active take plays, in project QN.
struct ItemWindow<'a> {
    take: &'a str,
    /// The item's start and end, QN.
    start_qn: f64,
    end_qn: f64,
    /// The take's length when the item loops it, QN.
    loop_qn: Option<f64>,
}

impl<'a> ItemWindow<'a> {
    fn of(p: &'a ProjectState, tempo: &TempoMap, item: &str) -> Option<Self> {
        let entry = p.items.get(item)?;
        if entry.item.muted {
            return None;
        }
        let takes = p.takes.get(item)?;
        let take = takes.takes.get(takes.active_idx as usize)?;
        let pos = entry.item.position.as_seconds();
        let start_qn = tempo.seconds_to_beat(pos);
        let end_qn = tempo.seconds_to_beat(pos + entry.item.length.as_seconds());
        let loop_qn = entry
            .item
            .loop_source
            .then_some(take.source_length)
            .flatten()
            .map(|len| tempo.seconds_to_beat(pos + len.as_seconds()) - start_qn)
            .filter(|l| *l > EDGE_QN);
        Some(Self {
            take: &take.guid,
            start_qn,
            end_qn,
            loop_qn,
        })
    }

    /// Each pass of the take over the item: the QN its own zero lands on.
    /// One pass unless the item loops its take.
    fn passes(&self) -> impl Iterator<Item = f64> + '_ {
        let (len, count) = match self.loop_qn {
            Some(len) => (len, u32::MAX),
            None => (0.0, 1),
        };
        (0..count)
            .map(move |i| f64::from(i).mul_add(len, self.start_qn))
            .take_while(|at| *at < self.end_qn - EDGE_QN)
    }

    /// Where a pass's take-relative span plays, cut to the item and to the
    /// pass; `None` when it starts outside both.
    fn place(&self, pass: f64, from: f64, to: f64) -> Option<(f64, f64)> {
        let pass_end = self
            .loop_qn
            .map_or(self.end_qn, |l| (pass + l).min(self.end_qn));
        let start = pass + from;
        if from < -EDGE_QN || start >= pass_end - EDGE_QN {
            return None;
        }
        Some((start, (pass + to).min(pass_end)))
    }
}

fn add_item(p: &ProjectState, w: &ItemWindow<'_>, out: &mut Timeline) {
    out.end_qn = out.end_qn.max(w.end_qn);
    let notes = p
        .midi_notes
        .get(w.take)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let ctrls = take_controls(p, w.take);
    for pass in w.passes() {
        out.notes.extend(
            notes
                .iter()
                .filter(|n| !n.muted && n.velocity > 0)
                .filter_map(|n| {
                    let (start_qn, end_qn) =
                        w.place(pass, n.start_ppq, n.start_ppq + n.length_ppq.max(0.0))?;
                    Some(Note {
                        start_qn,
                        end_qn: end_qn.max(start_qn),
                        channel: n.channel & 0x0f,
                        key: n.pitch & 0x7f,
                        velocity: n.velocity.min(127),
                        release_velocity: 64,
                    })
                }),
        );
        out.ctrls.extend(ctrls.iter().filter_map(|c| {
            let (qn, _) = w.place(pass, c.qn, c.qn)?;
            Some(Ctrl { qn, ..*c })
        }));
    }
}

/// A take's controllers, take-relative (`qn` from the item's start).
fn take_controls(p: &ProjectState, take: &str) -> Vec<Ctrl> {
    let mut out = Vec::new();
    let ctrl = |qn: f64, channel: u8, control| Ctrl {
        qn,
        channel: channel & 0x0f,
        control,
    };
    for c in p.midi_ccs.get(take).into_iter().flatten() {
        out.push(ctrl(
            c.position_ppq,
            c.channel,
            Control::Cc {
                controller: c.controller & 0x7f,
                value: c.value.min(127),
            },
        ));
    }
    for b in p.midi_pitch_bends.get(take).into_iter().flatten() {
        // Stored signed −8192..8191; the timeline's is 14-bit, 8192 centre.
        let bend = (i32::from(b.value) + 8192).clamp(0, 16_383) as u16;
        out.push(ctrl(b.position_ppq, b.channel, Control::PitchBend(bend)));
    }
    for c in p.midi_program_changes.get(take).into_iter().flatten() {
        out.push(ctrl(
            c.position_ppq,
            c.channel,
            Control::Program(c.program.min(127)),
        ));
    }
    for c in p.midi_channel_pressures.get(take).into_iter().flatten() {
        out.push(ctrl(
            c.position_ppq,
            c.channel,
            Control::ChannelPressure(c.pressure.min(127)),
        ));
    }
    for c in p.midi_poly_pressures.get(take).into_iter().flatten() {
        out.push(ctrl(
            c.position_ppq,
            c.channel,
            Control::PolyPressure {
                key: c.note & 0x7f,
                value: c.pressure.min(127),
            },
        ));
    }
    out
}

impl crate::sync::Standalone {
    /// `track`'s timeline in project `project` (`track_timeline`), stamped
    /// with `revision`. `None` without such a project or track.
    #[must_use]
    pub fn track_timeline(&self, project: &str, track: &str, revision: u64) -> Option<Timeline> {
        self.read_project(project, |p| track_timeline(p, track, revision))
            .flatten()
    }

    /// `project`'s revision, if it is open (moves on every edit; a
    /// timeline's content can only change when it does).
    #[must_use]
    pub fn project_revision(&self, project: &str) -> Option<u64> {
        self.read_project(project, |p| p.revision)
    }
}

#[cfg(test)]
mod tests {
    use daw_proto::midi::{MidiCC, MidiNote};
    use daw_proto::{Duration, MidiTakeLocation, ProjectContext, ProjectInfo, TrackRef};

    use super::*;
    use crate::audio_engine::render::LiveHints;
    use crate::sync::Standalone;

    const SONG: &str = "song";
    const TRACK: &str = "strings";

    fn song() -> Standalone {
        let daw = Standalone::new();
        daw.seed_project(ProjectInfo {
            guid: SONG.into(),
            name: SONG.into(),
            path: String::new(),
        });
        daw.write_project(SONG, |p| {
            p.tracks.push(daw_proto::Track {
                guid: TRACK.into(),
                name: TRACK.into(),
                ..Default::default()
            });
        });
        daw
    }

    /// An item from `start` to `end` seconds with `notes` (QN from the
    /// item's start); its take guid.
    fn item(daw: &Standalone, start: f64, end: f64, notes: &[(f64, f64, u8, u8)]) -> String {
        let loc: MidiTakeLocation = daw_proto::Midi::create_midi_item(
            daw,
            ProjectContext::Project(SONG.into()),
            TrackRef::Guid(TRACK.into()),
            start,
            end,
        )
        .expect("an item");
        let daw_proto::ItemRef::Guid(item) = loc.item else {
            panic!("by guid")
        };
        daw.write_project(SONG, |p| {
            let take = p.takes.get(&item).unwrap().takes[0].guid.clone();
            let notes = notes
                .iter()
                .enumerate()
                .map(|(i, &(at, len, channel, pitch))| MidiNote {
                    index: i as u32,
                    channel,
                    pitch,
                    velocity: 100,
                    start_ppq: at,
                    length_ppq: len,
                    ..Default::default()
                })
                .collect();
            p.midi_notes.insert(take.clone(), notes);
            take
        })
        .unwrap()
    }

    fn timeline(daw: &Standalone) -> Timeline {
        daw.track_timeline(SONG, TRACK, 1).expect("the track")
    }

    fn starts(t: &Timeline) -> Vec<(f64, f64, u8)> {
        t.notes
            .iter()
            .map(|n| (n.start_qn, n.end_qn, n.key))
            .collect()
    }

    #[test]
    fn items_merge_at_their_positions_cut_at_their_ends() {
        let daw = song();
        // 120 bpm: 1 s = 2 QN. One item at 0–2 s, one at 4–5 s whose last
        // note runs past its end and whose first starts before it.
        item(&daw, 0.0, 2.0, &[(0.0, 2.1, 0, 60), (2.0, 2.0, 1, 62)]);
        item(&daw, 4.0, 5.0, &[(-0.5, 1.0, 0, 70), (1.0, 4.0, 0, 72)]);
        let t = timeline(&daw);
        assert_eq!(
            starts(&t),
            vec![(0.0, 2.1, 60), (2.0, 4.0, 62), (9.0, 10.0, 72)]
        );
        assert_eq!(t.end_qn, 10.0);
        assert_eq!(t.channels(), vec![0, 1]);
        assert_eq!(t.tempo.len(), 1);
        assert_eq!((t.tempo[0].qn, t.tempo[0].bpm), (0.0, 120.0));
    }

    #[test]
    fn muted_items_and_notes_are_left_out_a_looped_item_repeats() {
        let daw = song();
        let muted = item(&daw, 0.0, 1.0, &[(0.0, 1.0, 0, 50)]);
        let looped = item(&daw, 2.0, 5.0, &[(0.0, 0.5, 0, 60), (1.5, 1.0, 0, 64)]);
        daw.write_project(SONG, |p| {
            let item_of = |take: &str| {
                p.takes
                    .iter()
                    .find(|(_, tl)| tl.takes.iter().any(|t| t.guid == take))
                    .map(|(i, _)| i.clone())
                    .unwrap()
            };
            let (m, l) = (item_of(&muted), item_of(&looped));
            p.items.get_mut(&m).unwrap().item.muted = true;
            p.items.get_mut(&l).unwrap().item.loop_source = true;
            // A two-QN (1 s) source looped over a 3 s item: three passes,
            // the last cut at the item's end.
            p.takes.get_mut(&l).unwrap().takes[0].source_length = Some(Duration::from_seconds(1.0));
            p.midi_ccs.insert(
                looped.clone(),
                vec![MidiCC {
                    channel: 0,
                    controller: 58,
                    value: 2,
                    position_ppq: 0.0,
                    ..Default::default()
                }],
            );
        });
        let t = timeline(&daw);
        assert_eq!(
            starts(&t),
            vec![
                (4.0, 4.5, 60),
                (5.5, 6.0, 64),
                (6.0, 6.5, 60),
                (7.5, 8.0, 64),
                (8.0, 8.5, 60),
                (9.5, 10.0, 64),
            ]
        );
        assert_eq!(
            t.ctrls.iter().map(|c| c.qn).collect::<Vec<_>>(),
            vec![4.0, 6.0, 8.0]
        );
    }

    #[test]
    fn the_tempo_map_carries_the_renderers_own_seconds() {
        let daw = song();
        daw.write_project(SONG, |p| {
            p.tempo_points
                .push(daw_proto::TempoPoint::from_seconds(0.0, 120.0));
            p.tempo_points
                .push(daw_proto::TempoPoint::from_seconds(4.0, 90.0));
        });
        // Item at 5 s: QN 8 + 1 s at 90 bpm = QN 9.5.
        item(&daw, 5.0, 6.0, &[(0.0, 1.0, 0, 60)]);
        let t = timeline(&daw);
        assert_eq!(
            t.tempo
                .iter()
                .map(|p| (p.qn, p.seconds, p.bpm))
                .collect::<Vec<_>>(),
            vec![(0.0, 0.0, 120.0), (8.0, 4.0, 90.0)]
        );
        assert!((t.notes[0].start_qn - 9.5).abs() < 1e-9);
    }

    #[test]
    fn a_claimed_track_delivers_no_item_midi_to_its_chain() {
        let daw = song();
        item(&daw, 0.0, 2.0, &[(0.0, 1.0, 0, 60)]);
        let events = |daw: &Standalone| {
            let hints = daw.live_hints();
            let snap = daw
                .read_project(SONG, |p| {
                    crate::audio_engine::render::RenderSnapshotFor::build_live(p, &hints)
                })
                .unwrap();
            let mut out = Vec::new();
            crate::audio_engine::render::collect_track_midi(&mut out, &snap, 0, 0.0, 1.0, 48_000.0);
            out.len()
        };
        assert_eq!(
            events(&daw),
            2,
            "unclaimed: its note-on and note-off are delivered"
        );
        let rev = daw.project_revision(SONG).unwrap();
        assert!(daw.set_track_midi_claimed(TRACK, true));
        assert!(
            daw.project_revision(SONG).unwrap() > rev,
            "a claim re-snapshots"
        );
        assert!(daw.track_midi_claimed(TRACK));
        assert_eq!(events(&daw), 0, "claimed: the plugin plays it itself");
        assert!(!daw.set_track_midi_claimed(TRACK, true), "no change");
        assert!(daw.set_track_midi_claimed(TRACK, false));
        assert_eq!(events(&daw), 2);
        let _ = LiveHints::default();
    }
}
