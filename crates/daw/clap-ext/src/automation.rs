//! `com.fasttrackstudio.host-automation/1`: a plugin's own lanes.
//!
//! A plugin reads and writes the host's automation lanes of its own
//! parameters (the header has the contract). Both sides: [`HostAutomation`] and [`HostAutomationVtable`]
//! for a host, [`HostAutomationClient`] for a plugin, and the typed points
//! they exchange as JSON.
//!
//! [`merge_points`] is the rule every host applies to a write, kept here
//! so two hosts never disagree about which of the user's points survive.

use std::ffi::{c_char, c_void};
use std::marker::PhantomData;
use std::sync::Arc;

use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::clap_plugin;
use clap_sys::stream::clap_ostream;
use serde::{Deserialize, Serialize};

use crate::ExtError;
use crate::stream::{VecOstream, c_string, str_arg, write_all};
use crate::sys::{self, fts_host_automation};

/// The curve from a point to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    #[default]
    Linear,
    /// Holds its value until the next point.
    Square,
    Slow,
    Fast,
    /// Curved by the point's `tension`.
    Bezier,
}

/// Who last wrote a point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    /// Through this extension.
    #[default]
    Plugin,
    /// Drawn, dragged or recorded by the user.
    User,
}

/// One automation point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationPoint {
    /// Project seconds.
    pub t: f64,
    /// Normalized, 0..1.
    pub value: f64,
    #[serde(default)]
    pub shape: Shape,
    /// -1..1, for [`Shape::Bezier`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tension: Option<f64>,
    #[serde(default)]
    pub by: By,
}

impl AutomationPoint {
    /// A linear point written by the plugin.
    #[must_use]
    pub const fn new(t: f64, value: f64) -> Self {
        Self {
            t,
            value,
            shape: Shape::Linear,
            tension: None,
            by: By::Plugin,
        }
    }

    #[must_use]
    pub const fn shaped(mut self, shape: Shape) -> Self {
        self.shape = shape;
        self
    }

    #[must_use]
    pub const fn by(mut self, by: By) -> Self {
        self.by = by;
        self
    }
}

/// `[from, to)` in project seconds; `to` may be infinite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeRange {
    pub from: f64,
    pub to: f64,
}

impl TimeRange {
    #[must_use]
    pub const fn new(from: f64, to: f64) -> Self {
        Self { from, to }
    }

    /// From the start to the end.
    pub const ALL: Self = Self::new(f64::NEG_INFINITY, f64::INFINITY);

    #[must_use]
    pub fn contains(&self, t: f64) -> bool {
        t >= self.from && t < self.to
    }
}

/// A lane's automation mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LaneMode {
    #[default]
    Read,
    Touch,
    Latch,
    Write,
    Trim,
    Off,
}

/// One of the plugin's parameters that has a lane in the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneInfo {
    pub param_id: u32,
    pub mode: LaneMode,
    pub shown: bool,
    pub points: u32,
    /// How many of them the user wrote.
    #[serde(default)]
    pub user_points: u32,
}

/// The `FTS_AUTOMATION_*` bits of a write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteFlags(pub u32);

impl WriteFlags {
    pub const CREATE_LANE: Self = Self(sys::FTS_AUTOMATION_CREATE_LANE);
    pub const SHOW_LANE: Self = Self(sys::FTS_AUTOMATION_SHOW_LANE);
    pub const KEEP_USER_EDITS: Self = Self(sys::FTS_AUTOMATION_KEEP_USER_EDITS);

    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// What a write did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WriteReport {
    /// The plugin's points now in the lane from this write.
    pub written: u32,
    /// The user's points left in place.
    #[serde(default)]
    pub kept: Vec<AutomationPoint>,
    /// The lane was made by this write.
    #[serde(default)]
    pub created: bool,
}

/// A user's edit of one of the plugin's lanes, as the `automation` topic
/// carries it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationEdit {
    pub param_id: u32,
    pub from: f64,
    pub to: f64,
    pub by: By,
}

#[derive(Serialize, Deserialize)]
struct PointsDoc {
    param_id: u32,
    points: Vec<AutomationPoint>,
}

