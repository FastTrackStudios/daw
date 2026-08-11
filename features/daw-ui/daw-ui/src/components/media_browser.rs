//! The media browser — a right-hand sidebar for auditioning media.
//!
//! Matching REAPER *functionality*, not REAPER's chrome: this panel is a
//! deliberate departure from the pixel-matching the other panels do, per
//! the two references it grew from (TK Media Browser, MIDI Browser — see
//! the daw-ui docs). The UI decisions that are ours:
//!
//! - **Every row carries its own preview.** A waveform or note roll drawn
//!   inline per entry, so a folder reads like a contact sheet instead of
//!   click-row-look-up-at-one-big-pane. The renderers are the arrange
//!   view's own [`Waveform`]-style shapes, reused through
//!   [`ItemPreview`] — one drawing per kind of content, everywhere.
//! - **Metadata is chips, not columns.** BPM, key and duration read off
//!   the *filename* the way sample packs actually encode them
//!   (`"Am 92bpm groove.wav"`), parsed by [`parse_media_name`] — the MIDI
//!   Browser's key-detection trick, kept.
//! - **Selection expands in place.** The selected row grows a taller
//!   preview band rather than opening a second pane.
//!
//! Pure and fixture-driven like every `*Preview`: entries in, markup out.
//! The live half — filesystem scan, audition transport, drag-to-arrange —
//! comes behind it; nothing here assumes a backend.

use crate::components::arrangement_view::{ItemPreview, NotePreview};
use crate::prelude::*;

/// What kind of media an entry is — decides the badge and the renderer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaKind {
    Audio,
    Midi,
}

/// One file in the browser.
#[derive(Clone, PartialEq, Debug)]
pub struct MediaEntry {
    /// The filename, as it is on disk.
    pub name: String,
    pub kind: MediaKind,
    /// Length in seconds, for the duration chip.
    pub duration: f32,
    /// Content preview, same currency as the arrange items'.
    pub preview: Option<ItemPreview>,
}

/// What a filename says about its media, in the conventions sample packs
/// actually use.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct MediaMeta {
    /// The name with its extension and any parsed tokens intact — the
    /// display name is the honest filename, the chips are the parse.
    pub bpm: Option<u32>,
    /// `"F#m"`, `"Am"`, `"C"` — normalised to note + optional `m`.
    pub key: Option<String>,
}

/// Read BPM and key out of a filename.
///
/// The conventions, from actual pack names: BPM is a number glued to
/// `bpm` (`92bpm`, `bpm140`) or a bare 2–3 digit token in the plausible
/// range; a key is a note letter, optional accidental, and an optional
/// minor mark (`F#MIN`, `Am`, `d min`). Case-blind, order-blind. A token
/// that parses as both (`120`) is a BPM — nobody writes a key as digits.
pub fn parse_media_name(name: &str) -> MediaMeta {
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    let mut meta = MediaMeta::default();

    for raw in stem.split(|c: char| !c.is_ascii_alphanumeric() && c != '#') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let lower = token.to_ascii_lowercase();

        // BPM: `92bpm` / `bpm92` / a bare plausible number.
        if meta.bpm.is_none() {
            let digits: String = lower.chars().filter(|c| c.is_ascii_digit()).collect();
            let rest: String = lower.chars().filter(|c| c.is_ascii_alphabetic()).collect();
            if !digits.is_empty() && (rest == "bpm" || rest.is_empty()) {
                if let Ok(n) = digits.parse::<u32>() {
                    let plausible = (40..=250).contains(&n);
                    if rest == "bpm" || plausible {
                        meta.bpm = Some(n);
                        continue;
                    }
                }
            }
        }

        // Key: note letter + optional #/b + optional minor mark.
        if meta.key.is_none() {
            if let Some(key) = parse_key(&lower) {
                meta.key = Some(key);
            }
        }
    }
    meta
}

/// `"f#min"` → `"F#m"`, `"am"` → `"Am"`, `"c"` → `"C"`. `None` when the
/// token is not a key — which is most tokens, so this refuses eagerly.
fn parse_key(token: &str) -> Option<String> {
    let mut chars = token.chars();
    let note = chars.next()?;
    if !('a'..='g').contains(&note) {
        return None;
    }
    let rest: String = chars.collect();
    let (accidental, rest) = match rest.strip_prefix('#') {
        Some(r) => ("#", r),
        None => match rest.strip_prefix('b') {
            // `bb…` could be the word "bass"; only accept a lone flat.
            Some(r) if r.len() <= 3 => ("b", r),
            _ => ("", rest.as_str()),
        },
    };
    let minor = matches!(rest, "m" | "min" | "minor");
    if !rest.is_empty() && !minor && rest != "maj" && rest != "major" {
        return None;
    }
    Some(format!(
        "{}{}{}",
        note.to_ascii_uppercase(),
        accidental,
        if minor { "m" } else { "" }
    ))
}

