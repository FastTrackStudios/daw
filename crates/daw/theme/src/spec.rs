//! The spec: every token, part and field a theme pack can implement.
//!
//! - A **token** is a named value a pack sets in its `<Variables>` (a
//!   colour, a size, a font, a switch). The views read them natively, and
//!   a pack's own pages read them as `$name`.
//! - A **part** is something a pack draws: a **page** a view asks for by
//!   name (`mcp.strip`, `tcp.row`, `editor.keys`, …) or a shared
//!   **component** the pages place (`mute`, `solo`, `volume`, …), so a mute
//!   looks the same on a strip as in the track panel.
//! - A **field** is a value a view hands a page while it draws
//!   (`track.name`, `button.on`, `meter.left`, …), bound as `bind:…`.
//!
//! The spec is versioned ([`VERSION`]): a pack says which version it was
//! written against, and a newer spec only ever adds names.

use frame::theme::runtime::Schema;

/// The spec's version. Bumped when a name is added; names are never taken
/// away or renamed within a major version.
pub const VERSION: u32 = 1;

/// A pack's private tokens start with this, and are never reported as
/// unknown: `local.cap.shadow`, `local.groove`.
pub const LOCAL: &str = "local.";

/// What kind of value a token holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Color,
    /// A length in px, a ratio or a count.
    Number,
    /// A font family's name.
    Font,
    /// A switch.
    Flag,
}

/// What a token is when a pack does not set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    /// Whatever another token is (which may fall back in turn).
    Token(&'static str),
    /// A value of its own, in Frame's syntax (`#rrggbbaa`, `3`, `true`).
    Value(&'static str),
    /// The view's own built-in look.
    Host,
}

/// One token of the spec.
#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub name: &'static str,
    pub kind: Kind,
    pub fallback: Fallback,
    pub doc: &'static str,
}

/// Whether a part is a page a view draws or a component pages place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    Page,
    Component,
}

/// One part of the spec.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub name: &'static str,
    pub kind: PartKind,
    /// Pages named `<name>.<anything>` are this part too, a more
    /// particular version of it (`fx.macro.tone.kick`).
    pub family: bool,
    pub doc: &'static str,
}

/// When a view hands a field to a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// When the page is recorded.
    Static,
    /// When the page is laid out.
    Layout,
    /// Every frame.
    Live,
}

/// One field of the spec.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub name: &'static str,
    pub class: Class,
}

const fn color(name: &'static str, fallback: Fallback, doc: &'static str) -> Token {
    Token {
        name,
        kind: Kind::Color,
        fallback,
        doc,
    }
}

const fn number(name: &'static str, fallback: Fallback, doc: &'static str) -> Token {
    Token {
        name,
        kind: Kind::Number,
        fallback,
        doc,
    }
}

const fn flag(name: &'static str, fallback: Fallback, doc: &'static str) -> Token {
    Token {
        name,
        kind: Kind::Flag,
        fallback,
        doc,
    }
}

const fn font(name: &'static str, fallback: Fallback, doc: &'static str) -> Token {
    Token {
        name,
        kind: Kind::Font,
        fallback,
        doc,
    }
}

use Fallback::{Host, Token as To, Value as Is};

