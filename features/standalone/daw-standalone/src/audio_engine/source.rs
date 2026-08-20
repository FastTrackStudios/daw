//! Audio sources — REAPER's streaming model.
//!
//! Real DAWs never decode whole files into RAM: uncompressed PCM is
//! random-access in the file, so playback reads the needed sample window
//! per block straight from disk (the OS page cache + read-ahead are the
//! buffer), and RAM stays flat regardless of project size. REAPER's
//! "media buffer" is exactly this plus worker-thread read-ahead.
//!
//! [`AudioSource`] is that abstraction:
//! - [`AudioSource::PcmFile`] — a memory-mapped WAV. Opening parses the
//!   header only (instant, no decode, no allocation proportional to the
//!   file); samples convert to f32 on the fly per block.
//! - [`AudioSource::Memory`] — fully decoded PCM for compressed formats
//!   (MP3/FLAC/OGG/AAC), the eager fallback.

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use super::decoder::DecodedAudio;

#[cfg(not(target_arch = "wasm32"))]
/// Sample encodings we read directly from WAV `data` chunks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcmFormat {
    I16,
    I24,
    I32,
    F32,
    F64,
}

#[cfg(not(target_arch = "wasm32"))]
impl PcmFormat {
    fn bytes_per_sample(self) -> usize {
        match self {
            PcmFormat::I16 => 2,
            PcmFormat::I24 => 3,
            PcmFormat::I32 | PcmFormat::F32 => 4,
            PcmFormat::F64 => 8,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// A memory-mapped uncompressed PCM file (WAV). The mapping is backed by
/// the OS page cache — reading pages them in on demand and the kernel may
/// evict them under pressure; resident memory stays proportional to what's
/// actually being played, not the project's media size.
pub struct PcmFile {
    map: memmap2::Mmap,
    /// Byte offset of the `data` chunk's samples.
    data_offset: usize,
    frames: usize,
    channels: u16,
    sample_rate: u32,
    format: PcmFormat,
}

#[cfg(not(target_arch = "wasm32"))]
impl PcmFile {
    /// Open + parse the RIFF/WAVE header. Cheap: no sample data is read.
    // The crate denies unsafe; the mmap call is the one necessary
    // exception — a read-only mapping whose only hazard (concurrent
    // truncation of a media file mid-play) REAPER accepts too.
    #[allow(unsafe_code)]
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("open {path:?}: {e}"))?;
        // SAFETY: the mapping is read-only; concurrent file truncation
        // would fault, which we accept for media files.
        let map = unsafe { memmap2::Mmap::map(&file) }.map_err(|e| format!("mmap: {e}"))?;
        // Hint the kernel we'll read mostly sequentially around the playhead.
        let _ = map.advise(memmap2::Advice::Sequential);

        let b = &map[..];
        if b.len() < 44 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
            return Err("not a RIFF/WAVE file".into());
        }

        // Walk chunks for `fmt ` and `data`.
        let mut pos = 12usize;
        let mut fmt: Option<(u16, u16, u32, u16)> = None; // (audio_format, channels, rate, bits)
        let mut data: Option<(usize, usize)> = None; // (offset, len)
        while pos + 8 <= b.len() {
            let id = &b[pos..pos + 4];
            let size = u32::from_le_bytes(b[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let body = pos + 8;
            match id {
                b"fmt " if body + 16 <= b.len() => {
                    let audio_format = u16::from_le_bytes(b[body..body + 2].try_into().unwrap());
                    let channels = u16::from_le_bytes(b[body + 2..body + 4].try_into().unwrap());
                    let rate = u32::from_le_bytes(b[body + 4..body + 8].try_into().unwrap());
                    let bits = u16::from_le_bytes(b[body + 14..body + 16].try_into().unwrap());
                    // WAVE_FORMAT_EXTENSIBLE: the real format is in the
                    // extension's SubFormat GUID (first two bytes).
                    let resolved = if audio_format == 0xFFFE && body + 26 <= b.len() {
                        u16::from_le_bytes(b[body + 24..body + 26].try_into().unwrap())
                    } else {
                        audio_format
                    };
                    fmt = Some((resolved, channels, rate, bits));
                }
                b"data" => {
                    data = Some((body, size.min(b.len().saturating_sub(body))));
                }
                _ => {}
            }
            // Chunks are word-aligned.
            pos = body + size + (size & 1);
        }

        let (audio_format, channels, sample_rate, bits) = fmt.ok_or("no fmt chunk")?;
        let (data_offset, data_len) = data.ok_or("no data chunk")?;
        let format = match (audio_format, bits) {
            (1, 16) => PcmFormat::I16,
            (1, 24) => PcmFormat::I24,
            (1, 32) => PcmFormat::I32,
            (3, 32) => PcmFormat::F32,
            (3, 64) => PcmFormat::F64,
            other => return Err(format!("unsupported wav format {other:?}")),
        };
        if channels == 0 || sample_rate == 0 {
            return Err("degenerate wav header".into());
        }
        let frame_bytes = format.bytes_per_sample() * channels as usize;
        let frames = data_len / frame_bytes;

        Ok(Self {
            map,
            data_offset,
            frames,
            channels,
            sample_rate,
            format,
        })
    }

    /// One sample (frame, channel) as f32. Bounds-checked; out of range
    /// reads 0.
    #[inline]
    pub(crate) fn sample(&self, frame: usize, channel: usize) -> f32 {
        if frame >= self.frames {
            return 0.0;
        }
        let ch = channel.min(self.channels as usize - 1);
        let bps = self.format.bytes_per_sample();
        let off = self.data_offset + (frame * self.channels as usize + ch) * bps;
        let b = &self.map[..];
        if off + bps > b.len() {
            return 0.0;
        }
        match self.format {
            PcmFormat::I16 => i16::from_le_bytes([b[off], b[off + 1]]) as f32 / 32768.0,
            PcmFormat::I24 => {
                let v =
                    ((b[off] as i32) << 8 | (b[off + 1] as i32) << 16 | (b[off + 2] as i32) << 24)
                        >> 8;
                v as f32 / 8_388_608.0
            }
            PcmFormat::I32 => {
                i32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]) as f32
                    / 2_147_483_648.0
            }
            PcmFormat::F32 => f32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]),
            PcmFormat::F64 => f64::from_le_bytes([
                b[off],
                b[off + 1],
                b[off + 2],
                b[off + 3],
                b[off + 4],
                b[off + 5],
                b[off + 6],
                b[off + 7],
            ]) as f32,
        }
    }

    /// Touch the page range covering `[start_frame, start_frame+frames)`
    /// so the kernel reads it ahead of the playhead (REAPER's media
    /// read-ahead, minus the dedicated thread).
    pub fn prefetch(&self, start_frame: usize, frames: usize) {
        let bps = self.format.bytes_per_sample() * self.channels as usize;
        let start = self.data_offset + start_frame.min(self.frames) * bps;
        let len = (frames * bps).min(self.map.len().saturating_sub(start));
        if len > 0 {
            let _ = self.map.advise_range(memmap2::Advice::WillNeed, start, len);
        }
    }
}

