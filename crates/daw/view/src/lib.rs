//! The contract a DAW's views are built on.
//!
//! An engine implements it; a view (the arrangement, the mixer, the MIDI
//! editor) depends on it alone, never on the app or the engine it runs
//! in:
//!
//! - **the protocol** ([`view`]: `EngineView`, its frames and asks) and a
//!   window's **link** to its engine ([`view_client`], [`window_queue`],
//!   [`wake`], [`changes`]);
//! - **the song** a view draws ([`song`]: the project, its rows, the
//!   read-back after an edit), its items' previews ([`midi`],
//!   [`wave_detail`]), lanes and automation ([`lanes`], [`automation`],
//!   [`leveler`]);
//! - **the transport and the edits** a view sends ([`engine`]), the
//!   edit cursor and selection ([`edit_state`]), where Stop leaves the
//!   cursor ([`play_return`]), the editing options ([`options`]), zooms
//!   asked of a view ([`zoom`]);
//! - **the overlays** an engine publishes ([`hit_overlay`], [`poi`],
//!   [`track_regions`], [`kit_gate`]) and what it is doing ([`jobs`]);
//! - **the Action List** a view's controls run ([`actions`]);
//! - the two hooks a window's modules reach its process through: the
//!   engine's [`body`] and the app's [`host`].

pub mod actions;
pub mod automation;
pub mod body;
pub mod changes;
pub mod edit_state;
pub mod engine;
pub mod folds;
pub mod hit_overlay;
pub mod host;
pub mod jobs;
pub mod kit_gate;
pub mod lanes;
pub mod leveler;
pub mod midi;
pub mod num;
pub mod open;
pub mod open_progress;
pub mod options;
#[cfg(feature = "native")]
pub mod page;
pub mod plan;
pub mod play_return;
pub mod poi;
pub mod routes;
pub mod scale;
pub mod song;
pub mod stretch;
pub mod track_regions;
pub mod view;
#[cfg(feature = "native")]
pub mod view_client;
pub mod wake;
pub mod wave_detail;
#[cfg(feature = "native")]
pub mod wave_pool;
pub mod window_queue;
pub mod zoom;
