//! `impl Automation for Reaper` — envelope read/write via the REAPER C API.
//!
//! ## Coverage
//!
//! Phase 1 (this file): full read path (`envelopes`, `envelope`,
//! `points`, `points_in_range`, `value_at`) and full point CRUD
//! (`add_point`, `delete_point`, `set_point`, `delete_points_in_range`).
//!
//! Phase 2 (deferred): `set_visible`, `set_armed`, `set_automation_mode`,
//! `global_automation_override`, `set_global_automation_override` — these
//! need state-chunk parsing or REAPER APIs not exposed in the pinned
//! reaper-rs version. Logged at debug so the trait still wires up.

use crate::safe_wrappers::envelope as env_sw;
use crate::item::{ReaperItem, ReaperTake};
use crate::track::{resolve_project, resolve_track};
use daw_proto::{
    Automation, ProjectContext,
    automation::{
        AddPointParams, Envelope, EnvelopeLocation, EnvelopePoint, EnvelopeRef, EnvelopeShape,
        EnvelopeType, SetPointParams, TimeRangeParams,
    },
    primitives::{AutomationMode, PositionInSeconds},
    track::TrackRef,
};
use daw_proto::{ItemRef, TakeEnvelopeKind, TakeRef};
use reaper_high::Reaper;
use reaper_medium::ProjectContext as ReaperProjectContext;
use reaper_low::raw::TrackEnvelope;
use tracing::debug;

/// Map a raw REAPER shape index to the proto enum. Unknown values
/// default to `Linear`.
fn shape_from_raw(raw: i32) -> EnvelopeShape {
    match raw {
        0 => EnvelopeShape::Linear,
        1 => EnvelopeShape::Square,
        2 => EnvelopeShape::SlowStartEnd,
        3 => EnvelopeShape::FastStart,
        4 => EnvelopeShape::FastEnd,
        5 => EnvelopeShape::Bezier,
        _ => EnvelopeShape::Linear,
    }
}

fn shape_to_raw(shape: EnvelopeShape) -> i32 {
    match shape {
        EnvelopeShape::Linear => 0,
        EnvelopeShape::Square => 1,
        EnvelopeShape::SlowStartEnd => 2,
        EnvelopeShape::FastStart => 3,
        EnvelopeShape::FastEnd => 4,
        EnvelopeShape::Bezier => 5,
    }
}

/// Map an `EnvelopeType` to the chunk-name tag REAPER uses to expose
/// the envelope (e.g. `<VOLENV2`, `<PANENV2`). Used to disambiguate
/// pre-/post-FX envelopes that share a display name.
fn envelope_type_chunk_tag(ty: EnvelopeType) -> &'static str {
    match ty {
        EnvelopeType::Volume => "<VOLENV2",
        EnvelopeType::VolumePrefx => "<VOLENV",
        EnvelopeType::Pan => "<PANENV2",
        EnvelopeType::PanPrefx => "<PANENV",
        EnvelopeType::Width => "<WIDTHENV2",
        EnvelopeType::WidthPrefx => "<WIDTHENV",
        EnvelopeType::Mute => "<MUTEENV",
        EnvelopeType::FxParam => "<PARMENV",
    }
}

/// Common envelope-types we enumerate when listing all envelopes on a
/// track. FX-parameter envelopes are intentionally excluded from this
/// enumeration (they require an FX index + param scan); use a future
/// `list_fx_envelopes` once that's implemented.
const TRACK_ENVELOPE_TYPES: &[EnvelopeType] = &[
    EnvelopeType::Volume,
    EnvelopeType::VolumePrefx,
    EnvelopeType::Pan,
    EnvelopeType::PanPrefx,
    EnvelopeType::Width,
    EnvelopeType::WidthPrefx,
    EnvelopeType::Mute,
];

