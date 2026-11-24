//! Waveforms as fine as the zoom asks for, down to the samples.
//!
//! An item's waveform is read once, at [`crate::midi::WAVE_BLOCK`]
//! samples a point (about 21 ms at 48 kHz): plenty for a song on a
//! screen, and a blur zoomed in on one hit — which is where drums are
//! edited. So when the view is closer than that, the stretch on screen is
//! read again from the source, at the source's rate, and reduced to a
//! point every half pixel; zoomed in further than a pixel a sample, the
//! samples themselves are kept and drawn as a line through them.
//!
//! The reads are off the paint: a frame asks ([`get`]) and draws what is
//! here — the detail read for the last zoom or pan, where it reaches, and
//! the coarse waveform past it — while the new detail is read; the window
//! is asked for a frame when it arrives ([`generation`]). What is drawn
//! only ever gets finer: a pan or a zoom never drops a waveform back to
//! the coarse one while it waits. They run in parallel on the
//! waveform pool (`crate::wave_pool`), the newest ask first: a
//! slip-edited kit is thousands of items, each read on its own, and what
//! is on screen now matters more than what was a scroll ago. What was read is kept for the
//! item as it was read — a moved or slipped item is read again.
//!
//! A track that plays its items in one lane — a kit's, sliced into
//! thousands by an edit — is read as a track instead ([`Source::track`]):
//! its playing items mixed, over the span on screen, held once for every
//! item of it. A slice cut or moved by an edit then has its detail already;
//! the track is read again over the screen when its items change (their
//! `version`) or the engine has taken an edit ([`engine_changed`]), and
//! what was held is drawn meanwhile — an edit never drops the picture back
//! to the coarse one.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// What a lane's detail is read from, and where it was when asked.
///
/// A real item — or a whole track (`track`), read through the track's
/// accessor rather than item by item: a slip-edited kit is thousands of
/// slices of a few takes.
#[derive(Clone, Debug, PartialEq, facet::Facet)]
pub struct Source {
    /// The item's guid — or, read as a track, the track's name for what
    /// is held of it (one for every item of it).
    pub guid: String,
    /// Project seconds: what of it this lane draws.
    pub at: f64,
    pub length: f64,
    /// Seconds into its source (an item's).
    pub offset: f64,
    /// Read as this track's playing items, mixed, in project time.
    pub track: Option<String>,
    /// A track's items as the window has them (a hash of where they are):
    /// changed, what was held of it is read again.
    pub version: u64,
    /// What the detail is drawn times: a track's mix has each item through
    /// its item volume, which an item's lane puts on itself.
    pub scale: f64,
}

impl Source {
    /// An item, read as its take plays.
    #[must_use]
    pub fn item(guid: &str, at: f64, length: f64, offset: f64) -> Self {
        Self {
            guid: guid.to_owned(),
            at,
            length,
            offset,
            track: None,
            version: 0,
            scale: 1.0,
        }
    }

    /// Track `track` (its items at `version`), drawn over `at..at +
    /// length`, read as it plays.
    #[must_use]
    pub fn track(track: &str, at: f64, length: f64, version: u64) -> Self {
        Self {
            guid: format!("track:{track}"),
            at,
            length,
            offset: 0.0,
            track: Some(track.to_owned()),
            version,
            scale: 1.0,
        }
    }

    /// The same, drawn `scale` times.
    #[must_use]
    pub const fn scaled(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }
}

impl Source {
    /// What a held read is good for: a track's items as they were, or an
    /// item where it was.
    const fn key(&self) -> (u64, u64, u64) {
        if self.track.is_some() {
            (self.version, 0, 0)
        } else {
            (
                self.at.to_bits(),
                self.length.to_bits(),
                self.offset.to_bits(),
            )
        }
    }
}

/// A stretch of an item's audio, fine.
#[derive(Clone, Debug, PartialEq, facet::Facet)]
pub struct Detail {
    /// Project seconds of the first point.
    pub from: f64,
    /// Seconds each point stands for.
    pub step: f64,
    /// Every point a sample (`(value, value)`), not a run's loudest.
    pub samples: bool,
    /// `(max, min)` per point; a sample's value twice.
    pub points: Vec<(f32, f32)>,
}

impl Detail {
    /// Project seconds after the last point.
    #[must_use]
    pub fn to(&self) -> f64 {
        crate::num::coord(self.points.len()).mul_add(self.step, self.from)
    }

    /// The points from `a` to `b` seconds, with each one's time.
    pub fn between(&self, a: f64, b: f64) -> impl Iterator<Item = (f64, (f32, f32))> + '_ {
        let step = self.step.max(f64::EPSILON);
        let first = crate::num::index(((a - self.from) / step).floor() - 1.0);
        let last = crate::num::index(((b - self.from) / step).ceil() + 1.0);
        self.points
            .iter()
            .enumerate()
            .skip(first)
            .take(last.saturating_sub(first).saturating_add(1))
            .map(move |(i, p)| (crate::num::coord(i).mul_add(step, self.from), *p))
    }
}

