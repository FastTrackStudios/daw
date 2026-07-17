//! Load keybind profiles/sections from styx into an `input::KeymapConfig`.
//!
//! The app-agnostic slice of reaper-input's `keybind_config/loader.rs`:
//! parse `profile.styx` (a [`ProfileConfig`]) + its section files (each a
//! [`SectionConfig`]) and bridge the accumulated bindings / which-key trees /
//! wheel bindings into a [`KeymapConfig`]. No REAPER-specific overlay,
//! workflow, or mouse-modifier handling — those stay in reaper-input.

use std::path::Path;

use input::config::KeymapConfig;
use input_config_proto::{KeybindDef, ProfileConfig, SectionConfig, WheelBindDef, WhichKeyTreeDef};

use crate::bridge::{keymap_config_from_defs, section_to_keymap_config};

/// Parse a single section's styx source into a [`KeymapConfig`].
///
/// Handy for the web build, which fetches styx section strings rather than
/// reading a directory off disk.
pub fn keymap_from_section_str(styx: &str) -> Result<KeymapConfig, String> {
    let section: SectionConfig = facet_styx::from_str(styx).map_err(|e| e.to_string())?;
    Ok(section_to_keymap_config(&section))
}

/// Load a [`KeymapConfig`] from a profile directory.
///
/// `dir` should contain a `profile.styx` naming the section files to load, in
/// order (later files win on key conflicts). Returns `None` if `profile.styx`
/// cannot be read or parsed. Unreadable/unparseable section files are skipped.
pub fn load_profile_keymap(dir: &Path) -> Option<KeymapConfig> {
    let profile = load_profile(dir)?;

    let mut bindings: Vec<KeybindDef> = Vec::new();
    let mut trees: Vec<WhichKeyTreeDef> = Vec::new();
    let mut wheel: Vec<WheelBindDef> = Vec::new();

    for filename in &profile.sections {
        let section_path = dir.join(filename);
        if let Some(section) = load_section(&section_path) {
            bindings.extend(section.bindings().iter().cloned());
            trees.extend(section.which_key().iter().cloned());
            wheel.extend(section.wheel().iter().cloned());
        }
    }

    Some(keymap_config_from_defs(&bindings, &trees, &wheel))
}

fn load_profile(dir: &Path) -> Option<ProfileConfig> {
    let contents = std::fs::read_to_string(dir.join("profile.styx")).ok()?;
    facet_styx::from_str::<ProfileConfig>(&contents).ok()
}

fn load_section(path: &Path) -> Option<SectionConfig> {
    let contents = std::fs::read_to_string(path).ok()?;
    facet_styx::from_str::<SectionConfig>(&contents).ok()
}
