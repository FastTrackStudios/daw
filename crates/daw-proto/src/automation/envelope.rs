//! Envelope types for automation

use crate::primitives::{AutomationMode, PositionInSeconds};
use crate::track::TrackRef;
use facet::Facet;

/// Type of envelope (which parameter it controls)
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Facet)]
pub enum EnvelopeType {
    /// Track volume
    #[default]
    Volume = 0,
    /// Track volume (pre-FX)
    VolumePrefx = 1,
    /// Track pan
    Pan = 2,
    /// Track pan (pre-FX)
    PanPrefx = 3,
    /// Track width
    Width = 4,
    /// Track width (pre-FX)
    WidthPrefx = 5,
    /// Track mute
    Mute = 6,
    /// FX parameter (uses fx_guid + param_index)
    FxParam = 7,
}

/// Reference to an envelope
#[repr(C)]
#[derive(Clone, Debug, Facet)]
pub enum EnvelopeRef {
    /// Reference by envelope type (for track envelopes)
    Type(EnvelopeType),
    /// Reference by FX parameter
    FxParam { fx_guid: String, param_index: u32 },
    /// Reference by display name
    ByName(String),
}

/// Complete envelope state
#[derive(Clone, Debug, Facet)]
pub struct Envelope {
    /// Track this envelope belongs to
    pub track_guid: String,
    /// Type of envelope
    pub envelope_type: EnvelopeType,
    /// Display name
    pub name: String,

    // FX param envelope specific
    /// FX GUID (if FxParam envelope)
    pub fx_guid: Option<String>,
    /// Parameter index (if FxParam envelope)
    pub param_index: Option<u32>,

    // State
    /// Whether envelope is visible in arrange view
    pub visible: bool,
    /// Whether envelope is armed for recording
    pub armed: bool,
    /// Automation playback/recording mode
    pub automation_mode: AutomationMode,

    // Points
    /// Number of points in the envelope
    pub point_count: u32,
}

/// Shape of automation curve between points
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Facet)]
pub enum EnvelopeShape {
    /// Linear interpolation
    #[default]
    Linear = 0,
    /// Square (instant jump)
    Square = 1,
    /// Slow start and end (S-curve)
    SlowStartEnd = 2,
    /// Fast start (logarithmic)
    FastStart = 3,
    /// Fast end (exponential)
    FastEnd = 4,
    /// Bezier curve (uses tension)
    Bezier = 5,
}

/// A point on an automation envelope
#[derive(Clone, Debug, Facet)]
pub struct EnvelopePoint {
    /// Index of this point in the envelope
    pub index: u32,
    /// Time position
    pub time: PositionInSeconds,
    /// Value (0.0-1.0 normalized)
    pub value: f64,
    /// Curve shape to next point
    pub shape: EnvelopeShape,
    /// Bezier tension (-1.0 to 1.0)
    pub tension: f64,
    /// Whether this point is selected
    pub selected: bool,
}

/// Location of an envelope (track + envelope reference)
#[derive(Clone, Debug, Facet)]
pub struct EnvelopeLocation {
    /// The track containing the envelope
    pub track: TrackRef,
    /// Reference to the envelope
    pub envelope: EnvelopeRef,
}

impl EnvelopeLocation {
    /// Create a new envelope location
    pub fn new(track: TrackRef, envelope: EnvelopeRef) -> Self {
        Self { track, envelope }
    }

    /// Create a volume envelope location
    pub fn volume(track: TrackRef) -> Self {
        Self::new(track, EnvelopeRef::Type(EnvelopeType::Volume))
    }

    /// Create a pan envelope location
    pub fn pan(track: TrackRef) -> Self {
        Self::new(track, EnvelopeRef::Type(EnvelopeType::Pan))
    }

    /// Create an FX parameter envelope location
    pub fn fx_param(track: TrackRef, fx_guid: String, param_index: u32) -> Self {
        Self::new(
            track,
            EnvelopeRef::FxParam {
                fx_guid,
                param_index,
            },
        )
    }
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            track_guid: String::new(),
            envelope_type: EnvelopeType::Volume,
            name: String::new(),
            fx_guid: None,
            param_index: None,
            visible: false,
            armed: false,
            automation_mode: AutomationMode::TrimRead,
            point_count: 0,
        }
    }
}

impl Default for EnvelopePoint {
    fn default() -> Self {
        Self {
            index: 0,
            time: PositionInSeconds::ZERO,
            value: 0.0,
            shape: EnvelopeShape::Linear,
            tension: 0.0,
            selected: false,
        }
    }
}
