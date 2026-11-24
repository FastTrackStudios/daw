//! The gallery: the named showcase shots a theme pack is pictured in.
//!
//! Every pack is shown the same way: the same song (the views' golden
//! session, built by code in the host's view tools), framed the same way,
//! at the same sizes. A shot says which view it is of, how that view is
//! framed and how big the picture is, and which of the spec's tokens and
//! parts it puts on screen. A renderer (Studio's `daw-view-tools
//! gallery`) draws each shot for each theme of a pack, and [`readme`]
//! writes the page a pack repository keeps beside the pictures
//! (`gallery/README.md`).
//!
//! [`uncovered`] holds the manifest to the spec: every token and part is
//! on screen in at least one shot, or named in [`UNSHOWN`] with the reason
//! no shot can show it.
//!
//! The frames name the golden session's tracks and times, so a shot is
//! only meaningful against the session version it was written for:
//! [`SESSION`]. A renderer refuses a manifest written for another one.

use crate::spec;

/// The golden session's version these shots frame.
pub const SESSION: u32 = 1;

/// The view a shot is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The arrangement: the track panel, the ruler, the lanes.
    Arrangement,
    /// The mixer: its strips, the racks and the rails.
    Mixer,
    /// The FX view: a strip's mix phases, macro by macro.
    Fx,
    /// The MIDI editor, its notes on a kit's lanes.
    Drums,
    /// The MIDI editor, its notes on a piano roll.
    Piano,
}

impl View {
    /// The view's name, as a host's `Uses` names it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Arrangement => "arrangement",
            Self::Mixer => "mixer",
            Self::Fx => "fx",
            Self::Drums | Self::Piano => "editor",
        }
    }
}

/// A picture's size, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

/// A desktop window.
pub const DESKTOP: Size = Size {
    width: 1600,
    height: 900,
};

/// A phone held upright.
pub const PHONE: Size = Size {
    width: 390,
    height: 844,
};

/// A close look at a few tracks or strips.
pub const CLOSEUP: Size = Size {
    width: 900,
    height: 600,
};

/// A stretch of the timeline, in seconds: `from` at the lanes' left edge,
/// `to` at their right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub from: f64,
    pub to: f64,
}

/// What is open over the view when it is shot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    None,
    /// The view's context menu, open where a right-click left it.
    Menu,
}

/// How a view is framed for a shot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The time across the view; `None` is the whole song.
    pub time: Option<Span>,
    /// The track (or strip) at the top (or the left), by its name in the
    /// golden session; `None` is the first.
    pub first: Option<&'static str>,
    /// The rows' height against the view's own, 1 being as it opens.
    pub zoom_y: f64,
    pub overlay: Overlay,
}

impl Frame {
    /// The view as it opens on the golden session.
    pub const OPENING: Self = Self {
        time: None,
        first: None,
        zoom_y: 1.0,
        overlay: Overlay::None,
    };
}

/// One showcase shot.
#[derive(Clone, Copy, Debug)]
pub struct Shot {
    /// `<view>.<subject>`, and `.phone` for a phone's version of one.
    pub name: &'static str,
    pub title: &'static str,
    /// What the shot is for, a sentence.
    pub about: &'static str,
    pub view: View,
    pub size: Size,
    pub frame: Frame,
    /// The spec's tokens the shot puts on screen, in groups.
    pub tokens: &'static [&'static [&'static str]],
    /// The spec's parts (pages and components) it draws, in groups.
    pub parts: &'static [&'static [&'static str]],
}

impl Shot {
    /// The shot named `name`.
    #[must_use]
    pub fn named(name: &str) -> Option<&'static Self> {
        SHOTS.iter().find(|s| s.name == name)
    }

    /// The tokens the shot puts on screen.
    pub fn tokens(&self) -> impl Iterator<Item = &'static str> {
        self.tokens.iter().flat_map(|g| g.iter().copied())
    }

