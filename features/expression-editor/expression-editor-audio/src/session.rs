//! An audio editing session against a DAW take.
//!
//! The audio counterpart of `expression_editor_daw::Session`: that one
//! loads a MIDI take and writes notes back, this one reads an audio
//! item's samples, analyses them, and renders the edits.
//!
//! Depends on the `daw` **facade**, never a backend, so the same
//! session drives a live REAPER item and a standalone project.
//!
//! ## The one structural difference from the MIDI session
//!
//! A MIDI take *is* the document — write it back and you are done. An
//! audio take is a recording that the document *describes*, so the
//! original samples have to be kept for the whole session: every render
//! reads them, and a second edit after a first render must resynthesise
//! from the recording rather than from the last render. Rendering a
//! render is how a vocal turns to glass after three passes.

use daw::service::audio_accessor::{AudioAccessors, GetSamplesRequest};
use daw::service::item::Items;
use daw::service::{ItemRef, ProjectContext, TakeRef};
use expression_editor_core::{Editor, Mode, Viewport};

use crate::analyze::{analyze_take, to_mono, Analysis, TakeConfig};

/// Where a take's audio lives.
///
/// No `PartialEq`: the facade's `ItemRef`/`TakeRef` do not have it, and
/// a location is compared by what it points at rather than by value.
#[derive(Clone, Debug)]
pub struct AudioTakeLocation {
    pub project: ProjectContext,
    pub item: ItemRef,
    pub take: TakeRef,
}

/// A loaded audio take, its analysis, and the editor over it.
pub struct AudioSession {
    pub location: AudioTakeLocation,
    pub editor: Editor,
    /// The recording. Kept for the session's lifetime — see the module
    /// note on why a render must never read a previous render.
    source: Vec<f64>,
    sample_rate: f64,
    /// Analysis paired with `editor.doc`; `commit` reconciles them.
    analysis: Analysis,
    /// The document as loaded, for a dirty check.
    baseline: expression_editor_core::ExpressionDoc,
}

/// How much audio one read pulls, in samples per channel.
///
/// Accessors are read in chunks because a five-minute stereo take at 48
/// kHz is a third of a gigabyte in `f64`, and asking for it in one call
/// makes the host allocate all of it before returning any. 64 k frames
/// is what SneakPeak uses against the same REAPER API, and there is no
/// reason to differ from a number already proven against real takes.
const CHUNK_SAMPLES: u32 = 1 << 16;

impl AudioSession {
    /// Load the first selected item as audio.
    ///
    /// Returns `None` when nothing is selected or the item yields no
    /// samples — an editor opened on nothing looks like a failed load,
    /// and saying so is better than showing an empty roll.
    pub fn load_selected<D: AudioAccessors + Items>(
        daw: &D,
        project: ProjectContext,
        viewport: Viewport,
        cfg: TakeConfig,
    ) -> Option<Self> {
        let item = daw.get_selected_items(project.clone()).into_iter().next()?;
        let location = AudioTakeLocation {
            project,
            item: ItemRef::Guid(item.guid.clone()),
            take: TakeRef::Active,
        };
        Self::load(
            daw,
            location,
            item.length.as_seconds(),
            item.volume,
            viewport,
            cfg,
        )
    }

