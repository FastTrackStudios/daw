//! The DAW's timeline state: what every view of a song and the engine
//! behind them agree on, as plain data.
//!
//! - [`cursor`]: the play cursor, the edit cursor, the time selection, and
//!   where Stop leaves the cursor;
//! - [`select`]: what is selected, in the order it was picked;
//! - [`clipboard`]: things lifted off the timeline, kept relative to the
//!   earliest one and the lowest row.
//!
//! How a view draws or gestures at the timeline (zoom, scroll, the grid,
//! the mouse map) is the view kit's (`timeline`), which re-exports these.

pub mod clipboard;
pub mod cursor;
pub mod select;