/// The lane after a write: `existing` (time order) with `[range)` replaced
/// by `new`, and the user's points kept when `keep_user` (the kept ones
/// returned beside it).
///
/// A kept user point keeps the stretch it shapes, from the point before it
/// to the point after it in the lane as it was: the plugin's new points
/// inside that stretch are left out, so the redraw plays as drawn. A run
/// of user points keeps one stretch around them all.
#[must_use]
pub fn merge_points(
    existing: &[AutomationPoint],
    range: TimeRange,
    new: &[AutomationPoint],
    keep_user: bool,
) -> (Vec<AutomationPoint>, Vec<AutomationPoint>) {
    let mut protected: Vec<TimeRange> = Vec::new();
    let mut kept = Vec::new();
    if keep_user {
        // Runs of the user's points inside the range, as index spans.
        let is_kept = |p: &AutomationPoint| p.by == By::User && range.contains(p.t);
        let mut start: Option<usize> = None;
        for (i, p) in existing.iter().enumerate().chain(core::iter::once((
            existing.len(),
            &AutomationPoint::new(f64::INFINITY, 0.0),
        ))) {
            match (start, i < existing.len() && is_kept(p)) {
                (None, true) => start = Some(i),
                (Some(s), false) => {
                    let from = s
                        .checked_sub(1)
                        .and_then(|b| existing.get(b))
                        .map_or(f64::NEG_INFINITY, |b| b.t);
                    let to = existing.get(i).map_or(f64::INFINITY, |a| a.t);
                    protected.push(TimeRange::new(from, to));
                    kept.extend(existing.get(s..i).unwrap_or_default());
                    start = None;
                }
                _ => {}
            }
        }
    }
    let inside_kept = |t: f64| protected.iter().any(|r| t > r.from && t < r.to);
    let mut out: Vec<AutomationPoint> = existing
        .iter()
        .filter(|p| !range.contains(p.t))
        .copied()
        .chain(kept.iter().copied())
        .chain(
            new.iter()
                .filter(|p| range.contains(p.t) && !inside_kept(p.t))
                .map(|p| p.by(By::Plugin)),
        )
        .collect();
    out.sort_by(|a, b| a.t.total_cmp(&b.t));
    (out, kept)
}

/// The caller of a host-automation function: the plugin it serves.
///
/// An opaque key (its `clap_plugin` address). A host maps it
/// to its instance and refuses parameters that are not that instance's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Caller(pub usize);

impl Caller {
    #[must_use]
    pub fn of(plugin: *const clap_plugin) -> Self {
        Self(plugin.addr())
    }
}

/// What a host implements to answer `host-automation/1`. Main-thread
/// calls, each naming its [`Caller`]; a parameter not the caller's is
/// [`ExtError::NotFound`].
pub trait HostAutomation: Send + Sync {
    /// # Errors
    ///
    /// The caller is unknown.
    fn lanes(&self, caller: Caller) -> Result<Vec<LaneInfo>, ExtError>;
    /// # Errors
    ///
    /// No lane, or not the caller's parameter.
    fn read(
        &self,
        caller: Caller,
        param_id: u32,
        range: TimeRange,
    ) -> Result<Vec<AutomationPoint>, ExtError>;
    /// # Errors
    ///
    /// No lane (without [`WriteFlags::CREATE_LANE`]), or not the caller's.
    fn write(
        &self,
        caller: Caller,
        param_id: u32,
        range: TimeRange,
        points: &[AutomationPoint],
        flags: WriteFlags,
    ) -> Result<WriteReport, ExtError>;
    /// # Errors
    ///
    /// No lane, or not the caller's parameter.
    fn clear(&self, caller: Caller, param_id: u32, range: TimeRange) -> Result<(), ExtError>;
    /// # Errors
    ///
    /// An edit is already open.
    fn begin_edit(&self, caller: Caller, label: &str) -> Result<(), ExtError>;
    /// # Errors
    ///
    /// No edit is open.
    fn end_edit(&self, caller: Caller) -> Result<(), ExtError>;
}

/// How the vtable finds the host's [`HostAutomation`], and the calling
/// plugin, from the raw host pointer.
pub trait HostAutomationLookup: 'static {
    /// # Safety
    ///
    /// `host` is the `clap_host` the plugin called the vtable with.
    unsafe fn lookup(host: *const clap_host) -> Option<(Arc<dyn HostAutomation>, Caller)>;
}

/// The `host-automation/1` vtable for a host type, as a `'static`.
pub struct HostAutomationVtable<L>(PhantomData<L>);

impl<L: HostAutomationLookup> HostAutomationVtable<L> {
    pub const VTABLE: fts_host_automation = fts_host_automation {
        lanes: Some(Self::lanes),
        read: Some(Self::read),
        write: Some(Self::write),
        clear: Some(Self::clear),
        begin_edit: Some(Self::begin_edit),
        end_edit: Some(Self::end_edit),
    };

    /// What `get_extension` returns for [`sys::FTS_EXT_HOST_AUTOMATION`].
    #[must_use]
    pub const fn as_ptr() -> *const c_void {
        let v: &'static fts_host_automation = &Self::VTABLE;
        std::ptr::from_ref(v).cast()
    }

