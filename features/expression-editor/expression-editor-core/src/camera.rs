//! The canvas camera: time/pitch ↔ pixel mapping, zoom, and magnets.
//!
//! MPElodyne's own notes on this (`View Magnets.md`) end with a list of
//! "likely roughness sources", and every one of them has the same
//! cause: rules that *mutate an already-produced camera in sequence*,
//! so a later rule fights an earlier one every frame.
//!
//! This module inverts that. A gesture produces one base camera, then
//! declares its magnets as weighted [`Influence`]s — candidate cameras,
//! not mutations. [`blend`] resolves them in a single pass (log-space
//! for scales so zoom stays perceptually even, linear for positions),
//! and [`Camera::constrain`] clamps once at the end. Two magnets
//! pulling in opposite directions now average smoothly instead of
//! alternating.

use crate::shape::{smoothstep, smoothstep_between};

/// Canvas size in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub w: f64,
    pub h: f64,
}

impl Viewport {
    pub fn new(w: f64, h: f64) -> Self {
        Self {
            w: w.max(1.0),
            h: h.max(1.0),
        }
    }
}

/// Where the canvas is looking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Document time at the left edge.
    pub t0: f64,
    /// Horizontal scale.
    pub units_per_px: f64,
    /// MIDI pitch at the vertical center.
    pub pitch_center: f64,
    /// Vertical scale — the height of one semitone row.
    pub px_per_semitone: f64,
}

/// Hard limits applied after every navigation. These are clamps, not
/// magnets: hitting one discards the requested move, which is why they
/// are applied exactly once rather than mid-gesture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// Editable time span, already cushioned.
    pub t_min: f64,
    pub t_max: f64,
    /// Row height floor for manual zoom — below the readable minimum
    /// Reset View uses, so an overview stays available.
    pub min_px_per_semitone: f64,
    pub max_px_per_semitone: f64,
    /// Fraction of the viewport allowed past an edge as whitespace.
    pub edge_whitespace: f64,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            t_min: 0.0,
            t_max: 1.0,
            min_px_per_semitone: 3.5,
            max_px_per_semitone: 400.0,
            edge_whitespace: 0.2,
        }
    }
}

impl Camera {
    pub fn x(&self, t: f64) -> f64 {
        (t - self.t0) / self.units_per_px
    }

    pub fn t_at(&self, x: f64) -> f64 {
        self.t0 + x * self.units_per_px
    }

    pub fn y(&self, pitch: f64, vp: Viewport) -> f64 {
        vp.h * 0.5 - (pitch - self.pitch_center) * self.px_per_semitone
    }

    pub fn pitch_at(&self, y: f64, vp: Viewport) -> f64 {
        self.pitch_center + (vp.h * 0.5 - y) / self.px_per_semitone
    }

    /// `(t_left, t_right)`.
    pub fn time_span(&self, vp: Viewport) -> (f64, f64) {
        (self.t0, self.t0 + vp.w * self.units_per_px)
    }

    /// `(pitch_low, pitch_high)`.
    pub fn pitch_span(&self, vp: Viewport) -> (f64, f64) {
        let half = vp.h * 0.5 / self.px_per_semitone;
        (self.pitch_center - half, self.pitch_center + half)
    }

    /// Zoom time by `factor` (>1 zooms in) keeping `anchor_t` pinned to
    /// its current pixel.
    pub fn zoom_time_about(&mut self, anchor_t: f64, factor: f64) {
        let x = self.x(anchor_t);
        self.units_per_px /= factor.max(1e-6);
        self.t0 = anchor_t - x * self.units_per_px;
    }

    /// Zoom pitch by `factor` keeping `anchor_pitch` pinned.
    pub fn zoom_pitch_about(&mut self, anchor_pitch: f64, factor: f64, vp: Viewport) {
        let y = self.y(anchor_pitch, vp);
        self.px_per_semitone *= factor.max(1e-6);
        self.pitch_center = anchor_pitch - (vp.h * 0.5 - y) / self.px_per_semitone;
    }

