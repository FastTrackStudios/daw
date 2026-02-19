//! Standalone DAW Cell Binary
//!
//! This is the entry point for running daw-standalone as a cell.
//! The actual implementations are in lib.rs for reuse in tests.
//!
//! ## Services Provided
//!
//! - **TransportService**: Play/pause/stop, position, tempo, looping
//! - **ProjectService**: Project and track management
//! - **MarkerService**: SONGSTART/SONGEND markers for song boundaries
//! - **RegionService**: Section regions (Intro, Verse, Chorus, etc.)
//! - **TempoMapService**: Tempo and time signature changes

fn main() {
    todo!("Wire up new cell runtime — old cell_runtime/run_cell! infrastructure was removed")
}