    fn reply<T: Serialize>(out: *const clap_ostream, r: Result<T, ExtError>) -> bool {
        let (ok, json) = match r
            .and_then(|v| serde_json::to_string(&v).map_err(|e| ExtError::Failed(e.to_string())))
        {
            Ok(j) => (true, j),
            Err(e) => (false, e.to_json()),
        };
        // SAFETY: the plugin's stream.
        (unsafe { write_all(out, json.as_bytes()) }) && ok
    }

    unsafe extern "C" fn lanes(host: *const clap_host, out: *const clap_ostream) -> bool {
        let Some((h, caller)) = (unsafe { L::lookup(host) }) else {
            return false;
        };
        Self::reply(out, h.lanes(caller))
    }

    unsafe extern "C" fn read(
        host: *const clap_host,
        param_id: clap_id,
        from: f64,
        to: f64,
        out: *const clap_ostream,
    ) -> bool {
        let Some((h, caller)) = (unsafe { L::lookup(host) }) else {
            return false;
        };
        let r = h
            .read(caller, param_id, TimeRange::new(from, to))
            .map(|points| PointsDoc { param_id, points });
        Self::reply(out, r)
    }

    unsafe extern "C" fn write(
        host: *const clap_host,
        param_id: clap_id,
        from: f64,
        to: f64,
        points: *const c_char,
        flags: u32,
        out: *const clap_ostream,
    ) -> bool {
        let Some((h, caller)) = (unsafe { L::lookup(host) }) else {
            return false;
        };
        let parsed = unsafe { str_arg(points) }
            .ok_or_else(|| ExtError::Failed("no points".into()))
            .and_then(|s| {
                serde_json::from_str::<Vec<AutomationPoint>>(s)
                    .map_err(|e| ExtError::Failed(format!("points: {e}")))
            });
        let r = parsed.and_then(|p| {
            h.write(
                caller,
                param_id,
                TimeRange::new(from, to),
                &p,
                WriteFlags(flags),
            )
        });
        Self::reply(out, r)
    }

    unsafe extern "C" fn clear(
        host: *const clap_host,
        param_id: clap_id,
        from: f64,
        to: f64,
    ) -> bool {
        unsafe { L::lookup(host) }
            .is_some_and(|(h, c)| h.clear(c, param_id, TimeRange::new(from, to)).is_ok())
    }

    unsafe extern "C" fn begin_edit(host: *const clap_host, label: *const c_char) -> bool {
        let label = unsafe { str_arg(label) }.unwrap_or("");
        unsafe { L::lookup(host) }.is_some_and(|(h, c)| h.begin_edit(c, label).is_ok())
    }

    unsafe extern "C" fn end_edit(host: *const clap_host) -> bool {
        unsafe { L::lookup(host) }.is_some_and(|(h, c)| h.end_edit(c).is_ok())
    }
}

/// The host's `host-automation/1`, from the plugin's side. Main thread.
#[derive(Clone, Copy)]
pub struct HostAutomationClient {
    host: *const clap_host,
    ext: *const fts_host_automation,
}

// SAFETY: as `HostActionsClient`: valid for the plugin's life, its
// functions called from the plugin's main thread only.
unsafe impl Send for HostAutomationClient {}
// SAFETY: as above.
unsafe impl Sync for HostAutomationClient {}

impl HostAutomationClient {
    /// Ask `host` for `host-automation/1`. `None` from a host without it:
    /// the plugin keeps and plays its curves itself.
    ///
    /// # Safety
    ///
    /// `host` is the plugin's valid `clap_host`, on the main thread.
    #[must_use]
    pub unsafe fn query(host: *const clap_host) -> Option<Self> {
        if host.is_null() {
            return None;
        }
        let get = unsafe { (*host).get_extension }?;
        let ext = unsafe { get(host, sys::FTS_EXT_HOST_AUTOMATION.as_ptr()) }
            .cast::<fts_host_automation>();
        (!ext.is_null()).then_some(Self { host, ext })
    }

    const fn ext(&self) -> &fts_host_automation {
        // SAFETY: non-null (`query`), valid for the plugin's life.
        unsafe { &*self.ext }
    }

    fn call<T: for<'de> Deserialize<'de>>(
        f: impl FnOnce(*const clap_ostream) -> bool,
    ) -> Result<T, ExtError> {
        let out = VecOstream::new();
        let ok = f(out.as_raw());
        let json = out.into_string();
        if !ok {
            return Err(ExtError::from_json_or(&json, "the host refused"));
        }
        serde_json::from_str(&json).map_err(|e| ExtError::Failed(format!("the host's answer: {e}")))
    }

