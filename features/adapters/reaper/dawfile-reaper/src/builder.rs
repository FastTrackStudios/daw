//! Fluent builder API for constructing REAPER projects programmatically.
//!
//! Provides a convenient, composable API for building RPP structures
//! without manually constructing deeply nested types. Designed for
//! test fixtures, project generation, and programmatic DAW workflows.
//!
//! # Example
//!
//! ```
//! use dawfile_reaper::builder::ReaperProjectBuilder;
//! use dawfile_reaper::RppSerialize;
//!
//! let project = ReaperProjectBuilder::new()
//!     .tempo(120.0)
//!     .sample_rate(48000)
//!     .track("Drums", |t| t
//!         .color(0x112233)
//!         .folder_start()
//!     )
//!     .track("Kick", |t| t
//!         .item(0.0, 4.0, |i| i
//!             .name("Kick Pattern")
//!             .source_wave("kick.wav")
//!         )
//!     )
//!     .track("Snare", |t| t
//!         .item(0.0, 4.0, |i| i.source_wave("snare.wav"))
//!         .folder_end(1)
//!     )
//!     .build();
//!
//! let rpp_text = project.to_rpp_string();
//! assert!(rpp_text.contains("Drums"));
//! ```

use crate::stock_fx::StockFx;
use crate::types::envelope::{Envelope, EnvelopePoint, EnvelopePointShape};
use crate::types::fx_chain::{FxChain, FxChainNode, FxPlugin, PluginType};
use crate::types::item::{
    ChannelMode, FadeCurveType, FadeSettings, Item, MidiEvent, MidiSource, MidiSourceEvent,
    PitchMode, PlayRateSettings, SourceBlock, SourceType, StretchMarker, Take,
};
use crate::types::marker_region::{MarkerRegion, MarkerRegionCollection};
use crate::types::project::{ProjectProperties, ReaperProject};
use crate::types::time_tempo::{TempoTimeEnvelope, TempoTimePoint};
use crate::types::track::{
    FolderSettings, FolderState, MuteSoloSettings, RecordSettings, Track, TrackSoloState,
    VolPanSettings,
};

// ===========================================================================
// FxBuilder
// ===========================================================================

/// Builder for constructing FX plugins within an FX chain.
pub struct FxBuilder {
    name: String,
    plugin_type: PluginType,
    file: String,
    bypassed: bool,
    offline: bool,
    fxid: Option<String>,
    preset_name: Option<String>,
    state_data: Vec<String>,
    raw_block: String,
    wak: Option<[i32; 2]>,
    float_pos: Option<[i32; 4]>,
}

impl FxBuilder {
    fn new(name: impl Into<String>, plugin_type: PluginType, file: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            plugin_type,
            file: file.into(),
            bypassed: false,
            offline: false,
            fxid: None,
            preset_name: None,
            state_data: vec![],
            raw_block: String::new(),
            wak: None,
            float_pos: None,
        }
    }

    /// Set the `WAK` line values (REAPER's wet/auto-bypass flags). REAPER
    /// writes `WAK 0 0` for a normal active FX.
    pub fn wak(mut self, a: i32, b: i32) -> Self {
        self.wak = Some([a, b]);
        self
    }

    /// Set the `FLOATPOS` rectangle (floating FX window position).
    pub fn float_pos(mut self, pos: [i32; 4]) -> Self {
        self.float_pos = Some(pos);
        self
    }

    /// Set the complete raw plugin block (`<VST …> … >`), used for verbatim
    /// state transplant (e.g. a converted Omnisphere chunk). When set, it is
    /// emitted as-is by the serializer instead of a synthesized header.
    pub fn raw_block(mut self, block: impl Into<String>) -> Self {
        self.raw_block = block.into();
        self
    }

    /// Mark this FX as bypassed.
    pub fn bypassed(mut self) -> Self {
        self.bypassed = true;
        self
    }

    /// Mark this FX as offline.
    pub fn offline(mut self) -> Self {
        self.offline = true;
        self
    }

    /// Set the FX GUID.
    pub fn fxid(mut self, guid: impl Into<String>) -> Self {
        self.fxid = Some(guid.into());
        self
    }

    /// Set the preset name.
    pub fn preset(mut self, name: impl Into<String>) -> Self {
        self.preset_name = Some(name.into());
        self
    }

    /// Add base64-encoded state data lines.
    pub fn state(mut self, data: impl Into<String>) -> Self {
        self.state_data.push(data.into());
        self
    }

    fn build(self) -> FxPlugin {
        FxPlugin {
            name: self.name,
            custom_name: None,
            plugin_type: self.plugin_type,
            file: self.file,
            bypassed: self.bypassed,
            offline: self.offline,
            fxid: self.fxid,
            preset_name: self.preset_name,
            float_pos: self.float_pos,
            wak: self.wak,
            parallel: false,
            state_data: self.state_data,
            raw_block: self.raw_block,
            param_envelopes: vec![],
            params_on_tcp: vec![],
            header_extra: None,
        }
    }
}

// ===========================================================================
// MidiSourceBuilder
// ===========================================================================

/// Builder for constructing MIDI source data within a take.
///
/// Generates REAPER's internal MIDI format: hex-encoded `E` lines with
/// delta-tick timing. No external MIDI crate needed — REAPER uses its own
/// simple text-based encoding.
///
/// # Example
///
/// ```
/// use dawfile_reaper::builder::MidiSourceBuilder;
///
/// let midi = MidiSourceBuilder::new()
///     .ticks_per_qn(960)
///     .note(0, 0, 60, 96, 480)   // C4, velocity 96, half note
///     .note(0, 0, 64, 96, 480)   // E4 at same time
///     .cc(0, 0, 1, 64)           // Mod wheel to 64
///     .build();
///
/// assert_eq!(midi.ticks_per_qn, 960);
/// assert!(!midi.events.is_empty());
/// ```
pub struct MidiSourceBuilder {
    ticks_per_qn: u32,
    /// Events stored as (absolute_tick, MidiEvent) for sorting before build.
    events: Vec<(u64, MidiEvent)>,
    /// Deferred note-offs: (absolute_tick_off, channel, note, velocity)
    pending_note_offs: Vec<(u64, u8, u8, u8)>,
    /// Current absolute tick position for sequential event building.
    cursor: u64,
}

