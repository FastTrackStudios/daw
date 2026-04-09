//! Top-level parse orchestration.
//!
//! Parsing an AAF file proceeds in five stages:
//!
//! 1. **Load CFB** — read all `"properties"` and `"index"` streams into
//!    memory via [`CfbStore`].
//! 2. **Header → ContentStorage** — locate the `Mobs` collection.
//! 3. **First pass** — collect all `MasterMob` and `SourceMob` objects into
//!    lookup tables.
//! 4. **Second pass** — parse each `CompositionMob`, resolving `SourceClip`
//!    chains via the lookup tables.
//! 5. **Assemble** — derive `session_sample_rate` and package the result.

pub mod auid;
pub mod cfb_store;
pub mod essence;
pub mod mobs;
pub mod pids;
pub mod properties;
pub mod segments;

use crate::error::{AafError, AafResult};
use crate::parse::auid::{CLASS_COMPOSITION_MOB, CLASS_MASTER_MOB, CLASS_SOURCE_MOB};
use crate::parse::cfb_store::CfbStore;
use crate::parse::mobs::{
    MasterMobData, SourceMobData, parse_composition_mob, parse_master_mob, parse_source_mob,
};
use crate::parse::pids::*;
use crate::parse::properties::Properties;
use crate::types::{AafComposition, AafMarker, AafSession, AafTimecode, AafTrack};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parse an AAF file at `path` and return a fully resolved [`AafSession`].
pub fn parse_session(path: &Path) -> AafResult<AafSession> {
    let store = CfbStore::load(path)?;

    // ── Stage 2: Header → ContentStorage ────────────────────────────────────
    let root = PathBuf::from("/");

    let header_raw = store
        .properties(&root)
        .ok_or_else(|| AafError::ObjectNotFound(root.clone()))?;
    let header_props = Properties::parse(header_raw, &root)?;

    let object_model_version = header_props
        .u32_le(PID_HEADER_OBJECT_MODEL_VERSION)
        .unwrap_or(1);

    let cs_name = header_props
        .strong_ref_name(PID_HEADER_CONTENT_STORAGE)
        .ok_or_else(|| AafError::MissingProperty {
            pid: PID_HEADER_CONTENT_STORAGE,
            path: root.clone(),
        })?;
    let cs_dir = root.join(&cs_name);

    let cs_raw = store
        .properties(&cs_dir)
        .ok_or_else(|| AafError::ObjectNotFound(cs_dir.clone()))?;
    let cs_props = Properties::parse(cs_raw, &cs_dir)?;

    let (_, mobs_coll_name) = cs_props
        .strong_ref_collection(PID_CONTENT_STORAGE_MOBS)
        .ok_or_else(|| AafError::MissingProperty {
            pid: PID_CONTENT_STORAGE_MOBS,
            path: cs_dir.clone(),
        })?;
    let mobs_coll_dir = cs_dir.join(&mobs_coll_name);

    // ── Stage 3: First pass — collect MasterMobs + SourceMobs ───────────────
    let mob_dirs = store.set_elements(&mobs_coll_dir);

    let mut master_mobs: HashMap<_, MasterMobData> = HashMap::new();
    let mut source_mobs: HashMap<_, SourceMobData> = HashMap::new();
    let mut comp_mob_dirs: Vec<PathBuf> = Vec::new();

    for mob_dir in &mob_dirs {
        let mob_raw = match store.properties(mob_dir) {
            Some(r) => r,
            None => continue,
        };
        let mob_props = Properties::parse(mob_raw, mob_dir)?;

        match mob_props.class_auid() {
            Some(c) if c == CLASS_COMPOSITION_MOB => {
                comp_mob_dirs.push(mob_dir.clone());
            }
            Some(c) if c == CLASS_MASTER_MOB => {
                if let Some(data) = parse_master_mob(&store, mob_dir, &mob_props)? {
                    master_mobs.insert(data.mob_id, data);
                }
            }
            Some(c) if c == CLASS_SOURCE_MOB => {
                if let Some(data) = parse_source_mob(&store, mob_dir, &mob_props)? {
                    source_mobs.insert(data.mob_id, data);
                }
            }
            _ => {} // Unknown class — skip
        }
    }

    // ── Derive session sample rate ───────────────────────────────────────────
    // Use the first audio-rate sample rate found among SourceMobs.
    let session_sample_rate = source_mobs
        .values()
        .filter_map(|s| s.audio_info)
        .map(|a| a.sample_rate)
        .find(|&r| r >= 8000)
        .unwrap_or(48000);

    // ── Stage 4: Second pass — parse CompositionMobs ─────────────────────────
    let mut compositions: Vec<AafComposition> = Vec::new();

    for comp_dir in &comp_mob_dirs {
        let comp_raw = store.properties(comp_dir).unwrap(); // safe: checked above
        let comp_props = Properties::parse(comp_raw, comp_dir)?;
        let comp =
            parse_composition_mob(&store, comp_dir, &comp_props, &master_mobs, &source_mobs)?;
        compositions.push(comp);
    }

    // ── Stage 5: Assemble ────────────────────────────────────────────────────
    let (tracks, markers, timecode_start) = primary_composition_data(&compositions);

    Ok(AafSession {
        object_model_version,
        session_sample_rate,
        tracks,
        markers,
        timecode_start,
        compositions,
    })
}

/// Extract the top-level `tracks`, `markers`, and `timecode` from the first
/// (or only) `CompositionMob`.  Returns empty collections if none exist.
fn primary_composition_data(
    compositions: &[AafComposition],
) -> (Vec<AafTrack>, Vec<AafMarker>, Option<AafTimecode>) {
    match compositions.first() {
        Some(c) => (c.tracks.clone(), c.markers.clone(), c.timecode),
        None => (Vec::new(), Vec::new(), None),
    }
}
