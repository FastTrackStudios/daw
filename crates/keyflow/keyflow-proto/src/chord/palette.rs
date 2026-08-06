//! The chords available in a key, grouped by what they're *for*.
//!
//! Three families, because that's how the choice actually gets made when
//! you're writing:
//!
//! - [`ChordRole::Diatonic`] — in the key. The stable furniture.
//! - [`ChordRole::ParallelKey`] — borrowed from the parallel major/minor
//!   (modal interchange). Same tonic, other mode: the ♭VI, ♭VII and iv
//!   that colour a major key without leaving it.
//! - [`ChordRole::Approach`] — chords that point somewhere. Each carries
//!   the degree it targets, because an approach chord is meaningless
//!   without saying what it approaches.
//!
//! The approach set is the secondary dominant (V7/x) and the tritone
//! substitution (♭II7/x) of every diatonic degree — the two routes that
//! do most of the work in the "stable chord, altered dominant, stable
//! chord" alternation. Alterations on top of those (♭9, ♯9, ♯11, ♭13)
//! are voicing choices layered over the same function, not separate
//! chords, so they belong to a voicing layer rather than here.
//!
//! Pitch sets rather than [`Chord`](super::Chord) values: this crate sits
//! below the chord *parser*, and what a chord-firing UI needs is a label,
//! a role and some notes.

use crate::key::Key;
use crate::primitives::MusicalNote;

/// What a chord is doing in the current key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChordRole {
    /// Built from the key's own scale.
    Diatonic,
    /// Borrowed from the parallel major/minor — same tonic, other mode.
    ParallelKey,
    /// Points at a diatonic degree (1-7). See [`ApproachKind`].
    Approach { target_degree: u8, kind: ApproachKind },
}

/// How an approach chord reaches its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApproachKind {
    /// V7 of the target — a fifth above, resolving down a fifth.
    SecondaryDominant,
    /// ♭II7 of the target — a semitone above, resolving down a semitone.
    /// Shares its tritone with the secondary dominant, which is why the
    /// two are interchangeable.
    TritoneSub,
}

/// One offer in the palette: what to call it, what it's for, and what to
/// play.
#[derive(Debug, Clone, PartialEq)]
pub struct ChordCandidate {
    /// Display name, e.g. `"Fm"`, `"A♭"`, `"G7"`.
    pub label: String,
    pub role: ChordRole,
    /// Root pitch class, 0-11.
    pub root_pc: u8,
    /// Semitones above the root.
    pub semitones: Vec<u8>,
}

impl ChordCandidate {
    /// Concrete MIDI notes with the root in `octave` (4 → middle C).
    ///
    /// Anything that would leave MIDI range is dropped rather than
    /// wrapped, so an extreme octave thins the chord instead of
    /// scrambling its voicing.
    pub fn notes(&self, octave: i32) -> Vec<u8> {
        let root = (octave + 1) * 12 + i32::from(self.root_pc);
        self.semitones
            .iter()
            .filter_map(|s| {
                let n = root + i32::from(*s);
                (0..=127).contains(&n).then_some(n as u8)
            })
            .collect()
    }
}

const MAJOR: [u8; 3] = [0, 4, 7];
const MINOR: [u8; 3] = [0, 3, 7];
const DIM: [u8; 3] = [0, 3, 6];
const AUG: [u8; 3] = [0, 4, 8];
const DOM7: [u8; 4] = [0, 4, 7, 10];

fn quality_suffix(semitones: &[u8]) -> &'static str {
    match semitones {
        s if s == MAJOR => "",
        s if s == MINOR => "m",
        s if s == DIM => "°",
        s if s == AUG => "+",
        s if s == DOM7 => "7",
        _ => "",
    }
}

/// Note name for a pitch class. Flats in flat keys, sharps otherwise —
/// so a borrowed ♭VI in C reads `A♭`, not `G♯`.
fn pc_name(pc: u8, prefer_sharp: bool) -> String {
    MusicalNote::from_semitone(pc % 12, prefer_sharp).name
}

