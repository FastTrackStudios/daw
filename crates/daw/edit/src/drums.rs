//! The drums, as an edit reads a recorded kit: which part of the kit a
//! track or a note row is ([`DrumFamily`], [`drum_family`]), and the role
//! a track plays when a kit's mics are folded into the lanes an edit
//! acts on ([`LaneRole`], [`kit_role`]): from its place in the folder
//! hierarchy first and its name second.
//!
//! Plain classification of names, no drawing: a view colours a role with
//! its own theme.

/// The part of the kit a lane belongs to.
///
/// Grouping is what makes a real kit navigable: the FTS map is 39 rows,
/// and a wall of evenly-striped lanes gives the eye nothing to steer by.
/// Banding by family means you find the toms by *shape* rather than by
/// reading every label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DrumFamily {
    Kick,
    Snare,
    Tom,
    HiHat,
    Cymbal,
    /// Ride is its own band rather than a cymbal, because it is played
    /// like a hat — a continuous part — not struck like a crash.
    Ride,
    Other,
}

impl DrumFamily {
    /// Its name, as a lane or a row band labels it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            DrumFamily::Kick => "Kick",
            DrumFamily::Snare => "Snare",
            DrumFamily::Tom => "Toms",
            DrumFamily::HiHat => "Hi-hat",
            DrumFamily::Cymbal => "Cymbals",
            DrumFamily::Ride => "Ride",
            DrumFamily::Other => "Other",
        }
    }
}

/// Which family a lane name belongs to.
///
/// Reads the FTS abbreviations first and exactly — `K`, `S`, `T1`, `H-`,
/// `C-`, `R-` — then falls back to the words a General MIDI map uses.
/// Matching a single letter by substring would put half the kit in the
/// wrong band.
pub fn drum_family(name: &str) -> DrumFamily {
    let n = name.to_ascii_lowercase();
    let head = n.split([' ', '-']).next().unwrap_or("");
    match head {
        "k" | "kl" => return DrumFamily::Kick,
        "s" | "sr" => return DrumFamily::Snare,
        "t1" | "t2" | "t3" | "t4" => return DrumFamily::Tom,
        "h" => return DrumFamily::HiHat,
        "r" => return DrumFamily::Ride,
        "c" => return DrumFamily::Cymbal,
        _ => {}
    }
    if n.contains("kick") {
        DrumFamily::Kick
    } else if n.contains("snare") || n.contains("stick") || n.contains("clap") {
        DrumFamily::Snare
    } else if n.contains("tom") {
        DrumFamily::Tom
    } else if n.contains("hh") || n.contains("hat") {
        DrumFamily::HiHat
    } else if n.contains("ride") {
        DrumFamily::Ride
    } else if n.contains("crash") || n.contains("china") || n.contains("splash") {
        DrumFamily::Cymbal
    } else {
        DrumFamily::Other
    }
}

/// The role a lane plays in a folded group.
///
/// Order is draw order **top to bottom** — `Other` is drawn first
/// (top), `Kick` last (bottom) — so sorting lanes by role puts the kick
/// at the bottom where a drum editor expects it.
///
/// A role is a label plus two draw/detect rules, not a drum-specific
/// type: a guitar workspace defines `DI` / `Amps` over the same `Lane`
/// machinery by adding variants here, and the fold, the group rule and
/// the apply path need no changes.
// r[impl drums.scope.generic-groups]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LaneRole {
    /// Cymbals, hats, rooms, returns — everything that is not a drum the
    /// editor targets. Drawn summed, not a detection source.
    Other,
    /// One sub-row per tom.
    Toms,
    Snare,
    Kick,
}

