//! The notes inside an item, for drawing it.
//!
//! An item's body should be what the item CONTAINS. Audio has peaks and
//! MIDI has notes, and until now every item in the arrangement was
//! drawn from the same synthetic envelope — a function of the track's
//! index and the time, which never touched the item, the take, or a
//! single sample. That is why a chord track looked like a shaker.
//!
//! Notes arrive the way peaks were always meant to: after the window is
//! already standing. Reading them is a round trip per item, and a
//! session with four hundred MIDI items would spend the whole open
//! doing it — so the window draws what it has, asks for the rest, and
//! redraws as the answers land.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One note, reduced to what a preview draws.
///
/// Not `MidiNote`: a preview needs where and how high, and carries
/// neither channel nor selection nor mute. Reduced at the edge so the
/// drawing has nothing to decide and the cache stays small — a big
/// session holds a lot of these.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Note {
    /// Where it starts, as a fraction of the item's length.
    ///
    /// A fraction rather than a time, because an item preview is drawn
    /// to the item's width and that is the only thing it is measured
    /// against. It also makes the cache independent of the zoom, so a
    /// scroll never invalidates one.
    pub at: f32,
    /// How long, as the same fraction. Never zero: a note with no
    /// length is a note you cannot see.
    pub len: f32,
    /// MIDI pitch, 0..=127.
    pub pitch: u8,
    /// How hard, 0..=127 — the preview draws it as brightness.
    pub velocity: u8,
}

/// The notes of the items that have been read so far.
///
/// Absent means "not asked yet or still reading", and an empty list
/// means "read, and it has none". The two have to be distinguishable:
/// an item with no notes should stop being asked about, and an item
/// still loading should not be drawn as empty.
/// An audio item's waveform, reduced to what a lane draws: the loudest
/// and the lowest the take reaches across each stretch, every channel
/// folded into one, from the item's own start.
///
/// The take's peaks, not the source's — the engine has already applied
/// the item's slip, play rate and stretch, so point `i` is item time
/// `i * step` and nothing here has to know how it got there.
#[derive(Clone, Debug, PartialEq, facet::Facet)]
pub struct Wave {
    /// Seconds each point stands for.
    pub step: f64,
    /// `(max, min)` per point, in −1..1.
    pub points: Vec<(f32, f32)>,
}

/// Audio items whose waveform no longer draws what they play — their
/// take's volume curve was rewritten — to be read again.
static STALE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Say `items`' waveforms are stale: every window reads them again (told
/// over its link, `crate::view::Change::Waves`).
pub fn mark_stale(items: impl IntoIterator<Item = String>) {
    let items: Vec<String> = items.into_iter().collect();
    #[cfg(feature = "native")]
    crate::body::body().view_changed_items(None, crate::view::Change::Waves, items.clone());
    mark_stale_here(items);
}

/// `items`' waveforms stale in this window: read again at its next
/// read-back.
pub fn mark_stale_here(items: Vec<String>) {
    if let Ok(mut stale) = STALE.lock() {
        stale.extend(items);
    }
}

/// Whether any item has been said stale since last asked.
#[must_use]
pub fn stale_pending() -> bool {
    STALE.lock().is_ok_and(|s| !s.is_empty())
}

/// The items said stale since last asked.
#[must_use]
pub fn take_stale() -> Vec<String> {
    STALE
        .lock()
        .map(|mut s| std::mem::take(&mut *s))
        .unwrap_or_default()
}

/// The volume envelope of `item`'s active take, `(seconds into the item,
/// linear gain)` by time: none when it has none.
#[cfg(feature = "local")]
fn take_volume(
    daw: &daw::standalone::Standalone,
    project: &daw::service::ProjectContext,
    item: &str,
) -> Vec<(f64, f64)> {
    use daw::service::{ItemRef, Takes as _};
    use daw_proto::automation::Automation as _;
    let Some(take) = daw.get_active_take(project.clone(), ItemRef::Guid(item.to_owned())) else {
        return Vec::new();
    };
    let location = daw_proto::EnvelopeLocation {
        track: daw_proto::TrackRef::Index(0),
        envelope: daw_proto::EnvelopeRef::Take {
            item_guid: item.to_owned(),
            take_guid: take.guid,
            kind: daw_proto::TakeEnvelopeKind::Volume,
        },
    };
    daw.points(project.clone(), location)
        .into_iter()
        .map(|p| (p.time.as_seconds(), p.value))
        .collect()
}

