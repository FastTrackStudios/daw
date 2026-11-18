//! High-level IO entry points for REAPER formats.
//!
//! This module provides direct helpers for:
//! - full project `.RPP` files
//! - standalone `.RfxChain` / `<FXCHAIN>` chunks
//! - generic chunk tree read/write

use std::fs;
use std::path::Path;

use daw_proto::fx::tree::FxTree;
use daw_proto::track::Track as DawTrack;

use crate::convert::{
    daw_track_to_rpp_track_chunk, fx_chain_to_tree, rpp_tracks_to_daw_tracks, tree_to_fx_chain,
};
use crate::primitives::{RppBlock, RppBlockContent, Token};
use crate::rpp_tree::{RChunk, read_rpp, read_rpp_chunk, write_rpp};
use crate::types::serialize::RppSerialize;
use crate::types::track::{
    FolderSettings, FolderState, MonitorMode, MuteSoloSettings, RecordMode, RecordSettings,
    Track as RppTrack, TrackSoloState, VolPanSettings,
};
use crate::types::{DecodeOptions, FxChain, ReaperProject};
use crate::{RppParseError, RppResult, parse_rpp_file};

/// Parse a full project text into typed [`ReaperProject`].
// r[impl rpp.parse.project]
pub fn parse_project_text(content: &str) -> RppResult<ReaperProject> {
    let parsed = parse_rpp_file(content)?;
    ReaperProject::from_rpp_project(&parsed).map_err(RppParseError::ParseError)
}

/// Parse project text with selective typed decode options.
pub fn parse_project_text_with_options(
    content: &str,
    options: DecodeOptions,
) -> RppResult<ReaperProject> {
    let parsed = parse_rpp_file(content)?;
    ReaperProject::from_rpp_project_with_options(&parsed, options)
        .map_err(RppParseError::ParseError)
}

/// Read and parse a full `.RPP` project file into typed [`ReaperProject`].
pub fn read_project(path: impl AsRef<Path>) -> RppResult<ReaperProject> {
    let content = fs::read_to_string(path)?;
    parse_project_text(&content)
}

/// Parse FX chain text (`<FXCHAIN ...>` or `<FXCHAIN_REC ...>`) into typed [`FxChain`].
pub fn parse_fxchain_text(content: &str) -> RppResult<FxChain> {
    FxChain::parse(content).map_err(RppParseError::ParseError)
}

/// Read and parse a standalone FX chain file.
pub fn read_fxchain(path: impl AsRef<Path>) -> RppResult<FxChain> {
    let content = fs::read_to_string(path)?;
    parse_fxchain_text(&content)
}

/// Write a typed [`FxChain`] to disk as REAPER chunk text.
pub fn write_fxchain(path: impl AsRef<Path>, chain: &FxChain) -> RppResult<()> {
    fs::write(path, chain.to_rpp_string())?;
    Ok(())
}

/// Parse an FX chain and convert it to daw-proto [`FxTree`].
pub fn parse_fxchain_tree(content: &str) -> RppResult<FxTree> {
    let chain = parse_fxchain_text(content)?;
    Ok(fx_chain_to_tree(&chain))
}

/// Read a standalone FX chain and convert it to daw-proto [`FxTree`].
pub fn read_fxchain_tree(path: impl AsRef<Path>) -> RppResult<FxTree> {
    let chain = read_fxchain(path)?;
    Ok(fx_chain_to_tree(&chain))
}

/// Serialize a daw-proto [`FxTree`] as REAPER FX chain text.
pub fn fx_tree_to_rfxchain_text(tree: &FxTree) -> String {
    tree_to_fx_chain(tree).to_rpp_string()
}

/// Write a daw-proto [`FxTree`] as standalone FX chain text.
pub fn write_fx_tree(path: impl AsRef<Path>, tree: &FxTree) -> RppResult<()> {
    fs::write(path, fx_tree_to_rfxchain_text(tree))?;
    Ok(())
}

