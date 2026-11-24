//! The vocal leveler as the window holds it.
//!
//! Each leveled item's four stages — De-Ess, De-Breath, Ride, Gate — as they were made, the dials
//! that say whether each is on and how far it goes, and their sum, what
//! the item's volume plays.
//!
//! The sum is drawn on the item itself, over its waveform (which is drawn
//! through the same sum: it is the take's volume envelope); the four are
//! drawn each in a lane of its own under the track when the track's
//! leveler is expanded (`FTS:level:lane`).
//!
//! A dial scales its stage's curve in dB — 100% as it was made, 0% flat,
//! 200% twice as deep — and off leaves it out of the sum. The curves as
//! made are kept as they were; what plays is them through the dials.
//!
//! Time: a stage's curve is in seconds of the recording, from the point
//! of it the item was read from (its [`Anchor`]). An item slipped since
//! starts somewhere else in its recording, and a split's new half starts
//! where the cut was: `origin` is how far the reading started before the
//! item does now, so the curves stay on the audio they were read from —
//! as the take's volume envelope does, which the engine slips with it.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use dsp::level::envelope::{self as env, Contributions, EnvPoint, GainSpan};

/// The track ext key holding its leveler's dials ([`Stages::to_text`]).
pub const STAGES_KEY: &str = "FTS:leveler:stages";

/// What starts a track's ext key holding where in its recording an item's
/// stages were read ([`Anchor::to_text`]).
pub const ANCHOR_KEY: &str = "FTS:leveler:from:";

/// The most a dial goes: twice as deep as made.
pub const AMOUNT_MAX: f64 = 2.0;

/// The sum drawn on an item: its foot and its top, dB — 0 dB across the
/// middle, so a line ridden ±12 dB moves a quarter of the item.
pub const ITEM_FLOOR_DB: f64 = -24.0;
pub const ITEM_TOP_DB: f64 = 24.0;

/// One of the leveler's four stages.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, facet::Facet)]
pub enum Stage {
    /// Each "ess" eased.
    DeEss,
    /// Breaths and soft noises tucked under.
    DeBreath,
    /// Each line brought toward the song's level, with the makeup.
    Ride,
    /// The noise between lines taken down.
    Gate,
}

impl Stage {
    /// The four, in the order their lanes stack.
    pub const ALL: [Self; 4] = [Self::DeEss, Self::DeBreath, Self::Ride, Self::Gate];

    /// Its name, as its lane is headed.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::DeEss => "De-Ess",
            Self::DeBreath => "De-Breath",
            Self::Ride => "Ride",
            Self::Gate => "Gate",
        }
    }

    /// Its name in [`Stages::to_text`].
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::DeEss => "de-ess",
            Self::DeBreath => "de-breath",
            Self::Ride => "ride",
            Self::Gate => "gate",
        }
    }

    /// The stage `text` names: `De-Ess`, `deess`, `ess`, `sibilance`,
    /// `breath`, `ride`, `gate` and the like, any case.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let plain: String = text
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match plain.as_str() {
            "deess" | "ess" | "deesser" | "sibilance" => Some(Self::DeEss),
            "debreath" | "breath" | "breaths" => Some(Self::DeBreath),
            "ride" | "rider" | "riding" => Some(Self::Ride),
            "gate" | "gating" => Some(Self::Gate),
            _ => None,
        }
    }

    /// Its lane's foot and top, dB: room for what it does at twice the
    /// depth it was made at, and 0 dB in sight.
    #[must_use]
    pub const fn range(self) -> (f64, f64) {
        match self {
            Self::DeEss => (-12.0, 3.0),
            Self::DeBreath => (-24.0, 3.0),
            Self::Ride => (-24.0, 24.0),
            Self::Gate => (-36.0, 6.0),
        }
    }

    /// Where it stacks among the four.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::DeEss => 0,
            Self::DeBreath => 1,
            Self::Ride => 2,
            Self::Gate => 3,
        }
    }
}

/// One stage's dial: on or off, and how far it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dial {
    pub on: bool,
    /// What its curve's dB are multiplied by: 1 as made, 0 to
    /// [`AMOUNT_MAX`].
    pub amount: f64,
}

impl Default for Dial {
    fn default() -> Self {
        Self {
            on: true,
            amount: 1.0,
        }
    }
}

