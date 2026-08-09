//! The expression editor as a REAPER module.
//!
//! A dockable panel plus the actions that put a take into it and write
//! one back. The panel holds a [`Session`], which is what keeps "open
//! the editor on the selected item" and "save it" talking about the
//! same take.
//!
//! Everything the panel does goes through the `daw` facade, so the
//! logic is identical to the standalone path and is tested there. What
//! *this* crate adds — and what its REAPER test covers — is the part
//! that only exists inside REAPER: the panel registering, docking,
//! opening on a selection, and its edits reaching a real take.

use std::sync::{Mutex, OnceLock};

use daw::module::{
    ActionDef, DawModule, DockPosition, ModuleContext, PanelComponent, PanelDef, PanelRenderer,
};
use daw::service::ProjectContext;
use dioxus::prelude::*;
use expression_editor_audio::{AudioSession, TakeConfig};
use expression_editor_core::Viewport;
use expression_editor_daw::Session;
use expression_editor_ui::ExpressionEditor;

pub const PANEL_ID: &str = "FTS_EXPRESSION_EDITOR";

/// Default MPE bend range. Must match the receiving instrument or every
/// pitch curve reads wrong by a factor; 48 is the MPE convention.
const DEFAULT_BEND_RANGE: f64 = 48.0;

/// What the panel currently holds.
///
/// One panel, two kinds of take. Which one is loaded is decided by the
/// item — a MIDI take gets the MPE editor, an audio take gets the
/// Melodyne surface — rather than by a mode the user has to remember to
/// set, because loading a vocal into the MIDI editor produces an empty
/// roll and no explanation.
pub enum Loaded {
    Midi(Box<Session>),
    Audio(Box<AudioSession>),
}

impl Loaded {
    fn editor(&self) -> &expression_editor_core::Editor {
        match self {
            Loaded::Midi(s) => &s.editor,
            Loaded::Audio(s) => &s.editor,
        }
    }

    fn editor_mut(&mut self) -> &mut expression_editor_core::Editor {
        match self {
            Loaded::Midi(s) => &mut s.editor,
            Loaded::Audio(s) => &mut s.editor,
        }
    }

    fn is_dirty(&self) -> bool {
        match self {
            Loaded::Midi(s) => s.is_dirty(),
            Loaded::Audio(s) => s.is_dirty(),
        }
    }
}

/// The panel's session, shared between the actions and the component.
///
/// A global rather than a signal because the actions that load and
/// write are REAPER actions — they run outside any component, and
/// cannot reach a hook's state.
static SESSION: OnceLock<Mutex<Option<Loaded>>> = OnceLock::new();

fn session() -> &'static Mutex<Option<Loaded>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

/// Load the selected item into the panel.
///
/// Audio is tried first, then MIDI. The order matters only because a
/// take is one or the other, and asking the audio path first means an
/// audio item never falls through to a MIDI reader that would find no
/// notes and report an empty take.
///
/// Returns false when nothing is selected or the item is neither, which
/// the caller should report — an editor opened on nothing looks like a
/// failed load.
pub fn load_selected() -> bool {
    let reaper = daw::reaper::Reaper;
    let viewport = Viewport::new(1100.0, 520.0);

    if let Some(s) = AudioSession::load_selected(
        &reaper,
        ProjectContext::Current,
        viewport,
        TakeConfig::default(),
    ) {
        tracing::info!(
            notes = s.editor.doc.notes.len(),
            rate = s.sample_rate(),
            "analysed audio take into editor"
        );
        *session().lock().unwrap() = Some(Loaded::Audio(Box::new(s)));
        return true;
    }

    match Session::load_selected(
        &reaper,
        ProjectContext::Current,
        DEFAULT_BEND_RANGE,
        viewport,
    ) {
        Some(s) => {
            tracing::info!(notes = s.editor.doc.notes.len(), "loaded MIDI take into editor");
            *session().lock().unwrap() = Some(Loaded::Midi(Box::new(s)));
            true
        }
        None => {
            tracing::warn!("no editable item selected");
            false
        }
    }
}

