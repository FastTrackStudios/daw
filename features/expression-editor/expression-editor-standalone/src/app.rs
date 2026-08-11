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

/// The document waiting for a window.
static STAGED: Mutex<Option<Editor>> = Mutex::new(None);

/// Hand a loaded document to the next [`App`] that mounts.
pub fn stage(editor: Editor) {
    *STAGED.lock().unwrap() = Some(editor);
}

/// Take the staged document, if there is one.
pub fn take_staged() -> Option<Editor> {
    STAGED.lock().unwrap().take()
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
            ExpressionEditor { editor }
        }
    }
}