impl MidiSourceBuilder {
    /// Create a new MIDI source builder with default 960 ticks per quarter note.
    pub fn new() -> Self {
        Self {
            ticks_per_qn: 960,
            events: vec![],
            pending_note_offs: vec![],
            cursor: 0,
        }
    }

    /// Set ticks per quarter note (default 960).
    pub fn ticks_per_qn(mut self, tpq: u32) -> Self {
        self.ticks_per_qn = tpq;
        self
    }

    /// Add a complete note (note-on + note-off pair) at the current cursor.
    ///
    /// - `delta_ticks`: offset from current cursor position
    /// - `channel`: MIDI channel (0-15)
    /// - `note`: MIDI note number (0-127)
    /// - `velocity`: Note-on velocity (1-127)
    /// - `duration_ticks`: Length of the note in ticks
    pub fn note(
        mut self,
        delta_ticks: u32,
        channel: u8,
        note: u8,
        velocity: u8,
        duration_ticks: u32,
    ) -> Self {
        let abs_tick = self.cursor + delta_ticks as u64;
        self.events.push((
            abs_tick,
            MidiEvent {
                delta_ticks: 0, // will be computed in build()
                bytes: vec![0x90 | (channel & 0x0F), note & 0x7F, velocity & 0x7F],
            },
        ));
        self.pending_note_offs
            .push((abs_tick + duration_ticks as u64, channel, note, 0));
        self.cursor = abs_tick;
        self
    }

    /// Add a raw note-on event.
    pub fn note_on(mut self, delta_ticks: u32, channel: u8, note: u8, velocity: u8) -> Self {
        let abs_tick = self.cursor + delta_ticks as u64;
        self.events.push((
            abs_tick,
            MidiEvent {
                delta_ticks: 0,
                bytes: vec![0x90 | (channel & 0x0F), note & 0x7F, velocity & 0x7F],
            },
        ));
        self.cursor = abs_tick;
        self
    }

    /// Add a raw note-off event.
    pub fn note_off(mut self, delta_ticks: u32, channel: u8, note: u8, velocity: u8) -> Self {
        let abs_tick = self.cursor + delta_ticks as u64;
        self.events.push((
            abs_tick,
            MidiEvent {
                delta_ticks: 0,
                bytes: vec![0x80 | (channel & 0x0F), note & 0x7F, velocity & 0x7F],
            },
        ));
        self.cursor = abs_tick;
        self
    }

    /// Add a Control Change event.
    pub fn cc(mut self, delta_ticks: u32, channel: u8, controller: u8, value: u8) -> Self {
        let abs_tick = self.cursor + delta_ticks as u64;
        self.events.push((
            abs_tick,
            MidiEvent {
                delta_ticks: 0,
                bytes: vec![0xB0 | (channel & 0x0F), controller & 0x7F, value & 0x7F],
            },
        ));
        self.cursor = abs_tick;
        self
    }

    /// Add a Program Change event.
    pub fn program_change(mut self, delta_ticks: u32, channel: u8, program: u8) -> Self {
        let abs_tick = self.cursor + delta_ticks as u64;
        self.events.push((
            abs_tick,
            MidiEvent {
                delta_ticks: 0,
                bytes: vec![0xC0 | (channel & 0x0F), program & 0x7F],
            },
        ));
        self.cursor = abs_tick;
        self
    }

    /// Add a Pitch Bend event. `value` is 14-bit (0-16383, 8192 = center).
    pub fn pitch_bend(mut self, delta_ticks: u32, channel: u8, value: u16) -> Self {
        let abs_tick = self.cursor + delta_ticks as u64;
        let lsb = (value & 0x7F) as u8;
        let msb = ((value >> 7) & 0x7F) as u8;
        self.events.push((
            abs_tick,
            MidiEvent {
                delta_ticks: 0,
                bytes: vec![0xE0 | (channel & 0x0F), lsb, msb],
            },
        ));
        self.cursor = abs_tick;
        self
    }

    /// Add a Channel Pressure (aftertouch) event.
    pub fn channel_pressure(mut self, delta_ticks: u32, channel: u8, pressure: u8) -> Self {
        let abs_tick = self.cursor + delta_ticks as u64;
        self.events.push((
            abs_tick,
            MidiEvent {
                delta_ticks: 0,
                bytes: vec![0xD0 | (channel & 0x0F), pressure & 0x7F],
            },
        ));
        self.cursor = abs_tick;
        self
    }

    /// Add a Polyphonic Aftertouch event.
    pub fn aftertouch(mut self, delta_ticks: u32, channel: u8, note: u8, pressure: u8) -> Self {
        let abs_tick = self.cursor + delta_ticks as u64;
        self.events.push((
            abs_tick,
            MidiEvent {
                delta_ticks: 0,
                bytes: vec![0xA0 | (channel & 0x0F), note & 0x7F, pressure & 0x7F],
            },
        ));
        self.cursor = abs_tick;
        self
    }

    /// Move the cursor to an absolute tick position.
    pub fn at(mut self, absolute_tick: u64) -> Self {
        self.cursor = absolute_tick;
        self
    }

    /// Advance the cursor by `ticks` without adding an event.
    pub fn advance(mut self, ticks: u32) -> Self {
        self.cursor += ticks as u64;
        self
    }

    /// Build the `MidiSource`, sorting events by absolute tick and computing deltas.
    pub fn build(mut self) -> MidiSource {
        // Merge pending note-offs into events
        for (abs_tick, ch, note, vel) in self.pending_note_offs.drain(..) {
            self.events.push((
                abs_tick,
                MidiEvent {
                    delta_ticks: 0,
                    bytes: vec![0x80 | (ch & 0x0F), note & 0x7F, vel & 0x7F],
                },
            ));
        }

        // By absolute tick, and at equal ticks note-offs BEFORE
        // note-ons.
        //
        // That second rule is not cosmetic. A reader pairs a note-on to
        // the next note-off of the same pitch, so a chord held exactly
        // to the bar line where the next one begins emits
        //
        //     on  C @ 3840   (the next chord)
        //     off C @ 3840   (the one that just ended)
        //
        // and the reader hands the SECOND chord the FIRST one's
        // note-off and calls it zero length — from MIDI that is correct
        // in every other respect. Offs first makes the pairing
        // unambiguous, and it is what every DAW writes. (daw#28.)
        //
        // Stable, so events that are neither keep the order they were
        // added in.
        self.events
            .sort_by_key(|(tick, event)| (*tick, u8::from(!is_note_off(event))));

        // Convert absolute ticks to delta ticks
        let mut last_tick: u64 = 0;
        let mut events = Vec::with_capacity(self.events.len());
        let mut event_stream = Vec::with_capacity(self.events.len());

        for (abs_tick, mut event) in self.events.drain(..) {
            let delta = (abs_tick - last_tick) as u32;
            event.delta_ticks = delta;
            last_tick = abs_tick;
            event_stream.push(MidiSourceEvent::Midi(event.clone()));
            events.push(event);
        }

        MidiSource {
            has_data: !events.is_empty(),
            ticks_per_qn: self.ticks_per_qn,
            ticks_timebase: Some("QN".to_string()),
            cc_interp: None,
            pooled_evts_guid: None,
            events,
            extended_events: vec![],
            event_stream,
            ignore_tempo: None,
            vel_lanes: vec![],
            bank_program_file: None,
            cfg_edit_view: None,
            cfg_edit: None,
            evt_filter: None,
            guid: None,
            unknown_lines: vec![],
        }
    }
}