/// `samples` (interleaved, `channels` of them, at `rate`, the first at
/// `from` seconds) as points `per` samples each — or, `per` 1 or less,
/// the samples themselves (the channel furthest from zero at each).
#[must_use]
pub fn reduce(samples: &[f64], channels: usize, rate: f64, from: f64, per: usize) -> Detail {
    let channels = channels.max(1);
    let frames: Vec<(f32, f32)> = samples
        .chunks(channels)
        .map(|frame| {
            let (mut max, mut min) = (f64::NEG_INFINITY, f64::INFINITY);
            for &v in frame {
                max = max.max(v);
                min = min.min(v);
            }
            let f = |v: f64| num_traits::ToPrimitive::to_f32(&v).unwrap_or(0.0);
            (f(max), f(min))
        })
        .collect();
    if per <= 1 {
        let points = frames
            .iter()
            .map(|&(max, min)| {
                let v = if max.abs() >= min.abs() { max } else { min };
                (v, v)
            })
            .collect();
        return Detail {
            from,
            step: 1.0 / rate,
            samples: true,
            points,
        };
    }
    let points = frames
        .chunks(per)
        .map(|run| {
            run.iter()
                .fold((f32::NEG_INFINITY, f32::INFINITY), |(mx, mn), &(a, b)| {
                    (mx.max(a), mn.min(b))
                })
        })
        .collect();
    Detail {
        from,
        step: crate::num::coord(per) / rate,
        samples: false,
        points,
    }
}

/// What was read, by item.
struct Held {
    key: (u64, u64, u64),
    /// The engine's edits taken when it was read ([`engine_changed`]).
    epoch: u64,
    detail: Arc<Detail>,
}

/// What is wanted, by item: the item, the span, the seconds a point, and
/// for a track the engine's edits it must be read after.
type Want = (Source, f64, f64, f64, u64);

#[derive(Default)]
struct Shared {
    held: HashMap<String, Held>,
    /// What is wanted, with when it was asked (higher is newer).
    wanted: HashMap<String, (Want, u64)>,
    /// Asked for and not yet read, so a frame does not ask again.
    reading: HashMap<String, Want>,
    /// Asks so far: the next one's order.
    asked: u64,
}

static SHARED: std::sync::LazyLock<Mutex<Shared>> =
    std::sync::LazyLock::new(|| Mutex::new(Shared::default()));
static GENERATION: AtomicU64 = AtomicU64::new(0);
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// The engine has taken item edits: every track held is read again where
/// it is on screen (what was held still drawn meanwhile).
pub fn engine_changed() {
    EPOCH.fetch_add(1, Ordering::Relaxed);
    GENERATION.fetch_add(1, Ordering::Relaxed);
    crate::wake::wake();
}

/// Goes up each time detail arrives: a window compares it with the one
/// it drew to know to draw again.
#[must_use]
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// The most samples one read takes: past that the stretch is read at a
/// lower rate (the coarse waveform is near enough there anyway).
#[cfg(feature = "local")]
const MOST_SAMPLES: f64 = 4_000_000.0;

/// `source`'s audio from `a` to `b` project seconds, as fine as has been
/// read: the detail held for it, if any reaches into the span.
///
/// When that is not all of the span at `step` seconds a point or finer, the
/// rest is asked for (with some to spare either side, so a small pan does
/// not read again) — and what is held is still given meanwhile, to draw
/// where it reaches. `None` only when nothing of the item here has been
/// read, or it has moved since.
#[must_use]
pub fn get(source: &Source, a: f64, b: f64, step: f64) -> Option<Arc<Detail>> {
    let epoch = EPOCH.load(Ordering::Relaxed);
    let track = source.track.is_some();
    let mut shared = SHARED.lock().ok()?;
    let entry = shared.held.get(&source.guid);
    let fresh =
        entry.is_some_and(|held| held.key == source.key() && (!track || held.epoch == epoch));
    // A track's, stale, is still drawn until it is read again; an item's
    // is for where the item was.
    let held = entry
        .filter(|held| track || held.key == source.key())
        .map(|held| Arc::clone(&held.detail));
    if fresh
        && held.as_ref().is_some_and(|d| {
            d.from <= a + 1e-9 && d.to() >= b - 1e-9 && (d.samples || d.step <= step * 1.5)
        })
    {
        return held;
    }
    let held = held.filter(|d| d.from < b && d.to() > a);
    let spare = (b - a) * 0.5;
    let want: Want = if track {
        (source.clone(), (a - spare).max(0.0), b + spare, step, epoch)
    } else {
        (
            source.clone(),
            (a - spare).max(source.at),
            (b + spare).min(source.at + source.length),
            step,
            0,
        )
    };
    if shared.reading.get(&source.guid) != Some(&want)
        && shared.wanted.get(&source.guid).map(|(w, _)| w) != Some(&want)
    {
        shared.asked = shared.asked.wrapping_add(1);
        let order = shared.asked;
        shared.wanted.insert(source.guid.clone(), (want, order));
        drop(shared);
        // One read for each ask, of whatever is newest when it runs.
        read_newest();
    }
    held
}

