//! REAPER `.rpp` interop for the `.session` document (`session-format`).
//!
//! The document crate knows no file format but its own; this module is the
//! REAPER adapter over it: [`from_rpp`] / [`to_rpp`] on a document, and
//! [`RppProject`] for the same on an open `DawProject`.
//!
//! ## The fidelity mechanism, in one paragraph
//!
//! [`crate::rpp_tree`] already reads any `.RPP` into a generic
//! token tree and writes it back; it is *not* rebuilt here. Import keeps the
//! original bytes as a content-addressed object ([`Provenance`]) and reads
//! the modelled entities out of the tree alongside. Export therefore has two
//! honest paths:
//!
//! - the document is **unmodified** → hand back the original bytes, which is
//!   byte-faithfulness in the only sense that means anything;
//! - the document is **modified** → re-parse the original, patch in exactly
//!   the values that changed, and stringify. Constructs the schema never
//!   modelled are still sitting in the tree, so they survive the trip.
//!
//! Patching is deliberately minimal: a field whose modelled value still
//! matches the tree is not rewritten at all. That is what makes an
//! "edited one track's volume" export a one-line diff rather than a
//! reformat of the whole project, and it is what lets the corpus test assert
//! that patching an *un*edited document is a no-op.
//!
//! ## What "the corpus surfaced something new" looks like
//!
//! Every key the importer does not model is counted into an
//! [`ImportReport`]. Unmodelled is not the same as dropped — the verbatim
//! object holds it either way — but the report is what makes a new construct
//! visible instead of quietly ignored, which is the thing #156 asks for.

mod export;
mod import;

pub use export::{ExportReport, to_rpp, to_rpp_patched};
pub use import::{ImportReport, from_rpp};

use crate::rpp_tree::{RChunk, RNode, RNodeTree};

use std::path::Path;

use session_format::document::SourceFormat;
use session_format::error::{DawError, DawResult};
use session_format::objects::ObjectStore;
use session_format::project::DawProject;

/// REAPER text in and out of an open project.
///
/// These were `DawProject`'s own methods while the document crate linked
/// this one; they are an extension trait now that the dependency points
/// the other way. Bring it into scope and the calls read as before:
/// `DawProject::import_rpp(text, name)`, `project.to_rpp()`.
pub trait RppProject: Sized {
    /// Import a REAPER project.
    fn import_rpp(text: &str, name: impl Into<String>) -> DawResult<(Self, ImportReport)>;

    /// Import a REAPER project from disk. The project name comes from the
    /// file stem.
    fn import_rpp_file(path: impl AsRef<Path>) -> DawResult<(Self, ImportReport)>;

    /// Export back to REAPER project text.
    ///
    /// Untouched projects come back **byte-identical**: the original bytes
    /// are in `objects/`, and there is no honest reason to regenerate them.
    /// A touched project is exported by patching that original, so the parts
    /// the schema never modelled survive too.
    fn to_rpp(&self) -> DawResult<String>;

    /// Export, and report every token the patch rewrote.
    fn to_rpp_patched(&self) -> DawResult<(String, ExportReport)>;
}

impl RppProject for DawProject {
    fn import_rpp(text: &str, name: impl Into<String>) -> DawResult<(Self, ImportReport)> {
        let mut objects = ObjectStore::new();
        let (document, report) = from_rpp(text, name, &mut objects)?;
        Ok((Self::new(document, objects), report))
    }

    fn import_rpp_file(path: impl AsRef<Path>) -> DawResult<(Self, ImportReport)> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)?;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        Self::import_rpp(&text, name)
    }

    fn to_rpp(&self) -> DawResult<String> {
        let document = self.document();
        let provenance = document.provenance.as_ref().ok_or_else(|| {
            DawError::Rpp("document has no .rpp provenance to export against".into())
        })?;
        if provenance.format != SourceFormat::Rpp {
            return Err(DawError::Rpp(format!(
                "document was imported from {:?}, not REAPER",
                provenance.format
            )));
        }
        // The verbatim shortcut is only honest while the document still
        // *is* the source. `modified` alone would not do: saving clears
        // it, and a reopened project would then export the bytes it came
        // from rather than the session it now holds.
        if !self.is_modified() && !provenance.edited {
            let bytes = self.objects().get(&provenance.source)?;
            return Ok(String::from_utf8_lossy(bytes).into_owned());
        }
        to_rpp(document, self.objects())
    }

    fn to_rpp_patched(&self) -> DawResult<(String, ExportReport)> {
        to_rpp_patched(self.document(), self.objects())
    }
}