/// A playable audio source: streamed PCM or decoded memory.
pub enum AudioSource {
    /// Fully decoded interleaved f32 (compressed formats).
    Memory(DecodedAudio),
    /// Memory-mapped WAV, converted per block (uncompressed formats).
    #[cfg(not(target_arch = "wasm32"))]
    PcmFile(PcmFile),
}

impl AudioSource {
    pub fn channels(&self) -> u16 {
        match self {
            AudioSource::Memory(d) => d.channels,
            #[cfg(not(target_arch = "wasm32"))]
            AudioSource::PcmFile(p) => p.channels,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        match self {
            AudioSource::Memory(d) => d.sample_rate,
            #[cfg(not(target_arch = "wasm32"))]
            AudioSource::PcmFile(p) => p.sample_rate,
        }
    }

    pub fn frame_count(&self) -> usize {
        match self {
            AudioSource::Memory(d) => d.frame_count(),
            #[cfg(not(target_arch = "wasm32"))]
            AudioSource::PcmFile(p) => p.frames,
        }
    }

    /// Duration in seconds.
    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate() == 0 {
            return 0.0;
        }
        self.frame_count() as f64 / self.sample_rate() as f64
    }

    /// Linearly interpolated stereo read between frames `i0` and `i1`
    /// (mono duplicates; >2ch reads the first pair) — the mixer's inner
    /// sampling primitive.
    #[inline]
    pub fn stereo_interp(&self, i0: usize, i1: usize, frac: f32) -> (f32, f32) {
        match self {
            AudioSource::Memory(d) => {
                let ch = d.channels.max(1) as usize;
                let s = &d.samples;
                let read = |i: usize, c: usize| -> f32 {
                    s.get(i * ch + c.min(ch - 1)).copied().unwrap_or(0.0)
                };
                let l = read(i0, 0) + (read(i1, 0) - read(i0, 0)) * frac;
                let r = read(i0, 1) + (read(i1, 1) - read(i0, 1)) * frac;
                (l, r)
            }
            #[cfg(not(target_arch = "wasm32"))]
            AudioSource::PcmFile(p) => {
                let l0 = p.sample(i0, 0);
                let l1 = p.sample(i1, 0);
                let l = l0 + (l1 - l0) * frac;
                // Mono (most stems): both channels read the same data —
                // skip the second decode entirely.
                if p.channels <= 1 {
                    return (l, l);
                }
                let r0 = p.sample(i0, 1);
                let r1 = p.sample(i1, 1);
                (l, r0 + (r1 - r0) * frac)
            }
        }
    }

