//! `impl AudioAccessors for Standalone` — decode a take's source file.
//!
//! REAPER's audio accessor is a handle you pull rendered samples from,
//! at whatever rate and channel count you ask for. Standalone has no
//! render graph to pull from, but it does have the take's source file,
//! and decoding it gives the same thing: the audio that take plays.
//!
//! That is enough for the callers this exists for — the expression
//! editor's audio session, and anything else that wants to *analyse* a
//! take rather than hear it. What it deliberately does not do is apply
//! item FX or track processing: neither is modelled here, and a caller
//! that got FX from REAPER and dry audio from standalone would be
//! comparing two different signals without being told.
//!
//! Accessors are handles rather than values because that is the shape
//! REAPER's API has and the facade follows it. Here a handle owns the
//! decoded buffer, so `get_samples` is a copy rather than a decode and
//! reading a take in chunks costs one decode rather than one per chunk.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use daw_proto::{
    AudioAccessors, AudioSampleData, GetSamplesRequest, ItemRef, ProjectContext, TakeRef, TrackRef,
};

use crate::sync::Standalone;

/// One open accessor: decoded audio, ready to serve.
struct OpenAccessor {
    /// Interleaved, at `channels` and `sample_rate`.
    samples: Vec<f32>,
    channels: u16,
    sample_rate: u32,
}

impl OpenAccessor {
    fn frames(&self) -> usize {
        if self.channels == 0 {
            return 0;
        }
        self.samples.len() / self.channels as usize
    }
}

/// Open accessors, keyed by the handle handed out.
///
/// Process-global rather than per-`Standalone`, because `Standalone` is
/// a cloneable handle to shared state and an accessor created through
/// one clone has to be readable through another — the same way a
/// REAPER accessor is valid wherever the API is.
fn table() -> &'static Mutex<HashMap<String, Arc<OpenAccessor>>> {
    static TABLE: OnceLock<Mutex<HashMap<String, Arc<OpenAccessor>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> String {
    static N: AtomicU64 = AtomicU64::new(1);
    format!("sa-acc-{}", N.fetch_add(1, Ordering::Relaxed))
}

/// The source file behind a take, if it has one.
fn take_source(
    daw: &Standalone,
    project: ProjectContext,
    item: ItemRef,
    take: TakeRef,
) -> Option<String> {
    use daw_proto::Takes;
    let takes = daw.get_takes(project, item);
    let index = match take {
        TakeRef::Active => takes.iter().position(|t| t.is_active).unwrap_or(0),
        TakeRef::Index(i) => i as usize,
        TakeRef::Guid(ref g) => takes.iter().position(|t| t.guid == *g)?,
    };
    takes.get(index)?.source_file_path.clone()
}

#[cfg(feature = "decode")]
fn decode(path: &str) -> Option<OpenAccessor> {
    let bytes = std::fs::read(path).ok()?;
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str());
    // The extension is a hint only: symphonia probes the content, and a
    // file named `.wav` that is really a FLAC still decodes.
    let decoded = match extension {
        Some(ext) => crate::audio_engine::decode_audio_with_extension(&bytes, ext),
        None => crate::audio_engine::decode_audio(&bytes),
    }?;
    Some(OpenAccessor {
        samples: decoded.samples,
        channels: decoded.channels,
        sample_rate: decoded.sample_rate,
    })
}

#[cfg(not(feature = "decode"))]
fn decode(_path: &str) -> Option<OpenAccessor> {
    // Without the decoder there is no way to read a source file, and
    // returning silence would be worse than declining: a caller would
    // analyse it and report a take with no notes.
    None
}

impl AudioAccessors for Standalone {
    /// Not supported: a track's audio is its items mixed through its
    /// FX, and standalone models neither the mix nor the FX.
    ///
    /// `None` rather than a silent buffer, so a caller finds out here
    /// instead of concluding the track is empty.
    fn create_track_accessor(&self, _project: ProjectContext, _track: TrackRef) -> Option<String> {
        None
    }

    fn create_take_accessor(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
    ) -> Option<String> {
        let path = take_source(self, project, item, take)?;
        let open = decode(&path)?;
        let id = next_id();
        table().lock().ok()?.insert(id.clone(), Arc::new(open));
        Some(id)
    }

    /// Always false: standalone's sources are files on disk, and this
    /// exists for REAPER's case of an item being re-rendered underneath
    /// an open accessor.
    fn has_state_changed(&self, _accessor_id: &str) -> bool {
        false
    }

    fn get_samples(&self, request: GetSamplesRequest) -> AudioSampleData {
        let Some(acc) = table()
            .lock()
            .ok()
            .and_then(|t| t.get(&request.accessor_id).cloned())
        else {
            return AudioSampleData::default();
        };

        // A zero in either field means "whatever you have", which is
        // how a caller discovers the format before reading in earnest.
        let out_channels = if request.num_channels == 0 {
            acc.channels as u32
        } else {
            request.num_channels
        };
        let rate = if request.sample_rate <= 0.0 {
            acc.sample_rate as f64
        } else {
            request.sample_rate
        };
        let want = request.num_samples as usize;
        if want == 0 || out_channels == 0 {
            return AudioSampleData {
                samples: Vec::new(),
                sample_rate: acc.sample_rate as f64,
                num_channels: acc.channels as u32,
                num_samples: 0,
            };
        }

        // Nearest-neighbour when the rates differ. Deliberately crude:
        // a caller that cares about quality should read at the source
        // rate, which it can because the probe above tells it what that
        // is. Resampling well here would only encourage asking for the
        // wrong rate.
        let ratio = acc.sample_rate as f64 / rate;
        let start_frame = request.start_time * acc.sample_rate as f64;
        let src_channels = acc.channels as usize;
        let frames = acc.frames();

        let mut samples = Vec::with_capacity(want * out_channels as usize);
        for i in 0..want {
            let src = (start_frame + i as f64 * ratio).round();
            let f = if src < 0.0 { usize::MAX } else { src as usize };
            for ch in 0..out_channels as usize {
                // Fewer source channels than asked for: repeat the last
                // one, so a mono file read as stereo is centred rather
                // than half-silent.
                let sc = ch.min(src_channels.saturating_sub(1));
                let v = if f < frames && src_channels > 0 {
                    acc.samples[f * src_channels + sc] as f64
                } else {
                    // Past the end is silence, not an error: an item
                    // longer than its source is ordinary.
                    0.0
                };
                samples.push(v);
            }
        }

        AudioSampleData {
            samples,
            sample_rate: rate,
            num_channels: out_channels,
            num_samples: want as u32,
        }
    }

    fn destroy_accessor(&self, accessor_id: &str) {
        if let Ok(mut t) = table().lock() {
            t.remove(accessor_id);
        }
    }
}