impl Default for MidiSourceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// EnvelopeBuilder
// ===========================================================================

/// Builder for constructing track envelopes (volume, pan, etc.).
///
/// # Example
///
/// ```
/// use dawfile_reaper::builder::EnvelopeBuilder;
///
/// let env = EnvelopeBuilder::new("VOLENV2")
///     .active()
///     .visible()
///     .linear(0.0, 1.0)
///     .bezier(2.0, 0.5, -0.3)
///     .linear(4.0, 1.0)
///     .build();
///
/// assert_eq!(env.points.len(), 3);
/// assert!(env.active);
/// ```
pub struct EnvelopeBuilder {
    envelope: Envelope,
}

impl EnvelopeBuilder {
    /// Create a new envelope builder for the given type (e.g. "VOLENV2", "PANENV2").
    pub fn new(envelope_type: impl Into<String>) -> Self {
        Self {
            envelope: Envelope {
                envelope_type: envelope_type.into(),
                guid: String::new(),
                active: false,
                visible: false,
                show_in_lane: false,
                lane_height: 0,
                armed: false,
                default_shape: 0,
                points: vec![],
                automation_items: vec![],
                extension_data: vec![],
            },
        }
    }

    /// Set the envelope GUID.
    pub fn guid(mut self, guid: impl Into<String>) -> Self {
        self.envelope.guid = guid.into();
        self
    }

    /// Mark the envelope as active.
    pub fn active(mut self) -> Self {
        self.envelope.active = true;
        self
    }

    /// Mark the envelope as visible.
    pub fn visible(mut self) -> Self {
        self.envelope.visible = true;
        self
    }

    /// Show the envelope in its own lane.
    pub fn show_in_lane(mut self) -> Self {
        self.envelope.show_in_lane = true;
        self
    }

    /// Set the lane height in pixels.
    pub fn lane_height(mut self, height: i32) -> Self {
        self.envelope.lane_height = height;
        self
    }

    /// Arm the envelope for recording.
    pub fn armed(mut self) -> Self {
        self.envelope.armed = true;
        self
    }

    /// Set the default point shape.
    pub fn default_shape(mut self, shape: EnvelopePointShape) -> Self {
        self.envelope.default_shape = shape as i32;
        self
    }

    /// Add a point with a specific shape.
    pub fn point(mut self, position: f64, value: f64, shape: EnvelopePointShape) -> Self {
        self.envelope.points.push(EnvelopePoint {
            position,
            value,
            shape,
            time_sig: None,
            selected: None,
            unknown_field_6: None,
            bezier_tension: None,
        });
        self
    }

    /// Add a linear-interpolated point.
    pub fn linear(self, position: f64, value: f64) -> Self {
        self.point(position, value, EnvelopePointShape::Linear)
    }

    /// Add a square (step) point — value jumps instantly.
    pub fn square(self, position: f64, value: f64) -> Self {
        self.point(position, value, EnvelopePointShape::Square)
    }

    /// Add a bezier curve point with tension (-1.0 to 1.0).
    pub fn bezier(mut self, position: f64, value: f64, tension: f64) -> Self {
        self.envelope.points.push(EnvelopePoint {
            position,
            value,
            shape: EnvelopePointShape::Bezier,
            time_sig: None,
            selected: None,
            unknown_field_6: None,
            bezier_tension: Some(tension),
        });
        self
    }

    /// Add a slow-start/end curve point.
    pub fn slow_start_end(self, position: f64, value: f64) -> Self {
        self.point(position, value, EnvelopePointShape::SlowStartEnd)
    }

    /// Add a fast-start curve point.
    pub fn fast_start(self, position: f64, value: f64) -> Self {
        self.point(position, value, EnvelopePointShape::FastStart)
    }

    /// Add a fast-end curve point.
    pub fn fast_end(self, position: f64, value: f64) -> Self {
        self.point(position, value, EnvelopePointShape::FastEnd)
    }

    /// Build the final `Envelope`.
    pub fn build(self) -> Envelope {
        self.envelope
    }
}

// ===========================================================================
// ItemBuilder
// ===========================================================================

/// Builder for constructing media items within a track.
pub struct ItemBuilder {
    item: Item,
    takes: Vec<Take>,
}

impl ItemBuilder {
    fn new(position: f64, length: f64) -> Self {
        Self {
            item: Item {
                position,
                length,
                ..Item::default()
            },
            takes: vec![],
        }
    }

