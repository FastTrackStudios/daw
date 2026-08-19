//! Key sequences, resolved from the shared input configuration.
//!
//! The counterpart to [`crate::scroll`], and for the same reason: a
//! binding written into a `match` in this crate is one the DAW and
//! REAPER cannot see, and it collides with whatever they already use.
//! `Z` was exactly that — hardcoded here, and already the "Zoom" which-key
//! prefix in the FTS REAPER profile, so the editor was quietly eating the
//! first key of every zoom sequence.
//!
//! So the bindings live in `input`'s keymap under the [`SURFACE`] mode
//! and are resolved through [`input::InputProcessor`], which owns the
//! **sequence state**: `z` alone resolves to nothing and leaves a
//! pending prefix, and the next key completes it. That is what makes
//! `z z` / `z i` work at all, and it is why this is a processor rather
//! than the flat table the scroll side gets away with.

use std::cell::RefCell;

use input::{
    ActionContext, InputCommand, InputEvent, InputProcessor, KeyChord, KeyCode, KeyEvent,
    KeyTrie, KeymapConfig, ModeId, Modifiers,
};

use expression_editor_core::memagic;
use expression_editor_core::tools::Mods;

/// The editor's mode key in the keymap.
pub const SURFACE: &str = "editor";

thread_local! {
    /// The processor, with its pending-sequence state.
    ///
    /// Thread-local rather than a signal because the state is a
    /// half-typed key sequence, not something the view renders — and a
    /// component that remounted mid-sequence would otherwise forget the
    /// prefix the user has already pressed.
    static PROCESSOR: RefCell<Option<InputProcessor>> = const { RefCell::new(None) };

    /// The chords typed so far, mirrored here because the processor
    /// reports its pending sequence only as a display string and the
    /// which-key overlay has to *walk the trie* to that point.
    static PENDING: RefCell<Vec<KeyChord>> = const { RefCell::new(Vec::new()) };
}

fn config() -> KeymapConfig {
    let mut config = input::config::load_default_config().unwrap_or_default();
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(Some(user)) = input::config::load_user_config() {
        config = KeymapConfig::merge(config, user);
    }
    config
}

/// The config as the editor's own processor sees it.
///
/// The bindings are written under an `editor` key so the keymap file
/// reads as one file per surface, but a processor resolves against its
/// *base mode* and exposes no way to change it. So the editor's bindings
/// become this processor's `normal`. The editor has no vim modes of its
/// own to lose — it is a canvas, not a text buffer.
fn editor_config() -> KeymapConfig {
    let mut cfg = config();
    if let Some(bindings) = cfg.keymap.get(SURFACE).cloned() {
        cfg.keymap.insert("normal".to_string(), bindings);
    }
    cfg
}

/// Translate a DOM-ish key name into the crate's [`KeyCode`].
///
/// Single characters are the common case and go through as characters;
/// anything longer is a named key.
fn key_code(key: &str) -> KeyCode {
    match key {
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "Enter" => KeyCode::Enter,
        "Escape" => KeyCode::Escape,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Delete" => KeyCode::Delete,
        _ => {
            if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok())
                && (1..=12).contains(&n)
            {
                return KeyCode::F(n);
            }
            KeyCode::Character(key.to_lowercase())
        }
    }
}

