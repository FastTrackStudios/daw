//! Project file import support for REAPER.
//!
//! Registers a `projectimport` handler so REAPER's File > Open dialog
//! can open `.als` (Ableton Live Set) files. The flow is:
//!
//! 1. REAPER calls `want_project_file()` to check if we handle the extension
//! 2. REAPER calls `enum_file_extensions()` to populate the file dialog filter
//! 3. REAPER calls `load_project()` with a `ProjectStateContext` to feed RPP lines into

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::sync::OnceLock;

use dawfile_ableton::devices::BuiltinParams;
use dawfile_ableton::{Device, DeviceFormat};
use dawfile_reaper::RppSerialize;
use dawfile_reaper::builder::{ItemBuilder, ReaperProjectBuilder, TrackBuilder};

// ============================================================================
// Extension registry
// ============================================================================

/// File extensions we handle, with human-readable descriptions for the file dialog.
static EXTENSIONS: &[(&str, &str)] = &[
    ("als", "Ableton Live Set (*.als)"),
    // Future: ("aaf", "AAF Session (*.aaf)"),
    // Future: ("dawproject", "DAWproject (*.dawproject)"),
];

/// Pre-computed CString pairs (extension, description), leaked for stable pointers.
static CACHED_EXTENSIONS: OnceLock<Vec<(CString, CString)>> = OnceLock::new();

fn cached_extensions() -> &'static Vec<(CString, CString)> {
    CACHED_EXTENSIONS.get_or_init(|| {
        EXTENSIONS
            .iter()
            .map(|(ext, desc)| (CString::new(*ext).unwrap(), CString::new(*desc).unwrap()))
            .collect()
    })
}

// ============================================================================
// C callbacks (registered with REAPER via projectimport)
// ============================================================================

/// Called by REAPER to check if we handle this file.
///
/// # Safety
///
/// `fn_` must be a valid null-terminated C string pointer from REAPER.
pub unsafe extern "C" fn want_project_file(fn_: *const c_char) -> bool {
    let path = unsafe { CStr::from_ptr(fn_) }.to_string_lossy();
    let lower = path.to_lowercase();
    EXTENSIONS
        .iter()
        .any(|(ext, _)| lower.ends_with(&format!(".{ext}")))
}

/// Called by REAPER to enumerate supported extensions for the file dialog.
///
/// REAPER calls with increasing `i` until we return null.
/// `descptr` is an output parameter for the description string.
///
/// # Safety
///
/// `descptr` must be a valid writable pointer (or null) from REAPER.
pub unsafe extern "C" fn enum_file_extensions(
    i: c_int,
    descptr: *mut *mut c_char,
) -> *const c_char {
    let cached = cached_extensions();

    let idx = i as usize;
    if idx >= cached.len() {
        return std::ptr::null();
    }

    if !descptr.is_null() {
        unsafe {
            *descptr = cached[idx].1.as_ptr() as *mut c_char;
        }
    }
    cached[idx].0.as_ptr()
}

/// Called by REAPER to load/import the project file.
///
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// - `fn_` must be a valid null-terminated C string pointer.
/// - `genstate` must be a valid `ProjectStateContext` pointer from REAPER.
pub unsafe extern "C" fn load_project(
    fn_: *const c_char,
    genstate: *mut reaper_low::raw::ProjectStateContext,
) -> c_int {
    let path = unsafe { CStr::from_ptr(fn_) }.to_string_lossy();

    match import_file(&path, genstate) {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!("Failed to import {}: {e}", path);
            -1
        }
    }
}

// ============================================================================
// Import dispatcher
// ============================================================================

fn import_file(
    path: &str,
    genstate: *mut reaper_low::raw::ProjectStateContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let lower = path.to_lowercase();

    let rpp_text = if lower.ends_with(".als") {
        import_ableton(path)?
    } else {
        return Err(format!("Unsupported format: {path}").into());
    };

    // Feed RPP text to REAPER line by line
    emit_rpp_to_context(genstate, &rpp_text)?;
    Ok(())
}

// ============================================================================
// RPP → ProjectStateContext emitter
// ============================================================================