/// Hold `detail` as `source`'s, as a read would.
#[cfg(test)]
pub fn hold(source: &Source, detail: Detail) {
    if let Ok(mut shared) = SHARED.lock() {
        shared.held.insert(
            source.guid.clone(),
            Held {
                key: source.key(),
                epoch: EPOCH.load(Ordering::Relaxed),
                detail: Arc::new(detail),
            },
        );
    }
}

/// Read the newest ask on the waveform pool. Each ask queues one of
/// these; whichever runs takes what is newest then, so a stale ask is
/// read last, if at all.
#[cfg(feature = "native")]
fn read_newest() {
    crate::wave_pool::spawn(|| {
        let (guid, want) = {
            let Ok(mut shared) = SHARED.lock() else {
                return;
            };
            let Some(guid) = newest(&shared.wanted) else {
                return;
            };
            let Some((want, _)) = shared.wanted.remove(&guid) else {
                return;
            };
            shared.reading.insert(guid.clone(), want.clone());
            (guid, want)
        };
        let read = read(&want);
        if let Ok(mut shared) = SHARED.lock() {
            if shared.reading.get(&guid) == Some(&want) {
                shared.reading.remove(&guid);
            }
            if let Some(detail) = read {
                shared.held.insert(
                    guid,
                    Held {
                        key: want.0.key(),
                        epoch: want.4,
                        detail: Arc::new(detail),
                    },
                );
                GENERATION.fetch_add(1, Ordering::Relaxed);
            }
        }
        // The detail read off the window's thread: drawn in the next frame.
        crate::wake::wake();
    });
}

/// Nowhere to read from in a page: the coarse waveform stays.
#[cfg(not(feature = "native"))]
const fn read_newest() {}

/// The item asked for last.
#[cfg(feature = "native")]
fn newest(wanted: &HashMap<String, (Want, u64)>) -> Option<String> {
    wanted
        .iter()
        .max_by_key(|(_, (_, order))| *order)
        .map(|(guid, _)| guid.clone())
}

/// Read one stretch from the engine, over the window's link to it.
#[cfg(feature = "native")]
fn read((source, a, b, step, _): &Want) -> Option<Detail> {
    crate::view_client::detail(crate::view::DetailAsk {
        song: crate::open::current_song().unwrap_or_default(),
        source: source.clone(),
        from: *a,
        to: *b,
        step: *step,
    })
}

/// Read `source` of `song` from `a` to `b` project seconds, a point every
/// `step` seconds at most, from the engine in this process: the engine's
/// answer to a window's ask (`crate::view::EngineView::detail`).
#[cfg(feature = "local")]
#[must_use]
pub fn read_in(
    daw: &daw::standalone::Standalone,
    song: &str,
    source: &Source,
    (a, b): (f64, f64),
    step: f64,
) -> Option<Detail> {
    use daw::service::{ItemRef, ProjectContext, TakeRef};
    use daw_proto::{AudioAccessors as _, GetSamplesRequest};
    let project = ProjectContext::Project(song.to_owned());
    // A track, read as it plays — its time is the project's; an item, by
    // the engine's name for it: a piece the window split off has its own
    // name in the window until the song is read back, and asked by that
    // the engine knows no such item — every slice of an edited kit was
    // drawn coarse for good.
    let (accessor, origin) = if let Some(track) = &source.track {
        (
            daw.create_track_accessor(project, daw::service::TrackRef::Guid(track.clone()))?,
            0.0,
        )
    } else {
        let guid = crate::engine::made(&source.guid);
        (
            daw.create_take_accessor(project, ItemRef::Guid(guid), TakeRef::Active)?,
            source.at,
        )
    };

    // The source's own rate and channels: a read of none says them.
    let format = daw.get_samples(GetSamplesRequest {
        accessor_id: accessor.clone(),
        sample_rate: 0.0,
        num_channels: 0,
        start_time: 0.0,
        num_samples: 0,
    });
    let span = (b - a).max(0.0);
    let native = format.sample_rate.max(1.0);
    let rate = native.min(MOST_SAMPLES / span.max(f64::EPSILON));
    let frames = num_traits::ToPrimitive::to_u32(&(span * rate).ceil()).unwrap_or(0);
    let channels = format.num_channels.max(1);
    let data = daw.get_samples(GetSamplesRequest {
        accessor_id: accessor.clone(),
        sample_rate: rate,
        num_channels: channels,
        start_time: a - origin,
        num_samples: frames,
    });
    daw.destroy_accessor(&accessor);
    let per = crate::num::index((step * rate / 2.0).floor()).max(1);
    let channels = usize::try_from(channels).unwrap_or(1);
    Some(reduce(&data.samples, channels, rate, a, per))
}