    /// The parts it draws.
    pub fn parts(&self) -> impl Iterator<Item = &'static str> {
        self.parts.iter().flat_map(|g| g.iter().copied())
    }

    /// Whether this shot puts `name`, a token or a part, on screen.
    #[must_use]
    pub fn shows(&self, name: &str) -> bool {
        self.tokens().chain(self.parts()).any(|n| n == name)
    }
}

// The groups of names the views draw everywhere they appear.

/// The ink and the ground every view sets text on.
const INK: &[&str] = &["text", "text.dim", "text.faint", "surface", "accent"];

/// The arrangement's ground, its grid and its ruler.
const LANES: &[&str] = &[
    "arrange.ground",
    "arrange.row",
    "arrange.row_alt",
    "arrange.divider",
    "grid.bar",
    "grid.beat",
    "grid.sub",
    "ruler.ground",
    "ruler.ink",
    "ruler.ink_dim",
    "playhead",
    "cursor.edit",
    "clip.edge",
    "clip.alpha",
];

/// The track panel: its rows, its column and the controls a row places.
const PANEL: &[&str] = &[
    "tcp.tint",
    "tcp.column",
    "tint.strength",
    "surface.raised",
    "surface.sunken",
    "border",
    "hardware",
    "hardware.edge",
    "hardware.mark",
    "field",
    "selected",
];

/// The tokens a pack's own pages read as `$name` (a strip's label, its
/// corners, its type). The views never read them natively; a shot of a
/// pack's pages shows them when the pack uses them.
const PAGE_TYPE: &[&str] = &[
    "text.bright",
    "radius.control",
    "size.label",
    "size.small",
    "font.ui",
];

/// The shared controls a track panel row and a mixer strip both place.
const CONTROLS: &[&str] = &[
    "mute", "solo", "arm", "monitor", "phase", "pan", "volume", "fx", "routing",
];

/// The clip palette, every entry.
const CLIPS: &[&str] = &[
    "clip.0", "clip.1", "clip.2", "clip.3", "clip.4", "clip.5", "clip.6", "clip.7", "clip.8",
    "clip.9", "clip.10", "clip.11", "clip.12", "clip.13", "clip.14", "clip.15",
];

/// A mixer strip's states and its meters.
const STRIP: &[&str] = &[
    "mute",
    "solo",
    "rec",
    "pan",
    "monitor.tape",
    "route.out",
    "route.send",
    "route.recv",
    "plate",
    "sheen",
    "combo",
    "meter.safe",
    "meter.warn",
    "meter.danger",
    "meter.well",
];

/// The mixer's rails and its FX rack.
const RACK: &[&str] = &["rail.ground", "rail.rule", "rail.button", "rack.well"];

/// The MIDI editor's chrome, common to both of its row spaces.
const EDITOR: &[&str] = &[
    "editor.bg",
    "editor.grid_beat",
    "editor.grid_sub",
    "editor.panel",
    "editor.panel_border",
    "editor.border_strong",
    "editor.surface_inset",
    "editor.surface_bar",
    "editor.gutter_bg",
    "editor.text",
    "editor.text_dim",
    "editor.text_bright",
    "editor.text_faint",
    "editor.accent",
    "editor.selected",
    "editor.playhead",
    "editor.control",
    "editor.control_active",
    "editor.control_selected",
];

/// The pitch classes' colours on a piano roll.
const PITCHES: &[&str] = &[
    "editor.pitch_0",
    "editor.pitch_1",
    "editor.pitch_2",
    "editor.pitch_3",
    "editor.pitch_4",
    "editor.pitch_5",
    "editor.pitch_6",
    "editor.pitch_7",
    "editor.pitch_8",
    "editor.pitch_9",
    "editor.pitch_10",
    "editor.pitch_11",
];

/// A kit's pieces' colours on drum lanes.
const KIT: &[&str] = &[
    "editor.drum_kick",
    "editor.drum_snare",
    "editor.drum_tom",
    "editor.drum_hat",
    "editor.drum_ride",
    "editor.drum_cymbal",
    "editor.drum_other",
];