/// Resolve a `TrackEnvelope*` from a [`EnvelopeLocation`]. Must be called
/// on the REAPER main thread.
fn resolve_envelope(
    project_ctx: &ProjectContext,
    location: &EnvelopeLocation,
) -> Option<*mut TrackEnvelope> {
    let project = resolve_project(project_ctx)?;
    let low = Reaper::get().medium_reaper().low();

    // Take envelopes are resolved before the track is, because they do
    // not have one to speak of: `EnvelopeLocation.track` is documented
    // as ignored for them, and a caller with only an item guid should
    // not have to invent a track ref to satisfy a lookup it does not
    // use.
    if let EnvelopeRef::Take {
        item_guid,
        take_guid,
        kind,
    } = &location.envelope
    {
        return resolve_take_envelope(item_guid, take_guid, *kind);
    }

    let track = resolve_track(&project, &location.track)?;
    let track_ptr = track.raw().ok()?.as_ptr();

    match &location.envelope {
        EnvelopeRef::Type(ty) => {
            let tag = envelope_type_chunk_tag(*ty);
            let env = env_sw::get_track_envelope_by_chunk_name(low, track_ptr, tag);
            if !env.is_null() { Some(env) } else { None }
        }
        EnvelopeRef::ByName(name) => {
            let env = env_sw::get_track_envelope_by_name(low, track_ptr, name);
            if !env.is_null() { Some(env) } else { None }
        }
        EnvelopeRef::FxParam { .. } => {
            // FxParam envelopes need a separate API path
            // (`GetFXEnvelope`); not implemented in Phase 1.
            None
        }
        EnvelopeRef::Send { .. } => {
            // Send envelopes need `GetTrackSendInfo_Value` /
            // `GetTrackSendEnvelope`; not yet ported. Standalone
            // backend supports them; REAPER backend follow-up.
            None
        }
        // Handled above, before the track lookup.
        EnvelopeRef::Take { .. } => None,
    }
}

/// REAPER's display name for a take envelope kind.
fn take_envelope_name(kind: TakeEnvelopeKind) -> &'static str {
    match kind {
        TakeEnvelopeKind::Volume => "Volume",
        TakeEnvelopeKind::Pan => "Pan",
        TakeEnvelopeKind::Mute => "Mute",
        TakeEnvelopeKind::Pitch => "Pitch",
    }
}

/// The action that toggles a take envelope into existence.
///
/// There is no `CreateTakeEnvelope` in the API — a take envelope is
/// brought into being by running the same action the user would, on the
/// selected item. Only the two verified in the wild are claimed here;
/// guessing the others would produce an action id that silently does
/// something else.
fn take_envelope_toggle_action(kind: TakeEnvelopeKind) -> Option<u32> {
    match kind {
        TakeEnvelopeKind::Volume => Some(40693),
        TakeEnvelopeKind::Pitch => Some(41612),
        TakeEnvelopeKind::Pan | TakeEnvelopeKind::Mute => None,
    }
}

/// Find a take envelope by kind, without creating it.
fn find_take_envelope(
    take: reaper_medium::MediaItemTake,
    kind: TakeEnvelopeKind,
) -> Option<*mut TrackEnvelope> {
    let low = Reaper::get().medium_reaper().low();
    let want = take_envelope_name(kind);

    // By name first, since it is one call...
    if let Ok(name) = std::ffi::CString::new(want) {
        let env = unsafe { low.GetTakeEnvelopeByName(take.as_ptr(), name.as_ptr()) };
        if !env.is_null() {
            return Some(env);
        }
    }
    // ...then by enumeration, because the name lookup is unreliable
    // across REAPER versions and localisations, and the enumeration is
    // what actually works.
    let count = unsafe { low.CountTakeEnvelopes(take.as_ptr()) };
    for i in 0..count {
        let env = unsafe { low.GetTakeEnvelope(take.as_ptr(), i) };
        if env.is_null() {
            continue;
        }
        let mut buf = vec![0i8; 128];
        let ok = unsafe { low.GetEnvelopeName(env, buf.as_mut_ptr(), buf.len() as i32) };
        if !ok {
            continue;
        }
        let name = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) };
        if name.to_string_lossy() == want {
            return Some(env);
        }
    }
    None
}

/// Resolve a take envelope, creating it if the take has none.
///
/// Creation is the interesting part. REAPER exposes no API for it, so
/// the envelope is toggled into existence by running the same action a
/// user would — which means the item has to be *selected* first, since
/// that is what the action operates on. The previous selection is put
/// back afterwards: an editor that silently reselects the user's items
/// is worse than one that cannot make an envelope.
fn resolve_take_envelope(
    item_guid: &str,
    take_guid: &str,
    kind: TakeEnvelopeKind,
) -> Option<*mut TrackEnvelope> {
    let item = ReaperItem::resolve_item(
        &ItemRef::Guid(item_guid.to_string()),
        ReaperProjectContext::CurrentProject,
    )?;
    let take_ref = if take_guid.is_empty() {
        TakeRef::Active
    } else {
        TakeRef::Guid(take_guid.to_string())
    };
    let take = ReaperTake::resolve_take(item, &take_ref)?;

    if let Some(env) = find_take_envelope(take, kind) {
        return Some(env);
    }

    let action = take_envelope_toggle_action(kind)?;
    let medium = Reaper::get().medium_reaper();
    let low = medium.low();

    // Select only this item, run the toggle, restore the selection.
    let previously: Vec<_> = (0..unsafe { low.CountSelectedMediaItems(std::ptr::null_mut()) })
        .filter_map(|i| {
            let p = unsafe { low.GetSelectedMediaItem(std::ptr::null_mut(), i) };
            (!p.is_null()).then_some(p)
        })
        .collect();
    unsafe {
        low.Main_OnCommand(40289, 0); // Item: Unselect all items
        low.SetMediaItemSelected(item.as_ptr(), true);
        low.Main_OnCommand(action as i32, 0);
        low.SetMediaItemSelected(item.as_ptr(), false);
        for p in previously {
            low.SetMediaItemSelected(p, true);
        }
    }

    find_take_envelope(take, kind)
}

