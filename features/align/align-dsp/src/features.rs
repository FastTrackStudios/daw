//! What two takes are matched on.
//!
//! One [`Frame`] per analysis hop, holding everything the matcher has an
//! opinion about. The set is deliberately wider than a single energy
//! envelope, because an envelope is ambiguous in exactly the place
//! alignment is hardest: a kick and a floor tom have the same shape and
//! the same level, and a broadband follower cannot tell a sung "ah" from
//! the "sss" in front of it.
//!
//! ## Why four bands and not a spectrogram
//!
//! Full spectral distance (MFCCs, chroma, a raw magnitude spectrum) is
//! the textbook answer and it is a worse fit here. Two takes of the same
//! part are sung by the same person into the same mic *at different
//! moments*, and the fine spectral detail differs from take to take in
//! ways that have nothing to do with timing — vibrato phase, mic
//! distance, how open the vowel happened to be. Four broad bands keep
//! what is stable (where the energy sits) and discard what is not, and
//! they cost one biquad cascade rather than an FFT per frame.
//!
//! The split points are chosen for voice and translate acceptably to a
//! kit: sub/body, mid, presence, air. A kick lands almost entirely in
//! band 0, a snare across 1 and 2, a hat in 3 — which is the property
//! that makes percussion alignable at all without onset detection.
//!
//! ## Normalization is per take, on purpose
//!
//! Each take is normalized against its own peak rather than against a
//! level shared by the pair. A dub recorded three dB quieter is the same
//! performance, and matching absolute levels would make the quieter take
//! read as "not sounding" wherever the reference is merely quiet. The
//! cost of the choice is that a take whose loudest moment is a cough
//! normalizes badly — which is what [`FeatureConfig::silence_level`] is
//! for.

use audiocore_dsp::biquad::{Biquad, FilterType};

/// One frame's alignable features.
///
/// Everything is normalized to `0.0..=1.0` (except `pitch`) so weights in
/// [`crate::CostConfig`] are comparable to one another and a preset moves
/// between material types by changing weights alone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    /// Broadband level, log scale, normalized against the take's peak.
    pub level: f64,
    /// Per-band levels, same scale as `level`: sub, mid, presence, air.
    pub bands: [f64; 4],
    /// Rise in air-band energy since the previous frame, normalized.
    ///
    /// The cheap stand-in for spectral flux, and the sharpest onset cue
    /// there is: consonants, picks and sticks all announce themselves up
    /// here before the body of the note arrives.
    pub flux: f64,
    /// Rise in broadband energy since the previous frame, normalized.
    /// Slower than `flux` and survives on material with no top end.
    pub delta: f64,
    /// Zero-crossing rate, normalized. Separates noise from tone without
    /// needing a pitch detector.
    pub zcr: f64,
    /// Pitch relative to the take's own median, in semitones, where the
    /// take has a pitch track. `None` for unvoiced frames — and for every
    /// frame when the caller has no pitch detection at all.
    pub pitch: Option<f64>,
    /// Whether this frame carries a pitch. Kept separately from `pitch`
    /// so a caller with voicing but no pitch value can still say so.
    pub voiced: bool,
    /// Below the silence threshold: nothing is sounding.
    pub silent: bool,
}

/// A take's features, and the rate they were taken at.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Features {
    pub frames: Vec<Frame>,
    /// Frames per second.
    pub frame_rate: f64,
}

impl Features {
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// The broadband level envelope, which is what the macro offset
    /// search correlates.
    pub fn envelope(&self) -> Vec<f64> {
        self.frames.iter().map(|f| f.level).collect()
    }
}

/// How features are extracted.
#[derive(Clone, Copy, Debug)]
pub struct FeatureConfig {
    /// Band edges in Hz: sub|mid, mid|presence, presence|air.
    pub crossovers: [f64; 3],
    /// Level below the take's peak, in dB, that reads as silence.
    ///
    /// A level rather than an absolute threshold because the input may
    /// be at any gain, and 60 dB below the loudest moment of a vocal is
    /// reliably "not singing" on anything that has been through a
    /// preamp.
    pub silence_db: f64,
    /// Where the normalized level scale bottoms out, in dB below peak.
    /// Frames quieter than this all read as 0.
    pub floor_db: f64,
}

