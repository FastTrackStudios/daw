//! Serialization trait for RPP chunk types.
//!
//! Provides a consistent interface for serializing REAPER data structures
//! back to valid RPP chunk text.

use super::envelope::Envelope;
use super::item::{
    FadeCurveType, Item, PitchMode, SoloState, SourceBlock, SourceType, Take,
};
use super::marker_region::{MarkerRegion, MarkerRegionCollection};
use super::project::ReaperProject;
use super::time_tempo::{TempoTimeEnvelope, TempoTimePoint};
use super::track::{
    AutomationMode, FolderState, FreeMode, MonitorMode, RecordMode, Track, TrackSoloState,
};

/// Trait for types that can serialize themselves to valid REAPER RPP chunk text.
///
/// # Example
///
/// ```ignore
/// use dawfile_reaper::RppSerialize;
///
/// let track = Track::default();
/// let rpp_text = track.to_rpp_string();
/// // => "<TRACK\n  NAME \"\"\n  ...\n>\n"
/// ```
pub trait RppSerialize {
    /// Serialize to RPP text with no leading indentation.
    fn to_rpp_string(&self) -> String {
        let mut out = String::new();
        self.write_rpp(&mut out, "");
        out
    }

    /// Write RPP text to a buffer at the given indentation prefix.
    ///
    /// Implementations should prepend `indent` to each top-level line they emit,
    /// and pass a deeper indent (e.g. `format!("{}  ", indent)`) to nested children.
    fn write_rpp(&self, out: &mut String, indent: &str);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn b(v: bool) -> i32 {
    if v { 1 } else { 0 }
}

fn rpp_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn fade_curve_to_i32(c: &FadeCurveType) -> i32 {
    match c {
        FadeCurveType::Linear => 0,
        FadeCurveType::Square => 1,
        FadeCurveType::SlowStartEnd => 2,
        FadeCurveType::FastStart => 3,
        FadeCurveType::FastEnd => 4,
        FadeCurveType::Bezier => 5,
        FadeCurveType::Unknown(v) => *v,
    }
}

fn solo_state_to_i32(s: &SoloState) -> i32 {
    match s {
        SoloState::NotSoloed => 0,
        SoloState::Soloed => -1,
        SoloState::SoloOverridden => 1,
        SoloState::Unknown(v) => *v,
    }
}

fn pitch_mode_to_i32(p: &PitchMode) -> i32 {
    match p {
        PitchMode::ProjectDefault => -1,
        PitchMode::SoundTouchPreset1 => 0,
        PitchMode::SoundTouchPreset2 => 1,
        PitchMode::SoundTouchPreset3 => 2,
        PitchMode::DiracLE(n) => 65536 + *n as i32,
        PitchMode::LowQualityWindowed(n) => 131072 + *n as i32,
        PitchMode::ElastiquePro(n) => 196608 + *n as i32,
        PitchMode::ElastiqueEfficient(n) => 262144 + *n as i32,
        PitchMode::ElastiqueSoloist(n) => 327680 + *n as i32,
        PitchMode::Elastique21Pro(n) => 393216 + *n as i32,
        PitchMode::Elastique21Efficient(n) => 458752 + *n as i32,
        PitchMode::Elastique21Soloist(n) => 524288 + *n as i32,
        PitchMode::Unknown(v) => *v,
    }
}

fn automation_mode_to_i32(m: &AutomationMode) -> i32 {
    match m {
        AutomationMode::TrimRead => 0,
        AutomationMode::Read => 1,
        AutomationMode::Touch => 2,
        AutomationMode::Write => 3,
        AutomationMode::Latch => 4,
        AutomationMode::Unknown(v) => *v,
    }
}

fn track_solo_to_i32(s: &TrackSoloState) -> i32 {
    match s {
        TrackSoloState::NoSolo => 0,
        TrackSoloState::Solo => 1,
        TrackSoloState::SoloInPlace => 2,
        TrackSoloState::Unknown(v) => *v,
    }
}

fn folder_state_to_i32(f: &FolderState) -> i32 {
    match f {
        FolderState::Regular => 0,
        FolderState::FolderParent => 1,
        FolderState::LastInFolder => 2,
        FolderState::Unknown(v) => *v,
    }
}

fn free_mode_to_i32(f: &FreeMode) -> i32 {
    match f {
        FreeMode::Disabled => 0,
        FreeMode::FreeItemPositioning => 1,
        FreeMode::FixedItemLanes => 2,
        FreeMode::Unknown(v) => *v,
    }
}

fn monitor_mode_to_i32(m: &MonitorMode) -> i32 {
    match m {
        MonitorMode::Off => 0,
        MonitorMode::On => 1,
        MonitorMode::Auto => 2,
        MonitorMode::Unknown(v) => *v,
    }
}

fn record_mode_to_i32(r: &RecordMode) -> i32 {
    match r {
        RecordMode::Input => 0,
        RecordMode::OutputStereo => 1,
        RecordMode::DisableMonitor => 2,
        RecordMode::OutputStereoLatencyComp => 3,
        RecordMode::OutputMidi => 4,
        RecordMode::OutputMono => 5,
        RecordMode::OutputMonoLatencyComp => 6,
        RecordMode::MidiOverdub => 7,
        RecordMode::MidiReplace => 8,
        RecordMode::MidiTouchReplace => 9,
        RecordMode::OutputMultichannel => 10,
        RecordMode::OutputMultichannelLatencyComp => 11,
        RecordMode::Unknown(v) => *v,
    }
}

fn envelope_shape_to_i32(s: &super::envelope::EnvelopePointShape) -> i32 {
    use super::envelope::EnvelopePointShape;
    match s {
        EnvelopePointShape::Linear => 0,
        EnvelopePointShape::Square => 1,
        EnvelopePointShape::SlowStartEnd => 2,
        EnvelopePointShape::FastStart => 3,
        EnvelopePointShape::FastEnd => 4,
        EnvelopePointShape::Bezier => 5,
        EnvelopePointShape::Default => -1,
    }
}

fn item_timebase_to_i32(t: &super::item::ItemTimebase) -> i32 {
    match t {
        super::item::ItemTimebase::ProjectDefault => -1,
        super::item::ItemTimebase::Time => 0,
        super::item::ItemTimebase::Beats => 1,
        super::item::ItemTimebase::Unknown(v) => *v,
    }
}

/// Re-indent a raw content block so each line gets the given indent prefix.
/// Structural lines (`<TAG` and `>`) get the base indent; content lines get one deeper.
fn write_raw_block(out: &mut String, raw: &str, indent: &str) {
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('<') || trimmed == ">" {
            out.push_str(indent);
            out.push_str(trimmed);
            out.push('\n');
        } else {
            out.push_str(indent);
            out.push_str("  ");
            out.push_str(trimmed);
            out.push('\n');
        }
    }
}

