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
use crate::types::fx_chain::{FxChain, FxChainNode, FxPlugin, PluginType};
use crate::types::item::{Item, SourceBlock, SourceType, Take};
use crate::types::project::{ProjectProperties, ReaperProject};
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
        }
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
            float_pos: None,
            wak: None,
            parallel: false,
            state_data: self.state_data,
            raw_block: String::new(),
            param_envelopes: vec![],
            params_on_tcp: vec![],
        }
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

    /// Add a MIDI source take.
    pub fn source_midi(self) -> Self {
        self.take("", SourceType::Midi)
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

    /// Set track GUID.
    pub fn guid(mut self, guid: impl Into<String>) -> Self {
        self.track.track_id = Some(guid.into());
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
            let mut b = FxBuilder::new(
                format!("VST: {}", name.into()),
                PluginType::Vst,
                file.into(),
            );
            b
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
    /// Accepts any type implementing [`StockFx`](crate::stock_fx::StockFx),
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
        self.track
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
        }
    }

    /// Set the REAPER version string (e.g. "7.0/linux-x86_64").
    pub fn version_string(mut self, v: impl Into<String>) -> Self {
        self.version_string = v.into();
        self
    }

    /// Set the project tempo in BPM.
    pub fn tempo(mut self, bpm: f64) -> Self {
        self.properties.tempo = Some((bpm as i32, 4, 4, 0));
        self
    }

    /// Set the project tempo with a specific time signature.
    pub fn tempo_with_time_sig(mut self, bpm: f64, num: i32, den: i32) -> Self {
        self.properties.tempo = Some((bpm as i32, num, den, 0));
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
            markers_regions: Default::default(),
            tempo_envelope: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::serialize::RppSerialize;
    use crate::types::track::FolderState;

    #[test]
    fn test_minimal_project() {
        let project = ReaperProjectBuilder::new().build();
        let rpp = project.to_rpp_string();
        assert!(rpp.starts_with("<REAPER_PROJECT 0.1"));
        assert!(rpp.contains("RIPPLE 0"));
    }

    #[test]
    fn test_project_with_tempo_and_sample_rate() {
        let project = ReaperProjectBuilder::new()
            .tempo(140.0)
            .sample_rate(96000)
            .build();

        let rpp = project.to_rpp_string();
        assert!(rpp.contains("TEMPO 140"));
        assert!(rpp.contains("SAMPLERATE 96000"));
    }

    #[test]
    fn test_track_builder_basic() {
        let track = TrackBuilder::new("Vocals")
            .volume(0.75)
            .pan(0.1)
            .color(0xFF8800)
            .selected()
            .build();

        assert_eq!(track.name, "Vocals");
        assert_eq!(track.volpan.as_ref().unwrap().volume, 0.75);
        assert_eq!(track.volpan.as_ref().unwrap().pan, 0.1);
        assert_eq!(track.peak_color, Some(0xFF8800_u32 as i32));
        assert!(track.selected);
    }

    #[test]
    fn test_track_builder_muted_soloed_armed() {
        let track = TrackBuilder::new("Lead")
            .muted()
            .armed()
            .build();

        assert!(track.mutesolo.as_ref().unwrap().mute);
        assert!(track.record.as_ref().unwrap().armed);
    }

    #[test]
    fn test_track_builder_with_items() {
        let track = TrackBuilder::new("Drums")
            .item(0.0, 4.0, |i| {
                i.name("Kick").source_wave("kick.wav")
            })
            .item(4.0, 4.0, |i| {
                i.name("Snare").source_wave("snare.wav")
            })
            .build();

        assert_eq!(track.items.len(), 2);
        assert_eq!(track.items[0].name, "Kick");
        assert_eq!(track.items[0].position, 0.0);
        assert_eq!(track.items[0].length, 4.0);
        assert_eq!(track.items[1].name, "Snare");
        assert_eq!(track.items[1].position, 4.0);
    }

    #[test]
    fn test_track_builder_with_fx() {
        let track = TrackBuilder::new("Guitar")
            .vst("ReaEQ (Cockos)", "reaeq.dll")
            .fx(|_| {
                FxBuilder::new(
                    "VST: ReaComp (Cockos)",
                    PluginType::Vst,
                    "reacomp.dll",
                )
                .bypassed()
                .fxid("{COMP-GUID}")
            })
            .build();

        let chain = track.fx_chain.as_ref().unwrap();
        assert_eq!(chain.nodes.len(), 2);

        if let FxChainNode::Plugin(p) = &chain.nodes[0] {
            assert!(p.name.contains("ReaEQ"));
            assert!(!p.bypassed);
        } else {
            panic!("Expected plugin");
        }

        if let FxChainNode::Plugin(p) = &chain.nodes[1] {
            assert!(p.name.contains("ReaComp"));
            assert!(p.bypassed);
            assert_eq!(p.fxid.as_deref(), Some("{COMP-GUID}"));
        } else {
            panic!("Expected plugin");
        }
    }

    #[test]
    fn test_track_folder_hierarchy() {
        let project = ReaperProjectBuilder::new()
            .track("Drums", |t| t.folder_start())
            .track("Kick", |t| {
                t.item(0.0, 4.0, |i| i.source_wave("kick.wav"))
            })
            .track("Snare", |t| {
                t.item(0.0, 4.0, |i| i.source_wave("snare.wav"))
                    .folder_end(1)
            })
            .track("Bass", |t| t.volume(0.7))
            .build();

        assert_eq!(project.tracks.len(), 4);

        // Drums is folder parent
        let drums = &project.tracks[0];
        assert_eq!(
            drums.folder.as_ref().unwrap().folder_state,
            FolderState::FolderParent
        );
        assert_eq!(drums.folder.as_ref().unwrap().indentation, 1);

        // Snare closes the folder
        let snare = &project.tracks[2];
        assert_eq!(
            snare.folder.as_ref().unwrap().folder_state,
            FolderState::LastInFolder
        );
        assert_eq!(snare.folder.as_ref().unwrap().indentation, -1);
    }

    #[test]
    fn test_wave_item_shorthand() {
        let track = TrackBuilder::new("Bass")
            .wave_item(0.0, 8.0, "bass_di.wav")
            .build();

        assert_eq!(track.items.len(), 1);
        let take = &track.items[0].takes[0];
        assert!(take.is_selected);
        assert_eq!(take.source.as_ref().unwrap().source_type, SourceType::Wave);
        assert_eq!(take.source.as_ref().unwrap().file_path, "bass_di.wav");
    }

    #[test]
    fn test_full_project_serialize_roundtrip() {
        let project = ReaperProjectBuilder::new()
            .tempo(120.0)
            .sample_rate(48000)
            .track("Drums", |t| {
                t.color(0x112233)
                    .folder_start()
                    .vst("ReaEQ (Cockos)", "reaeq.dll")
            })
            .track("Kick", |t| {
                t.item(0.0, 4.0, |i| {
                    i.name("Kick Pattern").source_wave("kick.wav")
                })
            })
            .track("Snare", |t| {
                t.item(0.0, 4.0, |i| i.source_wave("snare.wav"))
                    .folder_end(1)
            })
            .track("Bass", |t| {
                t.volume(0.7)
                    .armed()
                    .item(0.0, 8.0, |i| i.source_wave("bass.wav"))
            })
            .build();

        let rpp = project.to_rpp_string();

        // Verify it parses back
        let parsed = crate::io::parse_project_text(&rpp).expect("should parse builder output");
        assert_eq!(parsed.tracks.len(), 4);
        assert_eq!(parsed.tracks[0].name, "Drums");
        assert_eq!(parsed.tracks[1].name, "Kick");
        assert_eq!(parsed.tracks[2].name, "Snare");
        assert_eq!(parsed.tracks[3].name, "Bass");
    }

    #[test]
    fn test_input_fx() {
        let track = TrackBuilder::new("Vocals")
            .input_fx(|_| {
                FxBuilder::new("VST: ReaTune (Cockos)", PluginType::Vst, "reatune.dll")
            })
            .build();

        assert!(track.input_fx.is_some());
        assert_eq!(track.input_fx.as_ref().unwrap().nodes.len(), 1);
        assert!(track.fx_chain.is_none());
    }

    #[test]
    fn test_empty_track() {
        let project = ReaperProjectBuilder::new()
            .empty_track("Track 1")
            .empty_track("Track 2")
            .build();

        assert_eq!(project.tracks.len(), 2);
        assert_eq!(project.tracks[0].name, "Track 1");
        assert_eq!(project.tracks[1].name, "Track 2");
    }

    #[test]
    fn test_add_prebuilt_track() {
        let track = TrackBuilder::new("Custom")
            .volume(0.5)
            .channels(4)
            .build();

        let project = ReaperProjectBuilder::new().add_track(track).build();

        assert_eq!(project.tracks.len(), 1);
        assert_eq!(project.tracks[0].channel_count, 4);
    }

    #[test]
    fn test_item_builder_features() {
        let track = TrackBuilder::new("Test")
            .item(1.0, 2.0, |i| {
                i.name("Test Item")
                    .selected()
                    .muted()
                    .looped()
                    .color(0x334455)
                    .guid("{ITEM-GUID}")
                    .snap_offset(0.5)
                    .source_wave("test.wav")
            })
            .build();

        let item = &track.items[0];
        assert_eq!(item.name, "Test Item");
        assert!(item.selected);
        assert!(item.mute.as_ref().unwrap().muted);
        assert!(item.loop_source);
        assert_eq!(item.color, Some(0x334455));
        assert_eq!(item.item_guid.as_deref(), Some("{ITEM-GUID}"));
        assert_eq!(item.snap_offset, 0.5);
    }

    #[test]
    fn test_multiple_takes() {
        let track = TrackBuilder::new("Multitrack")
            .item(0.0, 4.0, |i| {
                i.take("take1.wav", SourceType::Wave)
                    .take("take2.wav", SourceType::Wave)
            })
            .build();

        assert_eq!(track.items[0].takes.len(), 2);
        assert!(track.items[0].takes[0].is_selected);
        assert!(!track.items[0].takes[1].is_selected);
    }

    #[test]
    fn test_nested_folders() {
        let project = ReaperProjectBuilder::new()
            .track("Instruments", |t| t.folder_start())
            .track("Drums", |t| t.folder_start())
            .track("Kick", |t| t)
            .track("Snare", |t| t.folder_end(1))
            .track("Bass", |t| t.folder_end(1))
            .build();

        assert_eq!(project.tracks.len(), 5);

        // Snare closes Drums folder
        assert_eq!(
            project.tracks[3].folder.as_ref().unwrap().indentation,
            -1
        );
        // Bass closes Instruments folder
        assert_eq!(
            project.tracks[4].folder.as_ref().unwrap().indentation,
            -1
        );
    }
}
