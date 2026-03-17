//! Offline guide track generation — generates Click, Count, and Guide MIDI items
//! from a parsed project's regions and tempo map.
//!
//! Produces MIDI items that can be inserted into the Click + Guide folder tracks
//! without requiring a running REAPER instance.

use crate::types::item::Item;
use crate::types::project::ReaperProject;
use crate::types::time_tempo::TempoTimeEnvelope;
use crate::types::track::Track;

const TICKS_PER_QN: u32 = 960;

/// Generate guide MIDI items for Click, Count, and Guide tracks.
///
/// Returns `(click_items, count_items, guide_items)` that can be placed
/// on the corresponding tracks in the Click + Guide folder.
pub fn generate_guide_items(
    project: &ReaperProject,
) -> (Vec<Item>, Vec<Item>, Vec<Item>) {
    let mut click_items = Vec::new();
    let mut count_items = Vec::new();
    let mut guide_items = Vec::new();

    // Get regions (sections)
    let regions: Vec<_> = project
        .markers_regions
        .all
        .iter()
        .filter(|m| m.is_region() && !m.name.is_empty())
        .filter(|m| {
            // Skip structural regions (SONG lane, etc.)
            let upper = m.name.to_uppercase();
            !matches!(
                upper.as_str(),
                "SONGSTART" | "SONGEND" | "=START" | "=END" | "PREROLL" | "POSTROLL"
            )
        })
        .collect();

    let region_names: Vec<String> = regions.iter().map(|r| {
        format!("{}@{:.1}", r.name, r.position)
    }).collect();
    eprintln!("[guide_gen] {} regions: {:?}", regions.len(), region_names);

    if regions.is_empty() {
        return (click_items, count_items, guide_items);
    }

    // Get tempo info
    let (default_bpm, default_num, default_denom) =
        if let Some((bpm, num, denom, _)) = project.properties.tempo {
            (bpm as f64, num as u32, denom as u32)
        } else {
            (120.0, 4, 4)
        };

    let tempo_env = project.tempo_envelope.as_ref();

    let mut prev_region_end = 0.0f64;

    for region in &regions {
        let section_start = region.position;
        let section_end = region.end_position.unwrap_or(section_start + 1.0);

        // Get tempo and time signature at the section start
        let (bpm, num, denom) =
            tempo_at_position(section_start, tempo_env, default_bpm, default_num, default_denom);

        let beat_unit = 4.0 / denom as f64;
        let qn_per_measure = beat_unit * num as f64;
        let seconds_per_qn = 60.0 / bpm;
        let measure_seconds = qn_per_measure * seconds_per_qn;

        // Count-in: one measure before section, clamped to prev region end
        let count_in_start = (section_start - measure_seconds).max(prev_region_end).max(0.0);

        // ── Click MIDI item ──
        // Full section + count-in, with accent on beat 1
        let click_start = count_in_start;
        let click_end = section_end;
        if click_end > click_start {
            let mut notes = Vec::new();
            let mut pos = count_in_start;
            let beat_seconds = seconds_per_qn * beat_unit;

            while pos < click_end - 0.001 {
                let is_beat_one = ((pos - count_in_start) / measure_seconds).fract() < 0.01
                    || ((pos - count_in_start) / measure_seconds).fract() > 0.99;
                let (midi_note, velocity) = if is_beat_one {
                    (76, 120) // Hi woodblock, accent
                } else {
                    (77, 100) // Lo woodblock, normal
                };
                notes.push((pos, midi_note, velocity));
                pos += beat_seconds;
            }

            if !notes.is_empty() {
                click_items.push(make_midi_item(click_start, click_end, &notes));
            }
        }

        // ── Count MIDI item ──
        // One measure of count beats before section start
        if count_in_start < section_start - 0.001 {
            let mut notes = Vec::new();
            let beat_seconds = seconds_per_qn * beat_unit;
            let mut pos = count_in_start;
            let mut beat_num = 1u32;

            while pos < section_start - 0.001 && beat_num <= num {
                // Count notes: different pitch per beat number
                let midi_note = 60 + beat_num as u8; // C4, C#4, D4, D#4...
                notes.push((pos, midi_note, 100));
                pos += beat_seconds;
                beat_num += 1;
            }

            if !notes.is_empty() {
                count_items.push(make_midi_item(count_in_start, section_start, &notes));
            }
        }

        // ── Guide MIDI item ──
        // Section cue: single note at count-in start indicating section type
        let section_midi = section_name_to_midi(&region.name);
        if let Some(midi_note) = section_midi {
            guide_items.push(make_midi_item(
                count_in_start,
                count_in_start + measure_seconds,
                &[(count_in_start, midi_note, 127)],
            ));
        }

        prev_region_end = section_end;
    }

    (click_items, count_items, guide_items)
}