/// Every token, by group.
pub const TOKENS: &[Token] = &[
    // ── ink ───────────────────────────────────────────────────────────
    color("text", Host, "text on a surface"),
    color("text.dim", Host, "secondary text"),
    color("text.faint", Host, "tertiary text; a track with no colour"),
    color("text.bright", Host, "text that must stand out"),
    color("accent", Host, "selection, focus and the output route"),
    color("selected", Host, "a selected item's ring"),
    // ── surfaces ──────────────────────────────────────────────────────
    color("surface", Host, "the window's ground"),
    color("surface.raised", Host, "a panel over the ground"),
    color("surface.sunken", Host, "a well cut into a panel"),
    color("border", Host, "a panel's edge"),
    color("hardware", Host, "a control's face"),
    color("hardware.edge", Host, "a control's edge"),
    color("hardware.mark", Host, "the ink printed on a control"),
    color("plate", Is("#00000059"), "the plate a control sits on"),
    color("sheen", Is("#ffffff12"), "a control's highlight"),
    color("field", Host, "a text field's ground"),
    color("combo", Host, "a drop-down's ground"),
    // ── states ────────────────────────────────────────────────────────
    color("mute", Host, "mute, lit"),
    color("solo", Host, "solo, lit"),
    color("rec", Host, "record arm, lit"),
    color("pan", Host, "the pan knob's arc"),
    color(
        "monitor.tape",
        Is("#f08c2e"),
        "input monitoring: tape style",
    ),
    color("route.out", To("accent"), "the output route"),
    color("route.send", To("meter.warn"), "a send"),
    color("route.recv", To("meter.danger"), "a receive"),
    // ── meters ────────────────────────────────────────────────────────
    color("meter.safe", Host, "a meter below -18 dB"),
    color("meter.warn", Host, "a meter near 0 dB"),
    color("meter.danger", Host, "a meter over 0 dB"),
    color("meter.well", To("surface.sunken"), "behind a meter"),
    // ── the track panel and the mixer's rails ─────────────────────────
    color("tcp.tint", Host, "a track panel row's ground"),
    color("tcp.column", Host, "the track panel's control column"),
    color("rail.ground", Host, "the rail beside the track panel"),
    color("rail.rule", Host, "the rules between rows"),
    color("rail.button", Host, "a rail's button"),
    color("rack.well", Host, "the FX rack's well"),
    number(
        "tint.strength",
        Host,
        "how much a track's colour tints its row, 0 to 1",
    ),
    flag(
        "strip.live",
        Is("false"),
        "the pack draws the live (performance) strip",
    ),
    flag("strip.touch", Is("false"), "the pack draws the touch strip"),
    // ── the arrangement ───────────────────────────────────────────────
    color("arrange.ground", Host, "the arrangement behind its rows"),
    color("arrange.row", Host, "a row"),
    color("arrange.row_alt", To("arrange.row"), "every other row"),
    color("arrange.divider", Host, "the line under a row"),
    color("grid.bar", Host, "a bar line"),
    color("grid.beat", Host, "a beat line"),
    color("grid.sub", Host, "a line finer than a beat"),
    color("ruler.ground", Host, "the ruler"),
    color("ruler.ink", Host, "the ruler's bar numbers"),
    color("ruler.ink_dim", Host, "the ruler's beats"),
    color("playhead", Host, "the play cursor"),
    color("clip.edge", Host, "a clip's outline"),
    number("clip.alpha", Host, "how opaque a clip's body is, 0 to 1"),
    color(
        "clip.0",
        Host,
        "the clip palette: a track's colour snaps to the nearest entry",
    ),
    color("clip.1", Host, "the clip palette"),
    color("clip.2", Host, "the clip palette"),
    color("clip.3", Host, "the clip palette"),
    color("clip.4", Host, "the clip palette"),
    color("clip.5", Host, "the clip palette"),
    color("clip.6", Host, "the clip palette"),
    color("clip.7", Host, "the clip palette"),
    color("clip.8", Host, "the clip palette"),
    color("clip.9", Host, "the clip palette"),
    color("clip.10", Host, "the clip palette"),
    color("clip.11", Host, "the clip palette"),
    color("clip.12", Host, "the clip palette"),
    color("clip.13", Host, "the clip palette"),
    color("clip.14", Host, "the clip palette"),
    color("clip.15", Host, "the clip palette"),
    // ── sizes and type, for a pack's pages ────────────────────────────
    number("radius.control", Is("3"), "a control's corner radius, px"),
    number("size.label", Is("11"), "a label's text size, px"),
    number("size.small", Is("9"), "small text (numbers, scales), px"),
    font("font.ui", Is("Inter"), "the family labels are set in"),
    // ── the MIDI editor ───────────────────────────────────────────────
    color("editor.bg", Host, "the editor's ground"),
    color("editor.row_a", Host, "a key row"),
    color("editor.row_b", Host, "a black key's row"),
    color("editor.grid_beat", Host, "a beat line"),
    color("editor.grid_sub", Host, "a line finer than a beat"),
    color("editor.panel", Host, "a panel"),
    color("editor.panel_border", Host, "a panel's edge"),
    color("editor.border_strong", Host, "a strong edge"),
    color("editor.surface_inset", Host, "an inset surface"),
    color("editor.surface_bar", Host, "the tool bar"),
    color("editor.gutter_bg", Host, "the keys' gutter"),
    color("editor.key_white", Host, "a white key"),
    color("editor.key_black", Host, "a black key"),
    color("editor.key_label", Host, "a key's label"),
    color("editor.text", Host, "text"),
    color("editor.text_dim", Host, "secondary text"),
    color("editor.text_bright", Host, "text that must stand out"),
    color("editor.text_faint", Host, "tertiary text"),
    color("editor.accent", Host, "focus"),
    color("editor.selected", Host, "a selected note"),
    color("editor.playhead", Host, "the play cursor"),
    color("editor.razor", Host, "a razor area"),
    color("editor.gold", Host, "the reference take"),
    color("editor.zone", Host, "a zone"),
    color("editor.reference", Host, "a reference lane"),
    color("editor.handle", Host, "a drag handle"),
    color("editor.control", Host, "a button"),
    color("editor.control_active", Host, "a button, on"),
    color("editor.control_hover", Host, "a button under the pointer"),
    color("editor.control_selected", Host, "a button, selected"),
    color("editor.octave_line", Host, "the line at every C"),
    color("editor.peaks", Host, "a waveform behind notes"),
    color("editor.pitch_0", Host, "a C"),
    color("editor.pitch_1", Host, "a C♯"),
    color("editor.pitch_2", Host, "a D"),
    color("editor.pitch_3", Host, "an E♭"),
    color("editor.pitch_4", Host, "an E"),
    color("editor.pitch_5", Host, "an F"),
    color("editor.pitch_6", Host, "an F♯"),
    color("editor.pitch_7", Host, "a G"),
    color("editor.pitch_8", Host, "an A♭"),
    color("editor.pitch_9", Host, "an A"),
    color("editor.pitch_10", Host, "a B♭"),
    color("editor.pitch_11", Host, "a B"),
    color("editor.drum_kick", Host, "a kick"),
    color("editor.drum_snare", Host, "a snare"),
    color("editor.drum_tom", Host, "a tom"),
    color("editor.drum_hat", Host, "a hi-hat"),
    color("editor.drum_ride", Host, "a ride"),
    color("editor.drum_cymbal", Host, "a cymbal"),
    color("editor.drum_other", Host, "any other piece of a kit"),
];

