use super::{mapped, PcmFile};

/// A RIFF chunk: id, size, body, and the pad byte an odd body needs.
fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 9);
    out.extend_from_slice(id);
    out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

/// A `fmt ` body: `tag`, channels, 48 kHz, `bits`; `extensible` adds the
/// WAVE_FORMAT_EXTENSIBLE tail whose sub-format GUID carries `tag`.
fn fmt(tag: u16, channels: u16, bits: u16, extensible: bool) -> Vec<u8> {
    let rate = 48_000u32;
    let align = channels * bits / 8;
    let mut f = Vec::new();
    f.extend_from_slice(&(if extensible { 0xFFFE } else { tag }).to_le_bytes());
    f.extend_from_slice(&channels.to_le_bytes());
    f.extend_from_slice(&rate.to_le_bytes());
    f.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
    f.extend_from_slice(&align.to_le_bytes());
    f.extend_from_slice(&bits.to_le_bytes());
    if extensible {
        f.extend_from_slice(&22u16.to_le_bytes());
        f.extend_from_slice(&bits.to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&tag.to_le_bytes());
        f.extend_from_slice(&[
            0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
        ]);
    }
    f
}

/// A WAV with `chunks` between the RIFF header and nothing else.
fn wav(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(&body);
    out
}

/// A ramp through the whole range, as 24-bit little-endian samples.
fn pcm24(samples: usize) -> Vec<u8> {
    (0..samples)
        .flat_map(|i| {
            let v = (i as i32 * 9_973) % 8_388_607 - 4_194_303;
            let b = v.to_le_bytes();
            [b[0], b[1], b[2]]
        })
        .collect()
}

fn float32(samples: usize) -> Vec<u8> {
    (0..samples)
        .flat_map(|i| ((i as f32 / samples as f32) * 1.8 - 0.9).to_le_bytes())
        .collect()
}

/// Every sample `PcmFile` reads from `bytes`, interleaved, and what the
/// resident decoder makes of the same bytes.
fn mapped_and_decoded(name: &str, bytes: &[u8]) -> (Vec<f32>, Vec<f32>) {
    let dir = std::env::temp_dir().join(format!("fts-sample-mapped-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let pcm = PcmFile::open(&path).unwrap();
    let channels = usize::from(pcm.channels());
    let mapped: Vec<f32> = (0..pcm.frames())
        .flat_map(|f| (0..channels).map(move |c| (f, c)))
        .map(|(f, c)| pcm.sample(f, c))
        .collect();
    drop(pcm);
    let _ = std::fs::remove_file(&path);
    let loaded = crate::decode_bytes(bytes, Some("wav")).unwrap();
    let frames = loaded.num_frames();
    let decoded = (0..frames)
        .flat_map(|f| loaded.channels.iter().map(move |ch| ch[f]))
        .collect();
    (mapped, decoded)
}

fn assert_same(mapped: &[f32], decoded: &[f32]) {
    assert_eq!(mapped.len(), decoded.len());
    let worst = mapped
        .iter()
        .zip(decoded)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 1e-6, "mapped and decoded differ by {worst}");
}

// What a session records (Unbreakable's 307 drum mics): 24-bit integer
// PCM, a `bext` and a `junk` chunk between `fmt ` and `data`.
#[test]
fn a_broadcast_wav_with_chunks_before_its_data_maps_as_it_decodes() {
    let bytes = wav(&[
        chunk(b"fmt ", &fmt(1, 1, 24, false)),
        chunk(b"bext", &[0u8; 602]),
        chunk(b"junk", &[0u8; 61]),
        chunk(b"data", &pcm24(4_801)),
    ]);
    let (mapped, decoded) = mapped_and_decoded("bwf24.wav", &bytes);
    assert_same(&mapped, &decoded);
}

#[test]
fn an_extensible_24_bit_stereo_wav_maps_as_it_decodes() {
    let bytes = wav(&[
        chunk(b"fmt ", &fmt(1, 2, 24, true)),
        chunk(b"data", &pcm24(2 * 3_000)),
    ]);
    let (mapped, decoded) = mapped_and_decoded("ext24.wav", &bytes);
    assert_same(&mapped, &decoded);
}

#[test]
fn a_float_wav_maps_as_it_decodes() {
    let bytes = wav(&[
        chunk(b"fmt ", &fmt(3, 2, 32, false)),
        chunk(b"bext", &[0u8; 10]),
        chunk(b"data", &float32(2 * 2_000)),
    ]);
    let (mapped, decoded) = mapped_and_decoded("f32.wav", &bytes);
    assert_same(&mapped, &decoded);
}

#[test]
fn an_open_mapping_is_counted_until_it_drops() {
    let dir = std::env::temp_dir().join(format!("fts-sample-mapped-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("counted.wav");
    let bytes = wav(&[
        chunk(b"fmt ", &fmt(1, 1, 24, false)),
        chunk(b"data", &pcm24(1_000)),
    ]);
    std::fs::write(&path, &bytes).unwrap();
    let pcm = PcmFile::open(&path).unwrap();
    // Other tests map files concurrently: this one's mapping is at least
    // what is counted while it is open.
    let open = mapped();
    assert!(open.files >= 1 && open.bytes >= bytes.len() as u64);
    assert_eq!(pcm.bytes().len(), bytes.len());
    drop(pcm);
    let _ = std::fs::remove_file(&path);
}