impl Dial {
    /// What its stage's dB are multiplied by as it plays: none when off.
    #[must_use]
    pub const fn gain(self) -> f64 {
        if self.on {
            self.amount.clamp(0.0, AMOUNT_MAX)
        } else {
            0.0
        }
    }

    /// The dial said: its amount, or that it is off.
    #[must_use]
    pub fn text(self) -> String {
        if self.on {
            format!("{:.0}%", self.amount * 100.0)
        } else {
            "Off".to_owned()
        }
    }
}

/// The four dials of a track's leveler.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stages {
    pub de_ess: Dial,
    pub de_breath: Dial,
    pub ride: Dial,
    pub gate: Dial,
}

impl Stages {
    /// `stage`'s dial.
    #[must_use]
    pub const fn get(&self, stage: Stage) -> Dial {
        match stage {
            Stage::DeEss => self.de_ess,
            Stage::DeBreath => self.de_breath,
            Stage::Ride => self.ride,
            Stage::Gate => self.gate,
        }
    }

    /// `stage`'s dial set to `dial` (its amount kept in range).
    pub const fn set(&mut self, stage: Stage, dial: Dial) {
        let dial = Dial {
            on: dial.on,
            amount: dial.amount.clamp(0.0, AMOUNT_MAX),
        };
        match stage {
            Stage::DeEss => self.de_ess = dial,
            Stage::DeBreath => self.de_breath = dial,
            Stage::Ride => self.ride = dial,
            Stage::Gate => self.gate = dial,
        }
    }

    /// As kept with the track: `de-ess=on:1.00;de-breath=off:0.50;…`.
    #[must_use]
    pub fn to_text(&self) -> String {
        Stage::ALL
            .iter()
            .map(|s| {
                let dial = self.get(*s);
                format!(
                    "{}={}:{:.3}",
                    s.key(),
                    if dial.on { "on" } else { "off" },
                    dial.amount
                )
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    /// Read back from [`Self::to_text`]; a stage missing or unreadable is
    /// as made.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut out = Self::default();
        for piece in text.split(';') {
            let Some((name, value)) = piece.split_once('=') else {
                continue;
            };
            let Some(stage) = Stage::parse(name) else {
                continue;
            };
            let Some((on, amount)) = value.split_once(':') else {
                continue;
            };
            let Ok(amount) = amount.trim().parse::<f64>() else {
                continue;
            };
            if !amount.is_finite() {
                continue;
            }
            out.set(
                stage,
                Dial {
                    on: on.trim() != "off",
                    amount,
                },
            );
        }
        out
    }

    /// `made` through the dials: each stage's dB scaled by its amount,
    /// and those off left out of the sum.
    #[must_use]
    pub fn apply(&self, made: &Contributions) -> Contributions {
        let scale = |points: &[EnvPoint], dial: Dial| -> Vec<EnvPoint> {
            if !dial.on {
                return Vec::new();
            }
            let k = dial.gain();
            points
                .iter()
                .map(|p| EnvPoint { db: p.db * k, ..*p })
                .collect()
        };
        let sibilance = if self.de_ess.on {
            let k = self.de_ess.gain();
            made.sibilance
                .iter()
                .map(|s| GainSpan { db: s.db * k, ..*s })
                .collect()
        } else {
            Vec::new()
        };
        Contributions {
            gate: scale(&made.gate, self.gate),
            breath: scale(&made.breath, self.de_breath),
            sibilance,
            ride: scale(&made.ride, self.ride),
            bypass: env::Bypass {
                gate: made.bypass.gate || !self.gate.on,
                breath: made.bypass.breath || !self.de_breath.on,
                sibilance: made.bypass.sibilance || !self.de_ess.on,
                ride: made.bypass.ride || !self.ride.on,
            },
        }
    }
}

/// Where in its recording an item's stages were read: the take's offset
/// into its source then, and the source.
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    /// Seconds into the source.
    pub offset: f64,
    pub path: String,
}

impl Anchor {
    /// As kept with the track: the offset, a tab, the path.
    #[must_use]
    pub fn to_text(&self) -> String {
        format!("{}\t{}", self.offset, self.path)
    }

    /// Read back from [`Self::to_text`].
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (offset, path) = text.split_once('\t')?;
        let offset = offset
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|o| o.is_finite())?;
        Some(Self {
            offset,
            path: path.to_owned(),
        })
    }
}