/// Whether a key spells its own diatonic chords with sharps. Rough but
/// right for the common cases: flat tonics spell flats.
fn prefers_sharps(key: &Key) -> bool {
    !matches!(key.root.semitone, 1 | 3 | 5 | 8 | 10)
}

/// Spelling follows *function*, not just the key.
///
/// A borrowed chord and a tritone sub are flat-side by construction —
/// ♭VI, ♭VII, ♭II — so they spell flat even in a sharp key. Getting this
/// from the key alone gives `C♯7` where the chart says `D♭7`.
fn spell_sharp(key: &Key, role: ChordRole) -> bool {
    match role {
        ChordRole::ParallelKey => false,
        ChordRole::Approach {
            kind: ApproachKind::TritoneSub,
            ..
        } => false,
        _ => prefers_sharps(key),
    }
}

/// Stack thirds within `scale` (cumulative semitone offsets) starting at
/// `degree_index`, taking `count` notes.
fn stack_thirds(scale: &[u8], degree_index: usize, count: usize) -> Vec<u8> {
    let len = scale.len();
    if len == 0 {
        return Vec::new();
    }
    let root = i32::from(scale[degree_index % len]);
    (0..count)
        .map(|i| {
            let idx = degree_index + i * 2;
            let octaves = (idx / len) as i32;
            let offset = i32::from(scale[idx % len]) + octaves * 12;
            (offset - root).rem_euclid(12 * 4) as u8
        })
        .collect()
}

/// The seven diatonic triads of `key`.
pub fn diatonic(key: &Key) -> Vec<ChordCandidate> {
    let scale = key.mode.interval_pattern();
    let sharp = prefers_sharps(key);
    (0..scale.len().min(7))
        .map(|i| {
            let semis = stack_thirds(&scale, i, 3);
            let root_pc = (key.root.semitone + scale[i]) % 12;
            ChordCandidate {
                label: format!("{}{}", pc_name(root_pc, sharp), quality_suffix(&semis)),
                role: ChordRole::Diatonic,
                root_pc,
                semitones: semis,
            }
        })
        .collect()
}

/// Triads borrowed from the parallel mode, minus anything already
/// diatonic — those aren't borrowed, they're shared.
pub fn parallel_key(key: &Key) -> Vec<ChordCandidate> {
    let parallel = if is_minor_ish(key) {
        Key::major(key.root.clone())
    } else {
        Key::minor(key.root.clone())
    };
    let own: Vec<(u8, Vec<u8>)> = diatonic(key)
        .into_iter()
        .map(|c| (c.root_pc, c.semitones))
        .collect();

    diatonic(&parallel)
        .into_iter()
        .filter(|c| !own.iter().any(|(pc, s)| *pc == c.root_pc && *s == c.semitones))
        .map(|mut c| {
            c.role = ChordRole::ParallelKey;
            c.label = format!(
                "{}{}",
                pc_name(c.root_pc, spell_sharp(key, ChordRole::ParallelKey)),
                quality_suffix(&c.semitones)
            );
            c
        })
        .collect()
}

/// Whether `key`'s third is minor — good enough to pick the parallel.
fn is_minor_ish(key: &Key) -> bool {
    key.mode.interval_pattern().get(2).is_some_and(|third| *third == 3)
}

/// Secondary dominants and tritone subs for each diatonic degree.
///
/// Both are dominant sevenths; they differ only in where they sit
/// relative to the target, and they share a tritone, which is why either
/// resolves.
pub fn approach(key: &Key) -> Vec<ChordCandidate> {
    let mut out = Vec::new();
    for (i, target) in diatonic(key).into_iter().enumerate() {
        let degree = (i + 1) as u8;
        for (kind, offset) in [
            (ApproachKind::SecondaryDominant, 7u8),
            (ApproachKind::TritoneSub, 1u8),
        ] {
            let root_pc = (target.root_pc + offset) % 12;
            let role = ChordRole::Approach {
                target_degree: degree,
                kind,
            };
            out.push(ChordCandidate {
                label: format!("{}7", pc_name(root_pc, spell_sharp(key, role))),
                role,
                root_pc,
                semitones: DOM7.to_vec(),
            });
        }
    }
    out
}

