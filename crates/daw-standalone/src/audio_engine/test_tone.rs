//! Programmatic audio generation for testing.
//!
//! Generates sine waves, noise, and multi-track test patterns without
//! needing external audio files. Useful for testing the audio engine
//! and for UI demos.

use super::DecodedAudio;

/// Generate a mono sine wave at the given frequency and duration.
pub fn sine_wave(frequency: f32, duration_seconds: f32, sample_rate: u32) -> DecodedAudio {
    let num_samples = (duration_seconds * sample_rate as f32) as usize;
    let mut samples = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let sample = (2.0 * std::f32::consts::PI * frequency * t).sin();
        samples.push(sample * 0.5); // -6dB to avoid clipping when mixing
    }

    DecodedAudio {
        samples,
        channels: 1,
        sample_rate,
    }
}

/// Generate a stereo sine wave panned to a position (-1.0 = left, 0.0 = center, 1.0 = right).
pub fn sine_wave_stereo(
    frequency: f32,
    duration_seconds: f32,
    sample_rate: u32,
    pan: f32,
) -> DecodedAudio {
    let num_frames = (duration_seconds * sample_rate as f32) as usize;
    let mut samples = Vec::with_capacity(num_frames * 2);

    // Constant-power panning
    let pan_norm = (pan.clamp(-1.0, 1.0) + 1.0) / 2.0; // 0..1
    let left_gain = (std::f32::consts::FRAC_PI_2 * (1.0 - pan_norm)).cos();
    let right_gain = (std::f32::consts::FRAC_PI_2 * pan_norm).cos();

    for i in 0..num_frames {
        let t = i as f32 / sample_rate as f32;
        let sample = (2.0 * std::f32::consts::PI * frequency * t).sin() * 0.5;
        samples.push(sample * left_gain);
        samples.push(sample * right_gain);
    }

    DecodedAudio {
        samples,
        channels: 2,
        sample_rate,
    }
}

/// Generate a chord (multiple frequencies mixed together).
pub fn chord(frequencies: &[f32], duration_seconds: f32, sample_rate: u32) -> DecodedAudio {
    let num_samples = (duration_seconds * sample_rate as f32) as usize;
    let mut samples = vec![0.0f32; num_samples];
    let gain = 0.4 / frequencies.len().max(1) as f32;

    for &freq in frequencies {
        for i in 0..num_samples {
            let t = i as f32 / sample_rate as f32;
            samples[i] += (2.0 * std::f32::consts::PI * freq * t).sin() * gain;
        }
    }

    DecodedAudio {
        samples,
        channels: 1,
        sample_rate,
    }
}

/// Generate a click track: short clicks at regular intervals.
pub fn click_track(
    bpm: f32,
    duration_seconds: f32,
    sample_rate: u32,
) -> DecodedAudio {
    let num_samples = (duration_seconds * sample_rate as f32) as usize;
    let mut samples = vec![0.0f32; num_samples];
    let click_interval = (60.0 / bpm * sample_rate as f32) as usize;
    let click_length = (0.01 * sample_rate as f32) as usize; // 10ms click

    for i in 0..num_samples {
        if i % click_interval < click_length {
            // Short burst of 1kHz sine
            let t = (i % click_interval) as f32 / sample_rate as f32;
            samples[i] = (2.0 * std::f32::consts::PI * 1000.0 * t).sin() * 0.7;
        }
    }

    DecodedAudio {
        samples,
        channels: 1,
        sample_rate,
    }
}

/// Create a multi-track test set: bass, chords, melody, and click.
///
/// Returns 4 tracks designed to sound musical when played together,
/// useful for testing the mixer and UI.
pub fn demo_tracks(duration_seconds: f32, sample_rate: u32) -> Vec<(&'static str, DecodedAudio)> {
    vec![
        (
            "Click",
            click_track(120.0, duration_seconds, sample_rate),
        ),
        (
            "Bass",
            sine_wave(110.0, duration_seconds, sample_rate), // A2
        ),
        (
            "Chords",
            chord(
                &[261.63, 329.63, 392.0], // C major
                duration_seconds,
                sample_rate,
            ),
        ),
        (
            "Melody",
            sine_wave_stereo(523.25, duration_seconds, sample_rate, 0.3), // C5, panned right
        ),
    ]
}
