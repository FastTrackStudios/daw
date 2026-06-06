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
    /// CSI's `TrackUniqueSelect`.
    TrackSelect,
    /// Additive select (CSI's `TrackSelect`).
    TrackSelectAdditive,
    /// CSI `TrackToggleFolderSpill`: drill into a folder / pop back
    /// out of the current spill. Plain tracks fall through to
    /// [`Action::TrackSelect`].
    FolderSpill,
    /// CSI `TrackToggleVCASpill`: spill a VCA lead's followers across
    /// the strips / pop back out. Non-leads fall through to select.
    VcaSpill,

    // ── Strip: sends (strip = send slot of the selected track) ─────
    /// Fader/v-pot ↔ send level of the strip's send slot.
    SendVolume,
    /// V-pot ↔ send pan.
    SendPan,
    /// Toggle send mute.
    SendMute,

    // ── Strip: FX menu (strip = FX slot of the selected track) ─────
    /// Focus the strip's FX and hop to its param zone (CSI's
    /// `GoFXSlot`). `zone` names the param zone to enter.
    FxMenuSelect {
        zone: String,
    },
    /// Toggle the strip's FX bypass (LED lit = enabled).
    FxMenuBypass,

    // ── Strip: FX params (strip = param slot of the focused FX) ────
    /// V-pot/fader ↔ the strip's parameter of the focused FX.
    FxParam,

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
    /// Send destination track name (sends zones).
    SendNameDisplay,
    /// Send level in dB.
    SendVolumeDisplay,
    /// Send pan position.
    SendPanDisplay,
    /// FX name (FX-menu zones).
    FxMenuNameDisplay,
    /// Parameter name of the focused FX (FX param zones).
    FxParamNameDisplay,
    /// Plugin-formatted parameter value ("−12.0 dB").
    FxParamValueDisplay,
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
    /// ±strip-count page). Oversized amounts clamp — CSI's
    /// `Bank Track ±999` = jump to first/last page.
    Bank {
        amount: i32,
    },
    /// Bank the SEND-slot window (sends zones).
    BankSends {
        amount: i32,
    },
    /// Bank the FX-PARAM window (FX param zones; plugins routinely
    /// expose more than 8 parameters).
    BankFxParams {
        amount: i32,
    },
    /// Un-solo every track (CSI `ClearAllSolo`).
    ClearAllSolo,
    /// Un-mute every track.
    UnmuteAll,
    /// Activate another zone by name (CSI `GoZone`).
    GoZone {
        zone: String,
    },
    /// Bound but intentionally inert (CSI `NoAction`).
    NoAction,
}