/// The editor's pages on every shot of it.
const EDITOR_PAGES: &[&str] = &[
    "editor.bar",
    "editor.button",
    "editor.lane",
    "editor.ruler",
    "editor.tick",
    "editor.marker",
    "editor.strip",
];

/// The golden session's song runs this long: the overview frames it all.
const SONG: Span = Span {
    from: 0.0,
    to: 96.0,
};

/// Every shot, in the order a gallery shows them.
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "arrangement.overview",
        title: "Arrangement",
        about: "The whole song: folders three deep, every clip colour, sections, markers, tempo and meter changes, the edit cursor, a time selection and the playhead.",
        view: View::Arrangement,
        size: DESKTOP,
        frame: Frame {
            time: Some(SONG),
            ..Frame::OPENING
        },
        tokens: &[
            INK,
            LANES,
            PANEL,
            PAGE_TYPE,
            CLIPS,
            &["mute", "solo", "rec", "pan"],
        ],
        parts: &[&["tcp.row", "controls"], CONTROLS],
    },
    Shot {
        name: "arrangement.track-states",
        title: "Track states",
        about: "One track per state: muted, soloed, solo-safe, armed, monitoring on and auto, polarity flipped, FX bypassed and missing, selected, several selected, a focused track, and a track with no colour.",
        view: View::Arrangement,
        size: DESKTOP,
        frame: Frame {
            time: Some(Span {
                from: 0.0,
                to: 32.0,
            }),
            first: Some("States"),
            zoom_y: 1.0,
            overlay: Overlay::None,
        },
        tokens: &[INK, PANEL, &["mute", "solo", "rec", "pan", "combo"]],
        parts: &[&["tcp.row"], CONTROLS],
    },
    Shot {
        name: "arrangement.clips",
        title: "Clips",
        about: "Audio drawn from synthesized waveforms (a kick, a tone, noise, silence, a clipped bus) and MIDI notes; clips selected, muted, locked, faded, crossfaded, overlapping, on take lanes, and past both edges of the view.",
        view: View::Arrangement,
        size: DESKTOP,
        frame: Frame {
            time: Some(Span {
                from: 4.0,
                to: 36.0,
            }),
            first: Some("Clips"),
            zoom_y: 1.4,
            overlay: Overlay::None,
        },
        tokens: &[INK, LANES, CLIPS, &["selected"]],
        parts: &[&["tcp.row"]],
    },
    Shot {
        name: "arrangement.ruler-tempo",
        title: "Ruler and tempo",
        about: "The ruler close up across the tempo and meter changes (4/4, 7/8, 3/4), the sections of every colour and the markers above them.",
        view: View::Arrangement,
        size: DESKTOP,
        frame: Frame {
            time: Some(Span {
                from: 28.0,
                to: 60.0,
            }),
            ..Frame::OPENING
        },
        tokens: &[INK, LANES],
        parts: &[],
    },
    Shot {
        name: "arrangement.automation",
        title: "Automation",
        about: "Volume, pan, mute and a plugin parameter, each in its mode, with points of every shape.",
        view: View::Arrangement,
        size: DESKTOP,
        frame: Frame {
            time: Some(Span {
                from: 0.0,
                to: 32.0,
            }),
            first: Some("Automation"),
            zoom_y: 1.0,
            overlay: Overlay::None,
        },
        tokens: &[INK, LANES, &["surface.sunken", "tcp.column"]],
        parts: &[&["tcp.row"]],
    },
    Shot {
        name: "tcp.closeup",
        title: "Track panel",
        about: "A few rows of the track panel tall enough to read every control: names, numbers, the folder buttons, the controls a row places, a long name cut short.",
        view: View::Arrangement,
        size: CLOSEUP,
        frame: Frame {
            time: Some(Span {
                from: 0.0,
                to: 16.0,
            }),
            first: Some("States"),
            zoom_y: 2.5,
            overlay: Overlay::None,
        },
        tokens: &[INK, PANEL, PAGE_TYPE, &["mute", "solo", "rec", "pan"]],
        parts: &[&["tcp.row", "controls"], CONTROLS],
    },
    Shot {
        name: "mixer.overview",
        title: "Mixer",
        about: "Every strip of the song with its rails: folders nested and stepped, the busses and the master with its FX, faders across their range, pans hard left, centre and right.",
        view: View::Mixer,
        size: DESKTOP,
        frame: Frame::OPENING,
        tokens: &[
            INK,
            STRIP,
            RACK,
            PAGE_TYPE,
            &[
                "surface.raised",
                "surface.sunken",
                "border",
                "hardware",
                "hardware.edge",
                "hardware.mark",
                "field",
                "selected",
                "tcp.tint",
                "tcp.column",
                "tint.strength",
            ],
        ],
        parts: &[
            &[
                "mcp.strip",
                "mcp.step",
                "mcp.rackback",
                "rails",
                "rail.button",
                "controls",
            ],
            CONTROLS,
        ],
    },
    Shot {
        name: "mixer.strip-states",
        title: "Strip states",
        about: "The strips of the state tracks: muted, soloed, armed, monitoring, polarity, FX bypassed and missing, sends and receives, selected.",
        view: View::Mixer,
        size: DESKTOP,
        frame: Frame {
            first: Some("States"),
            ..Frame::OPENING
        },
        tokens: &[INK, STRIP, &["selected", "hardware", "hardware.mark"]],
        parts: &[&["mcp.strip"], CONTROLS],
    },
    Shot {
        name: "mixer.inserts-signal",
        title: "Inserts and Signal",
        about: "The Tone rack's units on the kit, and a Signal FX open block by block beside one shut, with a block bypassed, one in error and one unavailable.",
        view: View::Mixer,
        size: DESKTOP,
        frame: Frame {
            first: Some("Kick"),
            ..Frame::OPENING
        },
        tokens: &[INK, RACK, &["meter.safe", "meter.warn", "hardware"]],
        parts: &[&[
            "mcp.strip",
            "mcp.rackback",
            "rack.unit",
            "rack.rail",
            "rack.phase",
        ]],
    },
    Shot {
        name: "mixer.meters",
        title: "Meters",
        about: "Meters frozen at chosen levels: quiet, near 0 dB, over, clipped, with their peak holds.",
        view: View::Mixer,
        size: CLOSEUP,
        frame: Frame {
            first: Some("Meters"),
            ..Frame::OPENING
        },
        tokens: &[
            INK,
            &["meter.safe", "meter.warn", "meter.danger", "meter.well"],
        ],
        parts: &[&["mcp.strip", "volume"]],
    },
    Shot {
        name: "fx.phases",
        title: "FX phases",
        about: "The FX view on the kick: each mix phase's header and its macros.",
        view: View::Fx,
        size: DESKTOP,
        frame: Frame {
            first: Some("Kick"),
            ..Frame::OPENING
        },
        tokens: &[INK, &["surface.raised", "hardware", "hardware.mark"]],
        parts: &[&["fx.phase", "fx.macro"]],
    },
    Shot {
        name: "editor.drums",
        title: "Drum lanes",
        about: "The drum groove on the kit's lanes, every piece in its colour, velocities in the strip below.",
        view: View::Drums,
        size: DESKTOP,
        frame: Frame {
            first: Some("Drum MIDI"),
            ..Frame::OPENING
        },
        tokens: &[EDITOR, KIT, &["editor.row_a", "editor.row_b"]],
        parts: &[EDITOR_PAGES, &["editor.keygroup"]],
    },
    Shot {
        name: "editor.piano",
        title: "Piano roll",
        about: "The melody on a piano roll: the keys, a line at every C, each pitch in its colour, velocities below.",
        view: View::Piano,
        size: DESKTOP,
        frame: Frame {
            first: Some("Melody"),
            ..Frame::OPENING
        },
        tokens: &[
            EDITOR,
            PITCHES,
            &[
                "editor.row_a",
                "editor.row_b",
                "editor.key_white",
                "editor.key_black",
                "editor.key_label",
                "editor.octave_line",
            ],
        ],
        parts: &[EDITOR_PAGES, &["editor.keys", "editor.key"]],
    },
    Shot {
        name: "editor.menu",
        title: "Editor menu",
        about: "The editor's context menu, open over the groove.",
        view: View::Drums,
        size: CLOSEUP,
        frame: Frame {
            first: Some("Drum MIDI"),
            overlay: Overlay::Menu,
            ..Frame::OPENING
        },
        tokens: &[&[
            "editor.panel",
            "editor.text",
            "editor.text_dim",
            "editor.control_hover",
        ]],
        parts: &[&["editor.menu", "editor.menuitem"]],
    },
    Shot {
        name: "arrangement.overview.phone",
        title: "Arrangement, phone",
        about: "The arrangement at a phone's width.",
        view: View::Arrangement,
        size: PHONE,
        frame: Frame {
            time: Some(Span {
                from: 0.0,
                to: 16.0,
            }),
            ..Frame::OPENING
        },
        tokens: &[INK, LANES, PANEL],
        parts: &[&["tcp.row"]],
    },
    Shot {
        name: "mixer.overview.phone",
        title: "Mixer, phone",
        about: "The mixer at a phone's width.",
        view: View::Mixer,
        size: PHONE,
        frame: Frame::OPENING,
        tokens: &[INK, STRIP],
        parts: &[&["mcp.strip", "rails"]],
    },
    Shot {
        name: "editor.piano.phone",
        title: "Piano roll, phone",
        about: "The piano roll at a phone's width.",
        view: View::Piano,
        size: PHONE,
        frame: Frame {
            first: Some("Melody"),
            ..Frame::OPENING
        },
        tokens: &[EDITOR, &["editor.key_white", "editor.key_black"]],
        parts: &[&["editor.keys", "editor.lane"]],
    },
];