/// Place generated guide items onto the Click, Count, and Guide tracks
/// in a project's track list.
pub fn apply_guide_items(tracks: &mut [Track], project: &ReaperProject) {
    let (click_items, count_items, guide_items) = generate_guide_items(project);

    for track in tracks.iter_mut() {
        let lower = track.name.to_lowercase();
        match lower.as_str() {
            "click" => track.items.extend(click_items.clone()),
            "count" => track.items.extend(count_items.clone()),
            "guide" => track.items.extend(guide_items.clone()),
            _ => {}
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Get tempo and time signature at a position using the tempo envelope.
fn tempo_at_position(
    position: f64,
    envelope: Option<&TempoTimeEnvelope>,
    default_bpm: f64,
    default_num: u32,
    default_denom: u32,
) -> (f64, u32, u32) {
    let Some(env) = envelope else {
        return (default_bpm, default_num, default_denom);
    };

    let mut bpm = default_bpm;
    let mut num = default_num;
    let mut denom = default_denom;

    for pt in &env.points {
        if pt.position > position {
            break;
        }
        bpm = pt.tempo;
        if let Some(ts) = pt.time_signature_encoded {
            num = (ts & 0xFFFF) as u32;
            denom = (ts >> 16) as u32;
            if num == 0 { num = default_num; }
            if denom == 0 { denom = default_denom; }
        }
    }

    (bpm, num, denom)
}

/// Create a MIDI item with note-on/note-off pairs as raw RPP content.
fn make_midi_item(start: f64, end: f64, notes: &[(f64, u8, u8)]) -> Item {
    let duration = end - start;
    let note_length_ticks = 120u32;

    // Sort notes by position
    let mut sorted: Vec<(f64, u8, u8)> = notes.to_vec();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    // Build MIDI E lines (delta-time encoded)
    let mut midi_lines = Vec::new();
    let mut last_tick = 0u32;

    for &(pos, note, vel) in &sorted {
        let relative = pos - start;
        // Convert seconds to ticks: at the item's local tempo
        // Use a simple linear mapping since items are short
        let tick = if duration > 0.0 {
            ((relative / duration) * (duration * TICKS_PER_QN as f64 * 2.0)).round() as u32
        } else {
            0
        };

        // Note On
        let delta = tick.saturating_sub(last_tick);
        midi_lines.push(format!("E {} 90 {:02x} {:02x}", delta, note, vel));
        last_tick = tick;

        // Note Off
        midi_lines.push(format!("E {} 80 {:02x} 00", note_length_ticks, note));
        last_tick += note_length_ticks;
    }

    let midi_content = midi_lines.join("\n");

    // Build raw RPP item content
    let raw = format!(
        "POSITION {start}\n\
         SNAPOFFS 0\n\
         LENGTH {duration}\n\
         LOOP 0\n\
         ALLTAKES 0\n\
         FADEIN 0 0 0 0 0 0 0\n\
         FADEOUT 0 0 0 0 0 0 0\n\
         MUTE 0 0\n\
         SEL 0\n\
         <SOURCE MIDI\n\
         HASDATA 1 {TICKS_PER_QN} QN\n\
         {midi_content}\n\
         E {last_tick} b0 7b 00\n\
         >\n"
    );

    Item {
        position: start,
        length: duration,
        raw_content: raw,
        ..Item::default()
    }
}

/// Map a section name to a MIDI note number for the guide cue.
///
/// Uses the same mapping as keyflow's section cue system.
fn section_name_to_midi(name: &str) -> Option<u8> {
    let upper = name.split_whitespace().next().unwrap_or("").to_uppercase();
    // Remove trailing numbers: "VS 1" → "VS", "CH 2" → "CH"
    match upper.as_str() {
        "INTRO" => Some(36),    // C2
        "VS" | "VERSE" => Some(38),   // D2
        "PRE" | "PRECHORUS" | "PRE-CHORUS" | "PRE-CH" => Some(40), // E2
        "CH" | "CHORUS" => Some(41),  // F2
        "BR" | "BRIDGE" => Some(43),  // G2
        "SOLO" => Some(45),    // A2
        "OUTRO" => Some(47),   // B2
        "BREAK" | "BREAKDOWN" => Some(48), // C3
        "INTERLUDE" => Some(50), // D3
        "INSTRUMENTAL" => Some(52), // E3
        "TAG" | "VAMP" => Some(53), // F3
        "HITS" => Some(55),    // G3
        _ => Some(60),         // C4 — generic section
    }
}
