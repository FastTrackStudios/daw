//! Integration test against the real `FileDecrypt.logicx` fixture.

use std::path::Path;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/FileDecrypt.logicx"
);

#[test]
fn fixture_exists() {
    assert!(
        Path::new(FIXTURE).exists(),
        "fixture not found at {FIXTURE}"
    );
}

#[test]
fn parse_bundle_metadata() {
    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");

    assert_eq!(session.sample_rate, 48000);
    assert!((session.bpm - 100.0).abs() < 0.01, "bpm should be 100");
    assert_eq!(session.time_sig_numerator, 4);
    assert_eq!(session.time_sig_denominator, 4);
    assert_eq!(session.key, "C");
    assert_eq!(session.key_gender, "major");
    assert!(
        session.creator_version.contains("Logic Pro"),
        "creator_version should mention Logic Pro, got: {}",
        session.creator_version
    );
}

#[test]
fn chunk_inventory_non_empty() {
    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");

    assert!(!session.chunks.is_empty(), "expected chunks, got none");

    // The Song (gnoS) chunk should always be first.
    let first = &session.chunks[0];
    assert_eq!(
        first.type_name, "Song",
        "first chunk should be Song, got: {}",
        first.type_name
    );

    // Sanity-check total count (we measured 498 in the Python script).
    assert!(
        session.chunks.len() > 400,
        "expected ~498 chunks, got {}",
        session.chunks.len()
    );
}

#[test]
fn chunk_inventory_dump() {
    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");

    use std::collections::HashMap;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for chunk in &session.chunks {
        *counts.entry(chunk.type_name.as_str()).or_default() += 1;
    }

    let mut sorted: Vec<_> = counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));

    println!(
        "\n=== Chunk type inventory ({} total) ===",
        session.chunks.len()
    );
    for (name, count) in &sorted {
        println!("  {:>8}  {}", count, name);
    }

    // Known types we expect to find
    assert!(
        counts.get("AuCO").copied().unwrap_or(0) > 100,
        "expected many AuCO chunks"
    );
    assert!(
        counts.get("Trak").copied().unwrap_or(0) > 10,
        "expected Trak chunks"
    );
    assert!(
        counts.get("Envi").copied().unwrap_or(0) > 5,
        "expected Envi chunks"
    );
    assert_eq!(
        counts.get("Song").copied().unwrap_or(0),
        1,
        "expected exactly 1 Song chunk"
    );
}

#[test]
fn tracks_extracted() {
    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");

    println!("\n=== Tracks ({}) ===", session.tracks.len());
    for track in &session.tracks {
        println!("  {:?}  '{}'", track.kind, track.name);
    }

    // The fixture has named channels: Audio Track 1-3, Midi Track 1-2,
    // Summing Groups, etc.
    assert!(!session.tracks.is_empty(), "expected at least one track");
}

#[test]
fn summing_groups_extracted() {
    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");

    println!(
        "\n=== Summing Groups ({}) ===",
        session.summing_groups.len()
    );
    for g in &session.summing_groups {
        println!("  '{}'  members: {:?}", g.name, g.member_names);
    }
}

#[test]
fn session_summary_smoke() {
    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");
    let summary = dawfile_logic::session_summary(&session);
    println!("\n{}", summary);
    assert!(summary.contains("100"), "summary should mention 100 BPM");
    assert!(summary.contains("48000"));
}

#[test]
fn aufl_audio_filenames() {
    use dawfile_logic::parse::aufl::parse_aufl;
    use dawfile_logic::types::LogicChunk;

    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");

    // Collect filenames from AuFl chunks
    let aufl_tag = *b"lFuA";
    let filenames: Vec<String> = session
        .chunks
        .iter()
        .filter(|c| c.tag == aufl_tag)
        .filter_map(|c| parse_aufl(&c.data))
        .map(|e| e.filename)
        .collect();

    println!("\n=== AuFl filenames ({}) ===", filenames.len());
    for f in &filenames {
        println!("  {}", f);
    }

    // We added audio regions, so there should be at least one .wav file
    assert!(!filenames.is_empty(), "expected at least one AuFl entry");
    assert!(
        filenames
            .iter()
            .any(|f| f.ends_with(".wav") || f.ends_with(".aif") || f.ends_with(".aiff")),
        "expected at least one WAV/AIFF file, got: {:?}",
        filenames
    );
}