    /// Set the item name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.item.name = name.into();
        self
    }

    /// Assign this item to a fixed lane index, REAPER 7+ comping.
    ///
    /// REAPER has no per-item `LANE` token — the lane lives in the item's
    /// vertical geometry, `YPOS <lane/n> <1/n> 2`, and is derived back from
    /// it on read. The lane *count* is not known until the track's
    /// `LANENAME` list is set, so this records the index and
    /// [`TrackBuilder::build`] resolves it into the `YPOS` line.
    pub fn fixed_lane(mut self, lane: i32) -> Self {
        self.item.lane = Some(lane);
        self
    }

    /// Set the item's clip gain (item trim, linear; 1.0 = 0 dB) via VOLPAN.
    pub fn gain(mut self, item_trim: f64) -> Self {
        let vp = self
            .item
            .volpan
            .get_or_insert(crate::types::item::VolPanSettings {
                item_trim: 1.0,
                take_pan: 0.0,
                take_volume: 1.0,
                take_pan_law: -1.0,
            });
        vp.item_trim = item_trim;
        self
    }

    /// Mark this item as selected.
    pub fn selected(mut self) -> Self {
        self.item.selected = true;
        self
    }

    /// Mark this item as muted.
    pub fn muted(mut self) -> Self {
        self.item.mute = Some(crate::types::item::MuteSettings {
            muted: true,
            solo_state: crate::types::item::SoloState::NotSoloed,
        });
        self
    }

    /// Set the item GUID.
    pub fn guid(mut self, guid: impl Into<String>) -> Self {
        self.item.item_guid = Some(guid.into());
        self
    }

    /// Set item color.
    pub fn color(mut self, color: i32) -> Self {
        self.item.color = Some(color);
        self
    }

    /// Enable loop source.
    pub fn looped(mut self) -> Self {
        self.item.loop_source = true;
        self
    }

    /// Set snap offset.
    pub fn snap_offset(mut self, offset: f64) -> Self {
        self.item.snap_offset = offset;
        self
    }

    /// Add a WAVE source take (most common case).
    pub fn source_wave(self, file_path: impl Into<String>) -> Self {
        self.take(file_path, SourceType::Wave)
    }

    /// Set the play rate (1.0 = normal speed).
    pub fn playrate(mut self, rate: f64) -> Self {
        let pr = self.item.playrate.get_or_insert(PlayRateSettings {
            rate: 1.0,
            preserve_pitch: false,
            pitch_adjust: 0.0,
            pitch_mode: PitchMode::ProjectDefault,
            unknown_field_5: 0,
            unknown_field_6: 0.0,
        });
        pr.rate = rate;
        self
    }

    /// Set pitch adjustment in semitones (enables preserve-pitch).
    pub fn pitch(mut self, semitones: f64) -> Self {
        let pr = self.item.playrate.get_or_insert(PlayRateSettings {
            rate: 1.0,
            preserve_pitch: false,
            pitch_adjust: 0.0,
            pitch_mode: PitchMode::ProjectDefault,
            unknown_field_5: 0,
            unknown_field_6: 0.0,
        });
        pr.pitch_adjust = semitones;
        pr.preserve_pitch = true;
        self
    }

    /// Set the pitch/time-stretch mode.
    pub fn pitch_mode(mut self, mode: PitchMode) -> Self {
        let pr = self.item.playrate.get_or_insert(PlayRateSettings {
            rate: 1.0,
            preserve_pitch: false,
            pitch_adjust: 0.0,
            pitch_mode: PitchMode::ProjectDefault,
            unknown_field_5: 0,
            unknown_field_6: 0.0,
        });
        pr.pitch_mode = mode;
        self
    }

    /// Set the channel mode.
    pub fn channel_mode(mut self, mode: ChannelMode) -> Self {
        self.item.channel_mode = mode;
        self
    }

    /// Set the slip offset (source start offset in seconds).
    pub fn slip_offset(mut self, offset: f64) -> Self {
        self.item.slip_offset = offset;
        self
    }

    /// Set fade-in time with a curve type.
    pub fn fade_in(mut self, time: f64, curve: FadeCurveType) -> Self {
        self.item.fade_in = Some(FadeSettings {
            curve_type: curve,
            time,
            unknown_field_3: 0.0,
            unknown_field_4: 1,
            unknown_field_5: 0,
            unknown_field_6: 0,
            unknown_field_7: 0,
        });
        self
    }

    /// Set fade-out time with a curve type.
    pub fn fade_out(mut self, time: f64, curve: FadeCurveType) -> Self {
        self.item.fade_out = Some(FadeSettings {
            curve_type: curve,
            time,
            unknown_field_3: 0.0,
            unknown_field_4: 1,
            unknown_field_5: 0,
            unknown_field_6: 0,
            unknown_field_7: 0,
        });
        self
    }

    /// Add a stretch marker at the given position.
    pub fn stretch_marker(mut self, position: f64, source_position: f64) -> Self {
        self.item.stretch_markers.push(StretchMarker {
            position,
            source_position,
            rate: None,
        });
        self
    }

    /// Add a stretch marker with a specific rate.
    pub fn stretch_marker_with_rate(
        mut self,
        position: f64,
        source_position: f64,
        rate: f64,
    ) -> Self {
        self.item.stretch_markers.push(StretchMarker {
            position,
            source_position,
            rate: Some(rate),
        });
        self
    }

    /// Add an empty MIDI source take.
    pub fn source_midi(self) -> Self {
        self.take("", SourceType::Midi)
    }

    /// Add a MIDI source take with events constructed via a builder closure.
    ///
    /// # Example
    ///
    /// ```
    /// use dawfile_reaper::builder::{TrackBuilder, MidiSourceBuilder};
    ///
    /// let track = TrackBuilder::new("Piano")
    ///     .item(0.0, 4.0, |i| i
    ///         .name("Piano MIDI")
    ///         .midi(|m| m
    ///             .note(0, 0, 60, 96, 480)    // C4
    ///             .note(0, 0, 64, 96, 480)    // E4
    ///             .note(0, 0, 67, 96, 480)    // G4
    ///         )
    ///     )
    ///     .build();
    /// ```
    pub fn midi(mut self, f: impl FnOnce(MidiSourceBuilder) -> MidiSourceBuilder) -> Self {
        let builder = f(MidiSourceBuilder::new());
        let midi_source = builder.build();
        self.takes.push(Take {
            is_selected: self.takes.is_empty(),
            name: String::new(),
            source: Some(SourceBlock {
                source_type: SourceType::Midi,
                file_path: String::new(),
                midi_data: Some(midi_source),
                raw_content: String::new(),
            }),
            ..Take::default()
        });
        self
    }

    /// Add a FLAC source take.
    pub fn source_flac(self, file_path: impl Into<String>) -> Self {
        self.take(file_path, SourceType::Flac)
    }

    /// Add a MP3 source take.
    pub fn source_mp3(self, file_path: impl Into<String>) -> Self {
        self.take(file_path, SourceType::Mp3)
    }

    /// Add a take with a specific source type and file.
    pub fn take(mut self, file_path: impl Into<String>, source_type: SourceType) -> Self {
        let file = file_path.into();
        let name = file.clone();
        self.takes.push(Take {
            is_selected: self.takes.is_empty(), // First take is selected
            name,
            source: Some(SourceBlock {
                source_type,
                file_path: file,
                midi_data: None,
                raw_content: String::new(),
            }),
            ..Take::default()
        });
        self
    }

    /// Override the name of the last-pushed take. Useful when the take's
    /// display label should differ from the source file path (e.g. show a
    /// PT region name instead of the long absolute path).
    pub fn take_name(mut self, name: impl Into<String>) -> Self {
        if let Some(last) = self.takes.last_mut() {
            last.name = name.into();
        }
        self
    }

    fn build(mut self) -> Item {
        self.item.takes = self.takes;
        self.item
    }
}