    pub fn pan_px(&mut self, dx: f64, dy: f64) {
        self.t0 -= dx * self.units_per_px;
        self.pitch_center += dy / self.px_per_semitone;
    }

    /// Clamp to `bounds`. Applied once, after blending.
    pub fn constrain(&mut self, bounds: Bounds, vp: Viewport) {
        let span = bounds.t_max - bounds.t_min;
        // Never show more than the cushioned item.
        let max_upp = span / vp.w;
        if max_upp > 0.0 {
            self.units_per_px = self.units_per_px.min(max_upp).max(1e-9);
        }
        let visible = vp.w * self.units_per_px;
        let slack = visible * bounds.edge_whitespace;
        self.t0 = self.t0.clamp(
            bounds.t_min - slack,
            (bounds.t_max + slack - visible).max(bounds.t_min - slack),
        );

        self.px_per_semitone = self
            .px_per_semitone
            .clamp(bounds.min_px_per_semitone, bounds.max_px_per_semitone);
        self.pitch_center = self.pitch_center.clamp(0.0, 127.0);
    }
}

/// A candidate camera and how strongly it pulls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Influence {
    pub camera: Camera,
    /// 0..1. Weights above 1 in aggregate are normalized.
    pub weight: f64,
}

/// Resolve `base` against every influence in one pass.
///
/// Scales blend geometrically — halving and doubling are equal-sized
/// steps to the eye, so a linear average of `units_per_px` would bias
/// every blend toward zoomed-out.
pub fn blend(base: Camera, influences: &[Influence]) -> Camera {
    let total: f64 = influences.iter().map(|i| i.weight.clamp(0.0, 1.0)).sum();
    if total <= 1e-9 {
        return base;
    }
    // Above a combined weight of 1 the influences fully replace the
    // base rather than overshooting past it.
    let norm = if total > 1.0 { 1.0 / total } else { 1.0 };
    let base_w = (1.0 - total * norm).max(0.0);

    let mut t0 = base.t0 * base_w;
    let mut pitch_center = base.pitch_center * base_w;
    let mut log_upp = base.units_per_px.max(1e-12).ln() * base_w;
    let mut log_pps = base.px_per_semitone.max(1e-12).ln() * base_w;

    for i in influences {
        let w = i.weight.clamp(0.0, 1.0) * norm;
        t0 += i.camera.t0 * w;
        pitch_center += i.camera.pitch_center * w;
        log_upp += i.camera.units_per_px.max(1e-12).ln() * w;
        log_pps += i.camera.px_per_semitone.max(1e-12).ln() * w;
    }

    Camera {
        t0,
        units_per_px: log_upp.exp(),
        pitch_center,
        px_per_semitone: log_pps.exp(),
    }
}

/// Content the camera frames: the time span plus the pitch span that
/// actually contains notes *and* their curves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Content {
    pub t_start: f64,
    pub t_end: f64,
    pub pitch_lo: f64,
    pub pitch_hi: f64,
}

/// The idealized "Reset View" camera — what `V` snaps to and what every
/// zoom-out path converges on.
///
/// Fits the content with `pad` fractional headroom above and below
/// (0.35 in the shipped feel) and a small horizontal cushion.
pub fn reset_view(content: Content, vp: Viewport, cushion: f64, pad: f64) -> Camera {
    let span = (content.t_end - content.t_start).max(1e-6);
    let t0 = content.t_start - span * cushion;
    let units_per_px = (span * (1.0 + 2.0 * cushion)) / vp.w;

    let pitch_span = (content.pitch_hi - content.pitch_lo).max(1.0) * (1.0 + 2.0 * pad);
    // Reset View keeps a readable floor of its own, above the manual
    // zoom-out floor, so `V` always lands somewhere legible.
    let px_per_semitone = (vp.h / pitch_span).max(7.0);

    Camera {
        t0,
        units_per_px,
        pitch_center: (content.pitch_lo + content.pitch_hi) * 0.5,
        px_per_semitone,
    }
}

