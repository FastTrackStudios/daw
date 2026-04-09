//! Serialize DawProject types to DawProject XML format.
//!
//! Produces `project.xml` and `metadata.xml` content suitable for packing
//! into a `.dawproject` ZIP archive.

pub mod xml;

use crate::types::*;
use xml::XmlWriter;

/// Serialize a `DawProject` to `project.xml` content (UTF-8 string).
pub fn write_project_xml(project: &DawProject) -> String {
    let mut w = XmlWriter::new();

    let version_attr: &[(&str, &str)] = &[("version", &project.version)];
    w.open("Project", version_attr);

    // Application
    if let Some(app) = &project.application {
        w.empty(
            "Application",
            &[("name", &app.name), ("version", &app.version)],
        );
    }

    // Transport
    write_transport(&mut w, &project.transport);

    // Structure (track hierarchy)
    if !project.tracks.is_empty() {
        w.wrap("Structure", &[], |w| {
            for track in &project.tracks {
                write_track(w, track);
            }
        });
    }

    // Arrangement
    if let Some(arr) = &project.arrangement {
        write_arrangement(&mut w, arr);
    }

    // Scenes
    if !project.scenes.is_empty() {
        w.wrap("Scenes", &[], |w| {
            for scene in &project.scenes {
                write_scene(w, scene);
            }
        });
    }

    w.close("Project");
    w.finish()
}

/// Serialize `ProjectMetadata` to `metadata.xml` content (UTF-8 string).
pub fn write_metadata_xml(meta: &ProjectMetadata) -> String {
    let mut w = XmlWriter::new();
    w.open("MetaData", &[]);
    write_opt_text(&mut w, "Title", meta.title.as_deref());
    write_opt_text(&mut w, "Artist", meta.artist.as_deref());
    write_opt_text(&mut w, "Album", meta.album.as_deref());
    write_opt_text(&mut w, "Composer", meta.composer.as_deref());
    write_opt_text(&mut w, "Songwriter", meta.songwriter.as_deref());
    write_opt_text(&mut w, "Producer", meta.producer.as_deref());
    write_opt_text(&mut w, "OriginalArtist", meta.original_artist.as_deref());
    write_opt_text(&mut w, "Arranger", meta.arranger.as_deref());
    write_opt_text(&mut w, "Year", meta.year.as_deref());
    write_opt_text(&mut w, "Genre", meta.genre.as_deref());
    write_opt_text(&mut w, "Copyright", meta.copyright.as_deref());
    write_opt_text(&mut w, "Website", meta.website.as_deref());
    write_opt_text(&mut w, "Comment", meta.comment.as_deref());
    w.close("MetaData");
    w.finish()
}

// ─── Transport ───────────────────────────────────────────────────────────────

fn write_transport(w: &mut XmlWriter, t: &Transport) {
    w.wrap("Transport", &[], |w| {
        let tempo_str = format!("{:.6}", t.tempo);
        w.empty(
            "Tempo",
            &[("value", &tempo_str), ("min", "60"), ("max", "200")],
        );
        let num = t.numerator.to_string();
        let den = t.denominator.to_string();
        w.empty(
            "TimeSignature",
            &[("numerator", &num), ("denominator", &den)],
        );
    });
}

// ─── Tracks ──────────────────────────────────────────────────────────────────

fn write_track(w: &mut XmlWriter, track: &Track) {
    let content_type = content_type_str(&track.content_type);
    let mut attrs: Vec<(&str, &str)> = vec![
        ("contentType", content_type),
        ("id", &track.id),
        ("name", &track.name),
    ];
    let color_storage;
    if let Some(color) = &track.color {
        color_storage = color.clone();
        attrs.push(("color", &color_storage));
    }

    w.open("Track", &attrs);

    if let Some(channel) = &track.channel {
        write_channel(w, channel);
    }

    for child in &track.children {
        write_track(w, child);
    }

    w.close("Track");
}

fn content_type_str(ct: &ContentType) -> &'static str {
    match ct {
        ContentType::Audio => "audio",
        ContentType::Notes => "notes",
        ContentType::Automation => "automation",
        ContentType::Video => "video",
        ContentType::Markers => "markers",
        ContentType::Unknown => "audio",
    }
}

fn write_channel(w: &mut XmlWriter, ch: &Channel) {
    let role = channel_role_str(&ch.role);
    let audio_ch = ch.audio_channels.to_string();
    w.open(
        "Channel",
        &[("role", role), ("audioChannels", &audio_ch), ("id", &ch.id)],
    );

    let vol = format!("{:.6}", ch.volume);
    w.empty("Volume", &[("value", &vol)]);

    let pan = format!("{:.6}", ch.pan);
    w.empty("Pan", &[("value", &pan)]);

    let mute = if ch.muted { "true" } else { "false" };
    w.empty("Mute", &[("value", mute)]);

    if !ch.sends.is_empty() {
        w.wrap("Sends", &[], |w| {
            for send in &ch.sends {
                let vol_str = format!("{:.6}", send.volume);
                let send_type = if send.pre_fader { "pre" } else { "post" };
                w.empty(
                    "Send",
                    &[
                        ("target", &send.target),
                        ("type", send_type),
                        ("volume", &vol_str),
                    ],
                );
            }
        });
    }

    if !ch.devices.is_empty() {
        w.wrap("Devices", &[], |w| {
            for device in &ch.devices {
                write_device(w, device);
            }
        });
    }

    w.close("Channel");
}