const fn page(name: &'static str, doc: &'static str) -> Part {
    Part {
        name,
        kind: PartKind::Page,
        family: false,
        doc,
    }
}

const fn component(name: &'static str, doc: &'static str) -> Part {
    Part {
        name,
        kind: PartKind::Component,
        family: false,
        doc,
    }
}

/// Every part.
pub const PARTS: &[Part] = &[
    // ── the mixer ─────────────────────────────────────────────────────
    page("mcp.strip", "one mixer channel strip"),
    page("mcp.rackback", "behind a strip's FX rack"),
    page("mcp.step", "one folder step under a nested strip"),
    page("rails", "the mixer's rails"),
    page("rail.button", "one rail button"),
    page("rack.unit", "one unit of the FX rack"),
    page("rack.rail", "the FX rack's rail row"),
    page("rack.phase", "a mix phase's header in the rack"),
    page("fx.phase", "a mix phase's header in the FX view"),
    Part {
        name: "fx.macro",
        kind: PartKind::Page,
        family: true,
        doc: "a macro knob; `fx.macro.<phase>` and `fx.macro.<phase>.<track>` are particular ones",
    },
    // ── the arrangement ───────────────────────────────────────────────
    page("tcp.row", "one track control panel row"),
    // ── the MIDI editor ───────────────────────────────────────────────
    page("editor.bar", "the tool bar"),
    page("editor.button", "a tool bar button"),
    page("editor.keys", "the keyboard"),
    page("editor.key", "one key"),
    page("editor.keygroup", "a group of keys (a drum kit's piece)"),
    page("editor.lane", "a lane"),
    page("editor.ruler", "the ruler"),
    page("editor.tick", "one ruler tick"),
    page("editor.marker", "a marker on the ruler"),
    page("editor.strip", "the controller strip"),
    page("editor.menu", "a menu"),
    page("editor.menuitem", "a menu item"),
    // ── a pack's own pages of components ──────────────────────────────
    page("controls", "a page holding the shared components"),
    // ── the shared controls ───────────────────────────────────────────
    component("mute", "mute"),
    component("solo", "solo"),
    component("arm", "record arm"),
    component("monitor", "input monitoring"),
    component("phase", "polarity"),
    component("pan", "the pan knob"),
    component("volume", "the volume control"),
    component("fx", "the FX button"),
    component("routing", "the routing button"),
];

const fn field(name: &'static str) -> Field {
    Field {
        name,
        class: Class::Static,
    }
}

const fn layout(name: &'static str) -> Field {
    Field {
        name,
        class: Class::Layout,
    }
}

const fn live(name: &'static str) -> Field {
    Field {
        name,
        class: Class::Live,
    }
}

