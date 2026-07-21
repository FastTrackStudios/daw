//! Chart transposition / notation display service.
//!
//! A **non-destructive** display transform over a parsed [`Chart`]: given a
//! [`ChartView`] (a target key, a [`NotationSystem`], and an optional capo),
//! [`apply_view`] returns a *new* `Chart` whose chord symbols are rewritten to
//! the requested key + notation. The source chart is never mutated.
//!
//! The engraver renders chord symbols from each [`ChordInstance::full_symbol`]
//! string and already understands letter chords (`A D E`), Nashville numbers
//! (`1 4 5 6m`), and Roman numerals (`I IV V vi`). So this service does *not*
//! touch the engraver — it only produces a `Chart` whose `full_symbol`s (and
//! the structured `root`/`bass` behind them) carry the desired spelling, and
//! the existing engraver renders it unchanged.
//!
//! # Capo convention (physical, not "reading")
//!
//! A capo at fret *N* raises sounding pitch *N* semitones above the fingered
//! shapes. So to SOUND `sounding_key` with a capo at fret *N*, the player
//! FINGERS shapes in `sounding_key - N` semitones. When a view sets both a
//! `target_key` and `capo > 0`, letter chords render in the **shape key**
//! ([`shape_key_for_capo`]) — that is what the player's hands do — while the
//! view still carries the sounding `target_key` so a UI can print
//! "Capo N (sounds <target_key>)" (see [`ChartView::capo_caption`]).
//!
//! Known-good anchor (a test): to sound **B** using **G** shapes, capo = **4**
//! (`G + 4 = B`).

use crate::chord::{Chord, ChordQuality, SuspendedType};
use crate::chart::types::{KeyChange, RhythmElement};
use crate::chart::{Chart, ChordInstance};
use crate::key::{Key, KeySpelling, ScaleMode, SpellingMode};
use crate::primitives::{Accidental, MusicalNote, RomanCase, RootNotation};

/// How chord roots are spelled for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotationSystem {
    /// Letter chords spelled for the effective key (`G C D Em`).
    Letters,
    /// Nashville numbers — scale degree in the effective key (`1 4 5 6m`).
    Nashville,
    /// Roman numerals — degree, case by quality (`I IV V vi`, `vii°`).
    Roman,
}

impl Default for NotationSystem {
    fn default() -> Self {
        Self::Letters
    }
}

/// A non-destructive display transform over a chart.
#[derive(Debug, Clone, Default)]
pub struct ChartView {
    /// Render the chart *sounding* in this key. `None` = the chart's own key
    /// (no transposition). Ignored for Nashville / Roman symbol text, which is
    /// key-invariant, but still used to label the displayed key metadata.
    pub target_key: Option<Key>,
    /// Which notation the chord roots are spelled in.
    pub notation: NotationSystem,
    /// Capo fret (`0` = no capo). See the module docs for the capo convention.
    pub capo: u8,
}

impl ChartView {
    /// The key whose *shapes* the player fingers under this view: the
    /// `target_key` transposed down by the capo fret. `None` when there is no
    /// target key. With `capo == 0` this equals `target_key`.
    pub fn shape_key(&self) -> Option<Key> {
        self.target_key
            .as_ref()
            .map(|k| shape_key_for_capo(k.clone(), self.capo))
    }

    /// A UI caption for a capo'd view: `Some("Capo 4 (sounds B)")` when the
    /// view has a capo and a sounding target key, else `None`. This service
    /// never renders UI text into the chart itself; call this from the UI.
    pub fn capo_caption(&self) -> Option<String> {
        match (&self.target_key, self.capo) {
            (Some(k), n) if n > 0 => Some(format!("Capo {} (sounds {})", n, k.short_name())),
            _ => None,
        }
    }
}

// ===========================================================================
// Interval + capo helpers
// ===========================================================================

/// Signed semitone distance from `from` to `to`, taking the shortest path
/// around the octave (result is in `-6..=6`). `interval_semitones(C, G) == -5`
/// (a 4th down is shorter than a 5th up).
pub fn interval_semitones(from: Key, to: Key) -> i8 {
    let diff = to.root.semitone as i8 - from.root.semitone as i8;
    if diff > 6 {
        diff - 12
    } else if diff < -6 {
        diff + 12
    } else {
        diff
    }
}

