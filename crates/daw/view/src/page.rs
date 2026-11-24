//! A page of bars and a view's frame, as the engine answers them
//! (`SESSION_PAGE`, `SESSION_VIEW_SPAN`). Working them out is the engine's
//! (`session_engine::page`).

use facet::Facet;

/// The page.
#[derive(Facet, Clone, Debug, PartialEq)]
pub struct PageOut {
    pub first_bar: i32,
    pub last_bar: i32,
    pub bars: i32,
    pub group: String,
    /// Seconds.
    pub from: f64,
    pub to: f64,
    /// What is shown of it, with a little either side.
    pub shown_from: f64,
    pub shown_to: f64,
    /// The tracks framed down the view, by GUID (none: every track).
    pub tracks: Vec<String>,
}

/// What a view frames, worked out.
#[derive(Facet, Clone, Debug, PartialEq)]
pub struct SpanOut {
    /// Seconds.
    pub span: Option<(f64, f64)>,
    pub tracks: Vec<String>,
}

/// What to look at.
#[derive(Facet)]
pub struct ViewArg {
    pub from: Option<f64>,
    pub to: Option<f64>,
    pub bars: Option<Vec<i32>>,
    pub section: Option<String>,
    /// Which of the sections of that name, from 1 (1).
    pub nth: Option<u32>,
    pub tracks: Option<Vec<String>>,
}