/// How a chord reads in the overlay: `z`, `Ctrl+i`, `Escape`.
fn chord_label(chord: &KeyChord) -> String {
    let mut s = String::new();
    if chord.modifiers.ctrl {
        s.push_str("Ctrl+");
    }
    if chord.modifiers.alt {
        s.push_str("Alt+");
    }
    if chord.modifiers.shift {
        s.push_str("Shift+");
    }
    s.push_str(&match &chord.key {
        KeyCode::Character(c) => c.clone(),
        KeyCode::ArrowUp => "Up".into(),
        KeyCode::ArrowDown => "Down".into(),
        KeyCode::ArrowLeft => "Left".into(),
        KeyCode::ArrowRight => "Right".into(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Escape => "Escape".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::Backspace => "Backspace".into(),
        KeyCode::Delete => "Delete".into(),
        KeyCode::F(n) => format!("F{n}"),
    });
    s
}

/// What the configuration says this key does here, in the editor mode.
///
/// Returns the actions to run — empty when the key is unbound *or* when
/// it opened or extended a sequence, which the caller must treat the
/// same way it treats a bound key: consumed. See [`is_pending`].
pub fn resolve(key: &str, mods: Mods) -> Vec<InputCommand> {
    let event = InputEvent::Key(KeyEvent {
        key: key_code(key),
        modifiers: Modifiers {
            ctrl: mods.ctrl,
            alt: mods.alt,
            shift: mods.shift,
            ..Default::default()
        },
    });
    let chord = KeyChord::new(key_code(key), Modifiers {
        ctrl: mods.ctrl,
        alt: mods.alt,
        shift: mods.shift,
        ..Default::default()
    });

    let commands = PROCESSOR.with(|p| {
        let mut slot = p.borrow_mut();
        let proc = slot.get_or_insert_with(|| {
            // A broken user keymap should cost the customisation, not
            // every binding.
            InputProcessor::from_config(editor_config()).unwrap_or_default()
        });
        proc.process(event, &ActionContext::new())
    });

    // Mirror the processor's sequence state so the overlay can walk to
    // it. Extended while a prefix is live, cleared the moment it is not.
    let pending = is_pending();
    PENDING.with(|p| {
        let mut seq = p.borrow_mut();
        if pending {
            seq.push(chord);
        } else {
            seq.clear();
        }
    });

    commands
}

/// Tell the processor a key came back up.
///
/// Call this from a real `onkeyup`, and only from there. It used to be
/// called at the end of every [`resolve`] instead — a fake release on
/// the theory that this dioxus/blitz build had no key-up event. It has
/// one, and faking it was what made holding `z` spasm: the processor
/// tracks held keys precisely so OS auto-repeat cannot re-enter a
/// sequence, and a surface that reports every press as instantly
/// released turns that suppression off. Forty repeats a second then
/// walked the zoom tree over and over.
///
/// Holding a prefix is a *state*, which is what makes spring-loading
/// possible: `z` down opens the tree and arms the tool, `z` up closes
/// it. Neither half works if the processor thinks the key is already up.
///
/// Returns the processor's own answer to "should the overlay go now?" —
/// `true` when a sticky-prefix run that fired at least one action has
/// ended. The sticky behaviour itself is entirely the processor's:
/// `InputProcessor` keeps a `sticky_anchor` and rewinds the sequence to
/// it after every match while the anchor is held, so holding `g` and
/// tapping `q`, `w`, `e` fires three grid commands. Nothing here needs
/// to re-open anything; it only has to report the release.
pub fn release(key: &str, mods: Mods) -> bool {
    let chord = KeyChord::new(
        key_code(key),
        Modifiers {
            ctrl: mods.ctrl,
            alt: mods.alt,
            shift: mods.shift,
            ..Default::default()
        },
    );
    let hide = PROCESSOR.with(|p| {
        p.borrow_mut()
            .as_mut()
            .map(|proc| proc.notify_key_release(chord))
            .unwrap_or(false)
    });
    // The mirror the overlay walks has to follow the processor's state,
    // which a sticky release just cleared.
    if !is_pending() {
        PENDING.with(|p| p.borrow_mut().clear());
    }
    hide
}

/// Abandon a half-typed sequence.
///
/// Escape has to reach both halves: the processor's own state and the
/// mirror the overlay reads, or the overlay stays up over a prefix that
/// is no longer pending.
pub fn cancel() {
    PROCESSOR.with(|p| {
        if let Some(proc) = p.borrow_mut().as_mut() {
            // The public way to drop a half-typed sequence. It reports
            // the abandoned first key as unhandled, which is exactly
            // what a cancel means; nothing here wants it.
            let _ = proc.timeout_expired();
        }
    });
    PENDING.with(|p| p.borrow_mut().clear());
}

/// One row of the which-key overlay.
#[derive(Clone, Debug, PartialEq)]
pub struct Continuation {
    /// The key to press next.
    pub key: String,
    /// What it does, or the name of the group it opens.
    pub label: String,
    /// Whether it opens a further group rather than running something.
    pub is_group: bool,
}

/// What can follow the keys typed so far, for the which-key overlay.
///
/// Empty when nothing is pending, which is the overlay's cue to stay
/// hidden. Sorted, so the list does not reshuffle between presses.
pub fn continuations() -> Vec<Continuation> {
    let prefix = PENDING.with(|p| p.borrow().clone());
    if prefix.is_empty() {
        return Vec::new();
    }
    walk(prefix)
}

/// What can follow `key`, whether or not it has been pressed.
///
/// The same walk [`continuations`] does, from a prefix the caller names
/// instead of the one being typed — so a panel can show what `k` offers
/// as a way of *teaching* it, rather than only as a reply to it.
///
/// Read from the keymap rather than from a table beside it, which is the
/// point: a rebound prefix relabels itself, and a user's own bindings
/// appear without anything here knowing they exist.
pub fn continuations_after(key: &str) -> Vec<Continuation> {
    walk(vec![KeyChord::new(key_code(key), Modifiers::default())])
}

fn walk(prefix: Vec<KeyChord>) -> Vec<Continuation> {
    PROCESSOR.with(|p| {
        // `get_or_insert_with`, because a panel may ask before any key
        // has been pressed — and a processor that does not exist yet has
        // no keymap to walk, which would show an empty panel exactly
        // once per session.
        let mut slot = p.borrow_mut();
        let proc = slot.get_or_insert_with(|| {
            InputProcessor::from_config(editor_config()).unwrap_or_default()
        });
        let Some(trie) = proc.keymaps().get(&ModeId::new("normal")) else {
            return Vec::new();
        };
        // Walk to the node the typed prefix names.
        let mut node = match trie {
            KeyTrie::Node(n) => n,
            KeyTrie::Leaf(_) => return Vec::new(),
        };
        for chord in &prefix {
            match node.get(chord) {
                Some(KeyTrie::Node(n)) => node = n,
                _ => return Vec::new(),
            }
        }
        let mut out: Vec<Continuation> = node
            .children
            .iter()
            .map(|(chord, child)| Continuation {
                key: chord_label(chord),
                label: match child {
                    KeyTrie::Leaf(action) => label_for(&format!("{action:?}")),
                    KeyTrie::Node(n) => n.name.clone(),
                },
                is_group: matches!(child, KeyTrie::Node(_)),
            })
            .collect();
        out.sort_by(|a, b| a.key.cmp(&b.key));
        out
    })
}

/// A readable name for an action id.
///
/// The shared keymap carries ids, not descriptions — REAPER's which-key
/// config has a `label` field and this one does not — so the names live
/// here, next to the dispatch that implements them.
pub fn label_for(action: &str) -> String {
    for (id, label) in LABELS {
        if action.contains(id) {
            return (*label).to_string();
        }
    }
    // Fall back to the last segment rather than an empty row: an
    // unlabelled binding should still be visible and pressable.
    action
        .rsplit(['.', '"'])
        .find(|s| !s.is_empty())
        .unwrap_or(action)
        .replace('_', " ")
}

/// Action id fragment to overlay label.
const LABELS: &[(&str, &str)] = &[
    ("cursor.left.measure", "Cursor back a measure"),
    ("cursor.right.measure", "Cursor on a measure"),
    ("cursor.left.select", "Extend selection left"),
    ("cursor.right.select", "Extend selection right"),
    ("cursor.left.grid", "Cursor back a grid step"),
    ("cursor.right.grid", "Cursor on a grid step"),
    ("notes.shorten", "Shorten by a grid step"),
    ("notes.lengthen", "Lengthen by a grid step"),
    ("track.next", "Next track"),
    ("track.prev", "Previous track"),
    // Longest first: `grid.1` is a prefix of `grid.16`.
    ("grid.16", "1/16 — sixteenth"),
    ("grid.toggle", "Snap on/off"),
    ("grid.triplet", "Triplet"),
    ("grid.dotted", "Dotted"),
    ("grid.adaptive", "Follow the zoom"),
    ("grid.1", "1 — measure"),
    ("grid.2", "1/2 — half"),
    ("grid.4", "1/4 — quarter"),
    ("grid.8", "1/8 — eighth"),
    // The degrees get their chord's *name* at the call site instead —
    // see `crate::roll`, which rewrites these rows from the live tuning.
    // These are the fallback for a panel with no editor to ask.
    ("chord.degree.", "Fire the chord on this degree"),
    ("chord.tonic_up", "Tonic up"),
    ("chord.tonic_down", "Tonic down"),
    ("chord.mode_next", "Next mode"),
    ("chord.mode_prev", "Previous mode"),
    ("chord.depth_next", "Deeper (7th, 9th…)"),
    ("chord.depth_prev", "Shallower"),
    ("chord.inversion_next", "Invert up"),
    ("chord.inversion_prev", "Invert down"),
    ("chord.octave_up", "Octave up"),
    ("chord.octave_down", "Octave down"),
    // Before the shorter ids they are prefixes of.
    ("velocity.ramp_up_smooth", "Ramp up (smooth)"),
    ("velocity.ramp_up", "Ramp up"),
    ("velocity.ramp_down", "Ramp down"),
    ("velocity.accent", "Accent pattern"),
    ("velocity.compress", "Compress"),
    ("velocity.expand", "Expand"),
    ("velocity.randomize", "Humanise"),
    ("velocity.flatten", "Flatten"),
    ("velocity.panel", "Velocity panel"),
    ("razor.reverse", "Retrograde"),
    ("razor.invert", "Invert pitches"),
    ("razor.delete", "Delete contents"),
    ("razor.duplicate", "Duplicate"),
    ("razor.split", "Split at edges"),
    ("razor.unselect", "Unselect contents"),
    ("razor.select", "Select contents"),
    ("razor.full_lane", "Full lane"),
    // Before `razor.clear`, which is a prefix of it.
    ("razor.clear_lane", "Clear this lane"),
    ("razor.clear", "Drop the areas"),
    ("razor.double", "Double the length"),
    ("razor.halve", "Halve the length"),
    ("view.reset", "Reset the view"),
    ("view.memagic.fit_item", "Fit item"),
    ("view.memagic.fit_notes", "Fit notes in view"),
    ("view.memagic.center", "Centre on notes"),
    ("view.memagic.top", "Top of range"),
    ("view.memagic.bottom", "Bottom of range"),
    ("view.memagic", "Zoom to what I am pointing at"),
];

/// Whether a sequence is half-typed, so the next key belongs to it.
///
/// The caller checks this after an empty [`resolve`]: a pending prefix
/// means the key was consumed by the sequence and must not also fall
/// through to the editor's own handling, or `z` would fire the tool
/// shortcut on its way to `z i`.
/// Whether the pending sequence is the zoom prefix, and nothing more.
///
/// The question the roll asks on a press: a drag *now* means the zoom
/// tool for the length of the hold, rather than the first half of a
/// sequence. Which is what makes `z` one idea at two speeds — tap it and
/// it is a prefix awaiting a target (`z i`), hold it and drag and it is
/// the tool.
///
/// Deliberately narrow. It is true only when `z` alone is pending, so a
/// half-typed longer sequence is never mistaken for a held tool.
pub fn zoom_prefix_held() -> bool {
    held_prefix().as_deref() == Some("z")
}

/// The single-key prefix currently held, if exactly one is.
///
/// The generalisation of [`zoom_prefix_held`], because `z` stopped being
/// the only key that is a prefix *and* a spring-loaded tool. `v` is the
/// second: tap it for the velocity tree, hold it and drag to set
/// velocity by hand.
///
/// Deliberately narrow, and for the same reason it always was: only a
/// lone pending chord counts, so a half-typed longer sequence is never
/// mistaken for a held tool.
pub fn held_prefix() -> Option<String> {
    PENDING.with(|p| {
        let pending = p.borrow();
        match pending.as_slice() {
            [only] => match &only.key {
                KeyCode::Character(c) => Some(c.clone()),
                _ => None,
            },
            _ => None,
        }
    })
}

pub fn is_pending() -> bool {
    PROCESSOR.with(|p| {
        p.borrow()
            .as_ref()
            .is_some_and(|proc| proc.pending_display().is_some())
    })
}

/// The action ids this surface knows how to carry out.
///
/// Listed so the editor's dispatch and the keymap cannot drift apart
/// silently: a binding naming something absent here is dead, which the
/// test below catches.
pub const ACTIONS: &[&str] = &[
    "cursor.left.measure",
    "cursor.right.measure",
    "cursor.left.grid",
    "cursor.right.grid",
    "cursor.left.select",
    "cursor.right.select",
    "notes.shorten",
    "notes.lengthen",
    "track.next",
    "track.prev",
    "grid.1",
    "grid.2",
    "grid.4",
    "grid.8",
    "grid.16",
    "grid.toggle",
    "grid.dotted",
    "grid.triplet",
    "grid.adaptive",
    "chord.degree.1",
    "chord.degree.2",
    "chord.degree.3",
    "chord.degree.4",
    "chord.degree.5",
    "chord.degree.6",
    "chord.degree.7",
    "chord.tonic_up",
    "chord.tonic_down",
    "chord.mode_next",
    "chord.mode_prev",
    "chord.depth_next",
    "chord.depth_prev",
    "chord.inversion_next",
    "chord.inversion_prev",
    "chord.octave_up",
    "chord.octave_down",
    "view.reset",
    "velocity.ramp_up",
    "velocity.ramp_down",
    "velocity.ramp_up_smooth",
    "velocity.accent",
    "velocity.compress",
    "velocity.expand",
    "velocity.randomize",
    "velocity.flatten",
    "velocity.panel",
    "razor.reverse",
    "razor.invert",
    "razor.delete",
    "razor.duplicate",
    "razor.split",
    "razor.select",
    "razor.unselect",
    "razor.full_lane",
    "razor.clear_lane",
    "razor.clear",
    "razor.double",
    "razor.halve",
    "view.memagic",
    "view.memagic.fit_item",
    "view.memagic.fit_notes",
    "view.memagic.center",
    "view.memagic.top",
    "view.memagic.bottom",
];

/// Carry out a resolved action against the editor.
///
/// Kept here beside [`ACTIONS`] and [`LABELS`] so the three cannot drift:
/// adding a binding means adding an arm, a label and an id in one place.
///
/// Returns whether it did anything, so an unknown id falls through
/// rather than silently swallowing the key.
pub fn dispatch(
    ed: &mut expression_editor_core::Editor,
    action: &str,
    region: memagic::Region,
    anchor: memagic::Anchor,
) -> bool {
    use memagic::{Horizontal, Modes, Scope, Vertical};

    // The razor verbs, which are not view changes and so never reach
    // the MeMagic table below.
    // `hjkl`, as the FTS REAPER profile defines it. `h`/`l` walk the
    // edit cursor and `j`/`k` change track — *not* note movement, which
    // is on the arrows there and here. Moving the cursor is what you do
    // constantly; moving notes is what you do on purpose.
    match action {
        "cursor.left.measure" => {
            let d = ed.measure();
            return ed.move_cursor(-d);
        }
        "cursor.right.measure" => {
            let d = ed.measure();
            return ed.move_cursor(d);
        }
        "cursor.left.grid" => {
            let d = ed.grid_step();
            return ed.move_cursor(-d);
        }
        "cursor.right.grid" => {
            let d = ed.grid_step();
            return ed.move_cursor(d);
        }
        "cursor.left.select" => {
            let d = ed.grid_step();
            return ed.move_cursor_extending(-d);
        }
        "cursor.right.select" => {
            let d = ed.grid_step();
            return ed.move_cursor_extending(d);
        }
        // `Shift+h`/`Shift+l` is note length in `midi.styx`, where the
        // arrange profile has it extending a time selection. The MIDI
        // editor's reading wins here, because this *is* the MIDI editor
        // — and the time selection keeps the Ctrl+Shift pair.
        "notes.shorten" => {
            let d = ed.grid_step();
            return ed.nudge_note_lengths(-d);
        }
        "notes.lengthen" => {
            let d = ed.grid_step();
            return ed.nudge_note_lengths(d);
        }
        "track.next" => return ed.step_track(1),
        "track.prev" => return ed.step_track(-1),
        _ => {}
    }

    // The grid tree. Divisions are `grid.<denominator>`, so the five
    // sizes are one arm rather than five — and adding 1/32 later is a
    // line in the keymap and nothing here.
    if let Some(denom) = action
        .strip_prefix("grid.")
        .and_then(|d| d.parse::<f64>().ok())
    {
        ed.set_grid_division(1.0 / denom);
        return true;
    }
    match action {
        "grid.toggle" => {
            ed.grid.enabled = !ed.grid.enabled;
            return true;
        }
        "grid.dotted" => {
            let on = !ed.grid.dotted;
            ed.set_grid_dotted(on);
            return true;
        }
        "grid.triplet" => {
            let on = !ed.grid.triplet;
            ed.set_grid_triplet(on);
            return true;
        }
        "grid.adaptive" => {
            use adaptive_grid::Density;
            // Off ↔ the middle setting. A cycle through all six
            // densities would make "is it on?" take five presses to
            // answer, and Medium is the one worth having.
            let next = if ed.grid.adaptive.is_adaptive() {
                Density::Fixed
            } else {
                Density::Medium
            };
            ed.set_grid_density(next);
            return true;
        }
        _ => {}
    }

    // The chord gun. Degrees first, since they are the ones fired in
    // anger; the rest set up what a degree means.
    if let Some(degree) = action
        .strip_prefix("chord.degree.")
        .and_then(|d| d.parse::<usize>().ok())
    {
        return ed.insert_chord(degree);
    }
    match action {
        "chord.tonic_up" => {
            ed.chord_gun.transpose(1);
            return true;
        }
        "chord.tonic_down" => {
            ed.chord_gun.transpose(-1);
            return true;
        }
        "chord.mode_next" => {
            ed.chord_gun.cycle_mode(true);
            return true;
        }
        "chord.mode_prev" => {
            ed.chord_gun.cycle_mode(false);
            return true;
        }
        "chord.depth_next" => {
            ed.chord_gun.cycle_depth(true);
            return true;
        }
        "chord.depth_prev" => {
            ed.chord_gun.cycle_depth(false);
            return true;
        }
        "chord.inversion_next" => {
            ed.chord_gun.cycle_inversion(true);
            return true;
        }
        "chord.inversion_prev" => {
            ed.chord_gun.cycle_inversion(false);
            return true;
        }
        "chord.octave_up" => {
            ed.chord_gun.octave = (ed.chord_gun.octave + 1).min(8);
            return true;
        }
        "chord.octave_down" => {
            ed.chord_gun.octave = (ed.chord_gun.octave - 1).max(0);
            return true;
        }
        _ => {}
    }

    match action {
        "razor.reverse" => return ed.razor_reverse(),
        "razor.invert" => return ed.razor_invert(),
        "razor.delete" => return ed.razor_delete_contents(),
        "razor.duplicate" => return ed.razor_duplicate(),
        "razor.split" => return ed.razor_split(),
        "razor.select" => return ed.razor_select_contents(),
        "razor.unselect" => return ed.razor_unselect_contents(),
        "razor.full_lane" => return ed.razor_full_lane(),
        "razor.clear_lane" => return ed.razor_clear_lane(),
        "razor.double" => return ed.razor_scale(2.0),
        "razor.halve" => return ed.razor_scale(0.5),
        "view.reset" => {
            ed.reset_view();
            return true;
        }
        "razor.clear" => {
            let had = !ed.razor.is_empty();
            ed.razor.clear();
            return had;
        }
        _ => {}
    }

    let cfg = memagic::Config::default();
    let modes = match action {
        // The contextual one: the region decides, which is the whole
        // point of MeMagic.
        "view.memagic" => return ed.memagic(region, anchor),
        "view.memagic.fit_item" => Modes {
            horizontal: Horizontal::FitItem,
            vertical: Vertical::FitNotes { scope: Scope::InItem },
        },
        "view.memagic.fit_notes" => Modes {
            horizontal: Horizontal::Keep,
            vertical: Vertical::FitNotes { scope: Scope::InView },
        },
        "view.memagic.center" => Modes {
            horizontal: Horizontal::Keep,
            vertical: Vertical::Center { scope: Scope::InView },
        },
        "view.memagic.top" => Modes {
            horizontal: Horizontal::Keep,
            vertical: Vertical::Highest { scope: Scope::InView },
        },
        "view.memagic.bottom" => Modes {
            horizontal: Horizontal::Keep,
            vertical: Vertical::Lowest { scope: Scope::InView },
        },
        _ => return false,
    };
    ed.memagic_with(modes, anchor, &cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Mods {
        Mods {
            ctrl: false,
            alt: false,
            shift: false,
        }
    }

    fn actions(cmds: &[InputCommand]) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                InputCommand::Action(a) => Some(a.0.clone()),
                InputCommand::ActionWithArgs { action, .. } => Some(action.0.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_prefix_waits_for_its_second_key() {
        // The behaviour the whole module exists for: `z` alone must do
        // nothing and stay pending, or the editor eats the first key of
        // every zoom sequence — which is exactly what the hardcoded
        // binding did.
        let first = resolve("z", plain());
        assert!(actions(&first).is_empty(), "z alone fired something");
        assert!(is_pending(), "z should leave a sequence half-typed");
        release("z", plain());

        // `z x`, not `z z`. A sequence whose second key repeats its own
        // prefix cannot survive being *held*: the OS repeats the key,
        // and the leaf fires without anyone pressing it twice. Holding
        // `z` is now the zoom tool, so `z` had to stop being its own
        // continuation.
        let second = resolve("x", plain());
        assert_eq!(actions(&second), vec!["view.memagic"]);
        assert!(!is_pending(), "the sequence should be finished");
        release("x", plain());
    }

    #[test]
    fn each_bound_sequence_reaches_its_action() {
        for (second, want) in [
            ("i", "view.memagic.fit_item"),
            ("n", "view.memagic.fit_notes"),
            ("c", "view.memagic.center"),
            ("t", "view.memagic.top"),
            ("b", "view.memagic.bottom"),
        ] {
            // Press and release each key, the way the surface does.
            // `resolve` no longer fakes the release for you — the
            // processor has to be able to tell a hold from a press.
            resolve("z", plain());
            release("z", plain());
            let got = actions(&resolve(second, plain()));
            release(second, plain());
            assert_eq!(got, vec![want.to_string()], "z {second}");
        }
    }

    #[test]
    fn every_binding_names_an_action_the_editor_can_run() {
        // Catches a keymap entry that points at nothing — a binding that
        // looks configured and does nothing when pressed.
        let cfg = config();
        let bound = cfg
            .keymap
            .get(SURFACE)
            .expect("the editor surface is in the shipped keymap");
        for action in bound.values() {
            assert!(
                ACTIONS.contains(&action.as_str()),
                "{action} is bound but the editor cannot run it"
            );
        }
    }
}