/// The key whose shapes a player fingers to SOUND `sounding` with a capo at
/// `capo_fret`: `sounding` transposed **down** by `capo_fret` semitones. The
/// scale mode (major/minor) is preserved.
///
/// `shape_key_for_capo(B, 4) == G` — G shapes + capo 4 sound in B.
pub fn shape_key_for_capo(sounding: Key, capo_fret: u8) -> Key {
    let down = (12 - (capo_fret % 12)) % 12;
    transpose_key_by(&sounding, down)
}

/// The capo fret at which fingering `shape` sounds `sounding`: the semitone
/// distance UP from `shape` to `sounding`, in `0..=11` (`None` never occurs
/// for in-octave keys, but the signature leaves room for future guards).
///
/// `capo_to_play(B, G) == Some(4)` — G shapes need a capo at 4 to sound B.
pub fn capo_to_play(sounding: Key, shape: Key) -> Option<u8> {
    let n = (sounding.root.semitone + 12 - shape.root.semitone) % 12;
    (n <= 11).then_some(n)
}

// ===========================================================================
// apply_view
// ===========================================================================

/// Apply a [`ChartView`] to `chart`, returning a NEW chart with every chord
/// symbol transposed + re-spelled to the requested key and notation. The input
/// chart is never mutated. The returned chart's declared key metadata reflects
/// the **displayed** (shape) key.
pub fn apply_view(chart: &Chart, view: &ChartView) -> Chart {
    // Fast path: the pure default view is the identity.
    if view.target_key.is_none() && view.capo == 0 && view.notation == NotationSystem::Letters {
        return chart.clone();
    }

    // The song's own functional key — the reference for scale-degree math and
    // the source of the transposition interval. Fall back to the target key,
    // then C major, so key-less charts still render.
    let song_key = chart
        .current_key
        .clone()
        .or_else(|| chart.initial_key.clone())
        .or_else(|| view.target_key.clone())
        .unwrap_or_else(|| Key::major(MusicalNote::c()));

    // The key the letters are drawn in. With a capo this is the shape key
    // (what the hands finger); otherwise the sounding target (or the song key).
    let display_key = match (&view.target_key, view.capo) {
        (Some(t), capo) if capo > 0 => shape_key_for_capo(t.clone(), capo),
        (Some(t), _) => t.clone(),
        (None, _) => song_key.clone(),
    };

    // Semitone shift from the song key to the displayed (letters) key.
    let letters_delta = (display_key.root.semitone + 12 - song_key.root.semitone) % 12;

    let ctx = Ctx {
        song_key: &song_key,
        display_key: &display_key,
        letters_delta,
        notation: view.notation,
    };

    let mut out = chart.clone();

    for section in &mut out.sections {
        for track in &mut section.tracks {
            for measure in &mut track.measures {
                for chord in &mut measure.chords {
                    rewrite_chord(chord, &ctx);
                }
                for element in &mut measure.rhythm_elements {
                    if let RhythmElement::Chord(chord) = element {
                        rewrite_chord(chord, &ctx);
                    }
                }
            }
        }
    }

    // Metadata / key changes carry the displayed key.
    let meta_delta = letters_delta;
    out.initial_key = Some(display_key.clone());
    out.current_key = Some(display_key.clone());
    out.ending_key = chart
        .ending_key
        .as_ref()
        .map(|k| transpose_key_by(k, meta_delta));
    for kc in &mut out.key_changes {
        rewrite_key_change(kc, meta_delta);
    }

    out
}

struct Ctx<'a> {
    song_key: &'a Key,
    display_key: &'a Key,
    letters_delta: u8,
    notation: NotationSystem,
}

