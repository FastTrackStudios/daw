//! Region/marker lane tests retired alongside the architect::rpc
//! ports of the `Markers` and `Regions` traits. The sync traits carry
//! only the canonical CRUD verbs; lane placement (set_marker_lane,
//! set_region_lane) lives on a sibling trait when revived.
//!
//! Kept as an empty integration target so future tests have a place to
//! land when the sibling trait is added.