impl Wave {
    /// This one through a gain curve — `(seconds into it, linear gain)`,
    /// by time, straight between points, the first's before it and the
    /// last's after — as it plays through its take's volume envelope.
    #[must_use]
    pub fn through(mut self, curve: &[(f64, f64)]) -> Self {
        if curve.is_empty() {
            return self;
        }
        let gain_at = |t: f64| {
            let at = curve.partition_point(|(x, _)| *x <= t);
            match (at.checked_sub(1).and_then(|i| curve.get(i)), curve.get(at)) {
                (Some(&(x0, g0)), Some(&(x1, g1))) if x1 > x0 => {
                    ((t - x0) / (x1 - x0)).mul_add(g1 - g0, g0)
                }
                (Some(&(_, g)), _) | (None, Some(&(_, g))) => g,
                (None, None) => 1.0,
            }
        };
        let step = self.step;
        for (k, point) in self.points.iter_mut().enumerate() {
            let t = (crate::num::coord(k) + 0.5) * step;
            let g = num_traits::ToPrimitive::to_f32(&gain_at(t)).unwrap_or(1.0);
            *point = (point.0 * g, point.1 * g);
        }
        self
    }

    /// The part of this one from `from` seconds into it, `length` long —
    /// silence where that reaches outside it. A slice of a take plays the
    /// take's own audio, so its waveform is this, cut where it was.
    #[must_use]
    pub fn slice(&self, from: f64, length: f64) -> Self {
        let step = self.step.max(f64::EPSILON);
        let count = crate::num::index((length / step).ceil());
        let skip = (from / step).round();
        let points = (0..count)
            .map(|k| {
                let at = crate::num::coord(k) + skip;
                if at < 0.0 {
                    (0.0, 0.0)
                } else {
                    self.points
                        .get(crate::num::index(at))
                        .copied()
                        .unwrap_or((0.0, 0.0))
                }
            })
            .collect();
        Self {
            step: self.step,
            points,
        }
    }

    /// A slice `length` long played through stretch `markers` (`(take
    /// time, source time)`, as the engine takes them): each point from
    /// the source time its take time plays, so a decay drawn out over a
    /// gap is drawn drawn out. `origin` is the source time this wave's
    /// first point stands for. Silence outside it, as [`Self::slice`].
    #[must_use]
    pub fn warped(&self, origin: f64, length: f64, markers: &[(f64, f64)]) -> Self {
        let step = self.step.max(f64::EPSILON);
        let count = crate::num::index((length / step).ceil());
        // The source time a take time plays: along the segment holding it
        // (before the first, the first's; past the last, the last's).
        let source = |take: f64| -> f64 {
            let segment = markers
                .windows(2)
                .find(|w| w.get(1).is_some_and(|end| take <= end.0))
                .or_else(|| markers.windows(2).last());
            match segment {
                Some(&[(t0, s0), (t1, s1)]) if t1 > t0 => {
                    (take - t0).mul_add((s1 - s0) / (t1 - t0), s0)
                }
                _ => markers.first().map_or(take, |&(t0, s0)| s0 + (take - t0)),
            }
        };
        let points = (0..count)
            .map(|k| {
                let at = ((source(crate::num::coord(k) * step) - origin) / step).round();
                if at < 0.0 {
                    (0.0, 0.0)
                } else {
                    self.points
                        .get(crate::num::index(at))
                        .copied()
                        .unwrap_or((0.0, 0.0))
                }
            })
            .collect();
        Self {
            step: self.step,
            points,
        }
    }

    /// Fold the engine's per-channel `[min, max, …]` blocks into one
    /// envelope. `None` for a take with no audio to show.
    #[must_use]
    pub fn from_peaks(data: &daw_proto::TakePeakData) -> Option<Self> {
        let channels = usize::try_from(data.num_channels.max(1)).ok()?;
        let stride = channels * 2;
        if data.peaks.len() < stride || data.sample_rate <= 0.0 {
            return None;
        }
        let points = data
            .peaks
            .chunks_exact(stride)
            .map(|block| {
                let (mut max, mut min) = (0.0_f64, 0.0_f64);
                for pair in block.chunks_exact(2) {
                    min = min.min(pair[0]);
                    max = max.max(pair[1]);
                }
                #[expect(clippy::cast_possible_truncation, reason = "a level in −1..1")]
                (max as f32, min as f32)
            })
            .collect();
        Some(Self {
            step: f64::from(data.samples_per_peak.max(1)) / data.sample_rate,
            points,
        })
    }
}

/// One child item under a folded item (a shut folder's row): where it
/// sits, and what it holds — see [`fold_waves`] and [`fold_notes`].
pub struct Child<'a> {
    /// Its position and length, in seconds.
    pub at: f64,
    pub length: f64,
    /// Its waveform, if it is audio and its peaks are read.
    pub wave: Option<&'a Wave>,
    /// Its notes, if it is MIDI and they are read.
    pub notes: Option<&'a [Note]>,
}

/// The most points a folded waveform holds, however long its span.
const FOLDED_POINTS: usize = 1 << 16;