/// The browser panel.
#[component]
pub fn MediaBrowserPanel(
    entries: Vec<MediaEntry>,
    /// The selected row, expanded in place.
    #[props(default)]
    selected: Option<usize>,
    /// The folder shown in the location row.
    #[props(default = String::from("Samples"))]
    location: String,
    width: f32,
    height: f32,
) -> Element {
    let t = daw_theme::Theme::default();
    // A raised surface, deliberately one step above the arrange it sits
    // beside — this panel is furniture, not project.
    let bg = t.chrome.surface_raised.css();
    let rule = t.chrome.surface_sunken.shade(-0.2).css();
    let field = t.chrome.surface_sunken.css();
    let ink = t.chrome.text.css();
    let dim = t.chrome.text_dim.css();
    let count = entries.len();

    rsx! {
        div {
            style: "width:{width}px; height:{height}px; display:flex; \
                    flex-direction:column; background:{bg}; \
                    border-left:1px solid {rule}; overflow:hidden;",

            // ── Header: title + search ──
            div {
                style: "flex:0 0 auto; padding:8px 10px 6px 10px;",
                div {
                    style: "font-size:11px; font-weight:600; color:{ink}; \
                            letter-spacing:0.06em; margin-bottom:6px; \
                            font-family:Fira Sans, DejaVu Sans, sans-serif;",
                    "MEDIA"
                }
                div {
                    style: "height:22px; background:{field}; border-radius:11px; \
                            display:flex; align-items:center; padding:0 9px;",
                    svg {
                        width: "10", height: "10", view_box: "0 0 10 10",
                        xmlns: "http://www.w3.org/2000/svg",
                        circle { cx: "4.2", cy: "4.2", r: "3.2", fill: "none",
                                 stroke: "{dim}", stroke_width: "1.4" }
                        path { d: "M 6.6 6.6 L 9.2 9.2", stroke: "{dim}",
                               stroke_width: "1.4", stroke_linecap: "round" }
                    }
                    div {
                        style: "margin-left:6px; font-size:10px; color:{dim}; \
                                font-family:Fira Sans, DejaVu Sans, sans-serif;",
                        "Search"
                    }
                }
            }

            // ── Location row ──
            div {
                style: "flex:0 0 auto; padding:0 10px 6px 10px; font-size:10px; \
                        color:{dim}; border-bottom:1px solid {rule}; \
                        font-family:Fira Sans, DejaVu Sans, sans-serif;",
                "▸ {location}"
            }

            // ── The rows ──
            div {
                style: "flex:1 1 0; min-height:0; overflow:hidden; padding:4px 6px;",
                for (i, entry) in entries.iter().enumerate() {
                    MediaRow {
                        key: "{entry.name}",
                        entry: entry.clone(),
                        selected: selected == Some(i),
                        width: width - 12.0,
                    }
                }
            }

            // ── Footer ──
            div {
                style: "flex:0 0 auto; padding:5px 10px; font-size:9px; color:{dim}; \
                        border-top:1px solid {rule}; \
                        font-family:Fira Sans, DejaVu Sans, sans-serif;",
                "{count} files"
            }
        }
    }
}