    /// Load a specific take.
    ///
    /// `volume` is the item's gain, which a take accessor does **not**
    /// apply — it hands back the source audio. Analysing without it
    /// would read a quiet item at the wrong level, which matters for
    /// more than looks: the silence floor separating consonants from
    /// gaps is an absolute threshold, so a fader-down item would have
    /// every frame below it and no sibilants at all.
    pub fn load<D: AudioAccessors>(
        daw: &D,
        location: AudioTakeLocation,
        length_secs: f64,
        volume: f64,
        viewport: Viewport,
        cfg: TakeConfig,
    ) -> Option<Self> {
        let accessor = daw.create_take_accessor(
            location.project.clone(),
            location.item.clone(),
            location.take.clone(),
        )?;
        let read = read_all(daw, &accessor, length_secs);
        daw.destroy_accessor(&accessor);

        let mut source = to_mono(&read.samples, read.channels.max(1) as usize);
        if source.is_empty() {
            return None;
        }
        if volume != 1.0 && volume > 0.0 {
            for s in &mut source {
                *s *= volume;
            }
        }
        let analysis = analyze_take(&source, read.sample_rate, cfg);

        let mut editor = Editor::new(analysis.doc.clone(), viewport);
        // The take is audio, so the surface is the audio one. Setting it
        // here rather than leaving it to the caller means a host cannot
        // load a vocal into the MIDI editor by omission.
        editor.set_mode(Mode::Audio);
        let baseline = editor.doc.clone();

        Some(Self {
            location,
            editor,
            source,
            sample_rate: read.sample_rate,
            analysis,
            baseline,
        })
    }

    /// Whether the document differs from what was analysed.
    pub fn is_dirty(&self) -> bool {
        self.editor.doc != self.baseline
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// The original recording.
    pub fn source(&self) -> &[f64] {
        &self.source
    }

    /// Reconcile the analysis with the edited document.
    pub fn commit(&mut self) {
        self.analysis.doc = self.editor.doc.clone();
        self.analysis.commit();
    }

    /// Render the edited take from the **original** recording.
    ///
    /// Always from `source`, never from a previous render: WORLD is
    /// analysis-resynthesis, and each pass costs a little of the top
    /// end and a little of the transients. Chaining them is how a vocal
    /// ends up sounding like glass after three edits.
    #[cfg(feature = "render")]
    pub fn render(&mut self) -> Vec<f64> {
        self.commit();
        self.analysis.render(&self.source)
    }

    /// Re-analyse the recording, discarding all edits.
    pub fn reanalyze(&mut self, cfg: TakeConfig) {
        self.analysis = analyze_take(&self.source, self.sample_rate, cfg);
        self.editor.doc = self.analysis.doc.clone();
        self.baseline = self.editor.doc.clone();
    }

    /// The analysis, for a host that wants the blobs directly.
    pub fn analysis(&self) -> &Analysis {
        &self.analysis
    }
}

struct Read {
    samples: Vec<f64>,
    sample_rate: f64,
    channels: u32,
}

/// Pull a whole take through an accessor, in chunks.
fn read_all<D: AudioAccessors>(daw: &D, accessor: &str, length_secs: f64) -> Read {
    // One probe read to learn the rate and channel count the host will
    // give us; asking for a rate it does not have would resample, and
    // analysing a resampled take then writing back at the original rate
    // puts every edit slightly out of place.
    let probe = daw.get_samples(GetSamplesRequest {
        accessor_id: accessor.to_string(),
        sample_rate: 0.0,
        num_channels: 0,
        start_time: 0.0,
        num_samples: 1,
    });
    let sample_rate = if probe.sample_rate > 0.0 {
        probe.sample_rate
    } else {
        48_000.0
    };
    let channels = probe.num_channels.max(1);

    let total = (length_secs.max(0.0) * sample_rate).ceil() as u32;
    let mut samples = Vec::with_capacity((total as usize) * channels as usize);
    let mut done = 0u32;
    while done < total {
        let want = CHUNK_SAMPLES.min(total - done);
        let chunk = daw.get_samples(GetSamplesRequest {
            accessor_id: accessor.to_string(),
            sample_rate,
            num_channels: channels,
            start_time: done as f64 / sample_rate,
            num_samples: want,
        });
        if chunk.samples.is_empty() {
            // A host that returns nothing mid-take has hit the end of
            // the source; stopping beats looping forever on an item
            // whose reported length overshoots its audio.
            break;
        }
        samples.extend_from_slice(&chunk.samples);
        done += want;
    }
    Read {
        samples,
        sample_rate,
        channels,
    }
}
