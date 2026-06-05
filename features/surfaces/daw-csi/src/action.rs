//! The bindable action registry — every name a zone file can bind a
//! widget to. CSI's `actions_` map, as a Facet enum so Styx parses
//! actions directly: bare variants are `@TrackVolume`-style tags,
//! parameterized ones carry a payload (`@Bank{amount -8}`).

use facet::Facet;

/// One bindable action. Strip-context actions resolve "which track"
/// from the widget's strip via the navigator; global actions ignore
/// the strip.
#[repr(u8)]
#[derive(Facet, Clone, Debug, PartialEq)]
pub enum Action {
    // ── Strip: control ──────────────────────────────────────────────
    /// Fader ↔ track volume (motor feedback follows).
    TrackVolume,
    /// V-pot ↔ track pan (ring feedback follows).
    TrackPan,
    /// Recenter pan (v-pot press convention).
    TrackPanReset,
    /// Toggle mute (LED follows).
    TrackMute,
    /// Toggle solo (LED follows).
    TrackSolo,
    /// Toggle record arm (LED follows).
    TrackRecordArm,
    /// Exclusive-select the strip's track (LED follows selection).
    TrackSelect,
    /// CSI `TrackToggleFolderSpill`: drill into a folder / pop back
    /// out of the current spill. Plain tracks fall through to
    /// [`Action::TrackSelect`].
    FolderSpill,

    // ── Strip: displays ─────────────────────────────────────────────
    /// Track name on an LCD row.
    TrackName,
    /// Pan position ("12L" / "C" / "40R").
    PanDisplay,
    /// Volume in dB.
    VolumeDisplay,
    /// "FOLDR>" on folder tracks, pan otherwise (folder-mode bottom
    /// row).
    FolderIndicator,
    /// A literal string.
    Fixed {
        text: String,
    },
    /// Blank the cell.
    Blank,

    // ── Master / global control ─────────────────────────────────────
    /// Master fader ↔ master volume.
    MasterVolume,
    Play,
    Stop,
    Record,
    ToggleLoop,
    /// Move the edit position by a fixed amount (transport rewind /
    /// fast-forward buttons).
    NudgePosition {
        seconds: f64,
    },
    /// Jog wheel: seconds per encoder tick (sign follows rotation).
    JogPosition {
        seconds_per_tick: f64,
    },
    /// Bank the navigator window by `amount` strips (±1 channel,
    /// ±strip-count page).
    Bank {
        amount: i32,
    },
    /// Activate another zone by name (CSI `GoZone`).
    GoZone {
        zone: String,
    },
    /// Bound but intentionally inert (CSI `NoAction`).
    NoAction,
}