// ===========================================================================
// TrackBuilder
// ===========================================================================

/// Builder for constructing REAPER tracks.
///
/// # Example
///
/// ```
/// use dawfile_reaper::builder::TrackBuilder;
///
/// let track = TrackBuilder::new("Guitar")
///     .volume(0.8)
///     .pan(-0.25)
///     .color(0xFF0000)
///     .muted()
///     .item(0.0, 4.0, |i| i
///         .name("Guitar DI")
///         .source_wave("guitar.wav")
///     )
///     .build();
///
/// assert_eq!(track.name, "Guitar");
/// ```
pub struct TrackBuilder {
    track: Track,
    fx_nodes: Vec<FxChainNode>,
    input_fx_nodes: Vec<FxChainNode>,
}

impl TrackBuilder {
    /// Create a new track builder with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            track: Track {
                name: name.into(),
                ..Track::default()
            },
            fx_nodes: vec![],
            input_fx_nodes: vec![],
        }
    }

    /// Set track volume (linear, 1.0 = 0dB).
    pub fn volume(mut self, vol: f64) -> Self {
        self.ensure_volpan().volume = vol;
        self
    }

    /// Set track pan (-1.0 = full left, 0.0 = center, 1.0 = full right).
    pub fn pan(mut self, pan: f64) -> Self {
        self.ensure_volpan().pan = pan;
        self
    }

    /// Set track color as RGB integer.
    pub fn color(mut self, color: u32) -> Self {
        self.track.peak_color = Some(color as i32);
        self
    }

    /// Put the track into fixed-lane (comp) mode (`FIXEDLANES`).
    pub fn fixed_lanes(mut self, settings: crate::types::track::FixedLanesSettings) -> Self {
        self.track.fixed_lanes = Some(settings);
        self
    }

    /// Set the per-lane display names (`LANENAME`).
    pub fn lane_names(mut self, settings: crate::types::track::LaneNameSettings) -> Self {
        self.track.lane_names = Some(settings);
        self
    }

    /// Set the lane record / comping state (`LANEREC`).
    pub fn lane_record(mut self, settings: crate::types::track::LaneRecordSettings) -> Self {
        self.track.lane_record = Some(settings);
        self
    }

    /// Set track GUID.
    pub fn guid(mut self, guid: impl Into<String>) -> Self {
        self.track.track_id = Some(guid.into());
        self
    }

    /// Set the track's TCP height in pixels (REAPER `TRACKHEIGHT`).
    pub fn height(mut self, px: i32) -> Self {
        self.track.track_height = Some(crate::types::track::TrackHeightSettings {
            height: px,
            folder_override: false,
        });
        self
    }

    /// Set the track's automation mode (corresponds to REAPER's `AUTOMODE`
    /// line). PT-imported tracks default to [`AutomationMode::Read`] so
    /// REAPER plays back any imported automation envelopes; standalone
    /// REAPER projects authored from scratch typically leave this at the
    /// builder default (`TrimRead`).
    pub fn automation_mode(mut self, mode: crate::types::track::AutomationMode) -> Self {
        self.track.automation_mode = mode;
        self
    }
    /// Shown in the mixer and not in the track panel (`SHOWINMIX` with
    /// the track list off): a track for mixing, out of the arrangement.
    pub fn mixer_only(mut self) -> Self {
        self.track.show_in_mixer = Some(crate::types::track::ShowInMixerSettings {
            show_in_mixer: true,
            unknown_field_2: 0.6667,
            unknown_field_3: 0.5,
            show_in_track_list: false,
            unknown_field_5: 0.5,
            unknown_field_6: 0,
            unknown_field_7: 0,
            unknown_field_8: 0,
        });
        self
    }

    /// Mark track as muted.
    pub fn muted(mut self) -> Self {
        self.ensure_mutesolo().mute = true;
        self
    }

    /// Mark track as soloed.
    pub fn soloed(mut self) -> Self {
        self.ensure_mutesolo().solo = TrackSoloState::Solo;
        self
    }

    /// Set the solo-defeat flag (this track ignores others' solo state).
    pub fn solo_defeated(mut self) -> Self {
        self.ensure_mutesolo().solo_defeat = true;
        self
    }

    /// Mark track as selected.
    pub fn selected(mut self) -> Self {
        self.track.selected = true;
        self
    }

    /// Arm the track for recording.
    pub fn armed(mut self) -> Self {
        let rec = self.track.record.get_or_insert(RecordSettings {
            armed: false,
            input: 0,
            monitor: crate::types::track::MonitorMode::Off,
            record_mode: crate::types::track::RecordMode::Input,
            monitor_track_media: false,
            preserve_pdc_delayed: false,
            record_path: 0,
        });
        rec.armed = true;
        self
    }

    /// Set the number of channels (default is 2).
    pub fn channels(mut self, n: u32) -> Self {
        self.track.channel_count = n;
        self
    }

    /// Make this track a folder parent (opens a folder level).
    pub fn folder_start(mut self) -> Self {
        self.track.folder = Some(FolderSettings {
            folder_state: FolderState::FolderParent,
            indentation: 1,
        });
        self
    }

    /// Close `levels` folder levels on this track.
    ///
    /// Use `folder_end(1)` to close one folder, `folder_end(2)` for two, etc.
    pub fn folder_end(mut self, levels: i32) -> Self {
        self.track.folder = Some(FolderSettings {
            folder_state: FolderState::LastInFolder,
            indentation: -levels,
        });
        self
    }

    /// Add a media item to this track.
    pub fn item(
        mut self,
        position: f64,
        length: f64,
        f: impl FnOnce(ItemBuilder) -> ItemBuilder,
    ) -> Self {
        let builder = f(ItemBuilder::new(position, length));
        self.track.items.push(builder.build());
        self
    }

    /// Add a simple wave item (shorthand for `.item()` with just a source file).
    pub fn wave_item(self, position: f64, length: f64, file: impl Into<String>) -> Self {
        let file = file.into();
        self.item(position, length, |i| i.source_wave(file))
    }

    /// Add a VST plugin to the track's FX chain.
    pub fn vst(self, name: impl Into<String>, file: impl Into<String>) -> Self {
        self.fx(|fx| {
            FxBuilder::new(
                format!("VST: {}", name.into()),
                PluginType::Vst,
                file.into(),
            )
        })
    }

    /// Add a VST3 plugin to the track's FX chain.
    pub fn vst3(self, name: impl Into<String>, file: impl Into<String>) -> Self {
        self.fx(|_| {
            FxBuilder::new(
                format!("VST3: {}", name.into()),
                PluginType::Vst3,
                file.into(),
            )
        })
    }

    /// Add a CLAP plugin to the track's FX chain.
    pub fn clap(self, name: impl Into<String>, file: impl Into<String>) -> Self {
        self.fx(|_| {
            FxBuilder::new(
                format!("CLAP: {}", name.into()),
                PluginType::Clap,
                file.into(),
            )
        })
    }

    /// Add a JS plugin to the track's FX chain.
    pub fn js(self, name: impl Into<String>) -> Self {
        self.fx(|_| {
            FxBuilder::new(
                format!("JS: {}", name.into()),
                PluginType::Js,
                String::new(),
            )
        })
    }

    /// Add an FX plugin to the track's FX chain using a builder closure.
    pub fn fx(mut self, f: impl FnOnce(FxBuilder) -> FxBuilder) -> Self {
        let dummy = FxBuilder::new("", PluginType::Vst, "");
        let builder = f(dummy);
        self.fx_nodes.push(FxChainNode::Plugin(builder.build()));
        self
    }

    /// Add a stock REAPER FX plugin to the track's FX chain.
    ///
    /// Accepts any type implementing [`StockFx`],
    /// such as `ReaComp`, `ReaEq`, `ReaGate`, etc.
    ///
    /// # Example
    ///
    /// ```
    /// use dawfile_reaper::builder::TrackBuilder;
    /// use dawfile_reaper::stock_fx::{ReaComp, ReaEq};
    ///
    /// let track = TrackBuilder::new("Vocals")
    ///     .stock_fx(ReaComp::new().threshold_db(-18.0).ratio(4.0))
    ///     .stock_fx(ReaEq::new().high_pass(0, 80.0, 0.7))
    ///     .build();
    /// ```
    pub fn stock_fx(mut self, fx: impl StockFx) -> Self {
        self.fx_nodes.push(fx.into_fx_node());
        self
    }

    /// Add a stock REAPER FX plugin to the track's input FX chain.
    pub fn input_stock_fx(mut self, fx: impl StockFx) -> Self {
        self.input_fx_nodes.push(fx.into_fx_node());
        self
    }

    /// Add an FX plugin to the track's input FX chain.
    pub fn input_fx(mut self, f: impl FnOnce(FxBuilder) -> FxBuilder) -> Self {
        let dummy = FxBuilder::new("", PluginType::Vst, "");
        let builder = f(dummy);
        self.input_fx_nodes
            .push(FxChainNode::Plugin(builder.build()));
        self
    }

    /// Add a track envelope (volume, pan, etc.) using a builder closure.
    ///
    /// # Example
    ///
    /// ```
    /// use dawfile_reaper::builder::TrackBuilder;
    ///
    /// let track = TrackBuilder::new("Vocals")
    ///     .envelope("VOLENV2", |e| e
    ///         .active()
    ///         .visible()
    ///         .linear(0.0, 1.0)
    ///         .bezier(2.0, 0.5, -0.3)
    ///         .linear(4.0, 1.0)
    ///     )
    ///     .build();
    ///
    /// assert_eq!(track.envelopes.len(), 1);
    /// ```
    pub fn envelope(
        mut self,
        envelope_type: impl Into<String>,
        f: impl FnOnce(EnvelopeBuilder) -> EnvelopeBuilder,
    ) -> Self {
        let builder = f(EnvelopeBuilder::new(envelope_type));
        self.track.envelopes.push(builder.build());
        self
    }

    /// Add a pre-built envelope directly.
    pub fn add_envelope(mut self, env: Envelope) -> Self {
        self.track.envelopes.push(env);
        self
    }

    /// Lock the track controls.
    pub fn locked(mut self) -> Self {
        self.track.locked = true;
        self
    }

    /// Disable FX on this track.
    pub fn fx_disabled(mut self) -> Self {
        self.track.fx_enabled = false;
        self
    }

    /// Add a receive from another track by index.
    pub fn receive(mut self, source_track_index: i32) -> Self {
        self.track
            .receives
            .push(crate::types::track::ReceiveSettings {
                source_track_index,
                mode: 0,
                volume: 1.0,
                pan: 0.0,
                mute: false,
                mono_sum: false,
                invert_polarity: false,
                source_audio_channels: 0,
                dest_audio_channels: 0,
                pan_law: -1.0,
                midi_channels: 0,
                automation_mode: -1,
            });
        self
    }

    /// Build the final Track.
    pub fn build(mut self) -> Track {
        if !self.fx_nodes.is_empty() {
            self.track.fx_chain = Some(FxChain {
                window_rect: None,
                show: 0,
                last_sel: 0,
                docked: false,
                nodes: self.fx_nodes,
                raw_content: String::new(),
            });
        }
        if !self.input_fx_nodes.is_empty() {
            self.track.input_fx = Some(FxChain {
                window_rect: None,
                show: 0,
                last_sel: 0,
                docked: false,
                nodes: self.input_fx_nodes,
                raw_content: String::new(),
            });
        }
        Self::resolve_fixed_lane_geometry(&mut self.track);
        self.track
    }

    /// Turn each item's fixed-lane index into the `YPOS` line REAPER
    /// actually reads.
    ///
    /// `ItemBuilder::fixed_lane` records the index only; there is no `LANE`
    /// token in an `<ITEM>` at any REAPER version (emitting one made REAPER
    /// reject the whole project — see the note in `Item::write_rpp`). A
    /// fixed-lanes track divides its height evenly, so lane `l` of `n` is
    /// `YPOS l/n 1/n 2`, which is exactly what `Item::from_block` derives
    /// the lane back from.
    ///
    /// The count comes from the track's `LANENAME` list when there is one,
    /// and otherwise from the highest lane any item claims — a track with
    /// lanes but no names still has to place its items.
    fn resolve_fixed_lane_geometry(track: &mut Track) {
        let named = track
            .lane_names
            .as_ref()
            .map_or(0, |ln| ln.lane_names.len());
        let claimed = track
            .items
            .iter()
            .filter_map(|i| i.lane)
            .max()
            .map_or(0, |max| (max + 1).max(0) as usize);
        let lanes = named.max(claimed);
        if lanes == 0 {
            return;
        }
        let height = 1.0 / lanes as f64;
        for item in &mut track.items {
            let Some(lane) = item.lane else { continue };
            if item.y_pos.is_some() {
                continue;
            }
            item.y_pos = Some(crate::types::item::ItemYPos {
                y: f64::from(lane) * height,
                height,
                mode: 2,
            });
        }
    }

    fn ensure_volpan(&mut self) -> &mut VolPanSettings {
        self.track.volpan.get_or_insert(VolPanSettings {
            volume: 1.0,
            pan: 0.0,
            pan_law: -1.0,
        })
    }

    fn ensure_mutesolo(&mut self) -> &mut MuteSoloSettings {
        self.track.mutesolo.get_or_insert(MuteSoloSettings {
            mute: false,
            solo: TrackSoloState::NoSolo,
            solo_defeat: false,
        })
    }
}