/// Write the panel's document back to the take it came from.
///
/// MIDI is rewritten in place — the take *is* the document. Audio is
/// rendered to a new file beside the original and the take repointed at
/// it, because a recording is the only copy of a performance and the
/// resynthesis is lossy. See `AudioSession::write_back`.
pub fn write_back() -> bool {
    let reaper = daw::reaper::Reaper;
    let mut guard = session().lock().unwrap();
    let Some(loaded) = guard.as_mut() else {
        tracing::warn!("nothing loaded");
        return false;
    };
    match loaded {
        Loaded::Midi(s) => {
            // Warn before overwriting, not after: expression on
            // ambiguous notes is dropped on the way out, and the user
            // should hear that while they can still fix it.
            for w in s.warnings() {
                tracing::warn!("{w}");
            }
            let indices = s.write_back(&reaper);
            tracing::info!(notes = indices.len(), "wrote MIDI take");
            true
        }
        Loaded::Audio(s) => match s.write_back(&reaper) {
            Ok(path) => {
                tracing::info!(%path, "rendered audio take");
                true
            }
            Err(e) => {
                tracing::error!("audio write-back failed: {e}");
                false
            }
        },
    }
}

/// Discard local edits and re-read the take.
pub fn reload() -> bool {
    let reaper = daw::reaper::Reaper;
    let mut guard = session().lock().unwrap();
    match guard.as_mut() {
        Some(Loaded::Midi(s)) => {
            s.reload(&reaper);
            true
        }
        // Audio re-reads its own recording rather than the host: the
        // samples have not changed, only the analysis is being redone,
        // and pulling them again would cost a decode for nothing.
        Some(Loaded::Audio(s)) => {
            s.reanalyze(TakeConfig::default());
            true
        }
        None => false,
    }
}

/// Whether the panel has unsaved edits.
pub fn is_dirty() -> bool {
    session()
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|s| s.is_dirty())
}

/// Note count currently loaded — cheap state a test can assert on
/// without reaching into the panel's rendering.
pub fn loaded_note_count() -> usize {
    session()
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| s.editor().doc.notes.len())
        .unwrap_or(0)
}

pub struct ExpressionEditorModule;

impl DawModule for ExpressionEditorModule {
    fn name(&self) -> &str {
        "expression-editor"
    }

    fn display_name(&self) -> &str {
        "FTS Expression Editor"
    }

    fn actions(&self) -> Vec<ActionDef> {
        vec![
            ActionDef::new(
                "FTS_EXPRESSION_EDITOR_TOGGLE",
                "FTS: Toggle Expression Editor",
                || {
                    daw::reaper_ui::dock::toggle_panel(PANEL_ID);
                },
            )
            .in_menu(),
            ActionDef::new(
                "FTS_EXPRESSION_EDITOR_OPEN",
                "FTS: Open Expression Editor on selected item",
                || {
                    if load_selected() {
                        daw::reaper_ui::dock::show_panel(PANEL_ID);
                    }
                },
            )
            .in_menu(),
            ActionDef::new(
                "FTS_EXPRESSION_EDITOR_WRITE",
                "FTS: Write Expression Editor back to take",
                || {
                    write_back();
                },
            ),
            ActionDef::new(
                "FTS_EXPRESSION_EDITOR_RELOAD",
                "FTS: Reload Expression Editor from take",
                || {
                    reload();
                },
            ),
            // An edit the integration test can trigger from outside the
            // process. The test binary talks to REAPER over a socket and
            // cannot see this extension's memory, so "did the editor
            // load and edit correctly" is only answerable by making a
            // change that shows up in the take.
            ActionDef::new(
                "FTS_EXPRESSION_EDITOR_TEST_TRANSPOSE",
                "FTS: Expression Editor — transpose loaded notes +12 (test)",
                || {
                    with_editor(|ed| {
                        let ids: Vec<_> = ed.doc.notes.iter().map(|n| n.id).collect();
                        ed.apply(&expression_editor_core::Edit::Transpose {
                            notes: ids,
                            semitones: 12,
                        });
                    });
                },
            ),
        ]
    }