impl Default for FeatureConfig {
    fn default() -> Self {
        Self {
            // Chest/body, vowel, consonant, air. The top split sits above
            // the sung fundamental range so band 3 is sibilance and stick
            // noise rather than pitch.
            crossovers: [300.0, 2_500.0, 6_000.0],
            silence_db: -60.0,
            floor_db: -60.0,
        }
    }
}

/// Extract features from a mono buffer.
///
/// `hop` and `window` must be the ones the caller's own analysis used, so
/// frame *i* here describes the same audio as frame *i* there. Frames are
/// taken while a whole window fits, which is the convention `tune_dsp`
/// uses and therefore the one that keeps the two frame counts equal.
pub fn extract(
    samples: &[f64],
    sample_rate: f64,
    hop: usize,
    window: usize,
    cfg: FeatureConfig,
) -> Features {
    let hop = hop.max(1);
    let window = window.max(1);
    let frame_rate = sample_rate / hop as f64;
    if samples.len() < window {
        return Features {
            frames: Vec::new(),
            frame_rate,
        };
    }

    let bands = split_bands(samples, sample_rate, cfg.crossovers);

    let count = (samples.len() - window) / hop + 1;
    let mut raw: Vec<RawFrame> = Vec::with_capacity(count);
    for i in 0..count {
        let start = i * hop;
        let end = (start + window).min(samples.len());
        raw.push(RawFrame {
            band_rms: [
                rms(&bands[0][start..end]),
                rms(&bands[1][start..end]),
                rms(&bands[2][start..end]),
                rms(&bands[3][start..end]),
            ],
            zcr: zero_crossing_rate(&samples[start..end]),
        });
    }

    normalize(raw, frame_rate, cfg)
}

/// One frame before anything is normalized. Kept separate because
/// normalization needs the whole take's peak, which is not known until
/// every frame has been measured.
struct RawFrame {
    band_rms: [f64; 4],
    zcr: f64,
}

fn normalize(raw: Vec<RawFrame>, frame_rate: f64, cfg: FeatureConfig) -> Features {
    let broadband: Vec<f64> = raw
        .iter()
        .map(|f| f.band_rms.iter().sum::<f64>() / 4.0)
        .collect();
    let peak = broadband.iter().copied().fold(0.0_f64, f64::max);

    // A take with no signal at all: every frame is silence, and saying so
    // is better than dividing by the floor and calling noise a
    // performance.
    if peak <= 1e-9 {
        return Features {
            frames: raw
                .iter()
                .map(|_| Frame {
                    silent: true,
                    ..Frame::default()
                })
                .collect(),
            frame_rate,
        };
    }

    let to_norm = |v: f64| {
        let db = 20.0 * (v.max(1e-9) / peak).log10();
        ((db - cfg.floor_db) / -cfg.floor_db).clamp(0.0, 1.0)
    };
    let silence = ((cfg.silence_db - cfg.floor_db) / -cfg.floor_db).clamp(0.0, 1.0);

    // Rises are measured on the normalized scale, then scaled by the
    // largest rise in the take: what matters is which onsets are the
    // strong ones *within this take*, not how loud the take is.
    let air: Vec<f64> = raw.iter().map(|f| to_norm(f.band_rms[3])).collect();
    let level: Vec<f64> = broadband.iter().map(|&v| to_norm(v)).collect();
    let rise = |series: &[f64], i: usize| {
        if i == 0 {
            0.0
        } else {
            (series[i] - series[i - 1]).max(0.0)
        }
    };
    let max_flux = (0..raw.len())
        .map(|i| rise(&air, i))
        .fold(0.0_f64, f64::max)
        .max(1e-9);
    let max_delta = (0..raw.len())
        .map(|i| rise(&level, i))
        .fold(0.0_f64, f64::max)
        .max(1e-9);

    let frames = raw
        .iter()
        .enumerate()
        .map(|(i, f)| Frame {
            level: level[i],
            bands: [
                to_norm(f.band_rms[0]),
                to_norm(f.band_rms[1]),
                to_norm(f.band_rms[2]),
                air[i],
            ],
            flux: (rise(&air, i) / max_flux).clamp(0.0, 1.0),
            delta: (rise(&level, i) / max_delta).clamp(0.0, 1.0),
            zcr: f.zcr.clamp(0.0, 1.0),
            pitch: None,
            voiced: false,
            silent: level[i] < silence,
        })
        .collect();

    Features { frames, frame_rate }
}

