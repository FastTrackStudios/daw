//! The DAW's editing contract.
//!
//! An edit algorithm is swappable. The DAW owns the edit surface (the
//! takes, the lanes, the cuts and stretch markers, one undo step per
//! edit); an algorithm only decides. A host that runs the DAW (Studio's
//! engine) loads algorithms as extensions that provide [`SERVICE`] and
//! calls the one chosen. The DAW has no algorithm of its own: without a
//! provider, an edit that needs one is refused. First-party (rhythm) and
//! third-party algorithms speak the same contract: the request and answer
//! types here, as JSON.
//!
//! - [`METHOD_PLACE`]: where a performance's audio plays once its hits are
//!   moved ([`PlaceRequest`] → [`PlaceAnswer`]): the pieces of the
//!   recording to lay on the timeline, cut, shifted and drawn out.
//!
//! A provider names its action for a method `<PREFIX>_<METHOD>` (Architect
//! services: an extension whose actions are `RHYTHM_*` answers `PLACE` as
//! `RHYTHM_PLACE`).

pub mod placement;

use serde::{Deserialize, Serialize};

/// The service an edit algorithm provides, and the version of this
/// contract.
pub const SERVICE: &str = "daw.edit";
/// This contract's version: a provider declares `[provides] "daw.edit" =
/// "0.1.0"`, a host asks for `^0.1`.
pub const VERSION: &str = "0.1.0";
/// Placing a performance's hits: [`PlaceRequest`] → [`PlaceAnswer`].
pub const METHOD_PLACE: &str = "PLACE";

/// Where the hits of one recorded performance (every mic of a kit, phase
/// locked) should play, over `span` of the recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceRequest {
    /// Each hit: where it was recorded (`at`, seconds in the recording)
    /// and how far it should move (`shift`, seconds; where it plays minus
    /// where it was recorded).
    pub hits: Vec<placement::Hit>,
    /// The stretch of the recording to lay out, seconds.
    pub span: (f64, f64),
    /// How far before an attack a cut goes, and how far after one the next
    /// piece may not reach back.
    #[serde(default)]
    pub cut: placement::Cut,
    /// How far the first and last piece may reach outside `span` (the
    /// audio that plays either side, untouched), when they may.
    #[serde(default)]
    pub bounds: (Option<f64>, Option<f64>),
}

/// The pieces of the recording to lay on the timeline, in the order they
/// play.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceAnswer {
    pub pieces: Vec<placement::Piece>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_and_its_answer_cross_as_json() {
        let request = PlaceRequest {
            hits: vec![
                placement::Hit { at: 1.0, shift: 0.0 },
                placement::Hit { at: 2.0, shift: 0.01 },
            ],
            span: (0.0, 3.0),
            cut: placement::Cut::default(),
            bounds: (None, Some(4.0)),
        };
        let text = serde_json::to_string(&request).unwrap();
        assert_eq!(serde_json::from_str::<PlaceRequest>(&text).unwrap(), request);
        let answer = PlaceAnswer {
            pieces: vec![placement::Piece { start: 0.0, end: 3.0, shift: 0.0, drawn_to: None }],
        };
        let text = serde_json::to_string(&answer).unwrap();
        assert_eq!(serde_json::from_str::<PlaceAnswer>(&text).unwrap(), answer);
    }
}