/// Build an [`Envelope`] proto struct from a track + envelope handle.
fn build_envelope(
    track_guid: &str,
    envelope_type: EnvelopeType,
    env: *mut TrackEnvelope,
) -> Envelope {
    let low = Reaper::get().medium_reaper().low();
    let name = env_sw::get_envelope_name(low, env).unwrap_or_default();
    let point_count = env_sw::count_envelope_points(low, env);
    Envelope {
        track_guid: track_guid.to_string(),
        envelope_type,
        name,
        fx_guid: None,
        param_index: None,
        // Visibility / armed / mode aren't queryable through the
        // envelope handle alone — needs a chunk parse. Phase 2.
        visible: true,
        armed: false,
        automation_mode: AutomationMode::TrimRead,
        point_count,
    }
}

fn collect_points(env: *mut TrackEnvelope) -> Vec<EnvelopePoint> {
    let low = Reaper::get().medium_reaper().low();
    let count = env_sw::count_envelope_points(low, env);
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        if let Some(p) = env_sw::get_envelope_point(low, env, i) {
            out.push(EnvelopePoint {
                index: i,
                time: PositionInSeconds::from_seconds(p.time),
                value: p.value,
                shape: shape_from_raw(p.shape),
                tension: p.tension,
                selected: p.selected,
            });
        }
    }
    out
}

impl Automation for crate::Reaper {
    // REAPER owns its own touch/write automation engine (CSurf touch
    // + native automation modes); these are accepted as no-ops so
    // surface drivers can call them uniformly across backends.
    fn touch_param(
        &self,
        _project: daw_proto::ProjectContext,
        _location: EnvelopeLocation,
    ) -> daw_proto::DawResult<()> {
        Ok(())
    }

    fn release_param(
        &self,
        _project: daw_proto::ProjectContext,
        _location: EnvelopeLocation,
    ) -> daw_proto::DawResult<()> {
        Ok(())
    }

    fn write_param(
        &self,
        _project: daw_proto::ProjectContext,
        _location: EnvelopeLocation,
        _value: f64,
    ) -> daw_proto::DawResult<()> {
        Err(daw_proto::DawError::operation_failed(
            "write_param: REAPER backend writes through native setters",
        ))
    }

    fn envelopes(&self, project: ProjectContext, track: TrackRef) -> Vec<Envelope> {
        debug!("Reaper::envelopes");
        (|| -> Option<Vec<Envelope>> {
            let proj = resolve_project(&project)?;
            let track_obj = resolve_track(&proj, &track)?;
            let track_guid = track_obj.guid().to_string_without_braces();
            let track_ptr = track_obj.raw().ok()?.as_ptr();
            let low = Reaper::get().medium_reaper().low();
            let mut out = Vec::new();
            for &ty in TRACK_ENVELOPE_TYPES {
                let env = env_sw::get_track_envelope_by_chunk_name(
                    low,
                    track_ptr,
                    envelope_type_chunk_tag(ty),
                );
                if !env.is_null() {
                    out.push(build_envelope(&track_guid, ty, env));
                }
            }
            Some(out)
        })()
        .unwrap_or_default()
    }

    fn envelope(&self, project: ProjectContext, location: EnvelopeLocation) -> Option<Envelope> {
        debug!("Reaper::envelope");
        let env = resolve_envelope(&project, &location)?;
        let proj = resolve_project(&project)?;
        let track = resolve_track(&proj, &location.track)?;
        let track_guid = track.guid().to_string_without_braces();
        let ty = match &location.envelope {
            EnvelopeRef::Type(t) => *t,
            _ => EnvelopeType::Volume,
        };
        Some(build_envelope(&track_guid, ty, env))
    }