/// A shut folder's item, `start..end` seconds, as the envelope of the
/// audio under it: per point, the loudest of every child sounding there.
///
/// At the finest of the children's resolutions, so folding loses nothing
/// a child showed open. `None` where no child under it has a waveform.
#[must_use]
pub fn fold_waves(start: f64, end: f64, children: &[Child<'_>]) -> Option<Wave> {
    let waves: Vec<(f64, &Wave)> = children
        .iter()
        .filter_map(|c| {
            c.wave
                .filter(|w| w.step > 0.0 && !w.points.is_empty())
                .map(|w| (c.at, w))
        })
        .collect();
    let finest = waves
        .iter()
        .map(|(_, w)| w.step)
        .fold(f64::INFINITY, f64::min);
    let span = end - start;
    if !finest.is_finite() || span <= 0.0 {
        return None;
    }
    let count = crate::num::index((span / finest).ceil()).clamp(1, FOLDED_POINTS);
    let step = span / crate::num::coord(count);
    // Each child's points once, into every folded point they overlap: a
    // slip-edited kit is tens of thousands of slices under one folder
    // item, and asking each folded point of every child was billions of
    // steps — ten seconds of a frozen window.
    let mut points = vec![(0.0_f32, 0.0_f32); count];
    let last = count.saturating_sub(1);
    for (at, wave) in &waves {
        for (j, (hi, lo)) in wave.points.iter().enumerate() {
            let begins = crate::num::coord(j).mul_add(wave.step, *at);
            let ends = begins + wave.step;
            if ends <= start || begins >= end {
                continue;
            }
            // A point that only meets a folded point's edge is not in it.
            let first = crate::num::index(((begins - start) / step + EDGE).floor()).min(last);
            let past = crate::num::index(((ends - start) / step - EDGE).ceil())
                .clamp(first.saturating_add(1), count);
            for point in points.iter_mut().take(past).skip(first) {
                *point = (point.0.max(*hi), point.1.min(*lo));
            }
        }
    }
    Some(Wave { step, points })
}

/// How near a folded point's edge, in folded points, a child's point that
/// meets it is taken to only meet it.
const EDGE: f64 = 1e-9;

/// A shut folder's item, `start..end` seconds, as the notes under it: each
/// child's notes placed where they sound and cut to the span, as fractions
/// of it. `None` where no child under it has notes there.
#[must_use]
pub fn fold_notes(start: f64, end: f64, children: &[Child<'_>]) -> Option<Vec<Note>> {
    let span = end - start;
    if span <= 0.0 {
        return None;
    }
    let mut out = Vec::new();
    for child in children {
        for note in child.notes.unwrap_or_default() {
            let from = f64::from(note.at).mul_add(child.length, child.at);
            let to = f64::from(note.len).mul_add(child.length, from);
            if to <= start || from >= end {
                continue;
            }
            let (from, to) = (from.max(start), to.min(end));
            out.push(Note {
                at: crate::num::narrow((from - start) / span),
                // Never zero: a note with no length cannot be seen.
                len: crate::num::narrow((to - from) / span).max(f32::MIN_POSITIVE),
                pitch: note.pitch,
                velocity: note.velocity,
            });
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Samples per point a lane asks the engine for: 5 ms at 48 kHz.
///
/// About two hundred points a second: a drum hit's attack and decay as
/// their own shape at the zooms a kit is edited at, drawn from what is
/// already here rather than read again item by item ([`crate::wave_detail`]
/// takes over only closer than that). At forty a second a hit was a few
/// steps of a staircase.
pub const WAVE_BLOCK: u32 = 256;

/// Samples per point the analyses read (gain staging, the mix chains'
/// levels): what their thresholds were measured at, and four times fewer
/// points to go through than the drawing's.
pub const ANALYSIS_BLOCK: u32 = 1024;

/// How many items' peaks are read at once.
const WAVE_READS: usize = 16;

/// A stretch of a recording's waveform: source seconds `from..to` (read
/// and cut from by the engine in this process).
#[derive(Clone)]
#[cfg_attr(not(feature = "local"), allow(dead_code))]
struct SourceWave {
    from: f64,
    to: f64,
    wave: Arc<Wave>,
}

/// What a waveform is drawn from: an item's recording, the stretch of it
/// the item starts on, and how the item plays it.
#[cfg(feature = "local")]
struct Cut {
    path: String,
    /// Source seconds at the item's start.
    offset: f64,
    /// Item seconds.
    length: f64,
    /// The take's stretch markers, `(take time, source time)`.
    markers: Vec<(f64, f64)>,
}

/// `guid`'s recording and how it plays it — `None` for a take played
/// faster or slower than recorded (varispeed is read, not derived).
#[cfg(feature = "local")]
fn cut_of(
    daw: &daw::standalone::Standalone,
    project: &daw::service::ProjectContext,
    guid: &str,
) -> Option<Cut> {
    use daw::service::{ItemRef, Items as _, TakeRef, Takes as _};
    let item = daw.get_item(project.clone(), ItemRef::Guid(guid.to_owned()))?;
    let take = daw.get_active_take(project.clone(), ItemRef::Guid(guid.to_owned()))?;
    if (take.play_rate - 1.0).abs() > 1e-9 && take.play_rate > 0.0 {
        return None;
    }
    let markers = daw_proto::StretchMarkers::get_stretch_markers(
        daw,
        project.clone(),
        ItemRef::Guid(guid.to_owned()),
        TakeRef::Active,
    )
    .into_iter()
    .map(|m| (m.position, m.source_position))
    .collect();
    Some(Cut {
        path: take.source_file_path?,
        offset: take.start_offset.as_seconds(),
        length: item.length.as_seconds(),
        markers,
    })
}

#[derive(Clone, Default)]
pub struct Previews {
    known: Arc<Mutex<HashMap<String, Vec<Note>>>>,
    /// The audio items' waveforms, by item GUID.
    waves: Arc<Mutex<HashMap<String, Arc<Wave>>>>,
    /// The recordings' waveforms as read, by file: each the stretch of the
    /// source an item read covered (`from..to`, source seconds), before
    /// any volume curve. An item cut from a recording — a slice, a comp's
    /// piece, the edit's copy — is its stretch of one of these, and is
    /// never read again ([`Self::derive`]).
    #[cfg_attr(not(feature = "local"), allow(dead_code))]
    sources: Arc<Mutex<HashMap<String, Vec<SourceWave>>>>,
    /// Bumped whenever notes or waveforms arrive — so a picture recorded
    /// before they did (a browser's waveforms come after the song opens)
    /// knows to record itself again.
    generation: Arc<std::sync::atomic::AtomicU64>,
    /// Audio items read before their recording could be read whole — a
    /// proxy still streaming in with no cache of its original's, or not
    /// attached yet — by guid: the song they are of (`None`: the current
    /// one) and the streaming state ([`crate::host::Host::streaming`]) they
    /// were read at. What such a read gives is at most the stretch decoded
    /// near the playhead — a lane of slivers, and a shut folder's row of
    /// them — so it is not kept: the item is drawn plain and read again
    /// once the streaming state has moved and its recording is no longer
    /// arriving ([`Self::retry_partial`]).
    partial: Arc<Mutex<HashMap<String, (Option<String>, u64)>>>,
}

impl Previews {
    /// The notes for an item, if they have been read.
    #[must_use]
    pub fn get(&self, guid: &str) -> Option<Vec<Note>> {
        self.known.lock().ok()?.get(guid).cloned()
    }

    /// How many times notes or waveforms have arrived: a holder of a
    /// picture drawn from them records it again when this has moved.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Acquire)
    }

    fn arrived(&self) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }

    /// Put `wave` in as `guid`'s, as a read would.
    #[cfg(test)]
    pub fn insert_wave(&self, guid: &str, wave: Wave) {
        if let Ok(mut waves) = self.waves.lock() {
            waves.insert(guid.to_owned(), Arc::new(wave));
        }
    }

    /// Put `wave` in as `guid`'s without reading it: a slice of a take
    /// whose waveform is already here (see [`Wave::slice`]).
    pub fn put_waves(&self, waves: impl IntoIterator<Item = (String, Wave)>) {
        if let Ok(mut held) = self.waves.lock() {
            for (guid, wave) in waves {
                held.insert(guid, Arc::new(wave));
            }
        }
        self.arrived();
    }

    /// The waveform for an audio item, if its peaks have been read.
    #[must_use]
    pub fn wave(&self, guid: &str) -> Option<Arc<Wave>> {
        self.waves.lock().ok()?.get(guid).cloned()
    }

    /// Keep what a read of `guid`'s peaks gave: its waveform, or — read
    /// before its recording could be read whole — nothing but a note to
    /// read it again ([`Self::retry_partial`]). A waveform it had already
    /// is kept rather than replaced by a partial read.
    fn keep_read(&self, guid: String, read: Read, song: Option<String>) {
        let wave = match read {
            Read::Whole(wave) => wave,
            Read::Partial { at } => {
                if let Ok(mut pending) = self.partial.lock() {
                    pending.insert(guid, (song, at));
                }
                return;
            }
        };
        if let Ok(mut pending) = self.partial.lock() {
            pending.remove(&guid);
        }
        if let Ok(mut waves) = self.waves.lock() {
            waves.insert(guid, Arc::new(wave));
        }
    }

    /// Of the items read partially, those of `song` that would read whole
    /// now (their waveform has come since): what to read again, and no
    /// more.
    #[must_use]
    pub fn partial_now_whole(&self, song: &str) -> Vec<String> {
        self.partial()
            .into_iter()
            .filter(|guid| !partial_here(song, guid))
            .collect()
    }

    /// The items read partially, waiting to be read whole.
    #[must_use]
    pub fn partial(&self) -> Vec<String> {
        self.partial
            .lock()
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// The partial reads due again, taken out of the list (a read that is
    /// still partial puts its item back), by song: each read at another
    /// streaming state than `version` — something has arrived or attached
    /// since — whose recording is not among those `arriving`. One that has
    /// arrived and still reads partial waits for the state to move again,
    /// so nothing is read over and over.
    #[cfg(feature = "native")]
    fn take_due(
        &self,
        version: u64,
        arriving: &std::collections::HashMap<String, f32>,
    ) -> HashMap<Option<String>, Vec<String>> {
        let Ok(mut pending) = self.partial.lock() else {
            return HashMap::new();
        };
        let due: Vec<String> = pending
            .iter()
            .filter(|(guid, (_, at))| *at != version && !arriving.contains_key(*guid))
            .map(|(guid, _)| guid.clone())
            .collect();
        let mut by_song: HashMap<Option<String>, Vec<String>> = HashMap::new();
        for guid in due {
            if let Some((song, _)) = pending.remove(&guid) {
                by_song.entry(song).or_default().push(guid);
            }
        }
        by_song
    }

    /// Read again, behind the window, the items read before their
    /// recording had all arrived — once the streaming state has moved and
    /// theirs is no longer arriving. Cheap to call every frame.
    #[cfg(feature = "native")]
    pub fn retry_partial(&self) {
        if self.partial.lock().map_or(true, |p| p.is_empty()) {
            return;
        }
        let (version, arriving) = crate::host::host().streaming();
        let due = self.take_due(version, &arriving);
        for (song, guids) in due {
            let previews = self.clone();
            crate::wave_pool::spawn(move || previews.fill_waves_blocking_in(song, guids));
        }
    }

    /// Read the peaks of every audio item named, now, on this thread —
    /// [`Self::fill_blocking`]'s twin, for the same reason: a window that
    /// opens on blank lanes and fills them in reads as a broken session.
    ///
    /// With the engine in this process, every item at once across the
    /// waveform pool ([`crate::wave_pool`]); otherwise over the engine's
    /// link, several at a time.
    pub fn fill_waves_blocking(&self, guids: Vec<String>) {
        self.fill_waves_blocking_in(None, guids);
    }

    /// [`Self::fill_waves_blocking`] for the items of `song` (a project
    /// guid) rather than whichever song is current when the read runs: a
    /// read behind a song's opening must not look for its items in the
    /// song a setlist switched to meanwhile. `None`: the current song.
    pub fn fill_waves_blocking_in(&self, song: Option<String>, guids: Vec<String>) {
        let Some(runtime) = crate::open::runtime() else {
            return;
        };
        #[cfg(feature = "native")]
        if crate::view_client::is_linked() {
            let Some(song) = song.or_else(crate::open::current_song) else {
                return;
            };
            runtime.block_on(self.fill_waves_linked(&song, guids));
            return;
        }
        #[cfg(not(feature = "native"))]
        let _ = song;
        runtime.block_on(self.fill_waves(guids));
    }

    /// Read `guids`' waveforms of `song` as the engine draws them, over the
    /// window's link to it (`crate::view_client::waves`): through each
    /// take's volume curve, a slice cut from its recording — in batches,
    /// each kept as it lands.
    #[cfg(feature = "native")]
    pub async fn fill_waves_linked(&self, song: &str, guids: Vec<String>) {
        /// Items a call: a sliced kit's thousands go as a few dozen calls,
        /// each answer a few megabytes at most.
        const BATCH: usize = 128;
        let at = stream_version();
        for batch in guids.chunks(BATCH) {
            let Some(read) = crate::view_client::waves_of(song, batch.to_vec()).await else {
                return;
            };
            for one in read {
                let (item, read) = match one {
                    crate::view::WaveRead::Whole { item, wave } => (item, Read::Whole(wave)),
                    crate::view::WaveRead::Partial { item } => (item, Read::Partial { at }),
                    crate::view::WaveRead::None { .. } => continue,
                };
                self.keep_read(item, read, Some(song.to_owned()));
            }
            self.arrived();
        }
    }

    /// Read every item's peaks from the engine in this process, in
    /// parallel on the waveform pool; `false` with no engine here. The
    /// engine's own reading — what it answers a window's ask with
    /// (`crate::view::EngineView::waves`).
    #[cfg(feature = "local")]
    pub fn read_here(&self, song: Option<String>, guids: &[String]) -> bool {
        use daw::service::ProjectContext;
        let (Some(daw), Some(song)) = (
            crate::open::local_engine(),
            song.or_else(crate::open::current_song),
        ) else {
            return false;
        };
        let project = ProjectContext::Project(song);
        // One timed unit (`PERF_RECENT {name: "waves.fill"}`).
        let span = tracing::info_span!(
            "waves.fill",
            waves.asked = guids.len(),
            waves.read = tracing::field::Empty
        );
        let _entered = span.enter();
        // Cut from a recording already drawn: no read at all.
        let left = self.derive(daw, &project, guids);
        if left.is_empty() {
            span.record("waves.read", 0);
            return true;
        }
        // Of the rest, one item a recording first — the longest that plays
        // it unstretched — and the others cut from it: a sliced kit is a
        // few dozen reads, not thousands.
        let cuts = crate::wave_pool::map(&left, |guid| cut_of(daw, &project, guid));
        let mut longest: HashMap<&str, (usize, f64)> = HashMap::new();
        for (i, cut) in cuts.iter().enumerate() {
            let Some(cut) = cut.as_ref().filter(|c| c.markers.len() < 2) else {
                continue;
            };
            let best = longest.entry(cut.path.as_str()).or_insert((i, cut.length));
            if cut.length > best.1 {
                *best = (i, cut.length);
            }
        }
        let first: Vec<String> = longest
            .values()
            .filter_map(|(i, _)| left.get(*i).cloned())
            .collect();
        self.read_waves(daw, &project, &first);
        let rest: Vec<String> = left.into_iter().filter(|g| !first.contains(g)).collect();
        let rest = self.derive(daw, &project, &rest);
        self.read_waves(daw, &project, &rest);
        span.record("waves.read", first.len() + rest.len());
        true
    }

    /// No engine in this process to read peaks from.
    #[cfg(all(feature = "native", not(feature = "local")))]
    pub fn read_here(&self, _song: Option<String>, _guids: &[String]) -> bool {
        false
    }

    /// Read `guids`' peaks from the engine, in parallel, each kept by its
    /// recording too.
    #[cfg(feature = "local")]
    fn read_waves(
        &self,
        daw: &daw::standalone::Standalone,
        project: &daw::service::ProjectContext,
        guids: &[String],
    ) {
        use daw::service::{ItemRef, TakeRef};
        use daw_proto::Peaks as _;
        if guids.is_empty() {
            return;
        }
        let job = crate::jobs::start("Waveforms", guids.len());
        // Before the reads: what arrives during them moves it again.
        let at = stream_version();
        let read = crate::wave_pool::map(guids, |guid| {
            job.advance(1);
            let peaks = daw.take_peaks(
                project.clone(),
                ItemRef::Guid(guid.clone()),
                TakeRef::Active,
                WAVE_BLOCK,
            );
            if streaming_partial(daw, project, guid) {
                return Some((guid.clone(), Read::Partial { at }, None));
            }
            let raw = Wave::from_peaks(&peaks)?;
            // Kept by its recording, before its volume curve, for every
            // item cut from it after (an unstretched one: its points are
            // the source's own).
            let cut = cut_of(daw, project, guid).filter(|c| c.markers.len() < 2);
            // Drawn as it plays: through its take's volume envelope (a
            // leveled drum's strokes even, as they sound).
            let curve = take_volume(daw, project, guid);
            Some((
                guid.clone(),
                Read::Whole(raw.clone().through(&curve)),
                cut.map(|c| (c, raw)),
            ))
        });
        let song = match project {
            daw::service::ProjectContext::Project(song) => Some(song.clone()),
            daw::service::ProjectContext::Current => None,
        };
        for (guid, read, source) in read.into_iter().flatten() {
            self.keep_read(guid, read, song.clone());
            if let (Some((cut, raw)), Ok(mut sources)) = (source, self.sources.lock()) {
                sources.entry(cut.path).or_default().push(SourceWave {
                    from: cut.offset,
                    to: cut.offset + cut.length,
                    wave: Arc::new(raw),
                });
            }
        }
        self.arrived();
    }

    /// Every one of `guids` cut from a recording whose waveform is here —
    /// its stretch of it (through its stretch markers), through its own
    /// volume curve — put in without a read. The ones that are not, back.
    #[cfg(feature = "local")]
    fn derive(
        &self,
        daw: &daw::standalone::Standalone,
        project: &daw::service::ProjectContext,
        guids: &[String],
    ) -> Vec<String> {
        let Ok(sources) = self.sources.lock().map(|s| s.clone()) else {
            return guids.to_vec();
        };
        if sources.is_empty() {
            return guids.to_vec();
        }
        let step = tracing::info_span!("waves.cut_of").entered();
        let cuts = crate::wave_pool::map(guids, |guid| cut_of(daw, project, guid));
        drop(step);
        let step = tracing::info_span!("waves.derive").entered();
        let jobs: Vec<(&String, Option<Cut>)> = guids.iter().zip(cuts).collect();
        let cut = crate::wave_pool::map(&jobs, |(guid, cut)| {
            let cut = cut.as_ref()?;
            // The source it plays: its markers' reach, else its own span.
            let (lo, hi) = if cut.markers.len() >= 2 {
                let (a, b) = cut
                    .markers
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (_, s)| {
                        (a.min(*s), b.max(*s))
                    });
                (a.min(cut.offset), b)
            } else {
                (cut.offset, cut.offset + cut.length)
            };
            let held = sources
                .get(&cut.path)?
                .iter()
                .find(|w| w.from <= lo + 1e-6 && hi <= w.to + 1e-6)?;
            let wave = if cut.markers.len() >= 2 {
                held.wave.warped(held.from, cut.length, &cut.markers)
            } else {
                held.wave.slice(cut.offset - held.from, cut.length)
            };
            let curve = take_volume(daw, project, guid);
            Some(((*guid).clone(), wave.through(&curve)))
        });
        drop(step);
        let mut left = Vec::new();
        let mut made = Vec::new();
        for (guid, wave) in guids.iter().zip(cut) {
            match wave {
                Some(w) => made.push(w),
                None => left.push(guid.clone()),
            }
        }
        if !made.is_empty() {
            self.put_waves(made);
        }
        left
    }

    /// Read the peaks, awaited — the web build's way in.
    pub async fn fill_waves(&self, guids: Vec<String>) {
        let Some(daw) = daw::rpc::Daw::try_get() else {
            return;
        };
        let Ok(project) = daw.current_project().await else {
            return;
        };
        self.fill_waves_in(&project, guids).await;
    }

    /// [`Self::fill_waves`] for `project`, whichever is current.
    pub async fn fill_waves_in(&self, project: &daw_control::Project, guids: Vec<String>) {
        use futures_util::StreamExt as _;
        // Several at once: each is a decode or a peaks-file read the engine
        // answers on its own, and a song of nine hundred items read one at
        // a time held its opening for half a second.
        let mut reads = futures_util::stream::iter(guids)
            .map(|guid| async move { (read_wave(project, &guid).await, guid) })
            .buffer_unordered(WAVE_READS);
        let song = project.guid().to_owned();
        let at = stream_version();
        while let Some((wave, guid)) = reads.next().await {
            let read = if partial_here(&song, &guid) {
                Read::Partial { at }
            } else if let Some(wave) = wave {
                Read::Whole(wave)
            } else {
                continue;
            };
            self.keep_read(guid, read, Some(song.clone()));
        }
        self.arrived();
    }

    /// Read the notes now, on this thread.
    ///
    /// For a renderer rather than a window: something drawing one frame
    /// and exiting has no later to redraw in, so an empty cache would
    /// mean every MIDI item drawn blank — a worse picture than the one
    /// this replaced.
    ///
    /// On a thread that is driving a runtime already (an Architect process
    /// view paints inside its runtime), it reads on a thread of its own,
    /// which it waits for.
    pub fn fill_blocking(&self, wanted: Vec<(String, f64)>) {
        let Some(runtime) = crate::open::runtime() else {
            return;
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            std::thread::scope(|s| {
                s.spawn(|| runtime.block_on(self.fill(wanted)));
            });
            return;
        }
        runtime.block_on(self.fill(wanted));
    }

    /// Read the notes, awaited: [`Self::fill_blocking`] where there is no
    /// thread to block (the web build).
    pub async fn fill(&self, wanted: Vec<(String, f64)>) {
        let Some(daw) = daw::rpc::Daw::try_get() else {
            return;
        };
        let Ok(project) = daw.current_project().await else {
            return;
        };
        self.fill_in(&project, wanted).await;
    }

    /// [`Self::fill`] from a given project rather than the current one —
    /// a song a page opens in the background.
    pub async fn fill_in(&self, project: &daw_control::Project, wanted: Vec<(String, f64)>) {
        for (guid, length) in wanted {
            let notes = read(project, &guid, length).await;
            if let Ok(mut known) = self.known.lock() {
                known.insert(guid, notes);
            }
        }
        self.arrived();
    }
}

/// What a read of an item's peaks gave.
enum Read {
    /// Its waveform, all of it.
    Whole(Wave),
    /// Only part of it, or none yet ([`streaming_partial`]), read at the
    /// streaming state `at` ([`crate::host::Host::streaming`]).
    Partial { at: u64 },
}

/// The streaming state now ([`crate::host::Host::streaming`]); nothing
/// streams through it in a page.
fn stream_version() -> u64 {
    crate::host::host().streaming().0
}

/// Whether `guid`'s peaks, as the engine reads them now, are not all of its
/// recording: a take streaming from a proxy that has not all arrived, with
/// no cache of its original's to draw from (the stretch decoded near the
/// playhead, silent everywhere else), or one with no audio attached yet (a
/// streamed song's takes before their streams are).
#[cfg(feature = "local")]
pub fn streaming_partial(
    daw: &daw::standalone::sync::Standalone,
    project: &daw::service::ProjectContext,
    guid: &str,
) -> bool {
    use daw::service::{ItemRef, Takes as _};
    use daw::standalone::audio_engine::AudioSource;
    let song = match project {
        daw::service::ProjectContext::Project(song) => song.clone(),
        daw::service::ProjectContext::Current => match crate::open::current_song() {
            Some(song) => song,
            None => return false,
        },
    };
    let Some(take) = daw.get_active_take(project.clone(), ItemRef::Guid(guid.to_owned())) else {
        return false;
    };
    match daw.audio_source(&song, &take.guid).as_deref() {
        None => true,
        Some(AudioSource::Streamed(streamed)) => !streamed.has_peaks(),
        Some(_) => false,
    }
}

/// [`streaming_partial`] for `guid` of `song`, on the engine in this
/// process (a page's own) — `false` with none here.
fn partial_here(song: &str, guid: &str) -> bool {
    #[cfg(feature = "web")]
    if let Some(partial) = crate::body::body().web_partial(song, guid) {
        return partial;
    }
    #[cfg(feature = "local")]
    if let Some(daw) = crate::open::local_engine() {
        return streaming_partial(
            daw,
            &daw::service::ProjectContext::Project(song.to_owned()),
            guid,
        );
    }
    let _ = (song, guid);
    false
}

/// Read one audio item's waveform from its active take's peaks. `None`
/// for an item the engine has no audio for — drawn plain, not faked.
async fn read_wave(project: &daw_control::Project, guid: &str) -> Option<Wave> {
    let item = project.items().by_guid(guid).await.ok()??;
    let data = item.active_take().peaks(WAVE_BLOCK).await.ok()?;
    Wave::from_peaks(&data)
}

/// Read one item's notes, as fractions of its length.
///
/// An empty answer is a real answer — an empty MIDI item is a thing
/// that exists, and drawing it as blank is right.
async fn read(project: &daw_control::Project, guid: &str, length: f64) -> Vec<Note> {
    let Ok(Some(item)) = project.items().by_guid(guid).await else {
        return Vec::new();
    };
    let Ok(notes) = item.active_take().midi().notes().await else {
        return Vec::new();
    };
    // The span the notes actually cover, because a take's PPQ tells us
    // ticks per quarter note and not how many quarter notes the ITEM
    // is. Measuring the notes themselves means the preview fills the
    // block whatever the tempo, which is what it is for: a shape, not a
    // clock.
    let span = notes
        .iter()
        .map(|n| n.start_ppq + n.length_ppq)
        .fold(0.0_f64, f64::max);
    let span = if span > 0.0 { span } else { length.max(1.0) };
    notes
        .iter()
        .map(|note| Note {
            at: (note.start_ppq / span) as f32,
            len: (note.length_ppq / span) as f32,
            pitch: note.pitch,
            velocity: note.velocity,
        })
        .collect()
}

#[cfg(test)]
mod partial_tests {
    use super::{Previews, Read, Wave};
    use std::collections::HashMap;

    fn wave(level: f32) -> Read {
        Read::Whole(Wave {
            step: 0.01,
            points: vec![(level, -level); 100],
        })
    }

    /// A waveform read before its recording could be read whole is not
    /// drawn — only what was decoded near the playhead had audio: a lane,
    /// and the shut folder's row over it, of slivers — but kept to read
    /// again, by song, once the streaming state has moved on from the one
    /// it was read at and its recording is no longer arriving.
    #[test]
    fn a_partial_read_is_not_drawn_and_is_read_again_once_its_recording_arrives() {
        let previews = Previews::default();
        let song = Some("song".to_owned());
        previews.keep_read("bass".into(), Read::Partial { at: 1 }, song.clone());
        assert!(
            previews.wave("bass").is_none(),
            "a partial read is not drawn"
        );
        assert_eq!(previews.partial(), ["bass"]);

        // Nothing has moved since it was read: not due.
        assert!(previews.take_due(1, &HashMap::new()).is_empty());
        // Moved, but its recording is still arriving: not due.
        let arriving = HashMap::from([("bass".to_owned(), 0.4_f32)]);
        assert!(previews.take_due(2, &arriving).is_empty());
        // Moved, and arrived: due, by its song, and out of the list.
        let due = previews.take_due(3, &HashMap::new());
        assert_eq!(
            due.get(&song).map(Vec::as_slice),
            Some(&["bass".to_owned()][..])
        );
        assert!(previews.partial().is_empty());

        // Read whole: drawn.
        previews.keep_read("bass".into(), wave(0.8), song);
        assert_eq!(previews.wave("bass").map(|w| w.points[50].0), Some(0.8));
        assert!(previews.partial().is_empty());
    }

    /// Read again and still partial (the proxy arrived but did not
    /// decode): it waits for the state to move again, not read every frame.
    #[test]
    fn a_read_still_partial_waits_for_the_next_change() {
        let previews = Previews::default();
        previews.keep_read("vox".into(), Read::Partial { at: 4 }, None);
        assert!(previews.take_due(4, &HashMap::new()).is_empty());
        assert_eq!(previews.take_due(5, &HashMap::new()).len(), 1);
        previews.keep_read("vox".into(), Read::Partial { at: 5 }, None);
        assert!(previews.take_due(5, &HashMap::new()).is_empty());
    }

    /// A waveform already drawn is not replaced by a partial read of it.
    #[test]
    fn a_partial_read_never_replaces_a_whole_one() {
        let previews = Previews::default();
        previews.keep_read("kick".into(), wave(0.9), None);
        previews.keep_read("kick".into(), Read::Partial { at: 0 }, None);
        assert_eq!(previews.wave("kick").map(|w| w.points[0].0), Some(0.9));
    }
}