// ---------------------------------------------------------------------------
// SourceBlock
// ---------------------------------------------------------------------------

impl RppSerialize for SourceBlock {
    fn write_rpp(&self, out: &mut String, indent: &str) {
        // If we have raw content, use it for fidelity
        if !self.raw_content.is_empty() {
            write_raw_block(out, &self.raw_content, indent);
            return;
        }

        let tag = match &self.source_type {
            SourceType::Wave => "WAVE",
            SourceType::Mp3 => "MP3",
            SourceType::Midi => "MIDI",
            SourceType::Video => "VIDEO",
            SourceType::Section => "SECTION",
            SourceType::Empty => "EMPTY",
            SourceType::Flac => "FLAC",
            SourceType::Vorbis => "VORBIS",
            SourceType::OfflineWave => "_OFFLINE_WAVE",
            SourceType::Unknown(s) => s.as_str(),
        };

        let inner = format!("{}  ", indent);
        out.push_str(&format!("{}<SOURCE {}\n", indent, tag));
        if !self.file_path.is_empty() {
            out.push_str(&format!("{}FILE \"{}\"\n", inner, rpp_escape(&self.file_path)));
        }
        out.push_str(&format!("{}>\n", indent));
    }
}

// ---------------------------------------------------------------------------
// Take
// ---------------------------------------------------------------------------

