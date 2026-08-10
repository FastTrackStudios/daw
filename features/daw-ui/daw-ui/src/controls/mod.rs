//! The controls a mixing engineer actually clicks.
//!
//! [`daw_theme_art`] draws the chrome and knows nothing else: props in, SVG
//! out, no `daw` dependency, no context, no pointer state, no branch on
//! render target. That is not tidiness — it is what lets the theme exporter
//! rasterise the same components into REAPER's PNGs. A component that could
//! ask "am I being exported?" would eventually answer differently in the two
//! renderings, and the app and the theme would quietly become two themes.
//!
//! Everything that art layer refuses to know lives here, one thin wrapper
//! per control:
//!
//! - **pointer state as Signals.** Hover has to be a *prop*, not a CSS
//!   pseudo-class: every non-browser target hands the `<svg>` subtree to a
//!   parser where `:hover` is inert, so nothing inside a control can know it
//!   is hovered. The same prop that drives live hover is what makes the
//!   exporter's three-cell sprite strip possible at all.
//! - **the track**, read from [`TrackStore`] rather than passed down, so a
//!   strip does not have to thread state through every control it holds.
//! - **the size**, in explicit inline pixels off the art's own source box.
//!   No stylesheet is assumed to arrive: these render in Blitz inside a
//!   REAPER panel, where external CSS does not load reliably.
//!
//! The strip's controls all live here now: [`MuteButton`], [`SoloButton`],
//! [`RecordArmButton`], [`PhaseButton`], [`MonitorButton`], [`FxButton`],
//! [`VolumeFader`], [`PanKnob`] and [`TrackName`].
//!
//! A continuous control adds a fourth: the value under the pointer is the
//! UI's, not the engine's, so it never waits on a round trip. That lives in
//! [`Drafts`], drained by [`ControlSync`].

mod drafts;
mod fader;
mod fx;
mod meters;
mod mute;
mod pan;
pub(crate) mod reach;
mod toggles;
mod sync;
mod track_store;

/// Which of REAPER's two control families to draw — re-exported from the
/// art layer, which owns the distinction because it is a fact about the
/// images (different boxes, different measurements), not about the wrapper.
pub use daw_theme_art::dress::Panel;
pub use drafts::{Drafts, Held};
pub use fader::VolumeFader;
pub use fx::FxButton;
pub use meters::{MeterFeed, Meters, TrackMeter, use_meters};
pub use mute::MuteButton;
pub use pan::{PanKnob, TrackName};
pub use toggles::{IoButton, MonitorButton, PhaseButton, RecordArmButton, SoloButton};
pub use sync::ControlSync;
pub use track_store::{TrackStore, use_daw_tracks, use_track, use_track_store};