fn channel_role_str(role: &ChannelRole) -> &'static str {
    match role {
        ChannelRole::Regular => "regular",
        ChannelRole::Master => "master",
        ChannelRole::Effect => "effect",
    }
}

// ─── Devices ─────────────────────────────────────────────────────────────────

fn write_device(w: &mut XmlWriter, device: &Device) {
    let tag = match device.format {
        DeviceFormat::Vst2 => "Vst2Plugin",
        DeviceFormat::Vst3 => "Vst3Plugin",
        DeviceFormat::Clap => "ClapPlugin",
        DeviceFormat::Au => "AuPlugin",
        DeviceFormat::Builtin | DeviceFormat::Unknown => "BuiltinDevice",
    };

    let bypass = if device.enabled { "false" } else { "true" };
    let mut attrs: Vec<(&str, &str)> = vec![("name", &device.name), ("bypass", bypass)];

    let path_storage;
    if let Some(path) = &device.plugin_path {
        path_storage = path.to_string_lossy().into_owned();
        attrs.push(("deviceFile", &path_storage));
    }

    match &device.state {
        None => w.empty(tag, &attrs),
        Some(state) => {
            w.open(tag, &attrs);
            match state {
                DeviceState::File(file) => {
                    w.empty("State", &[("file", file)]);
                }
                DeviceState::Base64(data) => {
                    w.text_elem("State", &[], data);
                }
            }
            w.close(tag);
        }
    }
}

// ─── Arrangement ─────────────────────────────────────────────────────────────

fn write_arrangement(w: &mut XmlWriter, arr: &Arrangement) {
    let unit = time_unit_str(arr.time_unit);
    w.open("Arrangement", &[("id", &arr.id)]);
    w.open("Lanes", &[("timeUnit", unit)]);

    for lane in &arr.lanes {
        write_lane(w, lane);
    }

    w.close("Lanes");
    w.close("Arrangement");
}

fn write_lane(w: &mut XmlWriter, lane: &Lane) {
    match &lane.content {
        LaneContent::Clips(clips) => {
            let unit_attr = lane.time_unit.map(time_unit_str);
            let mut attrs: Vec<(&str, &str)> = vec![("track", &lane.track), ("id", &lane.id)];
            if let Some(unit) = unit_attr {
                attrs.push(("timeUnit", unit));
            }
            w.open("Clips", &attrs);
            for clip in clips {
                write_clip(w, clip);
            }
            w.close("Clips");
        }
        LaneContent::Notes(notes) => {
            w.open("Notes", &[("track", &lane.track), ("id", &lane.id)]);
            for note in notes {
                write_note(w, note);
            }
            w.close("Notes");
        }
        LaneContent::Automation(auto_lane) => {
            write_automation_lane(w, auto_lane);
        }
        LaneContent::Markers(markers) => {
            w.open("Markers", &[("track", &lane.track), ("id", &lane.id)]);
            for marker in markers {
                let time = format!("{:.6}", marker.time);
                let mut attrs: Vec<(&str, &str)> = vec![("time", &time), ("name", &marker.name)];
                let color_storage;
                if let Some(color) = &marker.color {
                    color_storage = color.clone();
                    attrs.push(("color", &color_storage));
                }
                w.empty("Marker", &attrs);
            }
            w.close("Markers");
        }
    }
}

fn write_automation_lane(w: &mut XmlWriter, lane: &AutomationLane) {
    w.open(
        "AutomationLane",
        &[("target", &lane.target), ("id", &lane.id)],
    );
    w.open("AutomationPoints", &[]);
    for pt in &lane.points {
        let time = format!("{:.6}", pt.time);
        let value = format!("{:.6}", pt.value);
        let interp = match pt.interpolation {
            Interpolation::Linear => "linear",
            Interpolation::Hold => "hold",
        };
        w.empty(
            "RealPoint",
            &[
                ("time", &time),
                ("value", &value),
                ("interpolation", interp),
            ],
        );
    }
    w.close("AutomationPoints");
    w.close("AutomationLane");
}

// ─── Clips ───────────────────────────────────────────────────────────────────