    fn panels(&self) -> Vec<PanelDef> {
        vec![PanelDef {
            id: PANEL_ID,
            title: "FTS Expression Editor",
            component: PanelComponent::from_fn_ptr(EditorPanel as fn() -> _ as *const ()),
            default_dock: DockPosition::Floating,
            renderer: PanelRenderer::Native,
            default_size: (1180.0, 640.0),
            toggle_action: Some("FTS_EXPRESSION_EDITOR_TOGGLE"),
        }]
    }

    fn init(&self, _ctx: &ModuleContext) {
        tracing::info!("FTS Expression Editor module initialized");
    }
}

#[component]
pub fn EditorPanel() -> Element {
    // The panel mirrors the global session into a signal each frame.
    // The actions mutate the global from outside any component, so the
    // component cannot be the owner — but it does need to re-render
    // when a load happens.
    let mut editor = use_signal(|| {
        session()
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.editor().clone())
            .unwrap_or_else(empty_editor)
    });

    // Pull in a newly loaded take, and push edits back to the session
    // so a write action sees them.
    use_effect(move || {
        let mut guard = session().lock().unwrap();
        if let Some(s) = guard.as_mut() {
            if s.editor().doc != editor.read().doc {
                *s.editor_mut() = editor.read().clone();
            }
        }
    });

    rsx! {
        div {
            style: "width: 100%; height: 100%;",
            div {
                style: "display: flex; align-items: center; gap: 6px; padding: 4px 8px; \
                        background: #15151c; border-bottom: 1px solid #2b2b38; \
                        color: #c8cede; font-size: 11px; font-family: system-ui, sans-serif;",
                button {
                    style: BUTTON,
                    onclick: move |_| {
                        if load_selected() {
                            if let Some(s) = session().lock().unwrap().as_ref() {
                                editor.set(s.editor().clone());
                            }
                        }
                    },
                    "Load selected item"
                }
                button {
                    style: BUTTON,
                    onclick: move |_| {
                        // Push the panel's document down before writing,
                        // or the action writes whatever was last synced.
                        if let Some(s) = session().lock().unwrap().as_mut() {
                            *s.editor_mut() = editor.read().clone();
                        }
                        write_back();
                    },
                    "Write to take"
                }
                button {
                    style: BUTTON,
                    onclick: move |_| {
                        if reload() {
                            if let Some(s) = session().lock().unwrap().as_ref() {
                                editor.set(s.editor().clone());
                            }
                        }
                    },
                    "Reload"
                }
                span {
                    style: "margin-left: auto; color: #7b8397;",
                    if is_dirty() { "unsaved edits" } else { "in sync" }
                }
            }
            div {
                style: "height: calc(100% - 30px);",
                ExpressionEditor { editor }
            }
        }
    }
}

const BUTTON: &str = "height: 22px; padding: 0 9px; background: #1c1c26; \
                      border: 1px solid #2b2b38; border-radius: 5px; \
                      color: #c8cede; font-size: 11px; cursor: pointer;";

fn empty_editor() -> expression_editor_core::Editor {
    use expression_editor_core::{ExpressionDoc, TimeBase};
    expression_editor_core::Editor::new(
        ExpressionDoc::new(TimeBase::Ppq { ppq: 960.0 }, 0.0, 960.0 * 8.0),
        Viewport::new(1100.0, 520.0),
    )
}

/// Run a closure against the loaded editor.
///
/// Exists for the REAPER integration test, which needs to make an edit
/// through the editor's own path without a pointer — driving the real
/// `Edit` pipeline rather than poking the document is the only way the
/// test proves the pipeline works.
pub fn with_editor<R>(f: impl FnOnce(&mut expression_editor_core::Editor) -> R) -> Option<R> {
    let mut guard = session().lock().unwrap();
    guard.as_mut().map(|s| f(s.editor_mut()))
}

/// The module, for the extension's registry.
pub fn module() -> Box<dyn DawModule> {
    Box::new(ExpressionEditorModule)
}