/// The spec's names no shot can show, each with the reason.
pub const UNSHOWN: &[(&str, &str)] = &[
    (
        "strip.live",
        "a switch, not a look: it says the pack draws the live (performance) strip, which the gallery's mixer does not lay out",
    ),
    (
        "strip.touch",
        "a switch, not a look: it says the pack draws the touch strip, which only a touchscreen's mixer lays out",
    ),
    (
        "editor.razor",
        "a razor area exists only while one is dragged; the editor has no headless way to hold one",
    ),
    (
        "editor.gold",
        "the reference take is a comping state of a stack of takes; the gallery's editor opens one take",
    ),
    (
        "editor.reference",
        "a reference lane is drawn only beside a stack of takes being compared",
    ),
    (
        "editor.zone",
        "a zone is drawn by the audio editor's leveling, which the gallery does not open",
    ),
    (
        "editor.handle",
        "a drag handle shows under the pointer only",
    ),
    (
        "editor.peaks",
        "a waveform behind notes is drawn when the editor opens an audio take; the gallery opens MIDI",
    ),
];

/// Every spec name (token or part) that no shot shows and [`UNSHOWN`] does
/// not explain.
#[must_use]
pub fn uncovered() -> Vec<&'static str> {
    spec::TOKENS
        .iter()
        .map(|t| t.name)
        .chain(spec::PARTS.iter().map(|p| p.name))
        .filter(|name| !SHOTS.iter().any(|s| s.shows(name)))
        .filter(|name| !UNSHOWN.iter().any(|(n, _)| n == name))
        .collect()
}