#[test]
fn aurg_audio_regions() {
    use dawfile_logic::parse::aurg::parse_aurg;

    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");

    let aurg_tag = *b"gRuA";
    let regions: Vec<_> = session
        .chunks
        .iter()
        .filter(|c| c.tag == aurg_tag)
        .filter_map(|c| parse_aurg(&c.data))
        .collect();

    println!("\n=== AuRg pool entries ({}) ===", regions.len());
    for r in &regions {
        println!(
            "  '{}' src_offset={} frames={}",
            r.name, r.source_offset_frames, r.duration_frames
        );
    }

    assert!(!regions.is_empty(), "expected at least one AuRg region");
    assert!(
        regions.iter().all(|r| !r.name.is_empty()),
        "region name should not be empty"
    );
    assert!(
        regions.iter().all(|r| r.duration_frames > 0),
        "region duration should be > 0"
    );
}

#[test]
fn mseq_arrangement_positions() {
    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");

    // MSeq chunks with arrangement positions in header_meta[4..8].
    // 65536 ticks = 1 beat; 262144 ticks = 1 bar (4/4 at any BPM).
    let mseq_tag = *b"qeSM";
    let positions: Vec<(String, f64)> = session
        .chunks
        .iter()
        .filter(|c| c.tag == mseq_tag)
        .map(|c| {
            let ticks = i32::from_le_bytes(c.header_meta[4..8].try_into().unwrap_or([0; 4]));
            let beats = ticks as f64 / 65536.0;
            (c.type_name.clone(), beats)
        })
        .collect();

    println!("\n=== MSeq arrangement positions ({}) ===", positions.len());
    for (name, beats) in &positions {
        println!("  {} @ {:.2} beats ({:.2} bars)", name, beats, beats / 4.0);
    }

    assert!(
        !positions.is_empty(),
        "expected at least one MSeq arrangement position"
    );

    // At least one position should be non-zero (clips at different bar positions).
    let has_nonzero = positions.iter().any(|(_, b)| *b > 0.0);
    assert!(
        has_nonzero,
        "expected some MSeq clips at non-zero positions"
    );

    // All positions should be non-negative.
    assert!(
        positions.iter().all(|(_, b)| *b >= 0.0),
        "arrangement positions should be non-negative"
    );
}

#[test]
fn aufl_aueg_hex_dump() {
    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");

    // Dump first AuFl chunk bytes (up to 512 bytes)
    for chunk in &session.chunks {
        if chunk.type_name == "AuFl" {
            println!(
                "\n=== First AuFl chunk (offset={}, data_len={}) ===",
                chunk.offset, chunk.data_len
            );
            hex_dump(&chunk.data, 512);
            break;
        }
    }

    // Dump first AuRg chunk bytes
    for chunk in &session.chunks {
        if chunk.type_name == "AuRg" {
            println!(
                "\n=== First AuRg chunk (offset={}, data_len={}) ===",
                chunk.offset, chunk.data_len
            );
            hex_dump(&chunk.data, 256);
            break;
        }
    }

    // Dump first MSeq chunk bytes
    for chunk in &session.chunks {
        if chunk.type_name == "MSeq" {
            println!(
                "\n=== First MSeq chunk (offset={}, data_len={}) ===",
                chunk.offset, chunk.data_len
            );
            hex_dump(&chunk.data, 128);
            break;
        }
    }
}

/// Print hex + ASCII dump of up to `limit` bytes.
fn hex_dump(data: &[u8], limit: usize) {
    let data = &data[..data.len().min(limit)];
    for (i, chunk) in data.chunks(16).enumerate() {
        print!("  {:04x}  ", i * 16);
        for b in chunk {
            print!("{:02x} ", b);
        }
        for _ in chunk.len()..16 {
            print!("   ");
        }
        print!(" |");
        for &b in chunk {
            if b >= 0x20 && b < 0x7f {
                print!("{}", b as char);
            } else {
                print!(".");
            }
        }
        println!("|");
    }
}

