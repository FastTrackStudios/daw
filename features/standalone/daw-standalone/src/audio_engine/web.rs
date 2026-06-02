//! Browser bindings for [`ProjectRenderer`].
//!
//! A wasm-bindgen wrapper that an AudioWorklet processor instantiates
//! once on the worklet thread, then drives via `render(out_l, out_r)`
//! every block. Audio sources are supplied as already-decoded f32 PCM
//! (browsers decode via `AudioContext.decodeAudioData`), so the Rust
//! side never has to do any I/O.
//!
//! Build with `--features web --target wasm32-unknown-unknown`. The
//! companion JS in `examples/web_worklet/processor.js` shows the
//! minimal `AudioWorkletProcessor` glue.
//!
//! Reference layout (a typical app):
//!
//! ```text
//! main thread:                    audio thread (AudioWorklet):
//! ┌─────────────────┐             ┌────────────────────────────┐
//! │ fetch RPP text  │             │ WebRenderer (instance #1)  │
//! │ load_rpp_text() │             │   render(out_l, out_r)     │
//! │ decode audio    │  postMessage│     └─► ProjectRenderer    │
//! │ attach_source() ├────────────►│              └─► snapshot   │
//! │ play() / pause()│             │                  + mix      │
//! └─────────────────┘             └────────────────────────────┘
//! ```
//!
//! In the simplest setup both sides share a `Standalone` via the
//! worklet's `port` postMessage protocol carrying serialized actions
//! (proto trait methods). The Rust side here doesn't prescribe the
//! protocol — it just exposes the renderer.

#![cfg(all(target_arch = "wasm32", feature = "web"))]

use std::sync::Arc;

use wasm_bindgen::prelude::*;

use super::decoder::DecodedAudio;
use super::render::ProjectRenderer;
use crate::audio_engine::materialize::{attach_audio_source, detach_audio_source};
use crate::sync::Standalone;
use crate::transport_engine::{InstantSeconds, PlayStateRepr, SampleClock, TransportShared};

/// Browser-side wrapper. Owns a `Standalone` + selected project guid
/// + sample-rate-aware shared transport. Cheap to construct;
/// expensive operations (decode, parse) live on the calling side.
#[wasm_bindgen]
pub struct WebRenderer {
    daw: Standalone,
    project_guid: String,
    sample_rate: u32,
    shared: Arc<TransportShared>,
}