    /// Linearly interpolated read of one source channel (clamped to
    /// the last channel) — backs REAPER channel modes (mono-of-N,
    /// stereo pair N).
    #[inline]
    pub fn channel_interp(&self, i0: usize, i1: usize, frac: f32, ch: usize) -> f32 {
        match self {
            AudioSource::Memory(d) => {
                let nch = d.channels.max(1) as usize;
                let c = ch.min(nch - 1);
                let s0 = d.samples.get(i0 * nch + c).copied().unwrap_or(0.0);
                let s1 = d.samples.get(i1 * nch + c).copied().unwrap_or(0.0);
                s0 + (s1 - s0) * frac
            }
            #[cfg(not(target_arch = "wasm32"))]
            AudioSource::PcmFile(p) => {
                let s0 = p.sample(i0, ch);
                let s1 = p.sample(i1, ch);
                s0 + (s1 - s0) * frac
            }
        }
    }

    /// Prefetch a window (no-op for in-memory sources).
    #[allow(unused_variables)]
    pub fn prefetch(&self, start_frame: usize, frames: usize) {
        #[cfg(not(target_arch = "wasm32"))]
        if let AudioSource::PcmFile(p) = self {
            p.prefetch(start_frame, frames);
        }
    }

    /// Min/max of one channel over `[lo, hi)` frames, in one linear
    /// pass over the raw storage.
    ///
    /// This is what makes peak building cheap: `sample()` per frame
    /// costs a bounds check, an offset multiply and a format dispatch
    /// *each*, and a peaks pass touches every frame of every take.
    /// Here the dispatch happens once and the inner loop is a bare
    /// stride walk — an order of magnitude on a whole session.
    pub fn min_max_block(&self, lo: usize, hi: usize, channel: usize) -> (f32, f32) {
        let (mut mn, mut mx) = (f32::MAX, f32::MIN);
        match self {
            AudioSource::Memory(d) => {
                let ch = d.channels.max(1) as usize;
                let c = channel.min(ch - 1);
                let hi = hi.min(d.frame_count());
                if lo >= hi {
                    return (0.0, 0.0);
                }
                let mut i = lo * ch + c;
                let end = hi * ch;
                while i < end {
                    let v = d.samples[i];
                    mn = mn.min(v);
                    mx = mx.max(v);
                    i += ch;
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            AudioSource::PcmFile(p) => {
                let ch = p.channels.max(1) as usize;
                let c = channel.min(ch - 1);
                let hi = hi.min(p.frames);
                if lo >= hi {
                    return (0.0, 0.0);
                }
                let bps = p.format.bytes_per_sample();
                let stride = ch * bps;
                let start = p.data_offset + (lo * ch + c) * bps;
                let end = p.data_offset + (hi * ch + c) * bps;
                let b: &[u8] = &p.map;
                if end > b.len() {
                    // Truncated file: fall back to the checked reader.
                    for i in lo..hi {
                        let v = p.sample(i, c);
                        mn = mn.min(v);
                        mx = mx.max(v);
                    }
                } else {
                    match p.format {
                        PcmFormat::I16 => {
                            let mut off = start;
                            let (mut imn, mut imx) = (i16::MAX, i16::MIN);
                            while off < end {
                                let v = i16::from_le_bytes([b[off], b[off + 1]]);
                                imn = imn.min(v);
                                imx = imx.max(v);
                                off += stride;
                            }
                            mn = imn as f32 / 32768.0;
                            mx = imx as f32 / 32768.0;
                        }
                        PcmFormat::I24 => {
                            let mut off = start;
                            while off < end {
                                let v = ((b[off] as i32) << 8
                                    | (b[off + 1] as i32) << 16
                                    | (b[off + 2] as i32) << 24)
                                    >> 8;
                                let v = v as f32 / 8_388_608.0;
                                mn = mn.min(v);
                                mx = mx.max(v);
                                off += stride;
                            }
                        }
                        _ => {
                            for i in lo..hi {
                                let v = p.sample(i, c);
                                mn = mn.min(v);
                                mx = mx.max(v);
                            }
                        }
                    }
                }
            }
        }
        if mn > mx { (0.0, 0.0) } else { (mn, mx) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Minimal 16-bit mono WAV with a known ramp.
    fn write_wav(path: &Path, samples: &[i16], rate: u32) {
        let data_len = samples.len() * 2;
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(b"RIFF").unwrap();
        f.write_all(&((36 + data_len) as u32).to_le_bytes())
            .unwrap();
        f.write_all(b"WAVE").unwrap();
        f.write_all(b"fmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
        f.write_all(&1u16.to_le_bytes()).unwrap(); // mono
        f.write_all(&rate.to_le_bytes()).unwrap();
        f.write_all(&(rate * 2).to_le_bytes()).unwrap();
        f.write_all(&2u16.to_le_bytes()).unwrap();
        f.write_all(&16u16.to_le_bytes()).unwrap();
        f.write_all(b"data").unwrap();
        f.write_all(&(data_len as u32).to_le_bytes()).unwrap();
        for s in samples {
            f.write_all(&s.to_le_bytes()).unwrap();
        }
    }

    #[test]
    fn pcm_file_reads_back_samples() {
        let dir = std::env::temp_dir().join("fts_pcm_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ramp.wav");
        let samples: Vec<i16> = (0..1000).map(|i| (i * 16) as i16).collect();
        write_wav(&path, &samples, 48000);

        let p = PcmFile::open(&path).unwrap();
        assert_eq!(p.channels, 1);
        assert_eq!(p.sample_rate, 48000);
        assert_eq!(p.frames, 1000);
        assert!((p.sample(10, 0) - (160.0 / 32768.0)).abs() < 1e-6);
        // Mono duplicates to both channels through the source API.
        let src = AudioSource::PcmFile(p);
        let (l, r) = src.stereo_interp(10, 11, 0.0);
        assert_eq!(l, r);
        assert!((l - (160.0 / 32768.0)).abs() < 1e-6);
        // Interpolation midpoint.
        let (l, _) = src.stereo_interp(10, 11, 0.5);
        assert!((l - (168.0 / 32768.0)).abs() < 1e-6);
    }

    /// Real-corpus smoke: open the PT session's WAVs without reading them
    /// into RAM (skips when the drive is absent).
    #[test]
    fn opens_real_session_wavs() {
        let dir = std::path::Path::new(
            "/run/media/cody/15B0-07EB/Transfer May 26th Done/\u{1f535} PNG Project - PT Sessions/02 LORD OF THE FIGHT/Audio Files",
        );
        let Ok(entries) = std::fs::read_dir(dir) else {
            eprintln!("session drive absent — skipping");
            return;
        };
        let mut n = 0;
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "wav") {
                continue;
            }
            // macOS AppleDouble droppings ("._foo.wav") aren't audio.
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("._"))
            {
                continue;
            }
            let p = match PcmFile::open(&path) {
                Ok(p) => p,
                Err(err) => panic!("{path:?}: {err}"),
            };
            assert!(p.frames > 0);
            assert!(p.sample_rate >= 8000);
            // Read a sample from the middle — pages in one page only.
            let _ = p.sample(p.frames / 2, 0);
            n += 1;
            if n >= 25 {
                break;
            }
        }
        assert!(n > 0, "no wavs found");
    }
}
