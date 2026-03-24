//! Example CLAP plugin with REAPER API access.
//!
//! Demonstrates how a CLAP plugin can call daw-reaper services from
//! within the plugin. The timer callback runs on REAPER's main thread
//! and writes track count to global ExtState so tests can verify it.

use nih_plug::prelude::*;
use std::sync::Arc;

const PLUGIN_NAME: &str = "DAW Example Plugin";

// ── Parameters ──────────────────────────────────────────────────────

#[derive(Params)]
struct ExampleParams {
    #[id = "gain"]
    pub gain: FloatParam,
}

impl Default for ExampleParams {
    fn default() -> Self {
        Self {
            gain: FloatParam::new("Gain", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

// ── Plugin ──────────────────────────────────────────────────────────

struct ExamplePlugin {
    params: Arc<ExampleParams>,
}

impl Default for ExamplePlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(ExampleParams::default()),
        }
    }
}

impl Plugin for ExamplePlugin {
    const NAME: &'static str = PLUGIN_NAME;
    const VENDOR: &'static str = "FastTrackStudio";
    const URL: &'static str = "https://fasttrackstudio.com";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: std::num::NonZeroU32::new(2),
        main_output_channels: std::num::NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        _buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        if let Some(host_ptr) = context.raw_host_context() {
            let result = unsafe { daw::reaper::bootstrap::init_from_clap_host(host_ptr) };
            if let Some(mut bootstrap) = result {
                match bootstrap.session.plugin_register_add_timer(timer_callback) {
                    Ok(_) => {}
                    Err(_) => return true,
                }
                let _ = Box::leak(Box::new(bootstrap.session));
                let _ = Box::leak(Box::new(std::sync::Mutex::new(bootstrap.middleware)));
            }
        }

        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let gain = self.params.gain.value();
        for channel in buffer.iter_samples() {
            for sample in channel {
                *sample *= gain;
            }
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for ExamplePlugin {
    const CLAP_ID: &'static str = "com.fasttrackstudio.example-plugin";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Example CLAP plugin with REAPER API access via daw-reaper");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Utility];
}

nih_export_clap!(ExamplePlugin);

// ── Timer callback ──────────────────────────────────────────────────

/// Called at ~30Hz on REAPER's main thread.
///
/// Proves REAPER API access works by reading track count and writing
/// it to global ExtState where the integration test can read it back.
extern "C" fn timer_callback() {
    use daw::reaper::bootstrap::{HighReaper, LowReaper};
    use std::ffi::CString;

    let reaper = HighReaper::get();
    let low = reaper.medium_reaper().low();
    let project = reaper.current_project();
    let track_count = project.track_count();

    // Write track count to global ExtState for test verification
    let section = CString::new("FTS_EXAMPLE_PLUGIN").unwrap();
    let key = CString::new("track_count").unwrap();
    let val = CString::new(track_count.to_string()).unwrap();
    unsafe {
        low.SetExtState(section.as_ptr(), key.as_ptr(), val.as_ptr(), false);
    }

    // Also write a heartbeat tick so tests can confirm the timer fires
    static TICK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let tick = TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tick_key = CString::new("tick").unwrap();
    let tick_val = CString::new(tick.to_string()).unwrap();
    unsafe {
        low.SetExtState(
            section.as_ptr(),
            tick_key.as_ptr(),
            tick_val.as_ptr(),
            false,
        );
    }
}
