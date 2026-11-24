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
//! - [`METHOD_PLAN`]: where a kit's played hits should go
//!   ([`PlanRequest`] → [`PlanAnswer`]): a target for every hit, with why
//!   and how sure, the hits that are no hit of their own, the hits that
//!   ride another, and the few a person should look at.
//!
//! Beside the methods, what every edit and view shares: [`scope`], what an
//! edit acts on (tracks and stretches of time, and the profile they are
//! edited with), and [`drums`], how a kit's tracks are read (the role
//! each mic plays).
//!
//! A provider names its action for a method `<PREFIX>_<METHOD>` (Architect
//! services: an extension whose actions are `RHYTHM_*` answers `PLACE` as
//! `RHYTHM_PLACE`).

pub mod drums;
pub mod placement;
pub mod plan;
pub mod scope;

use serde::{Deserialize, Serialize};

/// The service an edit algorithm provides, and the version of this
/// contract.
pub const SERVICE: &str = "daw.edit";
/// This contract's version: a provider declares `[provides] "daw.edit" =
/// "0.3.2"`, a host asks for `^0.3` (0.2 added [`METHOD_PLAN`]; 0.3 the
/// gap fill: [`PlaceRequest::takes`] and [`PlaceRequest::fill`] in,
/// pieces that crossfade and stretch only their last span, and
/// [`PlaceAnswer::fills`] out; 0.3.1 what the fill leaves: the previous
/// stroke's decay replayed, a slower stretch down to
/// [`placement::Fill::least_rate_leftover`], a faded dip; 0.3.2 never a
/// silence: the replay down to [`placement::Fill::keep_min`], a stretch at
/// whatever rate fills, flagged [`placement::Filled::forced_rate`]).
pub const VERSION: &str = "0.3.2";
/// Placing a performance's hits: [`PlaceRequest`] → [`PlaceAnswer`].
pub const METHOD_PLACE: &str = "PLACE";
/// Planning where a kit's hits go: [`PlanRequest`] → [`PlanAnswer`].
pub const METHOD_PLAN: &str = "PLAN";

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
    /// The takes the recording over `span` is made of, in order (none: one
    /// take throughout): a gap's fill never reaches across a take's end.
    #[serde(default)]
    pub takes: Vec<placement::Take>,
    /// How a gap is filled.
    #[serde(default)]
    pub fill: placement::Fill,
}

/// The pieces of the recording to lay on the timeline, in the order they
/// play.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceAnswer {
    pub pieces: Vec<placement::Piece>,
    /// What was done at each gap (0.3).
    #[serde(default)]
    pub fills: Vec<placement::Filled>,
}

/// Where a kit's played hits should go: the hits as they play now, the
/// part as written, the bars, the sections and zones, and how to plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanRequest {
    /// The kit's hits that play, any order.
    pub hits: Vec<plan::PlayedHit>,
    /// The drum part as written, each note mapped to its piece (none:
    /// planned on the grid alone).
    #[serde(default)]
    pub notes: Vec<plan::Note>,
    /// The bars, in order, covering the hits.
    pub bars: Vec<plan::BarLine>,
    #[serde(default)]
    pub sections: Vec<plan::Section>,
    #[serde(default)]
    pub zones: Vec<plan::Zone>,
    #[serde(default)]
    pub settings: plan::Settings,
}

