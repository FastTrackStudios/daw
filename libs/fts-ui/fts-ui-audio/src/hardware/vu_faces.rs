//! The kit: every VU face this crate can draw, as a [`VuSpec`] each.
//!
//! A period movement — Weston, Modutec, Sifam — has an **ivory card with a
//! black scale and a red stretch above 0**, lit from behind by one or two
//! bulbs. The lamp is what varies between units, not the card: the LA-2A's is
//! warm, a rackmount's whiter. Blue-faced meters are largely a plugin
//! aesthetic rather than something these units wore, so ivory is the default
//! and blue is kept for a unit that genuinely has one.
//!
//! One const per face — see [`vu_kit`](crate::hardware::vu_kit) for how to
//! add one, and the `vu_sheet` test for how to look at it.

use super::vu_kit::{Bezel, Lamp, Needle, Vent, VuSpec};

/// The frame nearly every rack meter is mounted in: matte black, with the
/// opening chamfered inward and a vent under the glass.
const BLACK_FRAME: Bezel = Bezel {
    frame: "linear-gradient(180deg, #141517, #0a0b0c)",
    top: "#050506",
    left: "#0c0d0e",
    right: "#2b2d30",
    bottom: "#3b3d41",
    depth: 9.0,
    vent: Some(Vent {
        dark: "#000",
        light: "#2a2c2e",
        pitch: 4.0,
    }),
};

/// The reflection on the glass over a card.
const GLASS: &str = "linear-gradient(160deg, rgba(255,255,255,0.22) 0%, \
                     rgba(255,255,255,0.04) 38%, rgba(0,0,0,0.10) 100%)";

/// The needle on a dark-printed ivory card.
const DARK_NEEDLE: Needle = Needle {
    color: "#241a0c",
    width: 1.1,
    hub_r: 4.0,
    hub_opacity: 0.9,
};

// ─────────────────────────────────────────────────────────────────────────
// Amber — warm ivory under a yellow lamp. The LA-2A's, and most tube gear's.
// ─────────────────────────────────────────────────────────────────────────
pub static AMBER: VuSpec = VuSpec {
    card: "linear-gradient(180deg, #f6d79a 0%, #e8b45f 62%, #d99a3c 100%)",
    ink: "#3a2a12",
    hot: "#8f2010",
    needle: DARK_NEEDLE,
    lamp: Some(Lamp {
        color: "rgba(255,214,120,0.55)",
        x: 50.0,
        y: 8.0,
        reach: 68.0,
    }),
    glass: Some(GLASS),
    bezel: BLACK_FRAME,
};

// ─────────────────────────────────────────────────────────────────────────
// Ivory — neutral, under a white lamp. The 1176's Modutec, an SSL's.
// ─────────────────────────────────────────────────────────────────────────
pub static IVORY: VuSpec = VuSpec {
    card: "linear-gradient(180deg, #f4f2ea 0%, #ddd9cd 100%)",
    ink: "#2a241c",
    hot: "#a8281c",
    needle: Needle {
        color: "#1d1a15",
        ..DARK_NEEDLE
    },
    lamp: Some(Lamp {
        color: "rgba(255,246,224,0.30)",
        x: 50.0,
        y: 8.0,
        reach: 68.0,
    }),
    glass: Some(GLASS),
    bezel: BLACK_FRAME,
};

// ─────────────────────────────────────────────────────────────────────────
// Amber-blue — the dbx's, which is not a VU at all but a decibel readout,
// and prints like one: an amber card in blue ink, and no red stretch,
// because there is no 0 VU on it to be over.
// ─────────────────────────────────────────────────────────────────────────
pub static AMBER_BLUE: VuSpec = VuSpec {
    card: "linear-gradient(180deg, #f7cf86 0%, #eab259 58%, #d99b3f 100%)",
    ink: "#1c4f96",
    hot: "#1c4f96",
    needle: Needle {
        color: "#14161a",
        ..DARK_NEEDLE
    },
    lamp: Some(Lamp {
        color: "rgba(255,216,126,0.50)",
        x: 50.0,
        y: 8.0,
        reach: 68.0,
    }),
    glass: Some(GLASS),
    bezel: BLACK_FRAME,
};

// ─────────────────────────────────────────────────────────────────────────
// Blue — a blue-lit card printed in white.
// ─────────────────────────────────────────────────────────────────────────
pub static BLUE: VuSpec = VuSpec {
    card: "linear-gradient(180deg, #2f5f8f 0%, #16324e 100%)",
    ink: "#e8f2ff",
    hot: "#ff6a5c",
    needle: Needle {
        color: "#f4f8ff",
        ..DARK_NEEDLE
    },
    lamp: Some(Lamp {
        color: "rgba(150,205,255,0.32)",
        x: 50.0,
        y: 8.0,
        reach: 68.0,
    }),
    glass: Some(GLASS),
    bezel: BLACK_FRAME,
};