fn emit_rpp_to_context(
    genstate: *mut reaper_low::raw::ProjectStateContext,
    rpp_text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for line in rpp_text.lines() {
        let c_line = CString::new(line)?;
        unsafe {
            let ctx = &mut *genstate;
            ctx.AddLine(c_line.as_ptr());
        }
    }
    Ok(())
}

// ============================================================================
// Ableton → REAPER conversion
// ============================================================================

fn import_ableton(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let set = dawfile_ableton::read_live_set(path)?;

    let mut builder = ReaperProjectBuilder::new();
    builder = builder.tempo_with_time_sig(
        set.tempo,
        set.time_signature.numerator as i32,
        set.time_signature.denominator as i32,
    );

    // Tempo automation
    if !set.tempo_automation.is_empty() {
        builder = builder.tempo_envelope(|env| {
            let mut env = env;
            for point in &set.tempo_automation {
                env = env.point(point.time, point.value);
            }
            env
        });
    }

    // Markers (locators)
    for (i, loc) in set.locators.iter().enumerate() {
        builder = builder.marker(i as i32 + 1, loc.time, &loc.name);
    }

    // Audio tracks
    for track in &set.audio_tracks {
        builder = builder.track(&track.common.effective_name, |t| {
            build_audio_track(t, track)
        });
    }

    // MIDI tracks
    for track in &set.midi_tracks {
        builder = builder.track(&track.common.effective_name, |t| build_midi_track(t, track));
    }

    // Return tracks → REAPER tracks
    for track in &set.return_tracks {
        let name = format!("[Return] {}", track.common.effective_name);
        builder = builder.track(&name, |t| apply_common_track_props(t, &track.common));
    }

    // Group tracks → REAPER folder tracks
    for track in &set.group_tracks {
        let name = format!("[Group] {}", track.common.effective_name);
        builder = builder.track(&name, |t| {
            apply_common_track_props(t, &track.common).folder_start()
        });
    }

    let project = builder.build();
    Ok(project.to_rpp_string())
}

// ── Track builders ─────────────────────────────────────────────────────────

fn apply_common_track_props(
    t: TrackBuilder,
    common: &dawfile_ableton::TrackCommon,
) -> TrackBuilder {
    let mut t = t.volume(common.mixer.volume).pan(common.mixer.pan);

    if common.color != 0 {
        t = t.color(ableton_color_to_rgb(common.color));
    }

    if !common.mixer.speaker_on {
        t = t.muted();
    }

    t
}

fn build_audio_track(t: TrackBuilder, track: &dawfile_ableton::AudioTrack) -> TrackBuilder {
    let mut t = apply_common_track_props(t, &track.common);

    // Arrangement clips → items
    for clip in &track.arrangement_clips {
        let position = clip.common.time;
        let length = clip.common.current_end - clip.common.current_start;

        if length <= 0.0 {
            continue;
        }

        t = t.item(position, length, |item| build_audio_item(item, clip));
    }

    // FX / Devices
    for device in &track.common.devices {
        t = add_device_to_track(t, device);
    }

    t
}

fn build_audio_item(item: ItemBuilder, clip: &dawfile_ableton::AudioClip) -> ItemBuilder {
    let mut item = item.name(&clip.common.name);

    // Audio source
    if let Some(ref sr) = clip.sample_ref {
        if let Some(ref path) = sr.path {
            item = item.source_wave(path.to_string_lossy().into_owned());
        }
    }

    // Pitch
    if clip.pitch_coarse != 0.0 {
        item = item.pitch(clip.pitch_coarse);
    }

    // Fades
    if let Some(ref fades) = clip.fades {
        if fades.fade_in_length > 0.0 {
            item = item.fade_in(
                fades.fade_in_length,
                dawfile_reaper::types::item::FadeCurveType::Linear,
            );
        }
        if fades.fade_out_length > 0.0 {
            item = item.fade_out(
                fades.fade_out_length,
                dawfile_reaper::types::item::FadeCurveType::Linear,
            );
        }
    }

    item
}