#[test]
fn midi_items_extracted() {
    use dawfile_logic::ClipKind;

    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");

    // Collect all MIDI clips from all tracks.
    let mut midi_clips = Vec::new();
    for track in &session.tracks {
        for clip in &track.clips {
            if let ClipKind::Midi { notes } = &clip.kind {
                midi_clips.push((track.name.as_str(), clip.position_beats, notes.len()));
            }
        }
    }

    println!("\n=== MIDI clips from tracks ({}) ===", midi_clips.len());
    for (track, pos, note_count) in &midi_clips {
        println!(
            "  track='{}' position={:.2} beats ({:.2} bars) notes={}",
            track,
            pos,
            pos / 4.0,
            note_count
        );
    }

    // Independently verify MSeq-based arrangement positions.
    let mseq_tag = *b"qeSM";
    let trak_tag = *b"karT";
    let evsq_tag = *b"qSvE";

    let mut midi_positions = Vec::new();
    let chunks = &session.chunks;
    let mut i = 0;
    while i < chunks.len() {
        if chunks[i].tag != mseq_tag {
            i += 1;
            continue;
        }
        let mseq = &chunks[i];
        let ticks = i32::from_le_bytes(mseq.header_meta[4..8].try_into().unwrap_or([0; 4]));
        let beats = ticks as f64 / 65536.0;

        let mut j = i + 1;
        let mut is_midi = false;
        let mut event_count = 0usize;
        while j < chunks.len() {
            if chunks[j].tag == trak_tag {
                if chunks[j].data_len == 0 {
                    is_midi = true;
                }
            } else if chunks[j].tag == evsq_tag {
                // Count non-sentinel, non-end events
                for rec in chunks[j].data.chunks_exact(16) {
                    if rec[0] == 0xf1 {
                        break;
                    }
                    let pos_raw = u32::from_le_bytes(rec[4..8].try_into().unwrap_or([0; 4]));
                    if pos_raw != 0x8800_0000 {
                        event_count += 1;
                    }
                }
                break;
            } else if chunks[j].tag == mseq_tag {
                break;
            }
            j += 1;
        }

        if is_midi && event_count > 0 {
            midi_positions.push(beats);
        }
        i = j;
    }

    println!(
        "\n=== MIDI MSeq regions with events ({}) ===",
        midi_positions.len()
    );
    for pos in &midi_positions {
        println!("  {:.2} beats ({:.2} bars)", pos, pos / 4.0);
    }

    assert!(
        !midi_positions.is_empty(),
        "expected MIDI regions with note events"
    );
    // All positions must be non-negative.
    assert!(
        midi_positions.iter().all(|&p| p >= 0.0),
        "MIDI positions should be non-negative"
    );
}

#[test]
fn layout_deep_dump() {
    let session = dawfile_logic::read_session(FIXTURE).expect("parse failed");

    // All AuRg chunks — compare timeStamp (bytes 14-21) across regions
    println!("\n=== All AuRg chunks ===");
    for chunk in session.chunks.iter().filter(|c| c.type_name == "AuRg") {
        println!("  offset={} len={}", chunk.offset, chunk.data_len);
        hex_dump(&chunk.data, chunk.data.len());
        println!();
    }

    // First 2 Trak chunks
    println!("=== First 2 Trak chunks ===");
    for chunk in session
        .chunks
        .iter()
        .filter(|c| c.type_name == "Trak")
        .take(2)
    {
        println!("  offset={} len={}", chunk.offset, chunk.data_len);
        hex_dump(&chunk.data, 256);
        println!();
    }

    // First 2 EvSq chunks
    println!("=== First 2 EvSq chunks ===");
    for chunk in session
        .chunks
        .iter()
        .filter(|c| c.type_name == "EvSq")
        .take(2)
    {
        println!("  offset={} len={}", chunk.offset, chunk.data_len);
        hex_dump(&chunk.data, 256);
        println!();
    }
}