// ===========================================================================
// MarkerBuilder
// ===========================================================================

/// Builder for constructing markers and regions.
///
/// # Example
///
/// ```
/// use dawfile_reaper::builder::MarkerBuilder;
///
/// // Simple marker
/// let marker = MarkerBuilder::marker(1, 4.0, "Verse 1").build();
///
/// // Region with start and end
/// let region = MarkerBuilder::region(2, 4.0, 12.0, "Verse 1").lane(1).build();
/// ```
pub struct MarkerBuilder {
    marker: MarkerRegion,
}

impl MarkerBuilder {
    /// Create a marker (point) at the given position.
    pub fn marker(id: i32, position: f64, name: impl Into<String>) -> Self {
        Self {
            marker: MarkerRegion {
                id,
                position,
                name: name.into(),
                color: 0,
                flags: 0,
                locked: 0,
                guid: String::new(),
                additional: 0,
                end_position: None,
                lane: None,
                beat_position: None,
            },
        }
    }

    /// Create a region spanning from `start` to `end`.
    pub fn region(id: i32, start: f64, end: f64, name: impl Into<String>) -> Self {
        Self {
            marker: MarkerRegion {
                id,
                position: start,
                name: name.into(),
                color: 0,
                flags: 1, // region flag
                locked: 0,
                guid: String::new(),
                additional: 0,
                end_position: Some(end),
                lane: None,
                beat_position: None,
            },
        }
    }