/// Rewrite a single chord in place to the view's notation. Placeholder /
/// rootless entries (rests, spaces, floating rhythm) are left untouched.
fn rewrite_chord(chord: &mut ChordInstance, ctx: &Ctx) {
    // Nothing to transpose for empty-root placeholders.
    let Some(root_note) = chord.parsed.root.resolve(Some(ctx.song_key)) else {
        return;
    };

    let bass_note = chord
        .parsed
        .bass
        .as_ref()
        .and_then(|b| b.resolve(Some(ctx.song_key)));

    match ctx.notation {
        NotationSystem::Letters | NotationSystem::Nashville => {
            let new_root = notate_root(&root_note, ctx);
            let new_bass = bass_note.as_ref().map(|b| notate_root(b, ctx));

            let mut parsed = chord.parsed.clone();
            parsed.root = new_root.clone();
            parsed.bass = new_bass;

            chord.full_symbol = parsed.to_string();
            chord.root = new_root;
            chord.parsed = parsed;
            chord.display_override = None;
        }
        NotationSystem::Roman => {
            let (deg, acc) = degree_of(ctx.song_key, &root_note);
            let case = roman_case(chord.parsed.quality);
            let numeral = RootNotation::from_roman_numeral_with_accidental(deg, case, acc);

            let mut sym = numeral.to_string();
            sym.push_str(&roman_tail(&chord.parsed));
            if let Some(b) = &bass_note {
                let (bdeg, bacc) = degree_of(ctx.song_key, b);
                sym.push('/');
                sym.push_str(&RootNotation::from_scale_degree(bdeg, bacc).to_string());
            }

            let mut parsed = chord.parsed.clone();
            parsed.root = numeral.clone();
            if let Some(b) = &bass_note {
                let (bdeg, bacc) = degree_of(ctx.song_key, b);
                parsed.bass = Some(RootNotation::from_scale_degree(bdeg, bacc));
            }

            chord.full_symbol = sym;
            chord.root = numeral;
            chord.parsed = parsed;
            chord.display_override = None;
        }
    }
}

/// Build the root notation for a resolved note under Letters or Nashville.
fn notate_root(note: &MusicalNote, ctx: &Ctx) -> RootNotation {
    match ctx.notation {
        NotationSystem::Letters => {
            if ctx.letters_delta == 0 {
                // No transposition — keep the note's authored spelling so a
                // non-diatonic `Bb` stays `Bb` rather than being re-spelled to
                // the key's chromatic default (`A#` in C).
                RootNotation::from_note_name(note.clone())
            } else {
                let transposed = (note.semitone + ctx.letters_delta) % 12;
                let spelled = spell_in_key(ctx.display_key, transposed);
                RootNotation::from_note_name(spelled)
            }
        }
        NotationSystem::Nashville => {
            let (deg, acc) = degree_of(ctx.song_key, note);
            RootNotation::from_scale_degree(deg, acc)
        }
        // Roman handled separately in rewrite_chord.
        NotationSystem::Roman => {
            let (deg, acc) = degree_of(ctx.song_key, note);
            RootNotation::from_scale_degree(deg, acc)
        }
    }
}

fn rewrite_key_change(kc: &mut KeyChange, delta: u8) {
    kc.to_key = transpose_key_by(&kc.to_key, delta);
    if let Some(from) = &kc.from_key {
        kc.from_key = Some(transpose_key_by(from, delta));
    }
}

// ===========================================================================
// Notation primitives
// ===========================================================================

/// Spell a semitone (0-11) with the correct enharmonic for `key` — so G major
/// gives `C` not `B#`, and flat keys give flats. Uses relaxed spelling to
/// avoid double accidentals.
fn spell_in_key(key: &Key, semitone: u8) -> MusicalNote {
    let is_major = key.mode == ScaleMode::ionian();
    let spelling = KeySpelling::new(&key.root, is_major);
    spelling.spell(semitone, SpellingMode::Relaxed).to_note()
}

/// The scale degree (1-7) and any accidental of `note` relative to `key`.
/// Diatonic notes return `(degree, None)`; chromatic notes borrow the nearest
/// scale degree, preferring a flat spelling (`Bb` in C → `(7, Flat)` = `b7`).
fn degree_of(key: &Key, note: &MusicalNote) -> (u8, Option<Accidental>) {
    let rel = (note.semitone + 12 - key.root.semitone) % 12;

    // Relative semitone of each of the 7 diatonic degrees.
    let mut naturals = [(0u8, 0u8); 7];
    for d in 1..=7u8 {
        if let Some(n) = key.get_scale_degree(d) {
            naturals[(d - 1) as usize] = (d, (n.semitone + 12 - key.root.semitone) % 12);
        }
    }

    // Exact scale tone.
    for (d, r) in naturals {
        if r == rel {
            return (d, None);
        }
    }
    // A semitone below a scale degree → flatted that degree (preferred).
    for (d, r) in naturals {
        if (rel + 1) % 12 == r {
            return (d, Some(Accidental::Flat));
        }
    }
    // A semitone above a scale degree → sharped that degree.
    for (d, r) in naturals {
        if (rel + 11) % 12 == r {
            return (d, Some(Accidental::Sharp));
        }
    }
    (1, None)
}