impl LaneRole {
    /// The label drawn on the lane.
    pub fn label(self) -> &'static str {
        match self {
            LaneRole::Kick => "Kick",
            LaneRole::Snare => "Snare",
            LaneRole::Toms => "Toms",
            LaneRole::Other => "Other",
        }
    }

    /// Whether hits are detected on this lane and edited there.
    ///
    /// Kick, snare and toms are the drums the editor targets; the
    /// other lane — hats, cymbals, rooms, returns — is drawn summed
    /// for context and carries no hit list. A gesture on it selects
    /// the lane and nothing more.
    // r[impl flow.drums.editing.stack]
    pub fn detects(self) -> bool {
        !matches!(self, LaneRole::Other)
    }

    /// Whether the lane draws each member in its own sub-row (toms)
    /// rather than one summed waveform (kick, snare, other).
    pub fn splits_members(self) -> bool {
        matches!(self, LaneRole::Toms)
    }

    /// Whether hits are detected on this lane's signal by default.
    /// `Other` is never a source — it is rooms, hats and returns, where
    /// a "hit" means nothing the editor can act on.
    // r[impl drums.lanes.other]
    pub fn is_detection_source(self) -> bool {
        matches!(self, LaneRole::Kick | LaneRole::Snare | LaneRole::Toms)
    }

    /// Every role, top to bottom.
    pub const ALL: [LaneRole; 4] = [
        LaneRole::Other,
        LaneRole::Toms,
        LaneRole::Snare,
        LaneRole::Kick,
    ];
}

/// Whether a folder name is the kit itself (`Drums`, `Drum`, `Kit`).
pub fn is_kit_folder(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    matches!(
        n.as_str(),
        "drums" | "drum" | "kit" | "drum kit" | "drumkit"
    )
}

/// How much a folder looks like a real drum kit, for picking between
/// several that are all *named* like one.
///
/// A session routinely carries more than one folder called `Drums`: the
/// tracked kit, and a folder of reference stems or a printed drum mix. They
/// are indistinguishable by name — the real kit is obvious only from its
/// shape. Taking the first match opened the stems on a real session
/// (`set in stone`: a four-stem `Drums` folder sitting above the 20-track
/// kit), which then looked like "tom lanes are broken" because that folder
/// has no toms.
///
/// Ordered by `roles` first — covering kick *and* snare *and* toms is what
/// a kit is — then by role-claiming sub-folders, then by sheer size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct KitScore {
    /// Distinct targeted roles present (`Kick`, `Snare`, `Toms`). `Other`
    /// does not count: a folder of overheads is not a kit.
    pub roles: usize,
    /// Descendant folders that claim a role — `Kick/`, `Snare/`, `Toms/`.
    /// A tracked kit groups its mics; a stem folder is flat.
    pub sub_folders: usize,
    /// Leaf tracks beneath it, as the final tie-break.
    pub members: usize,
}

/// Score a candidate kit folder from its descendants, each `(name, is_folder)`.
#[must_use]
pub fn score_kit(descendants: &[(&str, bool)]) -> KitScore {
    let mut roles = [false; 3];
    let mut sub_folders = 0usize;
    let mut members = 0usize;
    for (name, is_folder) in descendants {
        let role = folder_role(name);
        if *is_folder {
            if role.is_some_and(|r| r != LaneRole::Other) {
                sub_folders = sub_folders.saturating_add(1);
            }
        } else {
            members = members.saturating_add(1);
        }
        match role {
            Some(LaneRole::Kick) => roles[0] = true,
            Some(LaneRole::Snare) => roles[1] = true,
            Some(LaneRole::Toms) => roles[2] = true,
            _ => {}
        }
    }
    KitScore {
        roles: roles.iter().filter(|r| **r).count(),
        sub_folders,
        members,
    }
}

/// The role a folder name claims for everything under it, if any.
///
/// `SUM` and similar bus names claim nothing: the folder *above* them
/// decides. Uses [`drum_family`] so the two never disagree
/// about what a "Tom" is.
pub fn folder_role(name: &str) -> Option<LaneRole> {
    match drum_family(name) {
        DrumFamily::Kick => Some(LaneRole::Kick),
        DrumFamily::Snare => Some(LaneRole::Snare),
        DrumFamily::Tom => Some(LaneRole::Toms),
        DrumFamily::HiHat | DrumFamily::Cymbal | DrumFamily::Ride => Some(LaneRole::Other),
        DrumFamily::Other => {
            let n = name.trim().to_ascii_lowercase();
            let head = n.split([' ', '-', '_']).next().unwrap_or("");
            // Words the drum map does not know but a session uses for
            // the whole non-drum remainder.
            if n.contains("overhead")
                || n.contains("room")
                || n.contains("cymbal")
                || n.contains("verb")
                || matches!(head, "oh" | "ohs" | "amb" | "ambience")
            {
                Some(LaneRole::Other)
            } else {
                None
            }
        }
    }
}