impl RppSerialize for Take {
    fn write_rpp(&self, out: &mut String, indent: &str) {
        if self.is_selected {
            out.push_str(&format!("{}TAKE SEL\n", indent));
        } else {
            out.push_str(&format!("{}TAKE\n", indent));
        }
        if !self.name.is_empty() {
            out.push_str(&format!("{}NAME \"{}\"\n", indent, rpp_escape(&self.name)));
        }
        if let Some(vp) = &self.volpan {
            out.push_str(&format!(
                "{}VOLPAN {} {} {} {}\n",
                indent, vp.item_trim, vp.take_pan, vp.take_volume, vp.take_pan_law
            ));
        }
        if self.slip_offset != 0.0 {
            out.push_str(&format!("{}SOFFS {}\n", indent, self.slip_offset));
        }
        if let Some(pr) = &self.playrate {
            out.push_str(&format!(
                "{}PLAYRATE {} {} {} {} {} {}\n",
                indent, pr.rate, b(pr.preserve_pitch), pr.pitch_adjust,
                pitch_mode_to_i32(&pr.pitch_mode), pr.unknown_field_5, pr.unknown_field_6
            ));
        }
        if let Some(color) = self.take_color {
            out.push_str(&format!("{}TAKECOLOR {}\n", indent, color));
        }
        if let Some(guid) = &self.take_guid {
            out.push_str(&format!("{}GUID {}\n", indent, guid));
        }
        if let Some(rp) = self.rec_pass {
            out.push_str(&format!("{}RECPASS {}\n", indent, rp));
        }
        if let Some(source) = &self.source {
            source.write_rpp(out, indent);
        }
    }
}

// ---------------------------------------------------------------------------
// Item
// ---------------------------------------------------------------------------