/// Serialize daw-proto tracks to a minimal REAPER project text.
pub fn daw_tracks_to_rpp_project_text(tracks: &[DawTrack]) -> String {
    let mut out = String::from("<REAPER_PROJECT 0.1 \"7.0/x64\" 0\n");
    out.push_str("  RIPPLE 0\n");
    for track in tracks {
        let chunk = daw_track_to_rpp_track_chunk(track);
        for line in chunk.lines() {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push('>');
    out
}

/// Parse daw-proto tracks from REAPER project text.
pub fn parse_daw_tracks_from_project_text(content: &str) -> RppResult<Vec<DawTrack>> {
    let project = parse_rpp_file(content)?;
    let mut parsed_tracks = Vec::new();
    for block in &project.blocks {
        if block.block_type == crate::primitives::BlockType::Track {
            let track = match crate::types::track::Track::from_block(block) {
                Ok(track) => track,
                Err(err) => {
                    let _ = err;
                    parse_track_block_lenient(block)
                }
            };
            parsed_tracks.push(track);
        }
    }
    Ok(rpp_tracks_to_daw_tracks(&parsed_tracks))
}

/// Lenient track parser used as fallback when strict parsing fails.
///
/// Reads only commonly needed fields and ignores malformed lines.
// r[impl rpp.parse.lenient]
fn parse_track_block_lenient(block: &RppBlock) -> RppTrack {
    let mut track = RppTrack::from_rpp_block("<TRACK\n>").unwrap_or_else(|_| RppTrack {
        name: String::new(),
        selected: false,
        locked: false,
        peak_color: None,
        beat: None,
        automation_mode: crate::types::track::AutomationMode::TrimRead,
        volpan: None,
        mutesolo: None,
        invert_phase: false,
        folder: None,
        bus_compact: None,
        show_in_mixer: None,
        free_mode: None,
        group_flags: None,
        group_flags_high: None,
        fixed_lanes: None,
        lane_solo: None,
        lane_record: None,
        lane_names: None,
        item_lanes: None,
        comp_areas: Vec::new(),
        record: None,
        track_height: None,
        input_quantize: None,
        channel_count: 2,
        rec_cfg: None,
        midi_color_map_fn: None,
        fx_enabled: false,
        track_id: None,
        perf: None,
        layouts: None,
        extension_data: Vec::new(),
        receives: Vec::new(),
        midi_output: None,
        custom_note_order: None,
        midi_note_names: Vec::new(),
        master_send: None,
        hardware_outputs: Vec::new(),
        items: Vec::new(),
        envelopes: Vec::new(),
        fx_chain: None,
        freeze: None,
        input_fx: None,
        raw_content: String::new(),
    });

    for child in &block.children {
        // Handle both Content (nom parser) and RawLine (fast parser) variants.
        // The fast parser stores TRACK children as RawLine for performance;
        // we tokenize on demand here.
        let tokens_owned;
        let tokens: &[Token] = match child {
            RppBlockContent::Content(t) => t,
            RppBlockContent::RawLine(raw) => {
                match crate::primitives::token::parse_token_line(raw) {
                    Ok((_, t)) => {
                        tokens_owned = t;
                        &tokens_owned
                    }
                    Err(_) => continue,
                }
            }
            _ => continue,
        };
        let Some(Token::Identifier(id)) = tokens.first() else {
            continue;
        };

        let get_num = |idx: usize| tokens.get(idx).and_then(Token::as_number);
        let get_bool = |idx: usize| get_num(idx).map(|v| v != 0.0);
        let get_i32 = |idx: usize| get_num(idx).map(|v| v as i32);
        let get_string = |idx: usize| {
            tokens
                .get(idx)
                .and_then(Token::as_string)
                .map(std::string::ToString::to_string)
        };

        match id.as_str() {
            "NAME" => {
                if let Some(name) = get_string(1) {
                    track.name = name;
                }
            }
            "SEL" => {
                if let Some(sel) = get_bool(1) {
                    track.selected = sel;
                }
            }
            "TRACKID" => {
                if let Some(guid) = get_string(1) {
                    track.track_id = Some(guid);
                }
            }
            "PEAKCOL" => {
                if let Some(pc) = get_i32(1) {
                    track.peak_color = Some(pc);
                }
            }
            "VOLPAN" => {
                if let (Some(vol), Some(pan), Some(pan_law)) = (get_num(1), get_num(2), get_num(3))
                {
                    track.volpan = Some(VolPanSettings {
                        volume: vol,
                        pan,
                        pan_law,
                    });
                }
            }
            "MUTESOLO" => {
                if let (Some(mute), Some(solo), Some(solo_defeat)) =
                    (get_bool(1), get_i32(2), get_bool(3))
                {
                    track.mutesolo = Some(MuteSoloSettings {
                        mute,
                        solo: TrackSoloState::from(solo),
                        solo_defeat,
                    });
                }
            }
            "ISBUS" => {
                if let (Some(folder_state), Some(indentation)) = (get_i32(1), get_i32(2)) {
                    track.folder = Some(FolderSettings {
                        folder_state: FolderState::from(folder_state),
                        indentation,
                    });
                }
            }
            "REC" => {
                if let (Some(armed), Some(input), Some(monitor), Some(record_mode)) =
                    (get_bool(1), get_i32(2), get_i32(3), get_i32(4))
                {
                    track.record = Some(RecordSettings {
                        armed,
                        input,
                        monitor: MonitorMode::from(monitor),
                        record_mode: RecordMode::from(record_mode),
                        monitor_track_media: get_bool(5).unwrap_or(false),
                        preserve_pdc_delayed: get_bool(6).unwrap_or(false),
                        record_path: get_i32(7).unwrap_or(0),
                    });
                }
            }
            _ => {}
        }
    }

    track
}

/// Parse any single-root REAPER chunk text into generic [`RChunk`] tree.
pub fn parse_chunk_text(content: &str) -> RppResult<RChunk> {
    read_rpp_chunk(content)
}

/// Read any single-root REAPER chunk file into generic [`RChunk`] tree.
pub fn read_chunk(path: impl AsRef<Path>) -> RppResult<RChunk> {
    read_rpp(path)
}

/// Write generic chunk tree to disk.
pub fn write_chunk(path: impl AsRef<Path>, root: &RChunk) -> RppResult<()> {
    write_rpp(path, root)
}