    fn set_visible(&self, _project: ProjectContext, _location: EnvelopeLocation, _visible: bool) {
        debug!("Reaper::set_visible — Phase 2, not implemented");
    }

    fn set_armed(&self, _project: ProjectContext, _location: EnvelopeLocation, _armed: bool) {
        debug!("Reaper::set_armed — Phase 2, not implemented");
    }

    fn set_automation_mode(
        &self,
        _project: ProjectContext,
        _location: EnvelopeLocation,
        _mode: AutomationMode,
    ) {
        debug!("Reaper::set_automation_mode — Phase 2, not implemented");
    }

    fn points(&self, project: ProjectContext, location: EnvelopeLocation) -> Vec<EnvelopePoint> {
        debug!("Reaper::points");
        let Some(env) = resolve_envelope(&project, &location) else {
            return Vec::new();
        };
        collect_points(env)
    }

    fn points_in_range(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        range: TimeRangeParams,
    ) -> Vec<EnvelopePoint> {
        debug!("Reaper::points_in_range");
        let all = self.points(project, location);
        let start = range.start.as_seconds();
        let end = range.end.as_seconds();
        all.into_iter()
            .filter(|p| {
                let t = p.time.as_seconds();
                t >= start && t <= end
            })
            .collect()
    }

    fn value_at(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        time: PositionInSeconds,
    ) -> f64 {
        debug!("Reaper::value_at");
        (|| -> Option<f64> {
            let env = resolve_envelope(&project, &location)?;
            let low = Reaper::get().medium_reaper().low();
            let (value, _, _, _) =
                env_sw::evaluate_envelope(low, env, time.as_seconds(), 44100.0, 1)?;
            Some(value)
        })()
        .unwrap_or(0.0)
    }

    fn add_point(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        params: AddPointParams,
    ) -> u32 {
        debug!("Reaper::add_point");
        (|| -> Option<u32> {
            let env = resolve_envelope(&project, &location)?;
            let low = Reaper::get().medium_reaper().low();
            let ok = env_sw::insert_envelope_point(
                low,
                env,
                params.time.as_seconds(),
                params.value,
                shape_to_raw(params.shape),
                0.0,
                false,
                true,
            );
            if !ok {
                return None;
            }
            // REAPER's InsertEnvelopePoint doesn't return the new index;
            // sort + scan to find the point closest to `params.time`.
            // Float equality is fragile so we pick the nearest neighbour
            // rather than asserting equality.
            let count = env_sw::count_envelope_points(low, env);
            let target = params.time.as_seconds();
            let mut best = (0u32, f64::INFINITY);
            for i in 0..count {
                if let Some(p) = env_sw::get_envelope_point(low, env, i) {
                    let d = (p.time - target).abs();
                    if d < best.1 {
                        best = (i, d);
                    }
                }
            }
            Some(best.0)
        })()
        .unwrap_or(0)
    }

    fn delete_point(&self, project: ProjectContext, location: EnvelopeLocation, index: u32) {
        debug!("Reaper::delete_point index={index}");
        if let Some(env) = resolve_envelope(&project, &location) {
            let low = Reaper::get().medium_reaper().low();
            let _ = env_sw::delete_envelope_point(low, env, index);
        }
    }

    fn set_point(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        params: SetPointParams,
    ) {
        debug!("Reaper::set_point index={}", params.index);
        if let Some(env) = resolve_envelope(&project, &location) {
            let low = Reaper::get().medium_reaper().low();
            let _ = env_sw::set_envelope_point(
                low,
                env,
                params.index,
                Some(params.time.as_seconds()),
                Some(params.value),
                Some(shape_to_raw(params.shape)),
                None,
                None,
                true,
            );
        }
    }

    fn delete_points_in_range(
        &self,
        project: ProjectContext,
        location: EnvelopeLocation,
        range: TimeRangeParams,
    ) {
        debug!("Reaper::delete_points_in_range");
        if let Some(env) = resolve_envelope(&project, &location) {
            let low = Reaper::get().medium_reaper().low();
            let _ = env_sw::delete_envelope_points_in_range(
                low,
                env,
                range.start.as_seconds(),
                range.end.as_seconds(),
            );
        }
    }

    fn global_automation_override(&self, _project: ProjectContext) -> Option<AutomationMode> {
        debug!("Reaper::global_automation_override — Phase 2, returning None");
        None
    }

    fn set_global_automation_override(
        &self,
        _project: ProjectContext,
        _mode: Option<AutomationMode>,
    ) {
        debug!("Reaper::set_global_automation_override — Phase 2, not implemented");
    }
}
