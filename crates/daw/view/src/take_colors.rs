//! A colour, and a name for it, for every take of a recording: "the blue
//! take".
//!
//! A song's takes are numbered by their recording pass (REAPER's
//! `…-260907_1321-01.wav` stamp: every mic of one pass shares it), in the
//! order they were recorded, and the n-th is the n-th of a set of named
//! colours — twelve hues, then each lighter, then darker — so no two takes
//! of a kit share one while it has fewer than 36. The window paints each
//! take's lane, each comp piece cut from it and the comp strip with it, and
//! `COMP_CANDIDATES` / `COMP_CHOOSE` say and take its name, from the same
//! numbering.
//!
//! ```ignore
//! let colors = TakeColors::of(["65-In-260906_1702.wav", "65-In-260906_1754.wav"]);
//! colors.name_of("66-Out-260906_1754.wav");   // Some("Orange")
//! colors.pass_named("orange");                 // Some("260906_1754")
//! ```

/// A colour as red, green and blue bytes.
pub type Rgb = [u8; 3];

/// The hues, in the order takes get them: neighbours far apart, each with
/// a name anyone in the room can say.
const HUES: [(&str, u32); 12] = [
    ("Red", 0x00e5_484d),
    ("Blue", 0x003e_8ef7),
    ("Yellow", 0x00f5_c518),
    ("Purple", 0x00b0_5ccf),
    ("Green", 0x0030_a46c),
    ("Orange", 0x00f7_6b15),
    ("Cyan", 0x0005_b2d2),
    ("Pink", 0x00e9_3d82),
    ("Lime", 0x0099_d52a),
    ("Indigo", 0x006e_6ade),
    ("Brown", 0x00ad_7f58),
    ("Teal", 0x0012_a594),
];

/// The colour numbered `index`, and its name: the hues, then each lighter,
/// then each darker, then round again numbered.
#[must_use]
pub fn swatch(index: usize) -> (String, Rgb) {
    let count = HUES.len();
    let (name, rgb) = HUES.get(index % count).copied().unwrap_or(HUES[0]);
    let [_, r, g, b] = rgb.to_be_bytes();
    let base = [r, g, b].map(f64::from);
    let round = index / count;
    let (shade, mix): (&str, Option<(f64, f64)>) = match round {
        0 => ("", None),
        1 => ("Light ", Some((255.0, 0.45))),
        2 => ("Dark ", Some((0.0, 0.40))),
        _ => ("", None),
    };
    let [r, g, b] = base.map(|c| {
        let c = mix.map_or(c, |(to, by)| (to - c).mul_add(by, c));
        num_traits::ToPrimitive::to_u8(&c.round()).unwrap_or(0)
    });
    let named = if round > 2 {
        format!("{name} {}", round.saturating_sub(1))
    } else {
        format!("{shade}{name}")
    };
    (named, [r, g, b])
}

/// The recording pass stamped in a take's name or file — `YYMMDD_HHMM`
/// and any `-NN` counters after it (`65-In-260907_1321-01.wav` →
/// `260907_1321-01`); `None` for a name with no stamp.
#[must_use]
pub fn stamp(name: &str) -> Option<String> {
    pass_of(name).map(str::to_owned)
}

/// [`stamp`], borrowed from `name`: the pass is the tail of the file's
/// stem from its `YYMMDD_HHMM` part on, so it needs no allocation — what
/// a lookup made every frame reads.
#[must_use]
pub fn pass_of(name: &str) -> Option<&str> {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = file
        .rsplit_once('.')
        .filter(|(_, ext)| !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .map_or(file, |(stem, _)| stem);
    let is_stamp = |part: &str| {
        let b = part.as_bytes();
        b.len() == 11
            && b.get(6) == Some(&b'_')
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 6 || c.is_ascii_digit())
    };
    let mut end = stem.len();
    loop {
        let start = stem.get(..end)?.rfind('-').map_or(0, |at| at + 1);
        if stem.get(start..end).is_some_and(is_stamp) {
            return stem.get(start..);
        }
        end = start.checked_sub(1)?;
    }
}

/// A set of takes numbered by pass, each with its colour.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TakeColors {
    /// The passes, numbered ([`Self::of`]): a take's number is its place.
    passes: Vec<String>,
}

impl TakeColors {
    /// The takes `items` (each an item's take name or recording file, any
    /// mic, and its length in seconds) are of, numbered: the takes — a
    /// tenth as long as the longest or more — in the order recorded, then
    /// the false starts, so the takes worth comping from get the plain
    /// colours. A pass's length is its longest item's, which slicing and
    /// comping never lengthen. Names with no pass stamp are left out — a
    /// glued edit is no take.
    #[must_use]
    pub fn of<'a>(items: impl IntoIterator<Item = (&'a str, f64)>) -> Self {
        let mut longest: std::collections::BTreeMap<String, f64> =
            std::collections::BTreeMap::new();
        for (name, length) in items {
            if let Some(pass) = stamp(name) {
                let at = longest.entry(pass).or_insert(0.0);
                *at = at.max(length);
            }
        }
        // A tenth of the longest: a punch-in over a verse is a take, the
        // few seconds of a false start are not.
        let full = longest.values().copied().fold(0.0_f64, f64::max) * 0.1;
        let (mut passes, short): (Vec<(String, f64)>, Vec<(String, f64)>) =
            longest.into_iter().partition(|(_, length)| *length >= full);
        passes.extend(short);
        Self {
            passes: passes.into_iter().map(|(pass, _)| pass).collect(),
        }
    }

    /// Whether there are no takes to tell apart.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    /// The number of the take `name` is of.
    #[must_use]
    pub fn index_of(&self, name: &str) -> Option<usize> {
        let pass = pass_of(name)?;
        self.passes.iter().position(|p| p == pass)
    }

    /// The colour of the take `name` is of.
    #[must_use]
    pub fn color_of(&self, name: &str) -> Option<Rgb> {
        self.index_of(name).map(|i| swatch(i).1)
    }

    /// What the take `name` is of is called: its colour's name.
    #[must_use]
    pub fn name_of(&self, name: &str) -> Option<String> {
        self.index_of(name).map(|i| swatch(i).0)
    }

    /// The pass whose colour is called `color` (any case, `light blue` or
    /// `Light Blue`).
    #[must_use]
    pub fn pass_named(&self, color: &str) -> Option<&str> {
        let want = color.split_whitespace().collect::<Vec<_>>().join(" ");
        self.passes
            .iter()
            .enumerate()
            .find(|(i, _)| swatch(*i).0.eq_ignore_ascii_case(&want))
            .map(|(_, pass)| pass.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{pass_of, stamp};

    #[test]
    fn a_pass_is_the_stems_tail_from_its_stamp_on() {
        for (name, pass) in [
            ("65-In-260907_1321-01.wav", Some("260907_1321-01")),
            ("/rec/66-Out-260906_1754.wav", Some("260906_1754")),
            ("Kick-260906_1754-02-03", Some("260906_1754-02-03")),
            ("glued-edit.wav", None),
            ("", None),
        ] {
            assert_eq!(pass_of(name), pass, "{name}");
            assert_eq!(stamp(name).as_deref(), pass, "{name}");
        }
    }
}