/// Read a node's tokens without the `&mut` dance `RNode::get_tokens` wants.
///
/// The tree's parser always populates `tokens`, so the fallback tokenizes
/// the raw line only for nodes something else built by hand.
pub(crate) fn tokens(node: &RNode) -> Vec<String> {
    match &node.tokens {
        Some(tokens) => tokens.iter().map(|token| token.token.clone()).collect(),
        None => crate::rpp_tree::tokenize(node.line.as_deref().unwrap_or_default())
            .into_iter()
            .map(|token| token.token)
            .collect(),
    }
}

/// The key (first token) of a node line, e.g. `POSITION` or `VOLPAN`.
pub(crate) fn key(node: &RNode) -> String {
    tokens(node).first().cloned().unwrap_or_default()
}

/// A node's parameter at `index`, 1-based past the key.
pub(crate) fn param(node: &RNode, index: usize) -> Option<String> {
    tokens(node).get(index).cloned()
}

/// A node's numeric parameter at `index`.
pub(crate) fn param_f64(node: &RNode, index: usize) -> Option<f64> {
    param(node, index).and_then(|token| token.parse().ok())
}

/// A node's integer parameter at `index`.
pub(crate) fn param_i64(node: &RNode, index: usize) -> Option<i64> {
    param(node, index)
        .and_then(|token| token.parse::<f64>().ok())
        .map(|value| value as i64)
}

/// A node's boolean parameter at `index`, REAPER's `0`/non-`0`.
pub(crate) fn param_bool(node: &RNode, index: usize) -> Option<bool> {
    param_i64(node, index).map(|value| value != 0)
}

/// The first direct child node with this key.
pub(crate) fn child_node<'a>(chunk: &'a RChunk, name: &str) -> Option<&'a RNode> {
    chunk.children.iter().find_map(|child| match child {
        RNodeTree::Node(node) if key(node) == name => Some(node),
        _ => None,
    })
}

/// A chunk's header token at `index` — `<TRACK {GUID}>` puts the GUID at 1.
pub(crate) fn header_param(chunk: &RChunk, index: usize) -> Option<String> {
    param(&chunk.header, index)
}

/// Whether a chunk name is one of REAPER's envelope chunks.
///
/// The list is REAPER's, not ours: track volume/pan/width/mute in both the
/// pre- and post-fader forms, send envelopes, and FX parameter envelopes.
pub(crate) fn is_envelope_chunk(name: &str) -> bool {
    matches!(
        name,
        "VOLENV"
            | "VOLENV2"
            | "VOLENV3"
            | "PANENV"
            | "PANENV2"
            | "WIDTHENV"
            | "WIDTHENV2"
            | "MUTEENV"
            | "AUXVOLENV"
            | "AUXPANENV"
            | "AUXMUTEENV"
            | "PARMENV"
            | "TRIMVOLENV"
            | "SPEEDENV"
            | "PITCHENV"
    )
}

/// Map a REAPER envelope chunk name onto the facade's envelope type.
///
/// Everything the facade has no dedicated variant for lands on
/// [`EnvelopeType::Custom`](daw_proto::automation::EnvelopeType::Custom) with
/// the chunk name kept as the envelope's `name`, so nothing is lost and a
/// later widening of the facade's enum is a pure addition.
pub(crate) fn envelope_type_for(name: &str) -> daw_proto::automation::EnvelopeType {
    use daw_proto::automation::EnvelopeType;
    match name {
        "VOLENV" | "VOLENV2" | "VOLENV3" | "AUXVOLENV" | "TRIMVOLENV" => EnvelopeType::Volume,
        "PANENV" | "PANENV2" | "AUXPANENV" => EnvelopeType::Pan,
        "WIDTHENV" | "WIDTHENV2" => EnvelopeType::Width,
        "MUTEENV" | "AUXMUTEENV" => EnvelopeType::Mute,
        "PARMENV" => EnvelopeType::FxParam,
        "SPEEDENV" => EnvelopeType::PlayRate,
        "PITCHENV" => EnvelopeType::Pitch,
        _ => EnvelopeType::Custom,
    }
}

/// Format a float the way REAPER does for a value it has just changed:
/// shortest representation that round-trips, no trailing `.0`.
///
/// Only used for values the editor actually altered — untouched tokens are
/// never reformatted, so this never churns a file.
pub(crate) fn format_f64(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        let mut rendered = format!("{value}");
        if rendered.contains('e') {
            rendered = format!("{value:.12}");
        }
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_render_without_gratuitous_decimals() {
        assert_eq!(format_f64(16.0), "16");
        assert_eq!(format_f64(18.25), "18.25");
        assert_eq!(format_f64(-1.0), "-1");
    }

    #[test]
    fn every_reaper_envelope_chunk_maps_to_a_type() {
        for name in ["VOLENV2", "PANENV", "MUTEENV", "PARMENV", "SPEEDENV"] {
            assert!(is_envelope_chunk(name), "{name} should be an envelope");
        }
        assert!(!is_envelope_chunk("ITEM"));
        assert!(!is_envelope_chunk("FXCHAIN"));
    }
}