/// How far into its recording a set of stages reaches, seconds from where
/// it was read.
#[must_use]
pub fn extent(lanes: &Contributions) -> f64 {
    let last = |points: &[EnvPoint]| points.last().map_or(0.0, |p| p.t_s);
    lanes
        .sibilance
        .iter()
        .map(|s| s.to_s)
        .chain([last(&lanes.gate), last(&lanes.breath), last(&lanes.ride)])
        .fold(0.0, f64::max)
}

/// The kept anchor an item with none of its own was read with.
///
/// Of `kept` (each an anchor and how far its stages reach), the one that
/// was read over `path` at `offset` — the latest read at or before it —
/// for an item with none of its own: a split's new half, a comp's slice,
/// read with the item it was cut from.
#[must_use]
pub fn covering(kept: &[(&Anchor, f64)], path: &str, offset: f64) -> Option<usize> {
    const SLACK: f64 = 1e-3;
    kept.iter()
        .enumerate()
        .filter(|(_, (anchor, reach))| {
            anchor.path == path
                && offset >= anchor.offset - SLACK
                && offset <= anchor.offset + reach + SLACK
        })
        .max_by(|(_, (a, _)), (_, (b, _))| a.offset.total_cmp(&b.offset))
        .map(|(i, _)| i)
}

/// Sibilance spans as a curve that steps: 0 dB, each span's dB across it,
/// 0 dB after.
#[must_use]
pub fn steps(spans: &[GainSpan]) -> Vec<EnvPoint> {
    let held = |t_s: f64, db: f64| EnvPoint {
        t_s,
        db,
        hold: true,
    };
    let mut out = Vec::with_capacity(spans.len().saturating_mul(2).saturating_add(1));
    if spans.first().is_some_and(|s| s.from_s > 0.0) {
        out.push(held(0.0, 0.0));
    }
    for span in spans {
        out.push(held(span.from_s, span.db));
        out.push(held(span.to_s, 0.0));
    }
    out
}

/// Each stage's curve as it plays, through its dial.
#[derive(Clone, Debug, Default)]
pub struct Curves {
    pub de_ess: Vec<EnvPoint>,
    pub de_breath: Vec<EnvPoint>,
    pub ride: Vec<EnvPoint>,
    pub gate: Vec<EnvPoint>,
}

impl Curves {
    /// `stage`'s.
    #[must_use]
    pub fn of(&self, stage: Stage) -> &[EnvPoint] {
        match stage {
            Stage::DeEss => &self.de_ess,
            Stage::DeBreath => &self.de_breath,
            Stage::Ride => &self.ride,
            Stage::Gate => &self.gate,
        }
    }
}

/// One leveled item: its four stages as made, the track's dials, each
/// stage as it plays and their sum — in the recording's seconds (`at`
/// turns them into the timeline's).
#[derive(Clone, Debug)]
pub struct Leveled {
    /// Its track, and the item.
    pub track: String,
    pub item: String,
    /// Where the item starts and how long it is, seconds.
    pub position: f64,
    pub length: f64,
    /// Item seconds per recording second.
    pub stretch: f64,
    /// Recording seconds from where the stages were read to where the
    /// item starts now: 0 until it is slipped or cut.
    pub origin: f64,
    /// The four as made.
    pub lanes: Contributions,
    /// The track's dials.
    pub stages: Stages,
    /// Each as it plays.
    pub curves: Curves,
    /// Their sum — what the item's volume plays.
    pub composite: Vec<EnvPoint>,
}

impl Leveled {
    /// Item `item` of `track` where it is now (`position`, `length`,
    /// `stretch`, `origin` as the fields), its stages `lanes` through
    /// `stages`.
    #[must_use]
    pub fn new(
        (track, item): (&str, &str),
        (position, length, stretch, origin): (f64, f64, f64, f64),
        lanes: Contributions,
        stages: Stages,
    ) -> Self {
        let mut out = Self {
            track: track.to_owned(),
            item: item.to_owned(),
            position,
            length,
            stretch,
            origin,
            lanes,
            stages,
            curves: Curves::default(),
            composite: Vec::new(),
        };
        out.resum();
        out
    }

    /// Timeline seconds for a stage's time.
    #[must_use]
    pub fn at(&self, t_s: f64) -> f64 {
        (t_s + self.origin).mul_add(self.stretch, self.position)
    }

    /// A stage's time for timeline seconds.
    #[must_use]
    pub fn lane_time(&self, t: f64) -> f64 {
        (t - self.position) / self.stretch.max(1e-9) - self.origin
    }