/// The Roman-numeral case for a chord quality: uppercase for major-ish
/// (major / augmented / suspended / power), lowercase for minor-ish
/// (minor / diminished).
fn roman_case(quality: ChordQuality) -> RomanCase {
    match quality {
        ChordQuality::Minor | ChordQuality::Diminished => RomanCase::Lower,
        _ => RomanCase::Upper,
    }
}

/// Everything a Roman numeral carries *after* the numeral itself: the triad
/// decoration (`°`, `+`, `sus4`) plus any seventh / extension / alteration /
/// addition / omission tail. The major/minor third is conveyed by the
/// numeral's case, so no `m` is emitted here.
fn roman_tail(parsed: &Chord) -> String {
    let mut out = String::new();

    // Triad decoration. `°`/`+` only when there is no seventh family (a
    // seventh family renders its own symbol in the tail below).
    let decoration = if parsed.family.is_none() {
        match parsed.quality {
            ChordQuality::Diminished => "°",
            ChordQuality::Augmented => "+",
            ChordQuality::Suspended(SuspendedType::Second) => "sus2",
            ChordQuality::Suspended(SuspendedType::Fourth) => "sus4",
            _ => "",
        }
    } else {
        match parsed.quality {
            ChordQuality::Suspended(SuspendedType::Second) => "sus2",
            ChordQuality::Suspended(SuspendedType::Fourth) => "sus4",
            _ => "",
        }
    };
    out.push_str(decoration);

    // Seventh / extensions / alterations / additions / omissions. We render a
    // quality-neutralised clone (quality forced Major, so no base `m`/`dim`/`+`
    // leaks in) with an empty root and no bass, which yields exactly the tail.
    let mut neutral = parsed.clone();
    neutral.quality = ChordQuality::Major;
    neutral.root = RootNotation::empty();
    neutral.bass = None;
    out.push_str(&neutral.to_string());

    out
}

// ===========================================================================
// Key transposition
// ===========================================================================

/// Transpose a key up by `delta` semitones (0-11), preserving its mode and
/// choosing the conventional spelling for the new root.
fn transpose_key_by(key: &Key, delta: u8) -> Key {
    let new_semitone = (key.root.semitone + delta) % 12;
    let root = preferred_key_root(new_semitone);
    Key::new(root, key.mode)
}