    /// # Errors
    ///
    /// The host refused.
    pub fn lanes(&self) -> Result<Vec<LaneInfo>, ExtError> {
        let f = self.ext().lanes.ok_or(ExtError::Unsupported)?;
        Self::call(|out| unsafe { f(self.host, out) })
    }

    /// # Errors
    ///
    /// No lane, or not this plugin's parameter.
    pub fn read(&self, param_id: u32, range: TimeRange) -> Result<Vec<AutomationPoint>, ExtError> {
        let f = self.ext().read.ok_or(ExtError::Unsupported)?;
        Self::call::<PointsDoc>(|out| unsafe { f(self.host, param_id, range.from, range.to, out) })
            .map(|d| d.points)
    }

    /// # Errors
    ///
    /// The host refused the write.
    pub fn write(
        &self,
        param_id: u32,
        range: TimeRange,
        points: &[AutomationPoint],
        flags: WriteFlags,
    ) -> Result<WriteReport, ExtError> {
        let f = self.ext().write.ok_or(ExtError::Unsupported)?;
        let json =
            c_string(&serde_json::to_string(points).map_err(|e| ExtError::Failed(e.to_string()))?);
        Self::call(|out| unsafe {
            f(
                self.host,
                param_id,
                range.from,
                range.to,
                json.as_ptr(),
                flags.0,
                out,
            )
        })
    }

    #[must_use]
    pub fn clear(&self, param_id: u32, range: TimeRange) -> bool {
        self.ext()
            .clear
            .is_some_and(|f| unsafe { f(self.host, param_id, range.from, range.to) })
    }

    #[must_use]
    pub fn begin_edit(&self, label: &str) -> bool {
        let label = c_string(label);
        self.ext()
            .begin_edit
            .is_some_and(|f| unsafe { f(self.host, label.as_ptr()) })
    }

    #[must_use]
    pub fn end_edit(&self) -> bool {
        self.ext().end_edit.is_some_and(|f| unsafe { f(self.host) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(t: f64, v: f64, by: By) -> AutomationPoint {
        AutomationPoint::new(t, v).by(by)
    }

    #[test]
    fn a_write_replaces_its_range_and_keeps_what_is_outside() {
        let existing = [
            p(0.0, 0.5, By::Plugin),
            p(1.0, 0.6, By::Plugin),
            p(5.0, 0.7, By::Plugin),
        ];
        let (out, kept) = merge_points(
            &existing,
            TimeRange::new(0.5, 4.0),
            &[p(2.0, 0.1, By::User), p(4.5, 0.9, By::Plugin)],
            true,
        );
        assert_eq!(kept, []);
        let ts: Vec<f64> = out.iter().map(|p| p.t).collect();
        assert_eq!(ts, [0.0, 2.0, 5.0], "4.5 is outside the range");
        assert_eq!(
            out[1].by,
            By::Plugin,
            "what the plugin writes is the plugin's"
        );
    }

    #[test]
    fn keeping_user_edits_keeps_the_stretch_they_shape() {
        let existing = [
            p(0.0, 0.5, By::Plugin),
            p(1.0, 0.5, By::Plugin),
            p(2.0, 0.9, By::User),
            p(3.0, 0.5, By::Plugin),
            p(4.0, 0.5, By::Plugin),
        ];
        let new: Vec<_> = (0..=8)
            .map(|i| p(f64::from(i) * 0.5, 0.2, By::Plugin))
            .collect();
        let (out, kept) = merge_points(&existing, TimeRange::ALL, &new, true);
        assert_eq!(kept, [existing[2]]);
        let ts: Vec<f64> = out.iter().map(|p| p.t).collect();
        assert_eq!(ts, [0.0, 0.5, 1.0, 2.0, 3.0, 3.5, 4.0]);
        assert_eq!(out[3], existing[2]);
        // Without the flag, the redraw goes.
        let (out, kept) = merge_points(&existing, TimeRange::ALL, &new, false);
        assert_eq!(kept, []);
        assert!(out.iter().all(|p| p.by == By::Plugin));
    }

    #[test]
    fn points_read_as_the_header_writes_them() {
        let pt: AutomationPoint =
            serde_json::from_str(r#"{"t":1.5,"value":0.25,"shape":"square","by":"user"}"#).unwrap();
        assert_eq!(
            pt,
            AutomationPoint::new(1.5, 0.25)
                .shaped(Shape::Square)
                .by(By::User)
        );
        let bare: AutomationPoint = serde_json::from_str(r#"{"t":0,"value":1}"#).unwrap();
        assert_eq!(bare, AutomationPoint::new(0.0, 1.0));
    }
}