#[wasm_bindgen]
impl WebRenderer {
    /// Construct a fresh renderer + seed a project. The worklet should
    /// hand its actual `sampleRate` so the sample clock matches the
    /// browser's output.
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: u32) -> WebRenderer {
        let daw = Standalone::new();
        let project_guid = daw.seed_project(daw_proto::ProjectInfo {
            guid: uuid::Uuid::new_v4().to_string(),
            name: "web".into(),
            path: String::new(),
        });
        let bundle = daw.transport_engine_for(&project_guid);
        // The worklet drives advance() — don't double-tick from the
        // soft clock.
        bundle.disable_soft_clock();
        bundle.shared.set_sample_rate(sample_rate);
        let shared = bundle.shared.clone();
        WebRenderer {
            daw,
            project_guid,
            sample_rate,
            shared,
        }
    }

    /// Parse RPP text into the renderer's project. Audio sources are
    /// NOT materialized — call [`attach_audio_source_pcm`] for each
    /// take whose source you've decoded.
    #[cfg(any(feature = "rpp-project", feature = "rpp-project-wasm"))]
    #[wasm_bindgen(js_name = loadRppText)]
    pub fn load_rpp_text(&self, rpp_text: &str) -> Result<JsValue, JsValue> {
        // Replace the project entirely so reloads are clean.
        // (For incremental updates, callers should use the trait
        // surface directly.)
        let summary =
            crate::project_loader::load_rpp_text(&self.daw, "web", "/web/in-memory.rpp", rpp_text)
                .map_err(|e| JsValue::from_str(&e))?;
        let _ = summary; // counts available via separate accessors below
        Ok(JsValue::NULL)
    }

    /// Attach an already-decoded source for `take_guid`. JS callers
    /// typically get the PCM via `AudioContext.decodeAudioData` →
    /// `AudioBuffer.getChannelData(ch)` → `Float32Array`.
    #[wasm_bindgen(js_name = attachAudioSource)]
    pub fn attach_audio_source_pcm(
        &self,
        take_guid: &str,
        interleaved_pcm: &[f32],
        channels: u32,
        sample_rate: u32,
    ) {
        attach_audio_source(
            &self.daw,
            &self.project_guid,
            take_guid,
            DecodedAudio {
                samples: interleaved_pcm.to_vec(),
                channels: channels.max(1) as u16,
                sample_rate: sample_rate.max(1),
            },
        );
    }

    /// Drop a previously-attached source.
    #[wasm_bindgen(js_name = detachAudioSource)]
    pub fn detach_audio_source(&self, take_guid: &str) {
        detach_audio_source(&self.daw, &self.project_guid, take_guid);
    }

    /// Every distinct source path referenced by takes in the loaded
    /// project, sorted. Browsers fetch each one (e.g. via `fetch()`
    /// to a Nextcloud / S3 / static host base URL), decode via
    /// `AudioContext.decodeAudioData`, then call
    /// [`attachAudioSource`](Self::attach_audio_source_pcm) for each
    /// matching take.
    #[wasm_bindgen(js_name = pathsToResolve)]
    pub fn paths_to_resolve(&self) -> Vec<JsValue> {
        self.daw
            .media_bay()
            .paths_to_resolve(daw_proto::ProjectContext::Project(
                self.project_guid.clone(),
            ))
            .into_iter()
            .map(|s| JsValue::from_str(&s))
            .collect()
    }

    /// All `(take_guid, source_path)` pairs in the loaded project,
    /// returned as a flat `[take, path, take, path, …]` JS array.
    /// Browsers use this to map fetched files back to the takes
    /// they belong to.
    #[wasm_bindgen(js_name = takeSources)]
    pub fn take_sources(&self) -> Vec<JsValue> {
        let mut out = Vec::new();
        let _ = self.daw.read_project(&self.project_guid, |p| {
            for tl in p.takes.values() {
                for take in &tl.takes {
                    let Some(path) = take.source_file_path.as_ref() else {
                        continue;
                    };
                    if path.is_empty() {
                        continue;
                    }
                    out.push(JsValue::from_str(&take.guid));
                    out.push(JsValue::from_str(path));
                }
            }
        });
        out
    }

    /// Render `frames` stereo frames into the two output channels.
    /// The worklet calls this with the buffers AudioWorkletProcessor
    /// provides each `process()` invocation.
    pub fn render(&self, out_left: &mut [f32], out_right: &mut [f32]) {
        let frames = out_left.len().min(out_right.len());
        let playing = self.shared.play_state().is_advancing();
        if !playing {
            for s in out_left.iter_mut() {
                *s = 0.0;
            }
            for s in out_right.iter_mut() {
                *s = 0.0;
            }
            return;
        }
        let start = self.shared.playhead_samples().0.max(0) as u64;
        let block = ProjectRenderer::new(&self.daw, &self.project_guid, self.sample_rate)
            .render_block(start, frames);
        for i in 0..frames {
            out_left[i] = block.samples[i * 2];
            out_right[i] = block.samples[i * 2 + 1];
        }
        self.shared.advance(frames as u32);
    }

    // ── Transport ────────────────────────────────────────────────

    pub fn play(&self) {
        self.shared.set_play_state(PlayStateRepr::Playing);
    }
    pub fn pause(&self) {
        self.shared.set_play_state(PlayStateRepr::Paused);
    }
    pub fn stop(&self) {
        self.shared.set_play_state(PlayStateRepr::Stopped);
        self.shared
            .set_playhead(crate::transport_engine::InstantSamples(0));
    }
    #[wasm_bindgen(js_name = isPlaying)]
    pub fn is_playing(&self) -> bool {
        self.shared.play_state().is_advancing()
    }
    #[wasm_bindgen(js_name = positionSeconds)]
    pub fn position_seconds(&self) -> f64 {
        let clock = SampleClock::new(self.shared.sample_rate());
        clock.samples_to_seconds(self.shared.playhead_samples()).0
    }
    #[wasm_bindgen(js_name = seekSeconds)]
    pub fn seek_seconds(&self, seconds: f64) {
        let clock = SampleClock::new(self.shared.sample_rate());
        self.shared
            .set_playhead(clock.seconds_to_samples(InstantSeconds(seconds)));
    }

    // ── Introspection ────────────────────────────────────────────

    #[wasm_bindgen(js_name = projectGuid)]
    pub fn project_guid(&self) -> String {
        self.project_guid.clone()
    }
    #[wasm_bindgen(js_name = trackCount)]
    pub fn track_count(&self) -> u32 {
        use daw_proto::Tracks;
        Tracks::count(
            &self.daw,
            daw_proto::ProjectContext::Project(self.project_guid.clone()),
        )
    }
    #[wasm_bindgen(js_name = audioSourceCount)]
    pub fn audio_source_count(&self) -> u32 {
        self.daw.audio_source_count(&self.project_guid) as u32
    }
}
