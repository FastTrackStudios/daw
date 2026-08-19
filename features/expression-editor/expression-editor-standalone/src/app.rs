//! The root component, and the hand-off that gets a document into it.
//!
//! `open_standalone_with_state` takes a bare `fn() -> Element`, and the
//! REAPER panel mounts a component with no props for the same reason:
//! a root that needed props would be a root only one host could build.
//! So the loaded document is *staged* before the window opens and the
//! component takes it on mount.
//!
//! Staging is a take, not a read: the document moves into the
//! component's signal and the slot is left empty. A component that
//! re-mounted would otherwise silently reset the user's edits back to
//! the loaded state.

use std::sync::Mutex;

use dioxus::prelude::*;
use expression_editor_core::{Editor, ExpressionDoc, TimeBase, Viewport};
use expression_editor_ui::{ExpressionEditor, theme};

use crate::drum_host::SharedDrumHost;

/// The document waiting for a window.
static STAGED: Mutex<Option<Editor>> = Mutex::new(None);

/// The drum host waiting beside it, when the window is a drum
/// workspace. Staged the same way and for the same reason: the root
/// takes no props.
static STAGED_HOST: Mutex<Option<SharedDrumHost>> = Mutex::new(None);

/// Hand a loaded document to the next [`App`] that mounts.
pub fn stage(editor: Editor) {
    *STAGED.lock().unwrap() = Some(editor);
}

/// Stage a drum workspace: the document *and* its write half, so the
/// panel's Apply and the slip drag land on the daw.
// r[impl drums.quantize.apply]
pub fn stage_with_host(editor: Editor, host: SharedDrumHost) {
    *STAGED.lock().unwrap() = Some(editor);
    *STAGED_HOST.lock().unwrap() = Some(host);
}

/// Take the staged document, if there is one.
pub fn take_staged() -> Option<Editor> {
    STAGED.lock().unwrap().take()
}

/// Take the staged drum host, if the staged document came with one.
pub fn take_staged_host() -> Option<SharedDrumHost> {
    STAGED_HOST.lock().unwrap().take()
}

/// An empty document, for the case where nothing was staged.
///
/// Better than panicking: a window that opens empty is diagnosable, and
/// the runner has already printed what it loaded.
fn fallback() -> Editor {
    let doc = ExpressionDoc::new(TimeBase::Ppq { ppq: 960.0 }, 0.0, 960.0 * 8.0);
    Editor::new(doc, Viewport::new(1100.0, 520.0))
}

/// The whole window: the editor, and nothing else.
///
/// No arrangement view, no mixer, no transport. The editor is the
/// product being built here, and surrounding it with chrome that does
/// not exist yet would make this runner a second app to maintain.
#[component]
pub fn App() -> Element {
    let editor = use_signal(|| take_staged().unwrap_or_else(fallback));
    let host = use_signal(take_staged_host);
    // The panel's data channel: bins and previews recomputed by the
    // host on every control change. Empty without a host — the panel
    // is then purely visual, which is what a demo scene wants.
    let mut bins = use_signal(Vec::new);
    let mut previews = use_signal(Vec::new);

    let on_change = host.read().clone().map(|h| {
        EventHandler::new(move |p: expression_editor_ui::QuantizePanel| {
            let (b, pv) = h.preview(&p);
            bins.set(b);
            previews.set(pv);
        })
    });
    // r[impl drums.quantize.apply]
    let on_apply = host.read().clone().map(|h| {
        EventHandler::new(
            move |p: expression_editor_ui::QuantizePanel| match h.apply(&p) {
                Ok(done) => {
                    tracing::info!(pieces = done.pieces, items = done.items, "quantized kit")
                }
                Err(e) => tracing::warn!(error = ?e, "quantize refused"),
            },
        )
    });
    // r[impl drums.manual.slip]
    let on_slip = host.read().clone().map(|h| {
        EventHandler::new(move |(hit, next, delta): (f64, f64, f64)| {
            let next = if next.is_finite() { next } else { h.take_secs };
            let cfg = expression_editor_audio::quantize::SplitConfig {
                leading_pad_secs: 0.005,
                crossfade_secs: 0.005,
            };
            match h.slip(hit, next, delta, cfg) {
                Ok(done) => tracing::info!(pieces = done.pieces, "slipped hit"),
                Err(e) => tracing::warn!(error = ?e, "slip refused"),
            }
        })
    });
    rsx! {
        style {
            // Blitz sizes the root from these; without them the editor
            // lays out at its intrinsic height and the status bar
            // leaves the frame.
            "html, body {{ width: 100%; height: 100%; margin: 0; padding: 0; \
              overflow: hidden; background: {theme::BG}; }}"
        }
        div {
            // `vh`/`vw` rather than `100%`: percentage heights resolve
            // against the parent, and a headless Blitz mount gives
            // `body` no resolved height, so the editor would lay out to
            // its content and leave a band of background under the
            // status bar in every screenshot. The viewport units are
            // the window either way.
            style: "width: 100vw; height: 100vh;",
            ExpressionEditor {
                editor,
                quantize_bins: bins(),
                quantize_previews: previews(),
                on_quantize_change: on_change,
                on_quantize_apply: on_apply,
                on_slip,
            }
        }
    }
}