/// A picture's path in a gallery: `<theme>/<shot>.png`.
#[must_use]
pub fn image_path(theme: &str, shot: &str) -> String {
    format!("{theme}/{shot}.png")
}

/// One theme of a pack, as a gallery page lists it.
#[derive(Clone, Copy, Debug)]
pub struct Pictured<'a> {
    pub name: &'a str,
    pub title: &'a str,
}

/// The gallery's page (`gallery/README.md`): for each theme, a table of the
/// shots with their pictures, linked relative to the page. The same pack
/// and shots always write the same page.
#[must_use]
pub fn readme(pack_title: &str, themes: &[Pictured<'_>], shots: &[&Shot]) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "# {pack_title}: gallery\n");
    let _ = writeln!(
        out,
        "Every shot of the daw-theme gallery (spec {}, golden session {SESSION}), \
         rendered by `daw-view-tools gallery`. Generated: render it again rather \
         than editing it.\n",
        spec::VERSION
    );
    for theme in themes {
        let _ = writeln!(out, "## {} (`{}`)\n", theme.title, theme.name);
        let _ = writeln!(out, "| Shot | What it shows | Picture |");
        let _ = writeln!(out, "| --- | --- | --- |");
        for shot in shots {
            let path = image_path(theme.name, shot.name);
            let _ = writeln!(
                out,
                "| **{}**<br>`{}`<br>{}×{} | {} | [![{}]({path})]({path}) |",
                shot.title, shot.name, shot.size.width, shot.size.height, shot.about, shot.title,
            );
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spec_token_and_part_is_shown_or_explained() {
        assert_eq!(
            uncovered(),
            Vec::<&str>::new(),
            "these spec names appear in no shot and are not in UNSHOWN"
        );
    }

    #[test]
    fn every_name_a_shot_lists_is_in_the_spec() {
        for shot in SHOTS {
            for t in shot.tokens() {
                assert!(spec::token(t).is_some(), "{}: token `{t}`", shot.name);
            }
            for p in shot.parts() {
                assert!(spec::part(p).is_some(), "{}: part `{p}`", shot.name);
            }
        }
    }

    #[test]
    fn an_unshown_name_is_in_the_spec_and_in_no_shot() {
        for (name, why) in UNSHOWN {
            assert!(
                spec::token(name).is_some() || spec::part(name).is_some(),
                "`{name}` is not in the spec"
            );
            assert!(!why.is_empty(), "`{name}` has no reason");
            assert!(
                !SHOTS.iter().any(|s| s.shows(name)),
                "`{name}` is shown by a shot and also listed unshown"
            );
        }
    }

    #[test]
    fn shot_names_are_unique_and_say_their_view() {
        let mut seen = std::collections::BTreeSet::new();
        for shot in SHOTS {
            assert!(seen.insert(shot.name), "{} twice", shot.name);
            assert!(
                shot.name.split('.').count() >= 2,
                "{}: `<view>.<subject>`",
                shot.name
            );
            if shot.name.rsplit('.').next() == Some("phone") {
                assert_eq!(shot.size, PHONE, "{}", shot.name);
            }
        }
    }

    #[test]
    fn the_main_shots_have_a_phone_version() {
        for main in ["arrangement.overview", "mixer.overview", "editor.piano"] {
            assert!(Shot::named(main).is_some(), "{main}");
            assert!(
                Shot::named(&format!("{main}.phone")).is_some(),
                "{main}.phone"
            );
        }
    }

    #[test]
    fn a_frame_fits_its_span() {
        for shot in SHOTS {
            if let Some(span) = shot.frame.time {
                assert!(span.to > span.from, "{}", shot.name);
            }
            assert!(shot.frame.zoom_y > 0.0, "{}", shot.name);
        }
    }

    #[test]
    fn the_page_lists_every_shot_for_every_theme() {
        let shots: Vec<&Shot> = SHOTS.iter().collect();
        let themes = [
            Pictured {
                name: "dark",
                title: "Dark",
            },
            Pictured {
                name: "light",
                title: "Light",
            },
        ];
        let page = readme("Test", &themes, &shots);
        for t in &themes {
            for s in &shots {
                assert!(
                    page.contains(&image_path(t.name, s.name)),
                    "{} {}",
                    t.name,
                    s.name
                );
            }
        }
        assert_eq!(
            page,
            readme("Test", &themes, &shots),
            "the page is the same twice"
        );
    }
}