    /// Whether timeline `t` is in the item.
    #[must_use]
    pub fn holds(&self, t: f64) -> bool {
        t >= self.position && t <= self.position + self.length
    }

    /// Each stage as it plays and the sum again, after the stages or the
    /// dials changed.
    pub fn resum(&mut self) {
        let applied = self.stages.apply(&self.lanes);
        self.composite = env::composite(&applied);
        self.curves = Curves {
            de_ess: steps(&applied.sibilance),
            de_breath: applied.breath,
            ride: applied.ride,
            gate: applied.gate,
        };
    }

    /// The sum as the item's take volume envelope: item seconds, the
    /// points over the item and one either side.
    #[must_use]
    pub fn item_curve(&self) -> Vec<EnvPoint> {
        on_item(&self.composite, (self.origin, self.stretch), self.length)
    }
}

/// A curve in an item's own seconds.
///
/// `curve` (recording seconds) in an item's seconds — `origin` and
/// `stretch` as [`Leveled`]'s — keeping the points over its `length` and
/// one either side, so a slice of a long take carries its own stretch of
/// the curve and the curve at its edges is what it was.
#[must_use]
pub fn on_item(curve: &[EnvPoint], (origin, stretch): (f64, f64), length: f64) -> Vec<EnvPoint> {
    let moved: Vec<EnvPoint> = curve
        .iter()
        .map(|p| EnvPoint {
            t_s: (p.t_s + origin) * stretch,
            ..*p
        })
        .collect();
    let first = moved.partition_point(|p| p.t_s < 0.0).saturating_sub(1);
    let last = moved
        .partition_point(|p| p.t_s <= length)
        .saturating_add(1)
        .min(moved.len());
    moved
        .get(first..last)
        .map(<[EnvPoint]>::to_vec)
        .unwrap_or_default()
}

/// Where `db` sits on an item, `0` at its foot and `1` at its top
/// ([`ITEM_FLOOR_DB`] to [`ITEM_TOP_DB`]).
#[must_use]
pub fn item_y(db: f64) -> f64 {
    ((db - ITEM_FLOOR_DB) / (ITEM_TOP_DB - ITEM_FLOOR_DB)).clamp(0.0, 1.0)
}

/// A curve across `columns` columns, as `(column, dB)`.
///
/// From time `from` to `to`: where it is at each column's start, and inside each
/// the lowest and the highest it reaches, in the order it reaches them —
/// so a gate's dip narrower than a column is drawn, not stepped over, and
/// the line does not shimmer as the view scrolls.
#[must_use]
pub fn trace(points: &[EnvPoint], (from, to): (f64, f64), columns: usize) -> Vec<(f64, f64)> {
    if to <= from || columns == 0 {
        return Vec::new();
    }
    let width = (to - from) / crate::num::coord(columns);
    let mut out = Vec::with_capacity(columns.saturating_mul(3).saturating_add(1));
    for c in 0..=columns {
        let x = crate::num::coord(c);
        let start = x.mul_add(width, from);
        out.push((x, crate::automation::db_at(points, start)));
        if c == columns {
            break;
        }
        let end = start + width;
        let inside = points
            .get(
                points.partition_point(|p| p.t_s <= start)..points.partition_point(|p| p.t_s < end),
            )
            .unwrap_or_default();
        let lowest = inside.iter().min_by(|a, b| a.db.total_cmp(&b.db));
        let highest = inside.iter().max_by(|a, b| a.db.total_cmp(&b.db));
        if let (Some(lo), Some(hi)) = (lowest, highest) {
            let pair = if lo.t_s <= hi.t_s { [lo, hi] } else { [hi, lo] };
            for p in pair {
                out.push((x + (p.t_s - start) / width, p.db));
            }
        }
    }
    out
}

/// Every leveled item the window shows, by item guid.
fn store() -> &'static Mutex<HashMap<String, Leveled>> {
    static LEVELED: LazyLock<Mutex<HashMap<String, Leveled>>> = LazyLock::new(Default::default);
    &LEVELED
}

/// Put `by_track`'s leveled items in place of the window's.
pub fn adopt(by_track: impl IntoIterator<Item = (String, Vec<Leveled>)>) {
    let all: HashMap<String, Leveled> = by_track
        .into_iter()
        .flat_map(|(_, items)| items)
        .map(|l| (l.item.clone(), l))
        .collect();
    *store().lock().unwrap_or_else(PoisonError::into_inner) = all;
}