/// The conventional spelling of a major/minor key root by semitone: flats for
/// the black keys except `F#` (the usual notated choice).
fn preferred_key_root(semitone: u8) -> MusicalNote {
    let name = match semitone % 12 {
        0 => "C",
        1 => "Db",
        2 => "D",
        3 => "Eb",
        4 => "E",
        5 => "F",
        6 => "F#",
        7 => "G",
        8 => "Ab",
        9 => "A",
        10 => "Bb",
        11 => "B",
        _ => unreachable!(),
    };
    MusicalNote::from_string(name).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::types::{ChartSection, Measure};
    use crate::chord::ChordRhythm;
    use crate::sections::{Section, SectionType};
    use crate::time::{AbsolutePosition, MusicalDuration, MusicalPosition};
    use keyflow_syntax::parsing::Lexer;

    /// Build a single-section, single-measure chart in `key` from a list of
    /// chord source tokens (letters, Nashville, or Roman — parsed via the
    /// shared chord parser). Avoids the keyflow-text dev-dep so there is only
    /// one `keyflow-proto` in the graph.
    fn chart_in(key: Key, chords: &[&str]) -> Chart {
        let mut chart = Chart::new();
        chart.initial_key = Some(key.clone());
        chart.current_key = Some(key);

        let mut measure = Measure::new();
        for (i, src) in chords.iter().enumerate() {
            let mut lexer = Lexer::new((*src).to_string());
            let parsed = Chord::parse(&lexer.tokenize()).expect("chord should parse");
            let ci = ChordInstance::new(
                parsed.root.clone(),
                (*src).to_string(),
                parsed,
                ChordRhythm::Default,
                (*src).to_string(),
                MusicalDuration::new(0, 1, 0),
                AbsolutePosition::new(MusicalPosition::new(0, i as i32, 0), 0),
            );
            measure.chords.push(ci);
        }

        let section =
            ChartSection::new(Section::new(SectionType::Verse)).with_measures(vec![measure]);
        chart.sections.push(section);
        chart
    }

    /// Collect the visible chord `full_symbol`s across a chart, in order.
    fn symbols(chart: &Chart) -> Vec<String> {
        let mut out = Vec::new();
        for section in &chart.sections {
            for track in &section.tracks {
                for measure in &track.measures {
                    for chord in &measure.chords {
                        if !chord.full_symbol.is_empty()
                            && chord.full_symbol != "s"
                            && chord.full_symbol != "r"
                        {
                            out.push(chord.full_symbol.clone());
                        }
                    }
                }
            }
        }
        out
    }

    fn key(s: &str) -> Key {
        Key::parse(s).unwrap()
    }

    #[test]
    fn letters_a_to_g() {
        // A D E F#m in A major, transposed to sound in G.
        let c = chart_in(key("A"), &["A", "D", "E", "F#m"]);
        let view = ChartView {
            target_key: Some(key("G")),
            notation: NotationSystem::Letters,
            capo: 0,
        };
        let out = apply_view(&c, &view);
        assert_eq!(symbols(&out), vec!["G", "C", "D", "Em"]);
        // Metadata reflects the displayed key.
        assert_eq!(out.initial_key, Some(key("G")));
    }

    #[test]
    fn nashville_in_g() {
        let c = chart_in(key("G"), &["G", "C", "D", "Em"]);
        let view = ChartView {
            target_key: None,
            notation: NotationSystem::Nashville,
            capo: 0,
        };
        let out = apply_view(&c, &view);
        assert_eq!(symbols(&out), vec!["1", "4", "5", "6m"]);
    }

    #[test]
    fn roman_in_g() {
        let c = chart_in(key("G"), &["G", "C", "D", "Em"]);
        let view = ChartView {
            target_key: None,
            notation: NotationSystem::Roman,
            capo: 0,
        };
        let out = apply_view(&c, &view);
        assert_eq!(symbols(&out), vec!["I", "IV", "V", "vi"]);
    }

    #[test]
    fn slash_chord_transpose_bass() {
        // A/C# in A major → G/B when sounding in G.
        let c = chart_in(key("A"), &["A/C#", "D"]);
        let view = ChartView {
            target_key: Some(key("G")),
            notation: NotationSystem::Letters,
            capo: 0,
        };
        let out = apply_view(&c, &view);
        assert_eq!(symbols(&out), vec!["G/B", "C"]);
    }

    #[test]
    fn capo_helpers() {
        // Sound B using G shapes → capo 4.
        assert_eq!(capo_to_play(key("B"), key("G")), Some(4));
        assert_eq!(shape_key_for_capo(key("B"), 4), key("G"));
    }

    #[test]
    fn capo_view_renders_shape_key() {
        // Chart in B, sound in B with a capo 4 → render G shapes.
        let c = chart_in(key("B"), &["B", "E", "F#"]);
        let view = ChartView {
            target_key: Some(key("B")),
            notation: NotationSystem::Letters,
            capo: 4,
        };
        let out = apply_view(&c, &view);
        assert_eq!(symbols(&out), vec!["G", "C", "D"]);
        assert_eq!(view.capo_caption().as_deref(), Some("Capo 4 (sounds B)"));
        assert_eq!(view.shape_key(), Some(key("G")));
    }

    #[test]
    fn identity_view_is_equivalent() {
        let c = chart_in(key("C"), &["C", "G", "Am", "F"]);
        let view = ChartView::default();
        let out = apply_view(&c, &view);
        assert_eq!(symbols(&out), symbols(&c));
        assert_eq!(out.initial_key, c.initial_key);
    }

    #[test]
    fn non_diatonic_chord_renders_all_notations() {
        // Bb (bVII) in the key of C — non-diatonic, must not panic or drop.
        let c = chart_in(key("C"), &["C", "Bb"]);

        let letters = apply_view(
            &c,
            &ChartView {
                target_key: Some(key("C")),
                notation: NotationSystem::Letters,
                capo: 0,
            },
        );
        assert_eq!(symbols(&letters), vec!["C", "Bb"]);

        let nashville = apply_view(
            &c,
            &ChartView {
                target_key: None,
                notation: NotationSystem::Nashville,
                capo: 0,
            },
        );
        assert_eq!(symbols(&nashville), vec!["1", "b7"]);

        let roman = apply_view(
            &c,
            &ChartView {
                target_key: None,
                notation: NotationSystem::Roman,
                capo: 0,
            },
        );
        assert_eq!(symbols(&roman), vec!["I", "bVII"]);
    }
}