impl RppSerialize for Item {
    fn write_rpp(&self, out: &mut String, indent: &str) {
        // If raw_content is available, use it for round-trip fidelity
        if !self.raw_content.is_empty() {
            write_raw_block(out, &self.raw_content, indent);
            return;
        }

        let inner = format!("{}  ", indent);

        out.push_str(&format!("{}<ITEM\n", indent));
        out.push_str(&format!("{}POSITION {}\n", inner, self.position));
        out.push_str(&format!("{}SNAPOFFS {}\n", inner, self.snap_offset));
        out.push_str(&format!("{}LENGTH {}\n", inner, self.length));
        out.push_str(&format!("{}LOOP {}\n", inner, b(self.loop_source)));
        out.push_str(&format!("{}ALLTAKES {}\n", inner, b(self.play_all_takes)));
        if let Some(color) = self.color {
            out.push_str(&format!("{}COLOR {}\n", inner, color));
        }
        if let Some(beat) = &self.beat {
            out.push_str(&format!("{}BEAT {}\n", inner, item_timebase_to_i32(beat)));
        }
        out.push_str(&format!("{}SEL {}\n", inner, b(self.selected)));

        if let Some(fi) = &self.fade_in {
            out.push_str(&format!(
                "{}FADEIN {} {} {} {} {} {} {}\n",
                inner, fade_curve_to_i32(&fi.curve_type), fi.time, fi.unknown_field_3,
                fi.unknown_field_4, fi.unknown_field_5, fi.unknown_field_6, fi.unknown_field_7
            ));
        }
        if let Some(fo) = &self.fade_out {
            out.push_str(&format!(
                "{}FADEOUT {} {} {} {} {} {} {}\n",
                inner, fade_curve_to_i32(&fo.curve_type), fo.time, fo.unknown_field_3,
                fo.unknown_field_4, fo.unknown_field_5, fo.unknown_field_6, fo.unknown_field_7
            ));
        }
        if let Some(m) = &self.mute {
            out.push_str(&format!(
                "{}MUTE {} {}\n",
                inner, b(m.muted), solo_state_to_i32(&m.solo_state)
            ));
        }
        if let Some(iguid) = &self.item_guid {
            out.push_str(&format!("{}IGUID {}\n", inner, iguid));
        }
        if let Some(iid) = self.item_id {
            out.push_str(&format!("{}IID {}\n", inner, iid));
        }
        if !self.name.is_empty() {
            out.push_str(&format!("{}NAME \"{}\"\n", inner, rpp_escape(&self.name)));
        }
        if let Some(vp) = &self.volpan {
            out.push_str(&format!(
                "{}VOLPAN {} {} {} {}\n",
                inner, vp.item_trim, vp.take_pan, vp.take_volume, vp.take_pan_law
            ));
        }
        if self.slip_offset != 0.0 {
            out.push_str(&format!("{}SOFFS {}\n", inner, self.slip_offset));
        }
        if let Some(pr) = &self.playrate {
            out.push_str(&format!(
                "{}PLAYRATE {} {} {} {} {} {}\n",
                inner, pr.rate, b(pr.preserve_pitch), pr.pitch_adjust,
                pitch_mode_to_i32(&pr.pitch_mode), pr.unknown_field_5, pr.unknown_field_6
            ));
        }
        if let Some(guid) = &self.take_guid {
            out.push_str(&format!("{}GUID {}\n", inner, guid));
        }
        if let Some(rp) = self.rec_pass {
            out.push_str(&format!("{}RECPASS {}\n", inner, rp));
        }

        // Stretch markers
        for sm in &self.stretch_markers {
            out.push_str(&format!("{}SM {} {}", inner, sm.position, sm.source_position));
            if let Some(rate) = sm.rate {
                out.push_str(&format!(" {}", rate));
            }
            out.push('\n');
        }

        // Takes
        for take in &self.takes {
            take.write_rpp(out, &inner);
        }

        out.push_str(&format!("{}>\n", indent));
    }
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

impl RppSerialize for Envelope {
    fn write_rpp(&self, out: &mut String, indent: &str) {
        let inner = format!("{}  ", indent);

        out.push_str(&format!("{}<{}\n", indent, self.envelope_type));
        out.push_str(&format!("{}EGUID {}\n", inner, self.guid));
        out.push_str(&format!("{}ACT {} -1\n", inner, b(self.active)));
        out.push_str(&format!("{}VIS {} {} 1\n", inner, b(self.visible), b(self.show_in_lane)));
        out.push_str(&format!("{}LANEHEIGHT {} 0\n", inner, self.lane_height));
        out.push_str(&format!("{}ARM {}\n", inner, b(self.armed)));
        out.push_str(&format!("{}DEFSHAPE {} -1 -1\n", inner, self.default_shape));

        for pt in &self.points {
            out.push_str(&format!("{}PT {:.6} {:.6} {}", inner, pt.position, pt.value, envelope_shape_to_i32(&pt.shape)));
            if let Some(ts) = pt.time_sig {
                out.push_str(&format!(" {}", ts));
            }
            if let Some(sel) = pt.selected {
                out.push_str(&format!(" {}", b(sel)));
            }
            if let Some(unk) = pt.unknown_field_6 {
                out.push_str(&format!(" {}", unk));
            }
            if let Some(tension) = pt.bezier_tension {
                out.push_str(&format!(" {}", tension));
            }
            out.push('\n');
        }

        for ai in &self.automation_items {
            out.push_str(&format!(
                "{}POOLEDENVINST {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}\n",
                inner, ai.pool_index, ai.position, ai.length, ai.start_offset,
                ai.play_rate, b(ai.selected), ai.baseline, ai.amplitude,
                b(ai.loop_enabled), ai.position_qn, ai.length_qn, ai.instance_index,
                b(ai.muted), ai.start_offset_qn, ai.transition_time, ai.volume_envelope_max
            ));
        }

        for ext in &self.extension_data {
            write_raw_block(out, &ext.raw_content, &inner);
        }

        out.push_str(&format!("{}>\n", indent));
    }
}

// ---------------------------------------------------------------------------
// Track
// ---------------------------------------------------------------------------

impl RppSerialize for Track {
    fn write_rpp(&self, out: &mut String, indent: &str) {
        // If raw_content is available and non-empty, prefer it for fidelity
        if !self.raw_content.is_empty() {
            write_raw_block(out, &self.raw_content, indent);
            return;
        }

        let inner = format!("{}  ", indent);

        out.push_str(&format!("{}<TRACK\n", indent));
        out.push_str(&format!("{}NAME \"{}\"\n", inner, rpp_escape(&self.name)));
        if let Some(id) = &self.track_id {
            out.push_str(&format!("{}TRACKID \"{}\"\n", inner, rpp_escape(id)));
        }
        if let Some(pc) = self.peak_color {
            out.push_str(&format!("{}PEAKCOL {}\n", inner, pc));
        }
        if let Some(beat) = self.beat {
            out.push_str(&format!("{}BEAT {}\n", inner, beat));
        }
        out.push_str(&format!("{}SEL {}\n", inner, b(self.selected)));
        if self.locked {
            out.push_str(&format!("{}LOCK {}\n", inner, b(self.locked)));
        }
        out.push_str(&format!("{}AUTOMODE {}\n", inner, automation_mode_to_i32(&self.automation_mode)));

        if let Some(vp) = &self.volpan {
            out.push_str(&format!(
                "{}VOLPAN {} {} {}\n",
                inner, vp.volume, vp.pan, vp.pan_law
            ));
        }
        if let Some(ms) = &self.mutesolo {
            out.push_str(&format!(
                "{}MUTESOLO {} {} {}\n",
                inner, b(ms.mute), track_solo_to_i32(&ms.solo), b(ms.solo_defeat)
            ));
        }
        if self.invert_phase {
            out.push_str(&format!("{}IPHASE {}\n", inner, b(self.invert_phase)));
        }
        if let Some(f) = &self.folder {
            out.push_str(&format!(
                "{}ISBUS {} {}\n",
                inner, folder_state_to_i32(&f.folder_state), f.indentation
            ));
        }
        if let Some(bc) = &self.bus_compact {
            out.push_str(&format!(
                "{}BUSCOMP {} {} {} {} {}\n",
                inner, bc.arrange_collapse, bc.mixer_collapse, bc.wiring_collapse,
                bc.wiring_x, bc.wiring_y
            ));
        }
        if let Some(sim) = &self.show_in_mixer {
            out.push_str(&format!(
                "{}SHOWINMIX {} {} {} {} {} {} {} {}\n",
                inner, b(sim.show_in_mixer), sim.unknown_field_2, sim.unknown_field_3,
                b(sim.show_in_track_list), sim.unknown_field_5, sim.unknown_field_6,
                sim.unknown_field_7, sim.unknown_field_8
            ));
        }
        if let Some(fm) = self.free_mode {
            out.push_str(&format!("{}FREEMODE {}\n", inner, free_mode_to_i32(&fm)));
        }
        if let Some(fl) = &self.fixed_lanes {
            out.push_str(&format!(
                "{}FIXEDLANES {} {} {} {} {}\n",
                inner, fl.bitfield, b(fl.allow_editing), b(fl.show_play_only_lane),
                b(fl.mask_playback), fl.recording_behavior
            ));
        }
        if let Some(ls) = &self.lane_solo {
            out.push_str(&format!(
                "{}LANESOLO {} {} {} {} {} {} {} {}\n",
                inner, ls.playing_lanes, ls.unknown_field_2, ls.unknown_field_3,
                ls.unknown_field_4, ls.unknown_field_5, ls.unknown_field_6,
                ls.unknown_field_7, ls.unknown_field_8
            ));
        }
        if let Some(lr) = &self.lane_record {
            out.push_str(&format!(
                "{}LANEREC {} {} {}\n",
                inner, lr.record_enabled_lane, lr.comping_enabled_lane, lr.last_comping_lane
            ));
        }
        if let Some(ln) = &self.lane_names {
            out.push_str(&format!("{}LANENAME {}", inner, ln.lane_count));
            for name in &ln.lane_names {
                out.push_str(&format!(" \"{}\"", rpp_escape(name)));
            }
            out.push('\n');
        }
        if let Some(rec) = &self.record {
            out.push_str(&format!(
                "{}REC {} {} {} {} {} {} {}\n",
                inner, b(rec.armed), rec.input, monitor_mode_to_i32(&rec.monitor),
                record_mode_to_i32(&rec.record_mode), b(rec.monitor_track_media),
                b(rec.preserve_pdc_delayed), rec.record_path
            ));
        }
        if let Some(th) = &self.track_height {
            out.push_str(&format!(
                "{}TRACKHEIGHT {} {}\n",
                inner, th.height, b(th.folder_override)
            ));
        }
        if let Some(iq) = &self.input_quantize {
            out.push_str(&format!(
                "{}INQ {} {} {} {} {} {} {} {}\n",
                inner, b(iq.quantize_midi), iq.quantize_to_pos, b(iq.quantize_note_offs),
                iq.quantize_to, iq.quantize_strength, iq.swing_strength,
                iq.quantize_range_min, iq.quantize_range_max
            ));
        }
        out.push_str(&format!("{}NCHAN {}\n", inner, self.channel_count));
        if let Some(cfg) = &self.rec_cfg {
            out.push_str(&format!("{}RECCFG {}\n", inner, cfg));
        }
        if let Some(mcm) = &self.midi_color_map_fn {
            out.push_str(&format!("{}MIDICOLORMAPFN \"{}\"\n", inner, rpp_escape(mcm)));
        }
        out.push_str(&format!("{}FX {}\n", inner, b(self.fx_enabled)));
        if let Some(perf) = self.perf {
            out.push_str(&format!("{}PERF {}\n", inner, perf));
        }
        if let Some((tcp, mcp)) = &self.layouts {
            out.push_str(&format!(
                "{}LAYOUTS \"{}\" \"{}\"\n",
                inner, rpp_escape(tcp), rpp_escape(mcp)
            ));
        }

        // Master send
        if let Some(ms) = &self.master_send {
            out.push_str(&format!(
                "{}MAINSEND {} {}\n",
                inner, b(ms.enabled), ms.unknown_field_2
            ));
        }

        // Receives
        for recv in &self.receives {
            out.push_str(&format!(
                "{}AUXRECV {} {} {} {} {} {} {} {} {} {} {} {}\n",
                inner, recv.source_track_index, recv.mode, recv.volume, recv.pan,
                b(recv.mute), b(recv.mono_sum), b(recv.invert_polarity),
                recv.source_audio_channels, recv.dest_audio_channels, recv.pan_law,
                recv.midi_channels, recv.automation_mode
            ));
        }

        // Hardware outputs
        for hw in &self.hardware_outputs {
            out.push_str(&format!(
                "{}HWOUT {} {} {} {} {} {} {} {} {}\n",
                inner, hw.output_index, hw.send_mode, hw.volume, hw.pan,
                b(hw.mute), b(hw.invert_polarity), hw.send_source_channel,
                hw.unknown_field_8, hw.automation_mode
            ));
        }

        // MIDI output
        if let Some(mo) = &self.midi_output {
            out.push_str(&format!(
                "{}MIDIOUT {} {}\n",
                inner, mo.device, mo.channel
            ));
        }

        // MIDI note names
        for nn in &self.midi_note_names {
            out.push_str(&format!(
                "{}MIDINOTENAMES {} {} \"{}\" {} {}\n",
                inner, nn.channel, nn.note_number, rpp_escape(&nn.note_name),
                nn.unknown_field_4, nn.note_number_2
            ));
        }

        // Extension data
        for (key, value) in &self.extension_data {
            out.push_str(&format!("{}EXT {} {}\n", inner, key, value));
        }

        // Input FX chain
        if let Some(ifx) = &self.input_fx {
            // Write as FXCHAIN_REC instead of FXCHAIN
            out.push_str(&format!("{}<FXCHAIN_REC\n", inner));
            let ifx_inner = format!("{}  ", inner);
            if let Some(rect) = &ifx.window_rect {
                out.push_str(&format!(
                    "{}WNDRECT {} {} {} {}\n",
                    ifx_inner, rect[0], rect[1], rect[2], rect[3]
                ));
            }
            out.push_str(&format!("{}SHOW {}\n", ifx_inner, ifx.show));
            out.push_str(&format!("{}LASTSEL {}\n", ifx_inner, ifx.last_sel));
            out.push_str(&format!("{}DOCKED {}\n", ifx_inner, b(ifx.docked)));
            for node in &ifx.nodes {
                node.write_rpp(out, &ifx_inner);
            }
            out.push_str(&format!("{}>\n", inner));
        }

        // FX chain
        if let Some(fx) = &self.fx_chain {
            fx.write_rpp(out, &inner);
        }

        // Envelopes
        for env in &self.envelopes {
            env.write_rpp(out, &inner);
        }

        // Items
        for item in &self.items {
            item.write_rpp(out, &inner);
        }

        out.push_str(&format!("{}>\n", indent));
    }
}

// ---------------------------------------------------------------------------
// MarkerRegion
// ---------------------------------------------------------------------------

impl MarkerRegion {
    /// Write this marker/region as RPP MARKER line(s).
    ///
    /// Regions emit two MARKER lines: the start (with name) and the end (with "").
    pub fn write_marker_line(&self, out: &mut String, indent: &str) {
        // MARKER id position name flags color locked B guid additional [lane]
        out.push_str(&format!(
            "{}MARKER {} {} \"{}\" {} {} {} B {} {}",
            indent,
            self.id,
            self.position,
            rpp_escape(&self.name),
            self.flags,
            self.color,
            self.locked,
            self.guid,
            self.additional,
        ));
        if let Some(lane) = self.lane {
            out.push_str(&format!(" {}", lane));
        }
        out.push('\n');

        // Regions emit a second MARKER line for the end position
        if let Some(end) = self.end_position {
            out.push_str(&format!(
                "{}MARKER {} {} \"\" {}\n",
                indent, self.id, end, self.flags
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// TempoTimeEnvelope
// ---------------------------------------------------------------------------

impl RppSerialize for TempoTimeEnvelope {
    fn write_rpp(&self, out: &mut String, indent: &str) {
        if self.points.is_empty() {
            return;
        }

        let inner = format!("{}  ", indent);

        out.push_str(&format!("{}<TEMPOENVEX\n", indent));
        out.push_str(&format!("{}ACT 1 -1\n", inner));
        out.push_str(&format!("{}VIS 1 0 1\n", inner));
        out.push_str(&format!("{}LANEHEIGHT 0 0\n", inner));
        out.push_str(&format!("{}ARM 0\n", inner));
        out.push_str(&format!("{}DEFSHAPE 0 -1 -1\n", inner));

        for pt in &self.points {
            out.push_str(&format!(
                "{}PT {:.12} {:.10} {}",
                inner, pt.position, pt.tempo, pt.shape
            ));
            if let Some(ts) = pt.time_signature_encoded {
                out.push_str(&format!(" {}", ts));
                // When time signature is present, emit the full PT line
                out.push_str(&format!(
                    " {} {} {} \"{}\" {} {} {}",
                    b(pt.selected),
                    pt.unknown1,
                    pt.bezier_tension,
                    rpp_escape(&pt.metronome_pattern),
                    pt.unknown2,
                    pt.unknown3,
                    pt.unknown4,
                ));
            }
            out.push('\n');
        }

        out.push_str(&format!("{}>\n", indent));
    }
}

// ---------------------------------------------------------------------------
// ReaperProject
// ---------------------------------------------------------------------------

impl RppSerialize for ReaperProject {
    fn write_rpp(&self, out: &mut String, indent: &str) {
        let inner = format!("{}  ", indent);

        out.push_str(&format!(
            "{}<REAPER_PROJECT {} \"{}\" {}\n",
            indent, self.version, rpp_escape(&self.version_string), self.timestamp
        ));

        // Minimal project properties
        if let Some((r1, r2)) = self.properties.ripple {
            out.push_str(&format!("{}RIPPLE {} {}\n", inner, r1, r2));
        } else {
            out.push_str(&format!("{}RIPPLE 0\n", inner));
        }

        if let Some(sr) = self.properties.sample_rate {
            out.push_str(&format!("{}SAMPLERATE {} {} {}\n", inner, sr.0, sr.1, sr.2));
        }
        if let Some(tempo) = self.properties.tempo {
            out.push_str(&format!(
                "{}TEMPO {} {} {} {}\n",
                inner, tempo.0, tempo.1, tempo.2, tempo.3
            ));
        }

        // Tempo envelope
        if let Some(ref tempo_env) = self.tempo_envelope {
            tempo_env.write_rpp(out, &inner);
        }

        // Markers and regions
        for marker in &self.markers_regions.all {
            marker.write_marker_line(out, &inner);
        }

        // Tracks
        for track in &self.tracks {
            track.write_rpp(out, &inner);
        }

        out.push_str(&format!("{}>\n", indent));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::fx_chain::{FxChain, FxChainNode, FxPlugin, PluginType};
    use crate::types::track::{
        FolderSettings, FolderState, MuteSoloSettings, TrackSoloState, VolPanSettings,
    };

    #[test]
    fn test_track_basic_roundtrip() {
        let track = Track {
            name: "Guitar".to_string(),
            volpan: Some(VolPanSettings {
                volume: 0.8,
                pan: -0.25,
                pan_law: -1.0,
            }),
            mutesolo: Some(MuteSoloSettings {
                mute: false,
                solo: TrackSoloState::NoSolo,
                solo_defeat: false,
            }),
            channel_count: 2,
            fx_enabled: true,
            ..Track::default()
        };

        let rpp = track.to_rpp_string();
        assert!(rpp.starts_with("<TRACK\n"));
        assert!(rpp.contains("NAME \"Guitar\""));
        assert!(rpp.contains("VOLPAN 0.8 -0.25 -1"));
        assert!(rpp.contains("MUTESOLO 0 0 0"));
        assert!(rpp.contains("NCHAN 2"));
        assert!(rpp.contains("FX 1"));
        assert!(rpp.ends_with(">\n"));
    }

    #[test]
    fn test_track_with_folder() {
        let track = Track {
            name: "Drums".to_string(),
            folder: Some(FolderSettings {
                folder_state: FolderState::FolderParent,
                indentation: 1,
            }),
            ..Track::default()
        };

        let rpp = track.to_rpp_string();
        assert!(rpp.contains("ISBUS 1 1"));
    }

    #[test]
    fn test_track_with_fx_chain() {
        let track = Track {
            name: "Lead".to_string(),
            fx_chain: Some(FxChain {
                window_rect: None,
                show: 0,
                last_sel: 0,
                docked: false,
                nodes: vec![FxChainNode::Plugin(FxPlugin {
                    name: "VST: ReaEQ (Cockos)".to_string(),
                    custom_name: None,
                    plugin_type: PluginType::Vst,
                    file: "reaeq.dll".to_string(),
                    bypassed: false,
                    offline: false,
                    fxid: Some("{EQ-GUID}".to_string()),
                    preset_name: None,
                    float_pos: None,
                    wak: None,
                    parallel: false,
                    state_data: vec!["ZXE=".to_string()],
                    raw_block: String::new(),
                    param_envelopes: vec![],
                    params_on_tcp: vec![],
                })],
                raw_content: String::new(),
            }),
            fx_enabled: true,
            ..Track::default()
        };

        let rpp = track.to_rpp_string();
        assert!(rpp.contains("<FXCHAIN"));
        assert!(rpp.contains("ReaEQ"));
        assert!(rpp.contains("FXID {EQ-GUID}"));
    }

    #[test]
    fn test_item_with_wave_source() {
        let item = Item {
            position: 1.0,
            length: 4.0,
            name: "Kick Pattern".to_string(),
            takes: vec![Take {
                is_selected: true,
                name: "kick.wav".to_string(),
                source: Some(SourceBlock {
                    source_type: SourceType::Wave,
                    file_path: "audio/kick.wav".to_string(),
                    midi_data: None,
                    raw_content: String::new(),
                }),
                ..Take::default()
            }],
            ..Item::default()
        };

        let rpp = item.to_rpp_string();
        assert!(rpp.starts_with("<ITEM\n"));
        assert!(rpp.contains("POSITION 1"));
        assert!(rpp.contains("LENGTH 4"));
        assert!(rpp.contains("NAME \"Kick Pattern\""));
        assert!(rpp.contains("TAKE SEL"));
        assert!(rpp.contains("<SOURCE WAVE"));
        assert!(rpp.contains("FILE \"audio/kick.wav\""));
    }

    #[test]
    fn test_track_parse_serialize_consistency() {
        // Build a track, serialize, parse back, verify key fields
        let track = Track {
            name: "Bass".to_string(),
            track_id: Some("{BASS-GUID}".to_string()),
            selected: true,
            volpan: Some(VolPanSettings {
                volume: 0.7,
                pan: 0.0,
                pan_law: -1.0,
            }),
            mutesolo: Some(MuteSoloSettings {
                mute: true,
                solo: TrackSoloState::NoSolo,
                solo_defeat: false,
            }),
            channel_count: 2,
            fx_enabled: true,
            ..Track::default()
        };

        let rpp = track.to_rpp_string();
        let parsed = Track::from_rpp_block(&rpp).expect("should parse serialized track");
        assert_eq!(parsed.name, "Bass");
        assert_eq!(parsed.track_id.as_deref(), Some("{BASS-GUID}"));
        assert!(parsed.selected);
        assert!(parsed.mutesolo.as_ref().unwrap().mute);
    }

    #[test]
    fn test_project_serialize() {
        let project = ReaperProject {
            version: 0.1,
            version_string: "7.0/x64".to_string(),
            timestamp: 12345,
            properties: super::super::project::ProjectProperties::new(),
            tracks: vec![
                Track {
                    name: "Track 1".to_string(),
                    ..Track::default()
                },
                Track {
                    name: "Track 2".to_string(),
                    ..Track::default()
                },
            ],
            items: vec![],
            envelopes: vec![],
            fx_chains: vec![],
            markers_regions: Default::default(),
            tempo_envelope: None,
            ruler_lanes: vec![],
            ruler_height: None,
        };

        let rpp = project.to_rpp_string();
        assert!(rpp.starts_with("<REAPER_PROJECT 0.1"));
        assert!(rpp.contains("RIPPLE 0"));
        assert!(rpp.contains("<TRACK"));
        assert!(rpp.contains("NAME \"Track 1\""));
        assert!(rpp.contains("NAME \"Track 2\""));
    }

    #[test]
    fn test_indentation_nesting() {
        // Verify that nested structures get progressively deeper indentation
        let track = Track {
            name: "Test".to_string(),
            items: vec![Item {
                position: 0.0,
                length: 1.0,
                takes: vec![Take {
                    is_selected: true,
                    name: "take1".to_string(),
                    source: Some(SourceBlock {
                        source_type: SourceType::Wave,
                        file_path: "test.wav".to_string(),
                        midi_data: None,
                        raw_content: String::new(),
                    }),
                    ..Take::default()
                }],
                ..Item::default()
            }],
            ..Track::default()
        };

        let rpp = track.to_rpp_string();
        // Track at indent 0, item at indent 1, source at indent 2
        assert!(rpp.contains("<TRACK\n"));
        assert!(rpp.contains("  <ITEM\n"));
        assert!(rpp.contains("    TAKE SEL\n"));
        assert!(rpp.contains("    <SOURCE WAVE\n"));
        assert!(rpp.contains("      FILE \"test.wav\"\n"));
    }
}
