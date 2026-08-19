//! The drum workspace's write half: the panel's Apply and the slip
//! drag, landed on the daw through the group rule.
//!
//! The UI never writes — `ExpressionEditor` exposes `on_quantize_*` and
//! `on_slip` callbacks and this is what the standalone runner plugs
//! into them. One host object owns the backend, the kit group's items,
//! and the trigger lanes' summed signal, so every gesture edits the
//! whole kit at once (r[drums.group.kit]) and lands as one undo step.

use std::sync::Arc;

use daw::service::{ItemRef, ProjectContext, Projects};
use daw::standalone::Standalone;
use expression_editor_audio::apply_quantize::{Applied, GroupError, apply_split, apply_warp};
use expression_editor_audio::detect::Transient;
use expression_editor_audio::panel_bridge::{self, PanelDetect, PanelTarget};
use expression_editor_audio::quantize::SplitConfig;
use expression_editor_audio::slip::slip_hit;
use expression_editor_core::kit::LaneRole;
use expression_editor_ui::quantize_panel::{Bin, HitPreview, QuantizePanel, WriteMode, histogram};

/// One role lane's contribution to the host: its members' edit items,
/// and (for a trigger lane) the summed signal detection runs on.
pub struct HostLane {
    pub role: LaneRole,
    pub items: Vec<ItemRef>,
    /// The members' mean, kept only for lanes that detect
    /// (`LaneRole::is_detection_source`). `None` elsewhere — the sum of
    /// the room mics would cost memory nothing reads.
    pub summed: Option<Vec<f64>>,
}

/// Everything a drum-workspace gesture needs to reach the daw.
pub struct DrumHost {
    daw: Standalone,
    ctx: ProjectContext,
    lanes: Vec<HostLane>,
    pub sample_rate: f64,
    /// The group's shared take length, seconds — the longest edit item.
    pub take_secs: f64,
    /// One beat, seconds, from the project tempo. What turns the
    /// panel's division/feel into `grid_secs`. r[impl drums.group.tempo]
    pub beat_secs: f64,
}

impl DrumHost {
    pub fn new(
        daw: Standalone,
        ctx: ProjectContext,
        lanes: Vec<HostLane>,
        sample_rate: f64,
        take_secs: f64,
        beat_secs: f64,
    ) -> Self {
        Self {
            daw,
            ctx,
            lanes,
            sample_rate,
            take_secs,
            beat_secs,
        }
    }

    /// Every member item of every role lane — the kit group. Edits are
    /// applied to all of it, never to one lane. r[impl drums.group.kit]
    pub fn group(&self) -> Vec<ItemRef> {
        self.lanes.iter().flat_map(|l| l.items.clone()).collect()
    }

    fn detect_of(panel: &QuantizePanel) -> PanelDetect {
        let d = &panel.detect;
        PanelDetect {
            threshold_db: d.threshold_db,
            sensitivity: d.sensitivity,
            crest_db: d.crest_db,
            high_pass_hz: d.high_pass_hz,
            low_pass_hz: d.low_pass_hz,
            gain: d.gain,
            retrigger_secs: d.retrigger_secs,
            time_offset_secs: d.time_offset_secs,
        }
    }

    fn target_of(&self, panel: &QuantizePanel) -> PanelTarget {
        PanelTarget {
            grid_secs: panel.grid_in(self.beat_secs),
            grid_offset_secs: 0.0,
            swing: panel.swing,
            grid_scan: panel.grid_scan,
            tolerance_secs: panel.tolerance,
            strength: panel.config.strength,
        }
    }

    fn trigger_lanes(&self) -> Vec<Vec<&[f64]>> {
        self.lanes
            .iter()
            .filter_map(|l| l.summed.as_deref())
            .map(|s| vec![s])
            .collect()
    }

    /// Detect + plan for the panel's current settings: the histogram
    /// bins and the per-hit preview the drawer shows.
    // r[impl drums.quantize.preview]
    pub fn preview(&self, panel: &QuantizePanel) -> (Vec<Bin>, Vec<HitPreview>) {
        let (hits, _plan, previews) = panel_bridge::preview(
            &self.trigger_lanes(),
            self.sample_rate,
            &Self::detect_of(panel),
            &self.target_of(panel),
        );
        (histogram(&hits, 24), previews)
    }

    /// The merged hit list at the panel's current detect settings.
    pub fn hits(&self, panel: &QuantizePanel) -> Vec<Transient> {
        panel_bridge::detect_group(
            &self.trigger_lanes(),
            self.sample_rate,
            &Self::detect_of(panel),
        )
    }

    /// Write the panel's plan to the whole kit, one undo step.
    // r[impl drums.quantize.apply]
    pub fn apply(&self, panel: &QuantizePanel) -> Result<Applied, GroupError> {
        let (_, plan, _) = panel_bridge::preview(
            &self.trigger_lanes(),
            self.sample_rate,
            &Self::detect_of(panel),
            &self.target_of(panel),
        );
        let items = self.group();
        self.daw.begin_undo_block(self.ctx.clone(), "Quantize kit");
        let out = match panel.mode {
            WriteMode::Split => {
                let cfg = SplitConfig {
                    leading_pad_secs: panel.pad,
                    crossfade_secs: panel.crossfade,
                };
                let pieces = plan.splits(self.take_secs, cfg);
                apply_split(&self.daw, self.ctx.clone(), &items, &pieces, cfg)
            }
            WriteMode::Warp => {
                let frames = (self.take_secs * self.sample_rate).ceil() as usize;
                match plan.alignment(frames, self.sample_rate) {
                    Some(a) => apply_warp(&self.daw, self.ctx.clone(), &items, &a),
                    None => Ok(Applied::default()),
                }
            }
        };
        self.daw
            .end_undo_block(self.ctx.clone(), "Quantize kit", None);
        out
    }

    /// Slip one hit across the whole kit, one undo step.
    // r[impl drums.manual.slip]
    pub fn slip(
        &self,
        hit: f64,
        next: f64,
        delta: f64,
        cfg: SplitConfig,
    ) -> Result<Applied, GroupError> {
        let items = self.group();
        self.daw.begin_undo_block(self.ctx.clone(), "Slip hit");
        let out = slip_hit(
            &self.daw,
            self.ctx.clone(),
            &items,
            hit,
            next,
            self.take_secs,
            delta,
            cfg,
        );
        self.daw.end_undo_block(self.ctx.clone(), "Slip hit", None);
        out
    }

    /// One undo step back — the whole last gesture.
    pub fn undo(&self) -> bool {
        self.daw.undo(self.ctx.clone())
    }
}

/// The host as the window shares it: the callbacks each hold a clone.
pub type SharedDrumHost = Arc<DrumHost>;