/// Everything, in the order a panel should show it.
pub fn palette(key: &Key) -> Vec<ChordCandidate> {
    let mut out = diatonic(key);
    out.extend(parallel_key(key));
    out.extend(approach(key));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c_major() -> Key {
        Key::major(MusicalNote::from_string("C").expect("C"))
    }

    #[test]
    fn c_major_diatonic_is_the_familiar_seven() {
        let labels: Vec<String> = diatonic(&c_major()).into_iter().map(|c| c.label).collect();
        assert_eq!(labels, ["C", "Dm", "Em", "F", "G", "Am", "B°"]);
    }

    /// The borrowed chords that give a major key its minor colour. None
    /// of them are already in C major, or they wouldn't be borrowed.
    #[test]
    fn parallel_minor_lends_the_flat_side() {
        let labels: Vec<String> = parallel_key(&c_major())
            .into_iter()
            .map(|c| c.label)
            .collect();
        for expected in ["Fm", "Ab", "Bb"] {
            assert!(
                labels.iter().any(|l| l == expected),
                "expected {expected} among {labels:?}"
            );
        }
        assert!(!labels.iter().any(|l| l == "C"), "C is shared, not borrowed");
    }

    /// The secondary dominant of ii in C is A7 — a fifth above D.
    #[test]
    fn secondary_dominant_sits_a_fifth_above_its_target() {
        let a7 = approach(&c_major())
            .into_iter()
            .find(|c| {
                c.role
                    == ChordRole::Approach {
                        target_degree: 2,
                        kind: ApproachKind::SecondaryDominant,
                    }
            })
            .expect("V7/ii exists");
        assert_eq!(a7.label, "A7");
        assert_eq!(a7.semitones, DOM7.to_vec());
    }

    /// The tritone sub sits a semitone above its target and shares the
    /// secondary dominant's tritone — that shared tritone is the whole
    /// reason the substitution works, so assert it rather than the name.
    #[test]
    fn tritone_sub_shares_the_dominant_tritone() {
        let all = approach(&c_major());
        let find = |kind| {
            all.iter()
                .find(|c| {
                    c.role
                        == ChordRole::Approach {
                            target_degree: 1,
                            kind,
                        }
                })
                .expect("approach exists")
        };
        let dom = find(ApproachKind::SecondaryDominant);
        let sub = find(ApproachKind::TritoneSub);

        assert_eq!(dom.label, "G7", "V7/I");
        assert_eq!(sub.label, "Db7", "bII7/I");
        assert_eq!((sub.root_pc + 6) % 12, dom.root_pc, "a tritone apart");

        let tritone = |c: &ChordCandidate| {
            let third = (c.root_pc + 4) % 12;
            let seventh = (c.root_pc + 10) % 12;
            let mut t = [third, seventh];
            t.sort_unstable();
            t
        };
        assert_eq!(tritone(dom), tritone(sub), "same third/seventh pair");
    }

    #[test]
    fn every_degree_gets_both_approaches() {
        let approaches = approach(&c_major());
        assert_eq!(approaches.len(), 14, "7 degrees x 2 routes");
        for degree in 1..=7u8 {
            for kind in [ApproachKind::SecondaryDominant, ApproachKind::TritoneSub] {
                assert!(
                    approaches.iter().any(|c| c.role
                        == ChordRole::Approach {
                            target_degree: degree,
                            kind
                        }),
                    "missing {kind:?} for degree {degree}"
                );
            }
        }
    }

    #[test]
    fn tonic_triad_realizes_to_middle_c() {
        let tonic = &diatonic(&c_major())[0];
        assert_eq!(tonic.notes(4), vec![60, 64, 67]);
    }

    #[test]
    fn flat_keys_spell_with_flats() {
        let e_flat = Key::major(MusicalNote::from_string("Eb").expect("Eb"));
        let labels: Vec<String> = diatonic(&e_flat).into_iter().map(|c| c.label).collect();
        assert!(
            labels.iter().any(|l| l.contains('b')),
            "E♭ major should spell flats, got {labels:?}"
        );
    }
}