/// Whether any item is leveled (so a frame need not look for them).
#[must_use]
pub fn any() -> bool {
    !store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_empty()
}

/// `f` of `item`'s leveling, if it is leveled — read in place, not copied
/// (a frame draws every leveled item on screen).
#[must_use]
pub fn with_item<R>(item: &str, f: impl FnOnce(&Leveled) -> R) -> Option<R> {
    store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(item)
        .map(f)
}

/// `track`'s leveled items, by position.
#[must_use]
pub fn of_track(track: &str) -> Vec<Leveled> {
    let mut out: Vec<Leveled> = store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .values()
        .filter(|l| l.track == track)
        .cloned()
        .collect();
    out.sort_by(|a, b| a.position.total_cmp(&b.position));
    out
}

/// `track`'s dials as the window has them.
#[must_use]
pub fn stages_of(track: &str) -> Option<Stages> {
    store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .values()
        .find(|l| l.track == track)
        .map(|l| l.stages)
}

/// Change `track`'s leveled items in place, at once.
///
/// Here and in its stage lanes, before the engine has it: a dial turned, a ride point
/// dragged. `f` is given each, and should [`Leveled::resum`] what it
/// changes.
pub fn update(track: &str, f: impl Fn(&mut Leveled)) {
    for l in store()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .values_mut()
        .filter(|l| l.track == track)
    {
        f(l);
    }
    crate::automation::update_leveled(track, &f);
}

/// Set `track`'s dial for `stage` in the window, every item summed again.
pub fn set_dial(track: &str, stage: Stage, dial: Dial) {
    update(track, |l| {
        l.stages.set(stage, dial);
        l.resum();
    });
}

// ── Over the window's link ───────────────────────────────────────────

/// A leveled item as it crosses to a window: where it is, its stages as
/// made and the track's dials — the window remakes the curves.
#[must_use]
pub fn to_wire(leveled: &Leveled) -> crate::view::WireLeveled {
    let points = |points: &[EnvPoint]| -> Vec<crate::view::WirePoint> {
        points
            .iter()
            .map(|p| crate::view::WirePoint {
                t: p.t_s,
                db: p.db,
                hold: p.hold,
            })
            .collect()
    };
    let lanes = &leveled.lanes;
    let dial = |stage: Stage| {
        let d = leveled.stages.get(stage);
        (d.on, d.amount)
    };
    crate::view::WireLeveled {
        track: leveled.track.clone(),
        item: leveled.item.clone(),
        position: leveled.position,
        length: leveled.length,
        stretch: leveled.stretch,
        origin: leveled.origin,
        gate: points(&lanes.gate),
        breath: points(&lanes.breath),
        sibilance: lanes
            .sibilance
            .iter()
            .map(|s| crate::view::WireSpan {
                from: s.from_s,
                to: s.to_s,
                db: s.db,
            })
            .collect(),
        ride: points(&lanes.ride),
        bypass: [
            lanes.bypass.gate,
            lanes.bypass.breath,
            lanes.bypass.sibilance,
            lanes.bypass.ride,
        ],
        dials: Stage::ALL.map(dial),
    }
}

/// A leveled item a window was told of ([`to_wire`]), its curves made.
#[must_use]
pub fn from_wire(wire: crate::view::WireLeveled) -> Leveled {
    let points = |points: Vec<crate::view::WirePoint>| -> Vec<EnvPoint> {
        points
            .into_iter()
            .map(|p| EnvPoint {
                t_s: p.t,
                db: p.db,
                hold: p.hold,
            })
            .collect()
    };
    let [gate, breath, sibilance, ride] = wire.bypass;
    let lanes = Contributions {
        gate: points(wire.gate),
        breath: points(wire.breath),
        sibilance: wire
            .sibilance
            .into_iter()
            .map(|s| GainSpan {
                from_s: s.from,
                to_s: s.to,
                db: s.db,
            })
            .collect(),
        ride: points(wire.ride),
        bypass: env::Bypass {
            gate,
            breath,
            sibilance,
            ride,
        },
    };
    let mut stages = Stages::default();
    for (stage, (on, amount)) in Stage::ALL.into_iter().zip(wire.dials) {
        stages.set(stage, Dial { on, amount });
    }
    Leveled::new(
        (wire.track.as_str(), wire.item.as_str()),
        (wire.position, wire.length, wire.stretch, wire.origin),
        lanes,
        stages,
    )
}