// ── Fire.logicx fixture ───────────────────────────────────────────────────────

const FIRE_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/Fire.logicx");

#[test]
fn fire_envi_meta_dump() {
    let session = dawfile_logic::read_session(FIRE_FIXTURE).expect("parse failed");
    let chunks = &session.chunks;
    const TAG_ENVI: [u8; 4] = *b"ivnE";
    const TAG_MSEQ: [u8; 4] = *b"qeSM";
    const TAG_TRAK: [u8; 4] = *b"karT";

    println!("\n=== First 12 Envi chunks (Fire) ===");
    let mut n = 0;
    for (ci, c) in chunks.iter().enumerate() {
        if c.tag != TAG_ENVI {
            continue;
        }
        let meta0 = u32::from_le_bytes(c.header_meta[0..4].try_into().unwrap_or([0; 4]));
        let meta4 = u32::from_le_bytes(c.header_meta[4..8].try_into().unwrap_or([0; 4]));
        let meta8 = u32::from_le_bytes(c.header_meta[8..12].try_into().unwrap_or([0; 4]));
        println!(
            "  chunk[{}] data_len={} meta[0]={} meta[4]={} meta[8]={}",
            ci, c.data_len, meta0, meta4, meta8
        );
        hex_dump(&c.data, 48);
        n += 1;
        if n >= 12 {
            break;
        }
    }

    println!("\n=== First 8 non-system MSeq clips (Fire) ===");
    let mut m = 0;
    let mut i = 0;
    while i < chunks.len() && m < 8 {
        if chunks[i].tag != TAG_MSEQ {
            i += 1;
            continue;
        }
        let mseq = &chunks[i];
        let pos = i32::from_le_bytes(mseq.header_meta[4..8].try_into().unwrap_or([0; 4])) as f64
            / 65536.0;
        let chan_raw = if mseq.data.len() >= 12 {
            u32::from_le_bytes(mseq.data[8..12].try_into().unwrap_or([0; 4]))
        } else {
            0
        };
        let name = if mseq.data.len() >= 18 {
            let nl = u16::from_le_bytes([mseq.data[16], mseq.data[17]]) as usize;
            if mseq.data.len() >= 18 + nl {
                String::from_utf8_lossy(&mseq.data[18..18 + nl]).into_owned()
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        let mut j = i + 1;
        while j < chunks.len() && chunks[j].tag != TAG_TRAK && chunks[j].tag != TAG_MSEQ {
            j += 1;
        }
        let trak_dlen = if j < chunks.len() && chunks[j].tag == TAG_TRAK {
            chunks[j].data_len
        } else {
            999
        };
        println!(
            "  [{}] @{:.1}b  name='{}' chan_raw={} chan_seq={} trak_dlen={}",
            i,
            pos,
            name,
            chan_raw,
            chan_raw >> 2,
            trak_dlen
        );
        i += 1;
        m += 1;
    }
}

#[test]
fn fire_envi_all_names() {
    let session = dawfile_logic::read_session(FIRE_FIXTURE).expect("parse failed");
    let chunks = &session.chunks;
    const TAG_ENVI: [u8; 4] = *b"ivnE";

    println!("\n=== All Envi chunks (seq, data_len, first_bytes, name) ===");
    for (ci, c) in chunks.iter().enumerate() {
        if c.tag != TAG_ENVI {
            continue;
        }
        let meta4 = u32::from_le_bytes(c.header_meta[4..8].try_into().unwrap_or([0; 4]));
        let seq = meta4 / 262_144;
        // Show first 20 bytes of data
        let preview: Vec<String> = c
            .data
            .iter()
            .take(20)
            .map(|b| format!("{:02x}", b))
            .collect();
        // Attempt pascal string extraction
        let pascal = if !c.data.is_empty() {
            let len = c.data[0] as usize;
            if len > 0 && len < 128 && c.data.len() > len {
                let s = &c.data[1..=len];
                if s.iter().all(|&b| b >= 0x20 && b < 0x7f) {
                    String::from_utf8_lossy(s).into_owned()
                } else {
                    format!("[not-ascii len={}]", len)
                }
            } else {
                format!("[len={} invalid]", len)
            }
        } else {
            "[empty]".into()
        };

        // Find printable ASCII runs >= 3 chars
        let mut runs: Vec<String> = Vec::new();
        let mut i = 0;
        while i < c.data.len() {
            if c.data[i] >= 0x20 && c.data[i] < 0x7f {
                let start = i;
                while i < c.data.len() && c.data[i] >= 0x20 && c.data[i] < 0x7f {
                    i += 1;
                }
                let run = &c.data[start..i];
                if run.len() >= 3 {
                    runs.push(String::from_utf8_lossy(run).into_owned());
                }
            } else {
                i += 1;
            }
        }
        println!(
            "  [{}] seq={} dlen={} pascal='{}' runs={:?}",
            ci, seq, c.data_len, pascal, runs
        );
    }
}

#[test]
fn fire_audio_mseq_names() {
    // Find all audio MSeq clips (followed by Trak with data_len=58) and show their names
    let session = dawfile_logic::read_session(FIRE_FIXTURE).expect("parse failed");
    let chunks = &session.chunks;
    const TAG_MSEQ: [u8; 4] = *b"qeSM";
    const TAG_TRAK: [u8; 4] = *b"karT";

    let mut i = 0;
    let mut count = 0;
    while i < chunks.len() {
        if chunks[i].tag != TAG_MSEQ {
            i += 1;
            continue;
        }
        let mseq = &chunks[i];
        let name_len = if mseq.data.len() >= 18 {
            u16::from_le_bytes([mseq.data[16], mseq.data[17]]) as usize
        } else {
            0
        };
        let name = if mseq.data.len() >= 18 + name_len && name_len > 0 {
            String::from_utf8_lossy(&mseq.data[18..18 + name_len]).into_owned()
        } else {
            String::new()
        };
        // Find next Trak
        let mut j = i + 1;
        while j < chunks.len() && chunks[j].tag != TAG_TRAK && chunks[j].tag != TAG_MSEQ {
            j += 1;
        }
        if j < chunks.len() && chunks[j].tag == TAG_TRAK && chunks[j].data_len == 58 {
            let pos = i32::from_le_bytes(mseq.header_meta[4..8].try_into().unwrap_or([0; 4]))
                as f64
                / 65536.0;
            println!("  AUDIO clip: chunk={} pos={:.1}b name='{}'", i, pos, name);
            count += 1;
        }
        i += 1;
    }
    println!("Total audio clips (trak_dlen=58): {}", count);
}

#[test]
fn fire_envi_name_hex() {
    // Dump full hex of specific Envi chunks to locate name bytes exactly
    use dawfile_logic::read_session;
    let session = read_session(FIRE_FIXTURE).expect("parse failed");
    let chunks = &session.chunks;
    const TAG_ENVI: [u8; 4] = *b"ivnE";

    // chunk[4307] = seq=35 "Kick}" and chunk[4346] = seq=56 "Snare"
    for target_chunk in [4307usize, 4346, 4439] {
        let c = &chunks[target_chunk];
        let meta4 = u32::from_le_bytes(c.header_meta[4..8].try_into().unwrap_or([0; 4]));
        println!(
            "\n=== chunk[{}] seq={} dlen={} ===",
            target_chunk,
            meta4 / 262144,
            c.data_len
        );
        for (i, row) in c.data.chunks(16).enumerate() {
            let hex: Vec<String> = row.iter().map(|b| format!("{:02x}", b)).collect();
            let ascii: String = row
                .iter()
                .map(|&b| {
                    if b >= 0x20 && b < 0x7f {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            println!("  {:04x}  {}  |{}|", i * 16, hex.join(" "), ascii);
        }
    }
}

#[test]
fn fire_envi_seq_range() {
    use dawfile_logic::LogicChunk;
    let session = dawfile_logic::read_session(FIRE_FIXTURE).expect("parse failed");
    let chunks = &session.chunks;
    const TAG_ENVI: [u8; 4] = *b"ivnE";
    const TAG_MSEQ: [u8; 4] = *b"qeSM";

    let mut envi_seqs: Vec<u32> = chunks
        .iter()
        .filter(|c| c.tag == TAG_ENVI)
        .map(|c| {
            let raw = u32::from_le_bytes(c.header_meta[4..8].try_into().unwrap_or([0; 4]));
            raw / 262_144
        })
        .collect();
    envi_seqs.sort();

    let mseq_seqs: Vec<u32> = chunks
        .iter()
        .filter(|c| c.tag == TAG_MSEQ && c.data.len() >= 12)
        .map(|c| {
            let raw = u32::from_le_bytes(c.data[8..12].try_into().unwrap_or([0; 4]));
            raw >> 2
        })
        .collect();

    println!("\nEnvi count: {}", envi_seqs.len());
    println!(
        "Envi seq min={} max={}",
        envi_seqs.first().unwrap_or(&0),
        envi_seqs.last().unwrap_or(&0)
    );
    println!(
        "Envi seqs (first 30): {:?}",
        &envi_seqs[..envi_seqs.len().min(30)]
    );
    println!(
        "Envi seqs (last 30): {:?}",
        &envi_seqs[envi_seqs.len().saturating_sub(30)..]
    );

    println!("\nMSeq count: {}", mseq_seqs.len());
    let mut mseq_unique: Vec<u32> = mseq_seqs.clone();
    mseq_unique.sort();
    mseq_unique.dedup();
    println!(
        "MSeq unique chan_seqs (first 30): {:?}",
        &mseq_unique[..mseq_unique.len().min(30)]
    );

    // How many MSeq chan_seqs have a matching Envi?
    let envi_set: std::collections::HashSet<u32> = envi_seqs.iter().copied().collect();
    let matched = mseq_seqs.iter().filter(|s| envi_set.contains(s)).count();
    let unmatched = mseq_seqs.len() - matched;
    println!(
        "\nMSeq clips matched by chan_seq: {} / {} (unmatched: {})",
        matched,
        mseq_seqs.len(),
        unmatched
    );
}

#[test]
fn fire_fixture_exists() {
    assert!(
        std::path::Path::new(FIRE_FIXTURE).exists(),
        "Fire fixture not found at {FIRE_FIXTURE}"
    );
}

#[test]
fn fire_parse_summary() {
    let session = dawfile_logic::read_session(FIRE_FIXTURE).expect("parse failed");

    println!(
        "\nBPM: {:.2}  SR: {}  Key: {} {}",
        session.bpm, session.sample_rate, session.key, session.key_gender
    );
    println!(
        "Chunks: {}  Tracks: {}  Summing groups: {}",
        session.chunks.len(),
        session.tracks.len(),
        session.summing_groups.len()
    );

    println!("\n=== Tracks ===");
    let mut total_clips = 0usize;
    for t in &session.tracks {
        println!(
            "  [chan_seq={}] {:?} '{}' clips={}",
            t.channel,
            t.kind,
            t.name,
            t.clips.len()
        );
        for clip in &t.clips {
            let kind = match &clip.kind {
                dawfile_logic::ClipKind::Audio { file_path } => {
                    format!("AUDIO file={}", file_path.as_deref().unwrap_or("?"))
                }
                dawfile_logic::ClipKind::Midi { notes } => {
                    format!("MIDI notes={}", notes.len())
                }
                dawfile_logic::ClipKind::Other => "OTHER".into(),
            };
            println!(
                "    @ {:.2}b len={:.2}b  {}",
                clip.position_beats, clip.length_beats, kind
            );
            total_clips += 1;
        }
    }
    println!("\nTotal clips attached: {}", total_clips);

    // Sanity assertions
    assert!(session.chunks.len() > 100, "expected many chunks");
    assert!(!session.tracks.is_empty(), "expected tracks");
    assert!(session.bpm > 0.0, "expected non-zero BPM");
}