/// One entry: badge, name, chips, and the inline preview.
#[component]
fn MediaRow(entry: MediaEntry, selected: bool, width: f32) -> Element {
    let t = daw_theme::Theme::default();
    let meta = parse_media_name(&entry.name);

    let ink = t.chrome.text.shade(0.25).css();
    let dim = t.chrome.text_dim.css();
    let row_bg = if selected {
        t.chrome.surface_sunken.shade(0.06).css()
    } else {
        "transparent".to_string()
    };
    let edge = if selected {
        t.chrome.accent.css()
    } else {
        "transparent".to_string()
    };
    // The preview inks: audio in the accent family, MIDI in the solo
    // amber — the two content kinds stay tellable apart at a glance.
    let mark = match entry.kind {
        MediaKind::Audio => t.chrome.accent.shade(-0.15),
        MediaKind::Midi => t.signal.solo.shade(-0.05),
    };
    let (badge, badge_ink) = match entry.kind {
        MediaKind::Audio => ("WAV", t.chrome.accent.css()),
        MediaKind::Midi => ("MID", t.signal.solo.css()),
    };
    // Selection expands in place: the preview band is the part that grows.
    let band_h = if selected { 44.0 } else { 20.0 };
    let dur = format_duration(entry.duration);

    rsx! {
        div {
            style: "position:relative; margin:2px 0; padding:4px 6px 4px 8px; \
                    background:{row_bg}; border-radius:4px; \
                    border-left:2px solid {edge}; overflow:hidden;",

            // Name line: badge, name, chips.
            div {
                style: "display:flex; align-items:center; gap:5px; \
                        margin-bottom:3px;",
                div {
                    style: "flex:0 0 auto; font-size:7px; font-weight:700; \
                            color:{badge_ink}; letter-spacing:0.08em; \
                            border:1px solid {badge_ink}; border-radius:2px; \
                            padding:0 3px; line-height:9px; opacity:0.85; \
                            font-family:Fira Sans, DejaVu Sans, sans-serif;",
                    "{badge}"
                }
                div {
                    style: "flex:1 1 auto; min-width:0; font-size:10px; color:{ink}; \
                            white-space:nowrap; overflow:hidden; \
                            text-overflow:ellipsis; \
                            font-family:Fira Sans, DejaVu Sans, sans-serif;",
                    "{entry.name}"
                }
                if let Some(bpm) = meta.bpm {
                    MetaChip { text: format!("{bpm}") }
                }
                if let Some(key) = &meta.key {
                    MetaChip { text: key.clone() }
                }
                div {
                    style: "flex:0 0 auto; font-size:8px; color:{dim}; \
                            font-variant-numeric:tabular-nums; \
                            font-family:DejaVu Sans Mono, monospace;",
                    "{dur}"
                }
            }

            // The inline preview — the same drawings the arrange items use.
            div {
                style: "position:relative; height:{band_h}px;",
                match &entry.preview {
                    Some(ItemPreview::Waveform(amps)) if !amps.is_empty() => rsx! {
                        crate::components::arrangement_view::Waveform {
                            amps: amps.clone(),
                            width: width - 16.0,
                            top: 0.0,
                            height: band_h,
                            colour: mark.css(),
                        }
                    },
                    Some(ItemPreview::Notes(notes)) if !notes.is_empty() => rsx! {
                        crate::components::arrangement_view::NoteRoll {
                            notes: notes.clone(),
                            length: entry.duration,
                            width: width - 16.0,
                            top: 0.0,
                            height: band_h,
                            colour: mark.css(),
                        }
                    },
                    _ => rsx! {
                        div {
                            style: "height:{band_h}px; border-radius:2px; \
                                    background:rgba(0,0,0,0.12);",
                        }
                    },
                }
            }
        }
    }
}

/// A small rounded metadata chip.
#[component]
fn MetaChip(text: String) -> Element {
    let t = daw_theme::Theme::default();
    let ink = t.chrome.text_dim.shade(0.15).css();
    let bg = t.chrome.surface_sunken.shade(0.04).css();
    rsx! {
        div {
            style: "flex:0 0 auto; font-size:8px; color:{ink}; background:{bg}; \
                    border-radius:7px; padding:1px 5px; line-height:10px; \
                    font-family:Fira Sans, DejaVu Sans, sans-serif;",
            "{text}"
        }
    }
}

fn format_duration(secs: f32) -> String {
    if secs >= 60.0 {
        format!("{}:{:04.1}", (secs / 60.0) as u32, secs % 60.0)
    } else {
        format!("{secs:.1}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The conventions the references' packs actually use, and the traps:
    /// a bare number is a BPM only in range, `bass` is not B-flat, and
    /// `120` never reads as a key.
    #[test]
    fn filenames_give_up_their_bpm_and_key() {
        let m = parse_media_name("F#MIN 92bpm [SHARK].mid");
        assert_eq!(m.bpm, Some(92));
        assert_eq!(m.key.as_deref(), Some("F#m"));

        let m = parse_media_name("Am_140_groove.wav");
        assert_eq!(m.bpm, Some(140));
        assert_eq!(m.key.as_deref(), Some("Am"));

        let m = parse_media_name("kick_punchy.wav");
        assert_eq!(m.bpm, None);
        assert_eq!(m.key, None);

        // "bass" must not read as B-flat; 808 is not a plausible BPM.
        let m = parse_media_name("bass_808_sub.wav");
        assert_eq!(m.key, None);
        assert_eq!(m.bpm, None);

        let m = parse_media_name("bpm175 D loop.wav");
        assert_eq!(m.bpm, Some(175));
        assert_eq!(m.key.as_deref(), Some("D"));
    }

    #[test]
    fn durations_format_both_sides_of_a_minute() {
        assert_eq!(format_duration(3.4), "3.4s");
        assert_eq!(format_duration(83.0), "1:23.0");
    }
}