/// The plan: a target for every hit, in an apply's shape.
pub type PlanAnswer = plan::Plan;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_and_its_answer_cross_as_json() {
        let request = PlaceRequest {
            hits: vec![
                placement::Hit {
                    at: 1.0,
                    shift: 0.0,
                },
                placement::Hit {
                    at: 2.0,
                    shift: 0.01,
                },
            ],
            span: (0.0, 3.0),
            cut: placement::Cut::default(),
            bounds: (None, Some(4.0)),
            takes: vec![placement::Take { from: 0.0, to: 3.0 }],
            fill: placement::Fill::default(),
        };
        let text = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<PlaceRequest>(&text).unwrap(),
            request
        );
        let answer = PlaceAnswer {
            pieces: vec![placement::Piece {
                start: 0.0,
                end: 3.0,
                shift: 0.0,
                drawn_to: None,
                drawn_from: None,
                crossfade: Some(0.004),
            }],
            fills: vec![placement::Filled {
                at: 2.01,
                recorded: 2.0,
                gap: 0.01,
                stretched: Some(placement::Stretched {
                    from: 1.9,
                    over: 0.05,
                    rate: 0.8,
                }),
                ..placement::Filled::default()
            }],
        };
        let text = serde_json::to_string(&answer).unwrap();
        assert_eq!(serde_json::from_str::<PlaceAnswer>(&text).unwrap(), answer);
    }

    #[test]
    fn a_02_request_and_answer_still_read() {
        let request: PlaceRequest =
            serde_json::from_str(r#"{"hits":[{"at":1.0,"shift":0.0}],"span":[0.0,2.0]}"#).unwrap();
        assert!(request.takes.is_empty());
        assert!((request.fill.least_rate - 0.8).abs() < 1e-12);
        let answer: PlaceAnswer = serde_json::from_str(
            r#"{"pieces":[{"start":0.0,"end":2.0,"shift":0.0,"drawn_to":null}]}"#,
        )
        .unwrap();
        assert!(answer.fills.is_empty());
        assert_eq!(answer.pieces[0].crossfade, None);
    }

    #[test]
    fn a_030_fill_and_its_report_still_read() {
        let request: PlaceRequest = serde_json::from_str(
            r#"{"hits":[],"span":[0.0,1.0],"fill":{"least_rate":0.8,"crossfade":0.004,"most_crossfade":0.01,"keep":0.03}}"#,
        )
        .unwrap();
        assert!((request.fill.least_rate_leftover - 0.5).abs() < 1e-12);
        let answer: PlaceAnswer = serde_json::from_str(
            r#"{"pieces":[],"fills":[{"at":1.0,"recorded":1.0,"gap":0.01,"extended":0.0,"reached_back":0.0,"crossfade":0.0,"left":0.01}]}"#,
        )
        .unwrap();
        assert!(answer.fills[0].replayed_decay.abs() < 1e-12);
        assert!((request.fill.keep_min - 0.010).abs() < 1e-12);
        assert_eq!(answer.fills[0].replay_from_attack, None);
        assert!(!answer.fills[0].forced_rate);
    }

    #[test]
    fn a_plan_request_needs_only_hits_and_bars() {
        let text = r#"{"hits":[{"id":"k1","piece":"Kick","time":1.5}],
            "bars":[{"bar":1,"at":1.5,"beats":4,"unit":4,"beat":0.375}]}"#;
        let request: PlanRequest = serde_json::from_str(text).unwrap();
        assert!(request.settings.lock_runs);
        assert_eq!(request.hits[0].heard, plan::Heard::Unknown);
        let back = serde_json::to_string(&request).unwrap();
        assert_eq!(serde_json::from_str::<PlanRequest>(&back).unwrap(), request);
    }

    #[test]
    fn reasons_cross_as_their_words() {
        use plan::Reason;
        for (reason, word) in [
            (Reason::Midi, "midi"),
            (Reason::DoubleRun, "double-run"),
            (Reason::Rides("s1".into()), "rides s1"),
            (Reason::FlamGrace("s2".into()), "flam-grace s2"),
            (Reason::FillGrid(24), "fill-grid 24"),
            (Reason::FalseDouble, "false-double"),
            (Reason::SeamDouble, "seam-double"),
        ] {
            let text = serde_json::to_string(&reason).unwrap();
            assert_eq!(text, format!("\"{word}\""));
            assert_eq!(serde_json::from_str::<Reason>(&text).unwrap(), reason);
        }
        assert!(serde_json::from_str::<Reason>("\"rides\"").is_err());
    }
}