    /// Set the marker/region color (REAPER color integer).
    pub fn color(mut self, color: i32) -> Self {
        self.marker.color = color;
        self
    }

    /// Assign to a ruler lane (v7.62+). 0 = default lane.
    pub fn lane(mut self, lane: i32) -> Self {
        self.marker.lane = Some(lane);
        self
    }

    /// Set the GUID for this marker/region.
    pub fn guid(mut self, guid: impl Into<String>) -> Self {
        self.marker.guid = guid.into();
        self
    }

    /// Lock this marker/region.
    pub fn locked(mut self) -> Self {
        self.marker.locked = 1;
        self
    }

    /// Set flags bitfield.
    pub fn flags(mut self, flags: i32) -> Self {
        self.marker.flags = flags;
        self
    }

    /// Build the final `MarkerRegion`.
    pub fn build(self) -> MarkerRegion {
        self.marker
    }
}

// ===========================================================================
// TempoEnvelopeBuilder
// ===========================================================================

/// Builder for constructing tempo/time-signature envelopes.
///
/// # Example
///
/// ```
/// use dawfile_reaper::builder::TempoEnvelopeBuilder;
///
/// let envelope = TempoEnvelopeBuilder::new(120.0, 4, 4)
///     .point(0.0, 120.0)
///     .point_with_time_sig(8.0, 140.0, 3, 4)
///     .ramp(16.0, 100.0)  // linear ramp to 100 BPM
///     .build();
///
/// assert_eq!(envelope.points.len(), 3);
/// ```
pub struct TempoEnvelopeBuilder {
    envelope: TempoTimeEnvelope,
}

impl TempoEnvelopeBuilder {
    /// Create a new tempo envelope with the given default tempo and time signature.
    pub fn new(default_tempo: f64, numerator: i32, denominator: i32) -> Self {
        Self {
            envelope: TempoTimeEnvelope::new(default_tempo, (numerator, denominator)),
        }
    }

    /// Add a tempo point at the given position (seconds) with square shape (instant jump).
    pub fn point(mut self, position: f64, tempo: f64) -> Self {
        self.envelope.points.push(TempoTimePoint {
            position,
            tempo,
            shape: 1, // square = instant
            ..TempoTimePoint::default()
        });
        self
    }

    /// Add a tempo point with a time signature change.
    ///
    /// Time signature is encoded as `65536 * denominator + numerator`.
    pub fn point_with_time_sig(
        mut self,
        position: f64,
        tempo: f64,
        numerator: i32,
        denominator: i32,
    ) -> Self {
        self.envelope.points.push(TempoTimePoint {
            position,
            tempo,
            shape: 1,
            time_signature_encoded: Some(65536 * denominator + numerator),
            ..TempoTimePoint::default()
        });
        self
    }

    /// Add a linear ramp point — tempo ramps linearly from the previous point to this one.
    pub fn ramp(mut self, position: f64, tempo: f64) -> Self {
        self.envelope.points.push(TempoTimePoint {
            position,
            tempo,
            shape: 0, // linear
            ..TempoTimePoint::default()
        });
        self
    }

    /// Add a bezier curve tempo point with the given tension (-1.0 to 1.0).
    pub fn bezier(mut self, position: f64, tempo: f64, tension: f64) -> Self {
        self.envelope.points.push(TempoTimePoint {
            position,
            tempo,
            shape: 5, // bezier
            bezier_tension: tension,
            ..TempoTimePoint::default()
        });
        self
    }

    /// Add a raw `TempoTimePoint` directly.
    pub fn raw_point(mut self, point: TempoTimePoint) -> Self {
        self.envelope.points.push(point);
        self
    }

    /// Build the final `TempoTimeEnvelope`.
    pub fn build(self) -> TempoTimeEnvelope {
        self.envelope
    }
}

// ===========================================================================
// ReaperProjectBuilder
// ===========================================================================