/// Every field, in the order a view's values are slotted.
pub const FIELDS: &[Field] = &[
    field("track.name"),
    field("track.label"),
    field("track.number"),
    field("track.color"),
    field("track.has_color"),
    field("track.selected"),
    field("track.folder"),
    field("track.collapsed"),
    field("track.depth"),
    field("track.input"),
    field("track.ground"),
    field("track.edge"),
    field("track.has_edge"),
    field("track.tint"),
    field("track.band"),
    field("track.label_size"),
    field("track.parent_send"),
    field("ui.rack"),
    field("folder.tint"),
    field("folder.level"),
    field("button.label"),
    field("button.label_size"),
    field("button.on"),
    field("button.hover"),
    field("button.pressed"),
    field("button.has_icon"),
    field("button.enabled"),
    field("button.readout"),
    field("key.black"),
    field("key.piano"),
    field("key.label"),
    field("key.labelled"),
    field("group.label"),
    field("lane.name"),
    field("lane.active"),
    field("lane.reference"),
    field("lane.tinted"),
    field("lane.tint"),
    field("tick.bar"),
    field("tick.label"),
    field("tick.labelled"),
    field("marker.label"),
    field("strip.label"),
    field("menu.label"),
    field("menu.shortcut"),
    field("menu.enabled"),
    field("menu.hover"),
    field("menu.rule"),
    field("unit.name"),
    field("unit.value"),
    field("unit.bypassed"),
    field("unit.glyph"),
    field("unit.expands"),
    field("unit.zoom_lit"),
    field("phase.name"),
    field("phase.tint"),
    field("phase.blank"),
    field("phase.shut"),
    field("phase.held"),
    field("macro.name"),
    field("macro.value"),
    field("macro.says"),
    field("macro.tint"),
    field("macro.held"),
    field("row.indent"),
    field("row.number"),
    field("row.number_size"),
    field("row.number_x"),
    field("row.number_baseline"),
    field("row.has_number"),
    field("row.icon"),
    field("row.icon_x"),
    field("row.icon_y"),
    field("row.icon_size"),
    field("row.name"),
    field("row.name_size"),
    field("row.input"),
    layout("mixer.h"),
    layout("ui.mixing"),
    layout("ui.recording"),
    live("track.mute"),
    live("track.solo"),
    live("track.armed"),
    live("track.monitor"),
    live("track.phase"),
    live("track.fader"),
    live("track.volume"),
    live("track.pan"),
    live("track.fx"),
    live("track.sends"),
    live("track.receives"),
    live("meter.left"),
    live("meter.right"),
    live("meter.hold_left"),
    live("meter.hold_right"),
    live("meter.clip"),
    live("ui.hover"),
    live("ui.pressed"),
    live("meter.peak"),
    live("track.volume_label"),
    live("track.volume_label_w"),
];

/// The token named `name`.
#[must_use]
pub fn token(name: &str) -> Option<&'static Token> {
    TOKENS.iter().find(|t| t.name == name)
}

/// The part a page or component named `name` is: itself, or the family it
/// is a particular one of.
#[must_use]
pub fn part(name: &str) -> Option<&'static Part> {
    PARTS.iter().find(|p| {
        p.name == name
            || (p.family
                && name
                    .strip_prefix(p.name)
                    .is_some_and(|rest| rest.starts_with('.')))
    })
}

/// The field named `name`.
#[must_use]
pub fn field_named(name: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.name == name)
}

/// The fields as the Frame schema a pack compiles against: every page of
/// every view can bind any of them.
#[must_use]
pub fn schema() -> Schema {
    FIELDS.iter().fold(Schema::new(), |s, f| match f.class {
        Class::Static => s.field(f.name),
        Class::Layout => s.layout(f.name),
        Class::Live => s.live(f.name),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for t in TOKENS {
            assert!(seen.insert(t.name), "token {} twice", t.name);
        }
        let mut seen = std::collections::BTreeSet::new();
        for p in PARTS {
            assert!(seen.insert(p.name), "part {} twice", p.name);
        }
        let mut seen = std::collections::BTreeSet::new();
        for f in FIELDS {
            assert!(seen.insert(f.name), "field {} twice", f.name);
        }
    }

    #[test]
    fn every_fallback_token_is_in_the_spec_and_ends() {
        for t in TOKENS {
            let mut at = t;
            for _ in 0..TOKENS.len() {
                match at.fallback {
                    To(next) => {
                        at = token(next).unwrap_or_else(|| {
                            panic!("{} falls back to {next}, not in the spec", t.name)
                        });
                    }
                    Is(_) | Host => break,
                }
            }
            assert!(
                !matches!(at.fallback, To(_)),
                "{} falls back in a loop",
                t.name
            );
        }
    }

    #[test]
    fn a_family_takes_its_particular_pages() {
        assert_eq!(part("fx.macro.tone.kick").map(|p| p.name), Some("fx.macro"));
        assert_eq!(part("fx.macro").map(|p| p.name), Some("fx.macro"));
        assert!(part("fx.macroish").is_none());
        assert!(part("mcp.strips").is_none());
    }

    #[test]
    fn the_schema_slots_every_field() {
        let s = schema();
        for f in FIELDS {
            assert!(s.slot(f.name).is_some(), "{}", f.name);
        }
    }
}
