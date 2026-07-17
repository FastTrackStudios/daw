//! input-keybinds — app-agnostic keybind-config → `KeymapConfig` bridge.
//!
//! Turns the styx/facet keybind config data model from `input-config-proto`
//! (`KeybindDef` / `WhichKeyTreeDef` / `WheelBindDef` / `SectionConfig` /
//! `ProfileConfig`) into an [`input::KeymapConfig`] that an
//! `input::InputProcessor` can dispatch. The key-notation translation
//! (`<C-s>` → `Ctrl+s`, `gg` → `g g`, which-key trees → flat space-separated
//! sequences) was extracted from reaper-input's bridge so that any consumer —
//! not just the DAW — can drive the same input engine from the same config
//! files.
//!
//! - [`bridge`] — pure conversion (deps: `input`, `input-config-proto`).
//! - [`loader`] — parse styx profiles/sections into a `KeymapConfig`.

pub mod bridge;
pub mod loader;

pub use bridge::{
    context_to_when_expr, keymap_config_from_defs, parse_reaper_modifier_string,
    section_to_keymap_config, translate_sequence,
};
pub use loader::{keymap_from_section_str, load_profile_keymap};