fn write_clip(w: &mut XmlWriter, clip: &Clip) {
    let time = format!("{:.6}", clip.time);
    let dur = format!("{:.6}", clip.duration);
    let mut attrs: Vec<(&str, &str)> = vec![("time", &time), ("duration", &dur), ("id", &clip.id)];

    let time_unit_storage;
    if let Some(unit) = clip.time_unit {
        time_unit_storage = time_unit_str(unit);
        attrs.push(("timeUnit", time_unit_storage));
    }
    let content_unit_storage;
    if let Some(unit) = clip.content_time_unit {
        content_unit_storage = time_unit_str(unit);
        attrs.push(("contentTimeUnit", content_unit_storage));
    }
    let name_storage;
    if let Some(name) = &clip.name {
        name_storage = name.clone();
        attrs.push(("name", &name_storage));
    }
    let color_storage;
    if let Some(color) = &clip.color {
        color_storage = color.clone();
        attrs.push(("color", &color_storage));
    }
    let fade_in_storage;
    if let Some(fi) = clip.fade_in {
        fade_in_storage = format!("{fi:.6}");
        attrs.push(("fadeInTime", &fade_in_storage));
    }
    let fade_out_storage;
    if let Some(fo) = clip.fade_out {
        fade_out_storage = format!("{fo:.6}");
        attrs.push(("fadeOutTime", &fade_out_storage));
    }

    w.open("Clip", &attrs);

    // Loop settings
    if let Some(ls) = &clip.loop_settings {
        let ls_str = format!("{:.6}", ls.loop_start);
        let le_str = format!("{:.6}", ls.loop_end);
        let ps_str = format!("{:.6}", ls.play_start);
        w.empty(
            "Loops",
            &[
                ("loopStart", &ls_str),
                ("loopEnd", &le_str),
                ("playStart", &ps_str),
            ],
        );
    }

    // Content
    match &clip.content {
        ClipContent::Audio(audio) => write_audio(w, audio),
        ClipContent::Notes(notes) => {
            w.open("Notes", &[]);
            for note in notes {
                write_note(w, note);
            }
            w.close("Notes");
        }
        ClipContent::Empty => {}
    }

    w.close("Clip");
}

fn write_audio(w: &mut XmlWriter, audio: &AudioContent) {
    let mut attrs: Vec<(&str, &str)> = Vec::new();
    let path_storage;
    if let Some(path) = &audio.path {
        path_storage = path.clone();
        attrs.push(("file", &path_storage));
    }
    if audio.embedded {
        attrs.push(("embedded", "true"));
    }
    let sr_storage;
    if let Some(sr) = audio.sample_rate {
        sr_storage = sr.to_string();
        attrs.push(("sampleRate", &sr_storage));
    }
    let ch_storage;
    if let Some(ch) = audio.channels {
        ch_storage = ch.to_string();
        attrs.push(("channels", &ch_storage));
    }
    let dur_storage;
    if let Some(d) = audio.duration {
        dur_storage = d.to_string();
        attrs.push(("duration", &dur_storage));
    }
    let algo_storage;
    if let Some(algo) = &audio.algorithm {
        algo_storage = algo.clone();
        attrs.push(("algorithm", &algo_storage));
    }

    if audio.warps.is_empty() {
        w.empty("Audio", &attrs);
    } else {
        w.open("Audio", &attrs);
        w.open("Warps", &[]);
        for warp in &audio.warps {
            let t = format!("{:.6}", warp.time);
            let ct = format!("{:.6}", warp.content_time);
            w.empty("Warp", &[("time", &t), ("contentTime", &ct)]);
        }
        w.close("Warps");
        w.close("Audio");
    }
}

fn write_note(w: &mut XmlWriter, note: &Note) {
    let time = format!("{:.6}", note.time);
    let dur = format!("{:.6}", note.duration);
    let ch = note.channel.to_string();
    let key = note.key.to_string();
    let vel = format!("{:.6}", note.velocity);
    let mut attrs: Vec<(&str, &str)> = vec![
        ("time", &time),
        ("duration", &dur),
        ("channel", &ch),
        ("key", &key),
        ("vel", &vel),
    ];
    let rel_storage;
    if let Some(rel) = note.release_velocity {
        rel_storage = format!("{rel:.6}");
        attrs.push(("rel", &rel_storage));
    }
    w.empty("Note", &attrs);
}

// ─── Scenes ──────────────────────────────────────────────────────────────────

fn write_scene(w: &mut XmlWriter, scene: &Scene) {
    let mut attrs: Vec<(&str, &str)> = vec![("id", &scene.id)];
    let name_storage;
    if let Some(name) = &scene.name {
        name_storage = name.clone();
        attrs.push(("name", &name_storage));
    }
    let color_storage;
    if let Some(color) = &scene.color {
        color_storage = color.clone();
        attrs.push(("color", &color_storage));
    }

    w.open("Scene", &attrs);
    if !scene.slots.is_empty() {
        w.wrap("Slots", &[], |w| {
            for slot in &scene.slots {
                write_clip_slot(w, slot);
            }
        });
    }
    w.close("Scene");
}

fn write_clip_slot(w: &mut XmlWriter, slot: &ClipSlot) {
    let has_stop = if slot.has_stop { "true" } else { "false" };
    w.open("ClipSlot", &[("id", &slot.id), ("hasStop", has_stop)]);
    if let Some(clip) = &slot.clip {
        write_clip(w, clip);
    }
    w.close("ClipSlot");
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn time_unit_str(unit: TimeUnit) -> &'static str {
    match unit {
        TimeUnit::Beats => "beats",
        TimeUnit::Seconds => "seconds",
    }
}

fn write_opt_text(w: &mut XmlWriter, tag: &str, value: Option<&str>) {
    if let Some(text) = value {
        w.text_elem(tag, &[], text);
    }
}