/// The role of a track, given its name and its folders **nearest
/// first** (`["SUM", "Kick", "Drums"]` for `Kick/SUM/In`).
///
/// Hierarchy wins over name: a track called `Trig` under the `Snare`
/// folder is a snare mic, and a track called `In` says nothing on its
/// own. The kit folder itself claims no role, so a `Hi-Hat` track
/// directly under `Drums` falls through to its name.
// r[impl drums.lanes.roles]
pub fn kit_role(name: &str, folders_nearest_first: &[&str]) -> LaneRole {
    for f in folders_nearest_first {
        if is_kit_folder(f) {
            break;
        }
        if let Some(role) = folder_role(f) {
            return role;
        }
    }
    folder_role(name).unwrap_or(LaneRole::Other)
}

/// Whether a member is parked — named `Unused` (any case) — and so drawn
/// faded and left out of detection.
pub fn is_unused_name(name: &str) -> bool {
    name.to_ascii_lowercase().contains("unused")
}

/// Whether a track is a trigger — a `Trig` / `Trigger` token in its name.
///
/// Matched as a whole token, not a substring: a mic called `Trigate` is
/// not a trigger, and the token is what the FTS naming actually writes
/// (`T3 Trig`, `Kick Trigger`).
pub fn is_trigger_name(name: &str) -> bool {
    name.to_ascii_lowercase()
        .split([' ', '-', '_'])
        .any(|t| t == "trig" || t == "trigger")
}

/// Which tom a track belongs to, `T1`–`T4` → `1`–`4`.
///
/// A trigger shares its tom's number (`T3 Trig` is tom 3), which is what
/// lets a trigger be drawn over the tom it triggers rather than beside
/// it. Any token may carry the number, because the mic name comes first
/// in some sessions and last in others (`T2 Close`, `Close T2`).
pub fn tom_number(name: &str) -> Option<u8> {
    name.to_ascii_lowercase()
        .split([' ', '-', '_'])
        .find_map(|t| match t {
            "t1" => Some(1),
            "t2" => Some(2),
            "t3" => Some(3),
            "t4" => Some(4),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_decides_before_the_name() {
        assert_eq!(kit_role("In", &["SUM", "Kick", "Drums"]), LaneRole::Kick);
        assert_eq!(kit_role("Trig", &["Snare", "Drums"]), LaneRole::Snare);
    }

    #[test]
    fn the_kit_folder_claims_nothing() {
        assert_eq!(kit_role("Hi-Hat", &["Drums"]), LaneRole::Other);
        assert_eq!(kit_role("T2 Close", &["Drums"]), LaneRole::Toms);
    }

    #[test]
    fn fts_abbreviations_read_exactly() {
        assert_eq!(drum_family("K In"), DrumFamily::Kick);
        assert_eq!(drum_family("S-Buzz"), DrumFamily::Snare);
        assert_eq!(drum_family("T3"), DrumFamily::Tom);
        assert_eq!(drum_family("Overheads"), DrumFamily::Other);
    }

    #[test]
    fn a_trigger_is_a_whole_token() {
        assert!(is_trigger_name("T3 Trig"));
        assert!(!is_trigger_name("Trigate"));
        assert_eq!(tom_number("Close T2"), Some(2));
    }

    #[test]
    fn a_kit_outscores_a_folder_of_stems() {
        let kit = score_kit(&[
            ("Kick", true),
            ("K In", false),
            ("Snare", true),
            ("S Top", false),
            ("T1", false),
        ]);
        let stems = score_kit(&[
            ("Kick", false),
            ("Snare", false),
            ("Drums", false),
            ("Room", false),
        ]);
        assert!(kit > stems);
    }
}
