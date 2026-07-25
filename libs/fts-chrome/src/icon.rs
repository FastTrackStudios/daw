//! The chrome's icon set — hand-rolled inline SVG.
//!
//! Deliberately not `lucide-dioxus`: the rig UIs render through Blitz, which
//! wants inline markup and no asset loading, and the chrome needs ~20 glyphs,
//! not an icon library. Every glyph is drawn on a 24×24 grid with a 2px round
//! stroke so they sit on one optical weight.

use dioxus::prelude::*;

/// A glyph. Views name one when they register a rail destination or a panel;
/// the chrome draws it. An enum (not an `Element`) so specs stay `PartialEq`
/// and can live in a signal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    // Workspaces.
    Home,
    Signal,
    Session,
    Arrangement,
    Mixer,
    Lyrics,
    Charts,
    // Rigs.
    Guitar,
    Bass,
    Drums,
    Keys,
    Synth,
    // Panels.
    Browser,
    Midi,
    Routing,
    Logs,
    Settings,
    Engine,
    Perform,
    // Chrome affordances.
    RailLeft,
    RailRight,
    Close,
    Minimize,
    Maximize,
}

impl Icon {
    /// The glyph's paths, drawn stroked on a 24×24 view box.
    fn paths(self) -> &'static [&'static str] {
        match self {
            Icon::Home => &["M3 10.5 12 3l9 7.5", "M5 9.5V21h14V9.5"],
            // A signal chain: source → node → out.
            Icon::Signal => &["M3 12h4", "M17 12h4", "M9 12h6", "M12 8v8"],
            Icon::Session => &["M4 5h16v14H4z", "M4 10h16", "M9 10v9"],
            Icon::Arrangement => &["M3 6h18", "M3 12h12", "M3 18h7"],
            Icon::Mixer => &["M6 3v18", "M12 3v18", "M18 3v18", "M3 8h6", "M9 15h6", "M15 6h6"],
            Icon::Lyrics => &["M5 4h14v16H5z", "M8 9h8", "M8 13h8", "M8 17h4"],
            Icon::Charts => &["M4 4v16h16", "M8 15l3-4 3 3 4-6"],
            Icon::Guitar => &["M14 4l6 6", "M11 7l6 6", "M9 9a5 5 0 1 0 6 6z"],
            Icon::Bass => &["M15 3l6 6", "M12 6l6 6", "M10 8a6 6 0 1 0 6 6z"],
            Icon::Drums => &["M3 9h18", "M3 9v7c0 2 4 3 9 3s9-1 9-3V9", "M12 6a9 3 0 1 0 0 6a9 3 0 1 0 0-6"],
            Icon::Keys => &["M3 5h18v14H3z", "M8 5v9", "M13 5v9", "M18 5v9"],
            Icon::Synth => &["M3 15c3 0 3-8 6-8s3 8 6 8 3-8 6-8", "M3 20h18"],
            Icon::Browser => &["M4 5h16v14H4z", "M4 9h16", "M8 13h8", "M8 16h5"],
            Icon::Midi => &["M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z", "M8 10v.01", "M16 10v.01", "M12 8v.01", "M9 15h6"],
            Icon::Routing => &["M6 4v5a3 3 0 0 0 3 3h9", "M6 20v-5", "M18 9l3 3-3 3"],
            Icon::Logs => &["M5 4h14v16H5z", "M9 8h6", "M9 12h6", "M9 16h3"],
            Icon::Settings => &[
                "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
                "M12 2v3", "M12 19v3", "M2 12h3", "M19 12h3",
                "M5 5l2 2", "M17 17l2 2", "M19 5l-2 2", "M7 17l-2 2",
            ],
            Icon::Engine => &["M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z", "M12 7v5l3 3"],
            Icon::Perform => &["M4 6h4v12H4z", "M11 6h2v12h-2z", "M16 6h4v12h-4z"],
            Icon::RailLeft => &["M4 5h16v14H4z", "M10 5v14"],
            Icon::RailRight => &["M4 5h16v14H4z", "M14 5v14"],
            Icon::Close => &["M6 6l12 12", "M18 6L6 18"],
            Icon::Minimize => &["M5 12h14"],
            Icon::Maximize => &["M5 5h14v14H5z"],
        }
    }
}

/// Render a glyph at `size` px in `currentColor`.
#[component]
pub fn Glyph(icon: Icon, #[props(default = 16)] size: u32) -> Element {
    rsx! {
        svg {
            width: "{size}", height: "{size}", view_box: "0 0 24 24", fill: "none",
            stroke: "currentColor", stroke_width: "1.8",
            stroke_linecap: "round", stroke_linejoin: "round",
            style: "display: block; flex-shrink: 0;",
            for (i, d) in icon.paths().iter().enumerate() {
                path { key: "{i}", d: "{d}" }
            }
        }
    }
}