fn build_midi_track(t: TrackBuilder, track: &dawfile_ableton::MidiTrack) -> TrackBuilder {
    let mut t = apply_common_track_props(t, &track.common);

    // Arrangement MIDI clips
    for clip in &track.arrangement_clips {
        let position = clip.common.time;
        let length = clip.common.current_end - clip.common.current_start;

        if length <= 0.0 {
            continue;
        }

        t = t.item(position, length, |item| build_midi_item(item, clip));
    }

    // FX / Devices
    for device in &track.common.devices {
        t = add_device_to_track(t, device);
    }

    t
}

fn build_midi_item(item: ItemBuilder, clip: &dawfile_ableton::MidiClip) -> ItemBuilder {
    let item = item.name(&clip.common.name);

    // Build MIDI source from key tracks
    item.source_midi().midi(|midi| {
        let mut midi = midi;
        for kt in &clip.key_tracks {
            for note in &kt.notes {
                if !note.is_enabled {
                    continue;
                }
                // Convert beat-relative time to ticks (960 tpqn default)
                let tick_pos = (note.time * 960.0) as u64;
                let tick_dur = (note.duration * 960.0) as u64;
                midi = midi
                    .at(tick_pos)
                    .note(0, 0, kt.midi_key, note.velocity, tick_dur as u32);
            }
        }
        midi
    })
}

// ── Device / FX mapping ────────────────────────────────────────────────────

fn add_device_to_track(mut t: TrackBuilder, device: &Device) -> TrackBuilder {
    if !device.is_on {
        return t; // Skip disabled devices
    }

    match device.format {
        DeviceFormat::Vst2 => {
            t = t.vst(&device.name, "");
        }
        DeviceFormat::Vst3 => {
            t = t.vst3(&device.name, "");
        }
        DeviceFormat::AudioUnit => {
            // REAPER on Linux doesn't support AU, skip
        }
        DeviceFormat::Builtin => {
            if let Some(ref params) = device.builtin_params {
                t = map_builtin_fx(t, params);
            }
        }
        _ => {} // MaxForLive, NoteAlgorithm, Unknown — skip
    }

    t
}

fn map_builtin_fx(mut t: TrackBuilder, params: &BuiltinParams) -> TrackBuilder {
    match params {
        BuiltinParams::Eq8(_) => {
            t = t.js("ReaEQ");
        }
        BuiltinParams::Compressor(_) | BuiltinParams::GlueCompressor(_) => {
            t = t.js("ReaComp");
        }
        BuiltinParams::Gate(_) => {
            t = t.js("ReaGate");
        }
        BuiltinParams::Limiter(_) => {
            t = t.js("ReaLimit");
        }
        BuiltinParams::Reverb(_) => {
            t = t.js("ReaVerbate");
        }
        BuiltinParams::Delay(_) | BuiltinParams::Echo(_) => {
            t = t.js("ReaDelay");
        }
        BuiltinParams::Utility(_) => {
            // Utility is just gain/phase/width — often not needed as separate FX
        }
        _ => {
            tracing::debug!("Unmapped Ableton built-in device, skipping");
        }
    }
    t
}

// ── Color mapping ──────────────────────────────────────────────────────────

/// Map Ableton color index (0-69) to a REAPER-compatible RGB integer.
///
/// Ableton uses a fixed 70-color palette. For now, we use a simplified
/// 7-hue mapping. A full palette can be added later.
fn ableton_color_to_rgb(color_index: i32) -> u32 {
    let (r, g, b): (u32, u32, u32) = match color_index % 7 {
        0 => (0xFF, 0x00, 0x00), // Red
        1 => (0xFF, 0xA5, 0x00), // Orange
        2 => (0xFF, 0xFF, 0x00), // Yellow
        3 => (0x00, 0xFF, 0x00), // Green
        4 => (0x00, 0xBF, 0xFF), // Blue
        5 => (0x80, 0x00, 0xFF), // Purple
        6 => (0xFF, 0x69, 0xB4), // Pink
        _ => (0x80, 0x80, 0x80), // Gray (unreachable)
    };
    (r << 16) | (g << 8) | b
}