/// Split into four bands with cascaded second-order Butterworth
/// crossovers.
///
/// Each stage lowpasses off one band and highpasses the remainder into
/// the next, so the bands sum back to something close to the input and no
/// frequency is counted twice. The phase response is not flat, which does
/// not matter: every measurement downstream is an energy over a window.
fn split_bands(samples: &[f64], sample_rate: f64, crossovers: [f64; 3]) -> [Vec<f64>; 4] {
    let nyquist = sample_rate / 2.0;
    // Butterworth Q for a single second-order section.
    const Q: f64 = core::f64::consts::FRAC_1_SQRT_2;

    let mut out = [
        vec![0.0; samples.len()],
        vec![0.0; samples.len()],
        vec![0.0; samples.len()],
        vec![0.0; samples.len()],
    ];
    let mut low = [Biquad::new(), Biquad::new(), Biquad::new()];
    let mut high = [Biquad::new(), Biquad::new(), Biquad::new()];
    for k in 0..3 {
        // A crossover at or above Nyquist is meaningless; park it just
        // below so the filter stays stable and the band above it is
        // simply empty.
        let f = crossovers[k].clamp(1.0, nyquist * 0.99);
        low[k].set(FilterType::Lowpass, f, Q, sample_rate);
        high[k].set(FilterType::Highpass, f, Q, sample_rate);
    }

    for (i, &s) in samples.iter().enumerate() {
        out[0][i] = low[0].tick(s, 0);
        let rest = high[0].tick(s, 0);
        out[1][i] = low[1].tick(rest, 0);
        let rest = high[1].tick(rest, 0);
        out[2][i] = low[2].tick(rest, 0);
        out[3][i] = high[2].tick(rest, 0);
    }
    out
}

fn rms(window: &[f64]) -> f64 {
    if window.is_empty() {
        return 0.0;
    }
    (window.iter().map(|s| s * s).sum::<f64>() / window.len() as f64).sqrt()
}

/// Zero crossings per sample, DC-corrected.
///
/// The mean is removed first because a window sitting on an offset — a
/// low note, or any signal that has not been high-passed — crosses zero
/// far less often than its waveform suggests, and the measurement would
/// read as "more tonal" purely from the offset.
fn zero_crossing_rate(window: &[f64]) -> f64 {
    if window.len() < 2 {
        return 0.0;
    }
    let mean = window.iter().sum::<f64>() / window.len() as f64;
    let mut crossings = 0usize;
    let mut previous = window[0] - mean >= 0.0;
    for &s in &window[1..] {
        let sign = s - mean >= 0.0;
        if sign != previous {
            crossings += 1;
        }
        previous = sign;
    }
    crossings as f64 / (window.len() - 1) as f64
}

impl Features {
    /// Attach a pitch track measured elsewhere.
    ///
    /// Pitch is not extracted here: a caller that wants it already has a
    /// detector (this repo's is `tune_dsp`'s YIN), and one that does not
    /// — a drum track, a batch aligner — should not pay for one. The
    /// values are semitones; they are re-centred on the take's own median
    /// so that a harmony part and the lead it doubles read as the same
    /// *shape* of melody rather than as a constant interval apart.
    ///
    /// `semitones` must be one entry per frame; extra entries are ignored
    /// and missing ones read as unvoiced.
    pub fn with_pitch(mut self, semitones: &[Option<f64>]) -> Self {
        let mut sorted: Vec<f64> = semitones.iter().flatten().copied().collect();
        if sorted.is_empty() {
            return self;
        }
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2];
        for (i, frame) in self.frames.iter_mut().enumerate() {
            match semitones.get(i).copied().flatten() {
                Some(v) => {
                    frame.pitch = Some(v - median);
                    frame.voiced = true;
                }
                None => {
                    frame.pitch = None;
                    frame.voiced = false;
                }
            }
        }
        self
    }
}
