//! Reading REAPER's `<KEYSIG>` block.
//!
//! REAPER's key signature is not reachable through the extension API — it
//! lives in the project chunk:
//!
//! ```text
//! <KEYSIG
//!   0 0 1 0xAB5
//!   4 1 1 0xAB5
//!   8 1 -1 0xAB5
//! >
//! ```
//!
//! Four fields per line: **measure**, **root**, **accidental**, **scale
//! mask**.
//!
//! The field that isn't obvious is `accidental`. It does *not* modify the
//! root — it chooses how the key is spelled. Root is already a pitch
//! class 0-11, so root `1` with accidental `+1` is C♯ major and the same
//! root with `-1` is D♭ major: identical pitches, different signature.
//! The fixture this module is tested against was built to demonstrate
//! exactly that, pairing 4/8 (C♯/D♭) and 16/20 (D♯/E♭).
//!
//! `accidental` takes `0` as well as ±1 — `0` means spell it naturally.
//! That matters at the edges of the circle: root `11` with `0` is B
//! major, and the same root with `-1` is C♭ major. Those two cases come
//! from a Cockos forum thread (t=287164) rather than the fixture, which
//! never uses root 11.
//!
//! `scale_mask` is a 12-bit set with the root at bit 0, so `0xAB5` —
//! bits 0,2,4,5,7,9,11 — is the major scale.
//!
//! The write side lives in [`crate::scaffold`]; this is the reader.

/// One key-signature change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySig {
    /// Measure the change takes effect, 0-based.
    pub measure: u32,
    /// Root pitch class, 0-11.
    pub root: u8,
    /// How to spell it: negative for flats, zero for naturals, positive
    /// for sharps. Not an offset — the root already carries the pitch.
    pub accidental: i8,
    /// 12-bit scale set, root at bit 0.
    pub scale_mask: u32,
}

/// Major scale: bits 0,2,4,5,7,9,11.
pub const SCALE_MASK_MAJOR: u32 = 0xAB5;

const SHARP_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
/// Flat spellings. Note 4 and 11: in a flat key those are F♭ and C♭, not
/// E and B — the naive table returns the natural name and gets C♭ major
/// wrong.
const FLAT_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "Fb", "F", "Gb", "G", "Ab", "A", "Bb", "Cb",
];

impl KeySig {
    /// The root spelled the way this signature asks for.
    pub fn root_name(&self) -> &'static str {
        let table = if self.accidental < 0 {
            &FLAT_NAMES
        } else {
            &SHARP_NAMES
        };
        table[usize::from(self.root % 12)]
    }

    /// Semitones of the scale above the root.
    pub fn scale_degrees(&self) -> Vec<u8> {
        (0..12u8)
            .filter(|i| self.scale_mask >> i & 1 == 1)
            .collect()
    }

    /// Whether the mask is the major scale.
    pub fn is_major(&self) -> bool {
        self.scale_mask == SCALE_MASK_MAJOR
    }

    /// Human-readable, e.g. `"Db major"`.
    pub fn display(&self) -> String {
        let quality = if self.is_major() { "major" } else { "scale" };
        format!("{} {quality}", self.root_name())
    }
}

/// Pull every key signature out of a project chunk.
///
/// Returns an empty vec when the project has none — which is the common
/// case, since REAPER only writes the block once a key is set.
pub fn parse(chunk: &str) -> Vec<KeySig> {
    let mut out = Vec::new();
    let mut inside = false;

    for line in chunk.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("<KEYSIG") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if trimmed == ">" {
            break;
        }
        if let Some(sig) = parse_line(trimmed) {
            out.push(sig);
        }
    }
    out
}

fn parse_line(line: &str) -> Option<KeySig> {
    let mut parts = line.split_whitespace();
    let measure = parts.next()?.parse().ok()?;
    let root = parts.next()?.parse().ok()?;
    let accidental = parts.next()?.parse().ok()?;
    let mask_text = parts.next()?;
    let scale_mask = mask_text
        .strip_prefix("0x")
        .or_else(|| mask_text.strip_prefix("0X"))
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .or_else(|| mask_text.parse().ok())?;

    Some(KeySig {
        measure,
        root,
        accidental,
        scale_mask,
    })
}

/// The line ending a chunk uses.
///
/// REAPER writes CRLF on every platform. A rewriter that assumes `\n`
/// finds nothing, changes nothing, and reports success — so this is
/// detected rather than assumed.
fn line_ending(chunk: &str) -> &'static str {
    if chunk.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Render key signatures as the `<KEYSIG>` block REAPER writes.
///
/// Empty input yields an empty string — REAPER omits the block entirely
/// when a project has none, and writing an empty one back would be a
/// gratuitous difference.
pub fn render(sigs: &[KeySig]) -> String {
    render_with(sigs, "\n")
}

fn render_with(sigs: &[KeySig], eol: &str) -> String {
    if sigs.is_empty() {
        return String::new();
    }
    let mut out = format!("  <KEYSIG{eol}");
    for sig in sigs {
        out.push_str(&format!(
            "    {} {} {} 0x{:X}{eol}",
            sig.measure, sig.root, sig.accidental, sig.scale_mask
        ));
    }
    out.push_str(&format!("  >{eol}"));
    out
}

/// Replace a project chunk's `<KEYSIG>` block with `sigs`.
///
/// Splices rather than rewrites: every other line is passed through
/// untouched, because this runs over a real project file and the only
/// safe edit is the smallest one. Where there is no block yet, the new
/// one goes just before the first `<TRACK`, which is where REAPER puts
/// it. Line endings are preserved.
pub fn splice(chunk: &str, sigs: &[KeySig]) -> String {
    let eol = line_ending(chunk);
    let mut out: Vec<String> = Vec::new();
    let mut inside = false;
    let mut wrote = false;

    let block_lines = |sigs: &[KeySig]| -> Vec<String> {
        if sigs.is_empty() {
            return Vec::new();
        }
        let mut v = vec!["  <KEYSIG".to_string()];
        for sig in sigs {
            v.push(format!(
                "    {} {} {} 0x{:X}",
                sig.measure, sig.root, sig.accidental, sig.scale_mask
            ));
        }
        v.push("  >".to_string());
        v
    };

    for line in chunk.split(eol) {
        if !inside && line.trim_start().starts_with("<KEYSIG") {
            inside = true;
            out.extend(block_lines(sigs));
            wrote = true;
            continue;
        }
        if inside {
            // Between `<KEYSIG` and its closing `>` are the old
            // signatures; they go.
            if line.trim() == ">" {
                inside = false;
            }
            continue;
        }
        if !wrote && !sigs.is_empty() && line.trim_start().starts_with("<TRACK") {
            out.extend(block_lines(sigs));
            wrote = true;
        }
        out.push(line.to_string());
    }

    if !wrote && !sigs.is_empty() {
        out.extend(block_lines(sigs));
    }
    out.join(eol)
}