/// Fluent builder for constructing complete REAPER projects.
///
/// # Example
///
/// ```
/// use dawfile_reaper::builder::ReaperProjectBuilder;
/// use dawfile_reaper::RppSerialize;
///
/// let project = ReaperProjectBuilder::new()
///     .tempo(120.0)
///     .sample_rate(48000)
///     .track("Guitar", |t| t
///         .volume(0.8)
///         .item(0.0, 8.0, |i| i.source_wave("guitar.wav"))
///     )
///     .build();
///
/// let text = project.to_rpp_string();
/// assert!(text.contains("Guitar"));
/// ```
pub struct ReaperProjectBuilder {
    version: f64,
    version_string: String,
    timestamp: i64,
    properties: ProjectProperties,
    tracks: Vec<Track>,
    markers_regions: MarkerRegionCollection,
    tempo_envelope: Option<TempoTimeEnvelope>,
}

impl ReaperProjectBuilder {
    /// Create a new project builder with REAPER defaults.
    pub fn new() -> Self {
        Self {
            version: 0.1,
            version_string: "7.0/x64".to_string(),
            timestamp: 0,
            properties: ProjectProperties::new(),
            tracks: vec![],
            markers_regions: MarkerRegionCollection::new(),
            tempo_envelope: None,
        }
    }

    /// Set the REAPER version string (e.g. "7.0/linux-x86_64").
    pub fn version_string(mut self, v: impl Into<String>) -> Self {
        self.version_string = v.into();
        self
    }

    /// Set the project tempo in BPM.
    pub fn tempo(mut self, bpm: f64) -> Self {
        self.properties.tempo = Some((bpm, 4, 4, 0));
        self
    }

    /// Set the project tempo with a specific time signature.
    pub fn tempo_with_time_sig(mut self, bpm: f64, num: i32, den: i32) -> Self {
        self.properties.tempo = Some((bpm, num, den, 0));
        self
    }

    /// Set the project sample rate.
    pub fn sample_rate(mut self, rate: i32) -> Self {
        self.properties.sample_rate = Some((rate, 0, 0));
        self
    }

    /// Set the project timestamp.
    pub fn timestamp(mut self, ts: i64) -> Self {
        self.timestamp = ts;
        self
    }

    /// Add a track using a builder closure.
    pub fn track(
        mut self,
        name: impl Into<String>,
        f: impl FnOnce(TrackBuilder) -> TrackBuilder,
    ) -> Self {
        let builder = f(TrackBuilder::new(name));
        self.tracks.push(builder.build());
        self
    }

    /// Add a pre-built track directly.
    pub fn add_track(mut self, track: Track) -> Self {
        self.tracks.push(track);
        self
    }

    /// Add an empty track with just a name.
    pub fn empty_track(mut self, name: impl Into<String>) -> Self {
        self.tracks.push(Track {
            name: name.into(),
            ..Track::default()
        });
        self
    }

    /// Add a marker at the given position.
    pub fn marker(mut self, id: i32, position: f64, name: impl Into<String>) -> Self {
        self.markers_regions
            .add(MarkerBuilder::marker(id, position, name).build());
        self
    }

    /// Add a marker with a REAPER color int (`0x01000000 | (b<<16)|(g<<8)|r`;
    /// `0` = no custom color).
    pub fn marker_with_color(
        mut self,
        id: i32,
        position: f64,
        name: impl Into<String>,
        color: i32,
    ) -> Self {
        self.markers_regions.add(
            MarkerBuilder::marker(id, position, name)
                .color(color)
                .build(),
        );
        self
    }

    /// Add a region spanning from `start` to `end`.
    pub fn region(mut self, id: i32, start: f64, end: f64, name: impl Into<String>) -> Self {
        self.markers_regions
            .add(MarkerBuilder::region(id, start, end, name).build());
        self
    }

    /// Add a pre-built `MarkerRegion` directly.
    pub fn add_marker(mut self, marker: MarkerRegion) -> Self {
        self.markers_regions.add(marker);
        self
    }

    /// Set the tempo envelope using a builder closure.
    ///
    /// # Example
    ///
    /// ```
    /// use dawfile_reaper::builder::ReaperProjectBuilder;
    /// use dawfile_reaper::RppSerialize;
    ///
    /// let project = ReaperProjectBuilder::new()
    ///     .tempo(120.0)
    ///     .tempo_envelope(|e| e
    ///         .point(0.0, 120.0)
    ///         .ramp(8.0, 140.0)
    ///         .point(16.0, 120.0)
    ///     )
    ///     .build();
    ///
    /// let rpp = project.to_rpp_string();
    /// assert!(rpp.contains("TEMPOENVEX"));
    /// ```
    pub fn tempo_envelope(
        mut self,
        f: impl FnOnce(TempoEnvelopeBuilder) -> TempoEnvelopeBuilder,
    ) -> Self {
        let default_tempo = self.properties.tempo.map(|t| t.0 as f64).unwrap_or(120.0);
        let (num, den) = self.properties.tempo.map(|t| (t.1, t.2)).unwrap_or((4, 4));
        let builder = f(TempoEnvelopeBuilder::new(default_tempo, num, den));
        self.tempo_envelope = Some(builder.build());
        self
    }

    /// Set a pre-built tempo envelope directly.
    pub fn set_tempo_envelope(mut self, envelope: TempoTimeEnvelope) -> Self {
        self.tempo_envelope = Some(envelope);
        self
    }

    /// Build the final ReaperProject.
    pub fn build(self) -> ReaperProject {
        ReaperProject {
            version: self.version,
            version_string: self.version_string,
            timestamp: self.timestamp,
            properties: self.properties,
            tracks: self.tracks,
            items: vec![],
            envelopes: vec![],
            fx_chains: vec![],
            markers_regions: self.markers_regions,
            tempo_envelope: self.tempo_envelope,
            ruler_lanes: vec![],
            ruler_height: None,
        }
    }
}

impl Default for ReaperProjectBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

/// Whether an event ends a note.
///
/// A note-on with velocity zero is a note-off — the running-status
/// idiom, and a reader that missed it would pair the wrong events for
/// exactly the reason the sort exists.
fn is_note_off(event: &MidiEvent) -> bool {
    match event.bytes.first() {
        Some(status) if status & 0xF0 == 0x80 => true,
        Some(status) if status & 0xF0 == 0x90 => event.bytes.get(2).is_some_and(|vel| *vel == 0),
        _ => false,
    }
}