/// Magnet that frames the nearer item edge once the pointer approaches
/// it.
///
/// Zero through the inner `dead_zone` of the item's half-span, rising
/// with smoothstep to full at the edge, where it leaves
/// `whitespace` of the viewport past the edge.
pub fn edge_magnet(
    base: Camera,
    mouse_t: f64,
    content: Content,
    vp: Viewport,
    dead_zone: f64,
    whitespace: f64,
) -> Option<Influence> {
    let center = (content.t_start + content.t_end) * 0.5;
    let radius = (content.t_end - content.t_start) * 0.5;
    if radius <= 0.0 {
        return None;
    }
    let offset = (mouse_t - center) / radius; // -1..1 inside the item
    let weight = smoothstep_between(dead_zone, 1.0, offset.abs());
    if weight <= 0.0 {
        return None;
    }
    let visible = vp.w * base.units_per_px;
    let pad = visible * whitespace;
    let t0 = if offset > 0.0 {
        // Right edge framed at the right side of the viewport.
        content.t_end + pad - visible
    } else {
        content.t_start - pad
    };
    Some(Influence {
        camera: Camera { t0, ..base },
        weight,
    })
}

/// How far a camera has travelled toward Reset View, 0..1, measured on
/// whichever axis has progressed further.
///
/// Used only while zooming *out*: zoom-in must never be pulled toward
/// the reset camera.
pub fn reset_progress(current: Camera, reset: Camera) -> f64 {
    let h = axis_progress(current.units_per_px, reset.units_per_px);
    let v = axis_progress(reset.px_per_semitone, current.px_per_semitone);
    h.max(v)
}

fn axis_progress(current: f64, target: f64) -> f64 {
    // Measured in log space so "80% of the way there" means the same
    // thing at every zoom depth.
    let (c, t) = (current.max(1e-12).ln(), target.max(1e-12).ln());
    if (t - c).abs() < 1e-9 {
        return 1.0;
    }
    (c / t).clamp(0.0, 1.0)
}

/// The reset-tail magnet: inert for the first `tail_start` of the path
/// toward Reset View, then rising to full across the remainder.
///
/// Deliberately late. Engaging it earlier is what makes a zoom-out feel
/// like it is being taken away from you.
pub fn reset_tail(current: Camera, reset: Camera, tail_start: f64) -> Option<Influence> {
    let progress = reset_progress(current, reset);
    let weight = smoothstep_between(tail_start, 1.0, progress);
    (weight > 0.0).then_some(Influence {
        camera: reset,
        weight,
    })
}

/// Guide the vertical position toward the pitch of content near the
/// pointer, with a smaller contribution from where the pointer itself
/// sits.
///
/// `local_pitch` is the weighted pitch center of notes near the mouse
/// time; `mouse_pitch` is the pointer's own pitch.
pub fn pitch_focus(
    base: Camera,
    local_pitch: Option<f64>,
    mouse_pitch: f64,
    local_weight: f64,
    mouse_weight: f64,
) -> Vec<Influence> {
    let mut out = Vec::new();
    if let Some(p) = local_pitch {
        out.push(Influence {
            camera: Camera {
                pitch_center: p,
                ..base
            },
            weight: local_weight,
        });
    }
    out.push(Influence {
        camera: Camera {
            pitch_center: mouse_pitch,
            ..base
        },
        weight: mouse_weight,
    });
    out
}

/// Center pull that fades in only at deep vertical zoom, so close-up
/// editing stays fluid and does not fight the pointer.
pub fn deep_zoom_center(
    base: Camera,
    content: Content,
    onset: f64,
    max_px_per_semitone: f64,
) -> Option<Influence> {
    let depth = base.px_per_semitone / max_px_per_semitone.max(1e-6);
    let weight = smoothstep(smoothstep_between(onset, 1.0, depth));
    (weight > 0.0).then_some(Influence {
        camera: Camera {
            pitch_center: (content.pitch_lo + content.pitch_hi) * 0.5,
            ..base
        },
        weight,
    })
}
