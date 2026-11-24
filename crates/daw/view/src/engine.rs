//! Changes, on their way to the engine.
//!
//! The window does not own the session — `daw-standalone` does. A fader
//! drag is not "move the fader", it is "tell the engine this track's
//! volume is now this", and the fader moves because the engine says so.
//!
//! That ordering is the whole point. It is what will let a second
//! window, or a real REAPER over `daw-sync`, see the same change: the
//! engine is the thing both are looking at, and a UI that mutated its
//! own copy would be a UI that disagrees with everyone else the moment
//! anything else touches the session.
//!
//! # Why the edits queue rather than being sent
//!
//! A fader drag fires an event per mouse move — a hundred a second on a
//! trackpad. Sent one RPC each, that is a hundred round trips to
//! deliver one number, ninety-nine of which are already stale when they
//! arrive.
//!
//! So continuous edits COALESCE: the queue keeps the latest value per
//! track and control, and the worker sends what is current when it gets
//! to it. Discrete edits — a mute, a rename — never coalesce, because
//! two mute toggles are two different intentions and collapsing them
//! into one would silently drop half of a double-click.

use std::collections::VecDeque;

/// Something the user did that the engine needs to know about.
#[repr(C)]
#[derive(Clone, PartialEq, Debug, facet::Facet)]
pub enum Edit {
    /// A barrier: when it is applied, every edit sent before it has been
    /// ([`ticket`], [`wait_done`]) — what an action that queued an edit
    /// for the window waits on, instead of watching the song for it.
    Done(u64),
    ToggleMute(String),
    ToggleSolo(String),
    ToggleArm(String),
    /// Select this track and nothing else — what a plain click means
    /// everywhere, and what makes "focus the selected track" a focus
    /// rather than an accumulation.
    Select(String),
    /// Add this track to the selection, leaving the rest alone. The
    /// modified click: REAPER's Ctrl, and the same modifier that makes
    /// a drag fine — they apply to different gestures, so one key does
    /// both without ambiguity.
    AddToSelection(String),
    /// A gain, not a dB value — `Track::volume`'s own unit.
    SetVolume(String, f64),
    SetPan(String, f64),
    Rename(String, String),
    /// Polarity. The one control on a strip that changes the signal
    /// without changing a level, which is why it sits on its own in
    /// the corner rather than in the button column.
    ///
    /// Carries the value rather than being a toggle, because the engine
    /// has a setter and not a toggle for it — and because the value the
    /// window is already SHOWING is the one the user meant. Re-reading
    /// the state to invert it would let a click disagree with the
    /// control it landed on.
    SetPhase(String, bool),
    /// Which of the track's fixed lanes play (bit `n`, lane `n`): the
    /// panel's lane steps play one lane at a time.
    SetLanePlays(String, u64),
    /// Whether the track's input is monitored, and when.
    ///
    /// Three states rather than a toggle, because REAPER's is three:
    /// off, on, and on-except-while-playing. A click walks them.
    SetInputMonitor(String, daw_proto::track::InputMonitoringMode),
    /// Whether the track feeds its parent at all.
    ///
    /// A folder's children normally sum into it; a track with this off
    /// is heard only through its own sends, which is how a parallel
    /// path is built. The routing widget's first lane says which.
    SetParentSend(String, bool),
    /// A parameter of an FX on a track's chain, found by its name
    /// (`FTS Comp`): index and normalized value (0..1). What the strip's
    /// Tone rack sends as it is turned (`tone::Store::flush`).
    SetFxParam(String, String, u32, f64),
    /// An FX on a track's chain switched on or bypassed, by its name.
    SetFxEnabled(String, String, bool),
    /// Send this track to that one.
    ///
    /// Two guids, because a send is a relation and not a property: the
    /// source owns it and the destination is a track the engine has to
    /// resolve. The index it comes back with is REAPER's, and the
    /// window does not guess it — the read that follows says what it
    /// was.
    AddSend(String, String),
    /// Take one of this track's routes away.
    ///
    /// Removing renumbers every route after it in the same list, which
    /// is why nothing here patches a list after one: the panel forgets
    /// the track and reads it again.
    RemoveRoute(String, crate::routes::At),
    /// How much goes down one of the track's routes.
    ///
    /// A gain, the way the track's own volume is: 1.0 is unity. Works
    /// on all three kinds — a receive is the same object as the send
    /// feeding it, so turning one down turns the other down, and a
    /// hardware output is how loud the track leaves the box.
    SetRouteVolume(String, crate::routes::At, f64),
    /// Where the route sits, -1.0 to 1.0.
    SetRoutePan(String, crate::routes::At, f64),
    /// Whether the route passes anything at all.
    SetRouteMute(String, crate::routes::At, bool),
    /// Where a SEND is tapped from: post-fader, pre-FX, post-FX.
    ///
    /// The difference between a reverb that follows the fader and one
    /// that does not, which is the first thing anybody changes about a
    /// send and the thing REAPER hides in a menu.
    ///
    /// Sends only, because that is what the service takes: a receive's
    /// mode is the mode of the send feeding it, set on the track that
    /// owns that send.
    SetSendMode(String, u32, daw_proto::routing::SendMode),
    /// The track's colour, as REAPER stores it (0xRRGGBB).
    ///
    /// Carries the value rather than cycling, because a colour is
    /// chosen and not stepped through, and the window is the thing
    /// holding the palette the user picked from.
    SetColor(String, u32),
    /// Trim, read, touch, write, latch, latch-preview.
    ///
    /// Governs whether moving a control writes envelope points while
    /// the transport runs, so it changes what every OTHER edit on this
    /// track means. That is why it is a value and not a toggle: there
    /// is no safe "next" mode to guess at.
    SetAutomationMode(String, daw_proto::primitives::AutomationMode),
    /// What the track records from — a hardware channel, or a MIDI
    /// device and channel.
    ///
    /// The one an engineer changes while tracking, which is exactly
    /// when a second screen showing the old answer is worst.
    SetRecordInput(String, daw_proto::track::RecordInput),
    /// Whether the track is drawn in the arrange panel, the mixer, or
    /// both. Both flags travel together because REAPER sets them
    /// together, and sending one would silently re-assert the other.
    SetVisibility(String, bool, bool),
    /// How tall the track's panel is drawn, in pixels.
    ///
    /// A view concern that lives in the project, so a height set here
    /// is a height REAPER opens with — which is the whole point of a
    /// scene that arranges the session rather than only this window.
    SetHeight(String, u32),
    /// Folder depth: positive opens a folder, negative closes one or
    /// more levels, zero is an ordinary track.
    ///
    /// The one edit that changes the SHAPE of the session rather than a
    /// value in it, so a window applying it has to re-read rather than
    /// patch — every depth below it moves.
    SetFolderDepth(String, i32),
    /// Join or leave one of REAPER's 128 group slots, in every family
    /// at once.
    SetGroupMembership(String, u32, bool),
    /// The track's role — lead, follow, neither — in ONE family of one
    /// slot. What a VCA actually is.
    SetGroupFlags(
        String,
        u32,
        daw_proto::track::GroupFamily,
        daw_proto::track::GroupRole,
    ),
    /// One of the per-slot modifiers that is not a lead/follow pair:
    /// reversed volume, no-lead-when-follow, and the rest.
    SetGroupModifier(String, u32, daw_proto::track::GroupModifier, bool),
    // ── The ruler ────────────────────────────────────────────────
    //
    // Markers and regions are addressed by REAPER's own number rather
    // than by a guid, because that is the only identity the live API
    // offers — there IS a guid in the project file and the live API
    // never fills it. The number is not stable across REAPER's
    // renumber action, which is why the event stream reports a
    // renumbering as one rather than as a deletion: a window holding
    // one of these can follow it rather than lose it.
    //
    // The guid field every other edit carries is empty here. These are
    // not about a track.
    /// Put a marker at a time, in a ruler lane.
    AddMarker(String, f64, u32),
    /// Move a marker to a time.
    MoveMarker(String, u32, f64),
    /// Rename one.
    RenameMarker(String, u32, String),
    /// Take it away.
    RemoveMarker(String, u32),
    /// Make a region spanning two times, in a ruler lane.
    ///
    /// A region in the SECTIONS lane IS a song section — the song
    /// model reads the arrangement from that lane — so this is how a
    /// verse comes into being.
    AddRegion(String, f64, f64, u32),
    /// Set both of a region's bounds. Both together because REAPER's
    /// setter takes both, and because dragging one end of a band the
    /// user can see is a change to the band.
    SetRegionBounds(String, u32, f64, f64),
    RenameRegion(String, u32, String),
    RemoveRegion(String, u32),
    /// Set the tempo of the stretch starting at a time, adding a tempo
    /// marker there if there is not one.
    ///
    /// What tempo mapping writes. The guid is empty: a tempo belongs to
    /// the project, not to a track.
    SetTempo(String, f64, f64),
    /// An ITEM's fade-in: its length in seconds and its shape. The guid
    /// is the item's, not a track's.
    SetFadeIn(String, f64, daw_proto::item::FadeShape),
    /// And its fade-out.
    SetFadeOut(String, f64, daw_proto::item::FadeShape),
    /// Select an item — alone, or added to the selection.
    SelectItem(String, bool),
    /// Take one item back OUT of the selection.
    ///
    /// The other half of the modified click. `SelectItem(_, false)`
    /// adds and `DeselectAllItems` empties; neither can remove one,
    /// which is why a Ctrl-click used to be a one-way gesture — you
    /// could build a selection up a piece at a time and not correct it.
    DeselectItem(String),
    /// Nothing selected. Carries no guid; the string is empty.
    DeselectAllItems(String),
    /// Open an undo step. Everything between this and `EndUndo` is ONE
    /// entry in the session's undo history.
    ///
    /// Carries the label the history will show. A gesture on a folded
    /// row is one gesture however many mics it reaches, and an undo
    /// that took back a third of it would be worse than one that took
    /// back none.
    BeginUndo(String),
    /// Close the undo step `BeginUndo` opened. Same label — REAPER's
    /// block API takes it at both ends.
    EndUndo(String),
    /// Everything selected. The same.
    SelectAllItems(String),
    /// Move an item to a position, in seconds.
    MoveItem(String, f64),
    /// Trim an item to a position and a length, in seconds.
    TrimItem(String, f64, f64),
    /// Split an item at a time: the item keeps the left, and a new one
    /// on the same track (its guid is the third field) takes the right.
    SplitItem(String, f64, String),
    /// Slip an item's contents: how far into the source it starts, in
    /// seconds. The item does not move; the audio inside it does.
    SlipItem(String, f64),
    /// Copy an item to a position on its own track, leaving the
    /// original where it is. The third field is the copy's guid.
    ///
    /// Invented by the window, the way `SplitItem`'s is, and local to
    /// it: the engine makes its own and the next read replaces the
    /// prediction. What the guid buys is a picture that is right on the
    /// next frame rather than whenever the session answers.
    CopyItem(String, f64, String),
    /// Put an item on a fixed lane (REAPER 7's comping lanes): a comp's
    /// piece onto the comp lane.
    SetItemLane(String, u32),
    /// An item's volume (linear): its dial, or gain staging.
    SetItemVolume(String, f64),
    /// An item's active take's stretch markers, `(take time, source
    /// time)` each — a slice's decay drawn out over the gap a slip left.
    SetStretchMarkers(String, Vec<(f64, f64)>),
    /// Where an item's hit is, seconds from its start: a drum slice's
    /// snap offset, which the next edit reads to know where the attack
    /// it must not play twice is.
    SetSnapOffset(String, f64),
    DeleteItem(String),
    /// An item's active take's volume curve, all its points (seconds into
    /// the item, linear gain): a point dragged on the item.
    SetTakeVolume(String, Vec<daw_proto::automation::EnvelopePoint>),
    /// A track's envelope's points, all of them: a point drawn, dragged
    /// or deleted in its lane ([`crate::automation`]).
    SetEnvelope(
        String,
        daw_proto::automation::EnvelopeRef,
        Vec<daw_proto::automation::EnvelopePoint>,
    ),
    /// An automation item's curve — its pool's, so every instance's.
    SetItemCurve(
        String,
        daw_proto::automation::EnvelopeRef,
        u32,
        Vec<daw_proto::automation::EnvelopePoint>,
    ),
    /// An automation item moved to a position, in seconds.
    MoveAutomationItem(String, daw_proto::automation::EnvelopeRef, u32, f64),
    /// An automation item made: at a position, a length (seconds), in a
    /// pool (`-1`: a fresh one, its curve the points given, seconds into
    /// it), its curve its pool's otherwise.
    AddAutomationItem(
        String,
        daw_proto::automation::EnvelopeRef,
        (f64, f64, i32),
        Vec<daw_proto::automation::EnvelopePoint>,
    ),
    /// An automation item's bounds: its index, position, length and how
    /// far into its source it starts (seconds) — an edge dragged.
    SetAutomationItemBounds(
        String,
        daw_proto::automation::EnvelopeRef,
        u32,
        (f64, f64, f64),
    ),
    /// An automation item deleted, by index (the envelope's points under
    /// it play again).
    DeleteAutomationItem(String, daw_proto::automation::EnvelopeRef, u32),
    /// Show or hide a track's envelope in a lane of its own.
    ShowEnvelope(String, daw_proto::automation::EnvelopeRef, bool),
    /// One envelope's automation mode.
    SetEnvelopeMode(
        String,
        daw_proto::automation::EnvelopeRef,
        daw_proto::primitives::AutomationMode,
    ),
    /// An envelope lane's height, in pixels.
    SetLaneHeight(String, daw_proto::automation::EnvelopeRef, u32),
    /// A track's fader or pan knob taken hold of (`true`) or let go: what
    /// Touch and Latch automation record by ([`Touched`]).
    Touch(String, Touched, bool),
    /// A named action — its id and its arguments (JSON) — run on the
    /// engine in order with the edits around it: how a control's gesture
    /// commits (`docs/design/actions-everywhere.md`). The window's edit
    /// worker runs it through the Action List ([`Applier`]), after every
    /// edit queued before it and before any after it — a drag's live
    /// edits, an undo bracket around them, a barrier an action waits on.
    /// It never crosses the view link as an edit.
    Run(String, String),
}

/// Which of a track's controls a [`Edit::Touch`] is about.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, facet::Facet)]
pub enum Touched {
    Volume,
    Pan,
}

impl Touched {
    /// The control a turn is on, if it is one automation writes.
    #[must_use]
    pub const fn of(volume: bool, pan: bool) -> Option<Self> {
        if volume {
            Some(Self::Volume)
        } else if pan {
            Some(Self::Pan)
        } else {
            None
        }
    }
}

impl Edit {
    /// The name the window gave an item this makes (a split's right half,
    /// a copy), which the engine answers with its own.
    #[must_use]
    pub fn predicted(&self) -> Option<&str> {
        match self {
            Self::SplitItem(_, _, made) | Self::CopyItem(_, _, made) => Some(made),
            _ => None,
        }
    }

    /// The track this is about.
    #[must_use]
    pub fn guid(&self) -> &str {
        match self {
            Self::Done(_) | Self::Run(..) => "",
            Self::ToggleMute(g)
            | Self::ToggleSolo(g)
            | Self::ToggleArm(g)
            | Self::Select(g)
            | Self::AddToSelection(g)
            | Self::SetVolume(g, _)
            | Self::SetPan(g, _)
            | Self::Rename(g, _)
            | Self::SetPhase(g, _)
            | Self::SetLanePlays(g, _)
            | Self::SetInputMonitor(g, _)
            | Self::SetParentSend(g, _)
            | Self::SetFxParam(g, ..)
            | Self::SetFxEnabled(g, ..)
            | Self::AddSend(g, _)
            | Self::RemoveRoute(g, _)
            | Self::SetRouteVolume(g, ..)
            | Self::SetRoutePan(g, ..)
            | Self::SetRouteMute(g, ..)
            | Self::SetSendMode(g, ..)
            | Self::SetColor(g, _)
            | Self::SetAutomationMode(g, _)
            | Self::SetRecordInput(g, _)
            | Self::SetVisibility(g, ..)
            | Self::SetHeight(g, _)
            | Self::SetFolderDepth(g, _)
            | Self::SetGroupMembership(g, ..)
            | Self::SetGroupFlags(g, ..)
            | Self::SetGroupModifier(g, ..)
            | Self::AddMarker(g, ..)
            | Self::MoveMarker(g, ..)
            | Self::RenameMarker(g, ..)
            | Self::RemoveMarker(g, ..)
            | Self::AddRegion(g, ..)
            | Self::SetRegionBounds(g, ..)
            | Self::RenameRegion(g, ..)
            | Self::RemoveRegion(g, ..)
            | Self::SetTempo(g, ..)
            | Self::SetFadeIn(g, ..)
            | Self::SetFadeOut(g, ..)
            | Self::SelectItem(g, _)
            | Self::DeselectItem(g)
            | Self::DeselectAllItems(g)
            | Self::BeginUndo(g)
            | Self::EndUndo(g)
            | Self::SelectAllItems(g)
            | Self::MoveItem(g, _)
            | Self::TrimItem(g, ..)
            | Self::SplitItem(g, ..)
            | Self::CopyItem(g, ..)
            | Self::SetItemLane(g, _)
            | Self::SetItemVolume(g, _)
            | Self::SetStretchMarkers(g, _)
            | Self::SetSnapOffset(g, _)
            | Self::SetTakeVolume(g, _)
            | Self::SlipItem(g, _)
            | Self::DeleteItem(g)
            | Self::SetEnvelope(g, ..)
            | Self::SetItemCurve(g, ..)
            | Self::MoveAutomationItem(g, ..)
            | Self::AddAutomationItem(g, ..)
            | Self::SetAutomationItemBounds(g, ..)
            | Self::DeleteAutomationItem(g, ..)
            | Self::ShowEnvelope(g, ..)
            | Self::SetEnvelopeMode(g, ..)
            | Self::SetLaneHeight(g, ..)
            | Self::Touch(g, ..) => g,
        }
    }

    /// Whether this edit is about the ruler rather than a track or an
    /// item. Its guid field is empty; it is addressed by number.
    #[must_use]
    pub const fn is_ruler(&self) -> bool {
        matches!(
            self,
            Self::SetTempo(..)
                | Self::AddMarker(..)
                | Self::MoveMarker(..)
                | Self::RenameMarker(..)
                | Self::RemoveMarker(..)
                | Self::AddRegion(..)
                | Self::SetRegionBounds(..)
                | Self::RenameRegion(..)
                | Self::RemoveRegion(..)
        )
    }

    /// Whether this is about an item rather than a track.
    #[must_use]
    pub const fn is_item(&self) -> bool {
        matches!(
            self,
            Self::SetFadeIn(..)
                | Self::SetFadeOut(..)
                | Self::SelectItem(..)
                | Self::DeselectItem(_)
                | Self::DeselectAllItems(_)
                | Self::SelectAllItems(_)
                | Self::MoveItem(..)
                | Self::TrimItem(..)
                | Self::SplitItem(..)
                | Self::CopyItem(..)
                | Self::SetItemLane(..)
                | Self::SetItemVolume(..)
                | Self::SetStretchMarkers(..)
                | Self::SetSnapOffset(..)
                | Self::SetTakeVolume(..)
                | Self::SlipItem(..)
                | Self::DeleteItem(_)
        )
    }

    /// Whether a newer edit of this kind replaces this one.
    ///
    /// True for the things a drag produces a stream of, false for the
    /// things a click produces one of. Two volumes are one volume; two
    /// mutes are two mutes, and collapsing them would eat half of a
    /// double-click.
    #[must_use]
    pub const fn coalesces(&self) -> bool {
        matches!(
            self,
            Self::SetVolume(..)
                | Self::SetPan(..)
                | Self::SetRouteVolume(..)
                | Self::SetRoutePan(..)
                | Self::SetFadeIn(..)
                | Self::SetFadeOut(..)
                | Self::MoveItem(..)
                | Self::TrimItem(..)
                | Self::SlipItem(..)
                | Self::SetEnvelope(..)
                | Self::SetItemCurve(..)
                | Self::MoveAutomationItem(..)
                | Self::SetAutomationItemBounds(..)
                | Self::SetLaneHeight(..)
                | Self::SetTakeVolume(..)
        )
    }

    /// Which envelope (and automation item) this edit is about, if any —
    /// part of a control's identity like [`Self::route`]: two lanes on
    /// one track are two controls.
    #[must_use]
    pub fn lane(&self) -> Option<String> {
        match self {
            Self::SetEnvelope(_, target, _)
            | Self::ShowEnvelope(_, target, _)
            | Self::SetEnvelopeMode(_, target, _)
            | Self::SetLaneHeight(_, target, _) => Some(crate::automation::target_key(target)),
            Self::SetItemCurve(_, target, item, _)
            | Self::MoveAutomationItem(_, target, item, _)
            | Self::SetAutomationItemBounds(_, target, item, _)
            | Self::DeleteAutomationItem(_, target, item) => {
                Some(format!("{}#{item}", crate::automation::target_key(target)))
            }
            _ => None,
        }
    }

    /// Whether this is about a track's automation.
    #[must_use]
    pub const fn is_automation(&self) -> bool {
        matches!(
            self,
            Self::SetEnvelope(..)
                | Self::SetItemCurve(..)
                | Self::MoveAutomationItem(..)
                | Self::AddAutomationItem(..)
                | Self::SetAutomationItemBounds(..)
                | Self::DeleteAutomationItem(..)
                | Self::ShowEnvelope(..)
                | Self::SetEnvelopeMode(..)
                | Self::SetLaneHeight(..)
        )
    }

    /// Which of the track's routes this edit is about, if any — part of a
    /// control's identity: two sends on one track are two controls, and a
    /// receive numbered as a send is not that send ([`Queue`]).
    #[must_use]
    pub const fn route(&self) -> Option<crate::routes::At> {
        match self {
            Self::RemoveRoute(_, at)
            | Self::SetRouteVolume(_, at, _)
            | Self::SetRoutePan(_, at, _)
            | Self::SetRouteMute(_, at, _) => Some(*at),
            Self::SetSendMode(_, index, _) => Some(crate::routes::At::new(
                daw_proto::routing::RouteType::Send,
                *index,
            )),
            _ => None,
        }
    }
}

/// Edits waiting to go out.
///
/// A drag's positions collapse to its latest (see [`Edit::coalesces`]),
/// found by index, not by a scan — a kit's slip edit queues a hundred
/// thousand edits at once, and a scan per push was minutes of the worker
/// comparing edits before it sent one. And never across an edit that
/// depends on the one replaced: a move of an item queued before a split
/// of it, and another after, are two moves — collapsed, the later
/// position would be applied before the split.
#[derive(Debug, Default)]
pub struct Queue {
    pending: VecDeque<Edit>,
    /// How many edits have gone out: an edit's number less this is where
    /// it is in `pending`.
    popped: u64,
    /// The number of the latest queued coalescing edit, per control.
    control: std::collections::HashMap<ControlKey, u64>,
    /// The number of the latest queued edit that does not coalesce, per
    /// item or track: nothing before it may take a later value.
    barrier: std::collections::HashMap<String, u64>,
}

/// A control's identity: its item or track, which edit, which route.
type ControlKey = (
    String,
    std::mem::Discriminant<Edit>,
    Option<crate::routes::At>,
    Option<String>,
);

impl Queue {
    /// Add an edit, replacing a superseded one if there is one.
    pub fn push(&mut self, edit: Edit) {
        let number = self
            .popped
            .saturating_add(u64::try_from(self.pending.len()).unwrap_or(u64::MAX));
        let guid = edit.guid().to_owned();
        if edit.coalesces() {
            let key: ControlKey = (
                guid,
                std::mem::discriminant(&edit),
                edit.route(),
                edit.lane(),
            );
            let live = self.control.get(&key).copied().filter(|&at| {
                at >= self.popped && self.barrier.get(&key.0).is_none_or(|&wall| wall < at)
            });
            if let Some(at) = live
                && let Some(slot) = usize::try_from(at.saturating_sub(self.popped))
                    .ok()
                    .and_then(|i| self.pending.get_mut(i))
            {
                *slot = edit;
                return;
            }
            self.control.insert(key, number);
        } else {
            self.barrier.insert(guid, number);
        }
        self.pending.push_back(edit);
    }

    /// The next edit to send.
    pub fn pop(&mut self) -> Option<Edit> {
        let next = self.pending.pop_front();
        if next.is_some() {
            self.popped = self.popped.saturating_add(1);
        }
        if self.pending.is_empty() {
            // Nothing queued refers to them now.
            self.control.clear();
            self.barrier.clear();
        }
        next
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// The monitoring mode a click moves to.
///
/// Off → on → on-except-while-playing → off. REAPER's own order, and
/// the one that puts the mode you reach for most one click away.
#[must_use]
pub const fn next_monitor(
    from: daw_proto::track::InputMonitoringMode,
) -> daw_proto::track::InputMonitoringMode {
    use daw_proto::track::InputMonitoringMode as M;
    match from {
        M::Off => M::Normal,
        M::Normal => M::NotWhenPlaying,
        M::NotWhenPlaying => M::Off,
    }
}

/// Where the transport is, polled off the event loop.
///
/// The engine is asked once per frame rather than subscribed to,
/// because a position is a LEVEL and not an event: missing one is
/// harmless (the next is along in a few milliseconds) and the
/// extrapolation covers the gap. A subscription would deliver a
/// backlog after a stall, which is the one thing a playhead must not
/// replay.
pub struct Transport {
    state: std::sync::Arc<std::sync::Mutex<Reading>>,
}

/// One read of the transport.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    /// The play position, in seconds.
    pub at: f64,
    pub playing: bool,
    pub looping: bool,
    pub recording: bool,
    /// The tempo at the play position.
    pub bpm: f64,
}

/// The one poller every panel reads — see [`Transport::shared`].
static SHARED: std::sync::OnceLock<Option<Transport>> = std::sync::OnceLock::new();

/// Where `project` is, seconds, and whether it is playing — read off its
/// own transport: a song playing behind the one on screen (every song of a
/// set is a voice of one engine) shows how far it has got. `None` with no
/// engine in this process.
#[cfg(feature = "local")]
#[must_use]
pub fn playhead_of(project: &str) -> Option<(f64, bool)> {
    let daw = crate::open::local_engine()?;
    let shared = daw.transport_engine_for(project).shared.clone();
    let rate = f64::from(shared.sample_rate().max(1));
    Some((
        shared.playhead_samples_f64() / rate,
        shared.play_state().is_advancing(),
    ))
}

/// No engine in this process to read a playhead off.
#[cfg(all(feature = "native", not(feature = "local")))]
#[must_use]
pub fn playhead_of(_project: &str) -> Option<(f64, bool)> {
    None
}

impl Transport {
    /// Start polling. `None` if the facade is not up.
    #[must_use]
    #[cfg(not(feature = "native"))]
    pub fn start() -> Option<Self> {
        // The browser: no thread to poll on, so a task on the page's
        // event loop, at about a frame.
        daw::rpc::Daw::try_get()?;
        let state = std::sync::Arc::new(std::sync::Mutex::new(Reading::default()));
        let writer = std::sync::Arc::clone(&state);
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                if let Some(mut read) = read_transport().await
                    && let Ok(mut slot) = writer.lock()
                {
                    // The engine renders ahead of the device; the playhead
                    // drawn is the one heard.
                    #[cfg(feature = "web")]
                    if read.playing {
                        read.at = (read.at - crate::host::host().output_latency()).max(0.0);
                    }
                    *slot = read;
                }
                gloo_timers::future::TimeoutFuture::new(16).await;
            }
        });
        Some(Self { state })
    }

    /// Start polling. `None` if the facade is not up.
    #[cfg(feature = "native")]
    #[must_use]
    pub fn start() -> Option<Self> {
        let runtime = crate::open::runtime()?;
        let state = std::sync::Arc::new(std::sync::Mutex::new(Reading::default()));
        let writer = std::sync::Arc::clone(&state);
        std::thread::Builder::new()
            .name("session-engine-transport".into())
            .spawn(move || {
                let mut asked = web_time::Instant::now() - std::time::Duration::from_secs(1);
                let mut last = Reading::default();
                loop {
                    // The playhead from the engine's stamped positions
                    // (`TransportSync::positions`, pushed), carried on to
                    // now — no call; the rest (loop, record, tempo) asked a
                    // few times a second. A system that pushes none (a
                    // REAPER) is asked for all of it.
                    let pushed = crate::open::current_song()
                        .as_deref()
                        .and_then(crate::view_client::playhead_of);
                    let read = match pushed {
                        Some((at, playing)) => {
                            if asked.elapsed() >= std::time::Duration::from_millis(250) {
                                asked = web_time::Instant::now();
                                if let Some(full) = runtime.block_on(read_transport()) {
                                    last = full;
                                }
                            }
                            Some(Reading {
                                at,
                                playing,
                                ..last
                            })
                        }
                        None => runtime.block_on(read_transport()),
                    };
                    if let Some(read) = read {
                        if let Ok(mut slot) = writer.lock() {
                            *slot = read;
                        }
                    }
                    // Faster than an audio block, slower than a frame.
                    std::thread::sleep(std::time::Duration::from_millis(8));
                }
            })
            .ok()?;
        Some(Self { state })
    }

    /// The process's poller, started on first use. Every panel reads this
    /// one rather than starting its own: a transport is one thing, and a
    /// poll thread per panel asks the engine the same question N times.
    /// `None` if the facade is not up.
    #[must_use]
    pub fn shared() -> Option<&'static Self> {
        SHARED.get_or_init(Self::start).as_ref()
    }

    /// The last position read, and whether it is moving.
    #[must_use]
    pub fn read(&self) -> (f64, bool) {
        let reading = self.reading();
        (reading.at, reading.playing)
    }

    /// Everything the last poll read.
    #[must_use]
    pub fn reading(&self) -> Reading {
        self.state
            .lock()
            .map_or_else(|_| Reading::default(), |slot| *slot)
    }
}

/// Whether the transport is moving (playing or recording): what keeps the
/// panels drawing frames with no hand on them.
#[must_use]
pub fn moving() -> bool {
    Transport::shared().is_some_and(|t| {
        let reading = t.reading();
        reading.playing || reading.recording
    })
}

/// What the transport has been asked to do.
///
/// A command, not a state: the engine owns whether it is playing, and
/// this window asks. Sending "play" while it is already playing has to
/// be harmless, which is why these are the engine's own verbs rather
/// than a boolean this side would have to keep in step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Move {
    /// Play if stopped, stop if playing.
    PlayStop,
    /// Back to the start, whether or not it is moving.
    Home,
    /// Move the play position to a time — what a click on the ruler
    /// means once the transport is following it.
    Seek,
    /// Stop, wherever it is.
    Stop,
    /// To the end of the project.
    End,
    /// Loop on or off.
    ToggleLoop,
    /// Record on or off.
    ToggleRecord,
    /// Move to a time and play from it — one command, in order, so the
    /// play cannot land before the move (what rolling into the next song
    /// of a set does).
    PlayFrom,
}

/// Jumps asked of the transport by a press here ([`transport`]: a seek,
/// Home, End, a play from a point), counted: what the arrangement follows
/// even while stopped (`panel::follow_cursor`).
static JUMPS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many jumps have been asked for so far.
#[must_use]
pub fn jumps() -> u64 {
    JUMPS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Send a transport command, off the event loop.
///
/// Fire and forget, like [`Applier`]: the transport's own poll is what
/// tells the window what happened, so waiting for the call to return
/// would be waiting for news the window is already subscribed to.
pub fn transport(command: Move, seconds: f64) {
    // A press by a person: playing together, everyone's.
    #[cfg(any(feature = "native", feature = "web"))]
    if !matches!(command, Move::ToggleLoop | Move::ToggleRecord) {
        crate::body::body().transport_pressed();
    }
    // A press runs the transport's named action (`SESSION_PLAY_STOP`,
    // `SESSION_SEEK`, `TRANSPORT_RECORD`, …) — on the engine over the link,
    // or here — so `watch` and the CLI see it; behind the window, as it was
    // fire and forget. The engine holds a play for a song still streaming
    // in and writes the takes a stop makes.
    #[cfg(feature = "native")]
    {
        // The jump is this window's to follow, wherever the engine is.
        if matches!(
            command,
            Move::Seek | Move::Home | Move::End | Move::PlayFrom
        ) {
            JUMPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let (id, args) = crate::actions::transport_action(command, seconds);
        crate::actions::off_the_window(id, args);
    }
    #[cfg(not(feature = "native"))]
    transport_here(command, seconds);
}

/// Move the transport of the engine in this process — what the engine's own
/// actions do (a window's press is [`transport`]).
pub fn transport_here(command: Move, seconds: f64) {
    if matches!(
        command,
        Move::Seek | Move::Home | Move::End | Move::PlayFrom
    ) {
        JUMPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    // Record is an action (`TRANSPORT_RECORD`): what a model runs — off the
    // caller's thread, since a stop writes the takes.
    #[cfg(feature = "local")]
    if matches!(command, Move::ToggleRecord) && crate::open::local_engine().is_some() {
        let _ = std::thread::Builder::new()
            .name("session-engine-record".into())
            .spawn(|| crate::body::body().record_toggle());
        return;
    }
    // Play on a song still streaming in waits until every recording it
    // plays is here a few seconds ahead — it starts then, by itself —
    // rather than playing what has not arrived as silence. A second press
    // while it waits calls it off.
    #[cfg(feature = "native")]
    if matches!(command, Move::PlayStop) && buffering::hold_play() {
        return;
    }
    transport_following(command, seconds);
}

/// Play held until a streaming song can play (see [`transport`]).
#[cfg(feature = "native")]
pub mod buffering {
    use std::sync::atomic::{AtomicU64, Ordering};

    /// The wait in progress: 0 none, else its number (a press calls off
    /// the one before by moving it on).
    static WAITING: AtomicU64 = AtomicU64::new(0);

    /// Whether a play press is waiting for the song to arrive.
    #[must_use]
    pub fn waiting() -> bool {
        WAITING.load(Ordering::Acquire) != 0
    }

    /// Whether this play press is held: the current song is stopped and
    /// streaming and cannot play yet (a wait starts, and plays it when it
    /// can), or a wait was in progress (this press calls it off).
    pub(super) fn hold_play() -> bool {
        if WAITING.swap(0, Ordering::AcqRel) != 0 {
            tracing::info!(
                play.outcome = "called-off",
                "play: the wait for the song is called off"
            );
            return true;
        }
        let Some(song) = crate::open::current_song() else {
            return false;
        };
        if !crate::body::body().is_stopped(&song) {
            return false;
        }
        let Some(ready) = crate::body::body().readiness(&song) else {
            return false;
        };
        if ready.ready {
            return false;
        }
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        WAITING.store(id, Ordering::Release);
        tracing::info!(
            play.outcome = "waiting",
            play.waiting_on = ready.waiting_on,
            play.buffered = ready.buffered,
            "play: waiting for the song to arrive"
        );
        let started = std::thread::Builder::new()
            .name("session-play-wait".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    if WAITING.load(Ordering::Acquire) != id {
                        return;
                    }
                    let now = crate::body::body().readiness(&song);
                    if now.as_ref().is_none_or(|r| r.ready) {
                        if WAITING
                            .compare_exchange(id, 0, Ordering::AcqRel, Ordering::Acquire)
                            .is_ok()
                        {
                            tracing::info!(
                                play.outcome = "started",
                                "play: the song is here — playing"
                            );
                            super::transport_following(super::Move::PlayStop, 0.0);
                        }
                        return;
                    }
                }
            });
        if started.is_err() {
            WAITING.store(0, Ordering::Release);
            return false;
        }
        true
    }
}

/// [`transport`], for the shared transport moving this engine: not a press
/// here, so not sent back to the others.
pub fn transport_following(command: Move, seconds: f64) {
    #[cfg(not(feature = "native"))]
    wasm_bindgen_futures::spawn_local(run_transport(command, seconds));
    #[cfg(feature = "native")]
    {
        let Some(runtime) = crate::open::runtime() else {
            return;
        };
        std::thread::Builder::new()
            .name("session-engine-transport-cmd".into())
            .spawn(move || runtime.block_on(run_transport(command, seconds)))
            .ok();
    }
}

/// A transport command, against the engine.
async fn run_transport(command: Move, seconds: f64) {
    let Some(daw) = daw::rpc::Daw::try_get() else {
        return;
    };
    let Ok(project) = daw.current_project().await else {
        return;
    };
    let transport = project.transport();
    let outcome = match command {
        Move::PlayStop => {
            let playing = transport.is_playing().await.unwrap_or(false);
            let at = transport.get_position().await.unwrap_or(0.0);
            let behaviour = crate::play_return::get();
            let step = crate::play_return::with_locate(|l| l.press(behaviour, playing, at));
            run_step(&transport, step.unwrap_or(crate::play_return::Step::Stop)).await
        }
        Move::Home => transport.goto_start().await,
        Move::Seek => {
            let playing = transport.is_playing().await.unwrap_or(false);
            crate::play_return::with_locate(|l| l.moved(playing, seconds.max(0.0)));
            transport.set_position(seconds.max(0.0)).await
        }
        Move::Stop => {
            let playing = transport.is_playing().await.unwrap_or(false);
            let behaviour = crate::play_return::get();
            let step = crate::play_return::with_locate(|l| l.stop(behaviour, playing));
            run_step(&transport, step.unwrap_or(crate::play_return::Step::Stop)).await
        }
        Move::End => transport.goto_end().await,
        Move::ToggleLoop => transport.toggle_loop().await,
        Move::ToggleRecord => transport.toggle_recording().await,
        Move::PlayFrom => {
            crate::play_return::with_locate(|l| l.started_from(seconds.max(0.0)));
            match transport.set_position(seconds.max(0.0)).await {
                Ok(()) => transport.play().await,
                Err(error) => Err(error),
            }
        }
    };
    if let Err(error) = outcome {
        tracing::warn!(error = %error, command = ?command, "the transport refused");
    }
}

/// What [`crate::play_return`] decided a play/stop press does.
async fn run_step(
    transport: &daw_control::Transport,
    step: crate::play_return::Step,
) -> daw_control::Result<()> {
    match step {
        crate::play_return::Step::Play => transport.play().await,
        crate::play_return::Step::Stop => transport.stop().await,
        crate::play_return::Step::StopAt(to) => {
            transport.stop().await?;
            transport.set_position(to).await
        }
    }
}

/// One read of the transport.
async fn read_transport() -> Option<Reading> {
    let daw = daw::rpc::Daw::try_get()?;
    let project = daw.current_project().await.ok()?;
    let transport = project.transport();
    let at = transport.get_position().await.ok()?;
    let playing = transport.is_playing().await.ok()?;
    Some(Reading {
        at,
        playing,
        looping: transport.is_looping().await.unwrap_or(false),
        recording: transport.is_recording().await.unwrap_or(false),
        bpm: transport.get_tempo().await.unwrap_or(0.0),
    })
}

/// A dB value as a linear gain. `Track::volume`'s unit.
#[must_use]
pub fn db_to_gain(db: f64) -> f64 {
    // Silence rather than an infinitesimal gain: the bottom of the
    // fader means off, and a gain of 1e-14 is off with extra steps.
    if db <= crate::scale::FADER_BOTTOM_DB {
        return 0.0;
    }
    10.0_f64.powf(db / 20.0)
}

/// Applies edits to the engine, off the event loop.
///
/// The window's thread must not block: a `set_volume` that waited for
/// the engine would stall the frame, and stalling the frame while
/// dragging a fader is the one moment a user notices a stall most. So
/// edits go down a channel and a worker applies them.
///
/// One worker, not a task per edit, so edits arrive in the order they
/// were made. Two mutes racing would be a mute that ends up in the
/// wrong state and no way to tell which.
pub struct Applier {
    #[cfg(feature = "native")]
    edits: std::sync::mpsc::Sender<Edit>,
    /// The browser's: a queue, and whether a task is draining it.
    #[cfg(not(feature = "native"))]
    queue: std::rc::Rc<std::cell::RefCell<(Queue, bool)>>,
}

impl Applier {
    /// Start the worker. `None` if the facade is not up.
    #[cfg(not(feature = "native"))]
    #[must_use]
    pub fn start() -> Option<Self> {
        daw::rpc::Daw::try_get()?;
        Some(Self {
            queue: std::rc::Rc::default(),
        })
    }

    /// Send an edit: queued, and drained in order by one task, so edits
    /// land in the order they were made (and coalesce, as natively).
    #[cfg(not(feature = "native"))]
    pub fn send(&self, edit: Edit) {
        let mut slot = self.queue.borrow_mut();
        slot.0.push(edit);
        if slot.1 {
            return;
        }
        slot.1 = true;
        let queue = std::rc::Rc::clone(&self.queue);
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                let next = {
                    let mut slot = queue.borrow_mut();
                    let next = slot.0.pop();
                    if next.is_none() {
                        slot.1 = false;
                    }
                    next
                };
                let Some(edit) = next else { break };
                // A control's action, in its place among the edits: run
                // on the page's list (`crate::web_actions`).
                #[cfg(feature = "web")]
                if let Edit::Run(id, args) = &edit {
                    if let Err(e) = crate::body::body()
                        .web_run(id.clone(), args.as_bytes().to_vec())
                        .await
                    {
                        tracing::warn!(action.id = %id, error = %e, "control action refused");
                    }
                    continue;
                }
                apply_reported(&edit).await;
            }
        });
    }

    /// Start the worker. `None` if the facade is not up, in which case
    /// the window runs read-only rather than pretending to edit.
    #[cfg(feature = "native")]
    #[must_use]
    pub fn start() -> Option<Self> {
        let runtime = crate::open::runtime()?;
        let (tx, rx) = std::sync::mpsc::channel::<Edit>();
        std::thread::Builder::new()
            .name("session-engine-edits".into())
            .spawn(move || {
                // Drain, coalesce, apply. Draining first is what makes
                // the coalescing work: a hundred queued fader positions
                // become one before any of them is sent.
                //
                // A named action (`Edit::Run`) is a wall: what came before
                // it goes first, it runs, then what came after — nothing
                // coalesces across it.
                let mut queue = Queue::default();
                while let Ok(first) = rx.recv() {
                    let mut arrived = vec![first];
                    while let Ok(more) = rx.try_recv() {
                        arrived.push(more);
                    }
                    for edit in arrived {
                        if let Edit::Run(id, args) = edit {
                            drain(runtime, &mut queue);
                            run_in_order(&id, &args);
                        } else {
                            queue.push(edit);
                        }
                    }
                    drain(runtime, &mut queue);
                }
            })
            .ok()?;
        Some(Self { edits: tx })
    }

    /// Send an edit. Dropped if the worker has gone, because a window
    /// that could not edit is better than one that panics trying.
    #[cfg(feature = "native")]
    pub fn send(&self, edit: Edit) {
        let _ = self.edits.send(edit);
    }
}

/// Everything queued, carried out in order (see [`send_batch`]).
#[cfg(feature = "native")]
fn drain(runtime: &tokio::runtime::Runtime, queue: &mut Queue) {
    if queue.is_empty() {
        return;
    }
    let mut batch = Vec::with_capacity(queue.len());
    while let Some(edit) = queue.pop() {
        batch.push(edit);
    }
    send_batch(runtime, batch);
}

/// A control's action ([`Edit::Run`]), run where every action runs
/// (`crate::actions::run`: on the engine over the link), in its place among
/// the edits. The names the engine gave the items it made (`made`, as a
/// gesture action says them) are remembered as [`send_batch`] remembers
/// them. A refusal is said once, as a key's is.
#[cfg(feature = "native")]
fn run_in_order(id: &str, args: &str) {
    let _span = tracing::info_span!("window.commit", window.commit_action = id).entered();
    match crate::actions::run(id, args.as_bytes()) {
        Ok(out) => {
            let said: serde_json::Value = serde_json::from_slice(&out).unwrap_or_default();
            let made = said.get("made").and_then(serde_json::Value::as_array);
            for pair in made.into_iter().flatten() {
                if let (Some(predicted), Some(real)) = (
                    pair.get("predicted").and_then(serde_json::Value::as_str),
                    pair.get("real").and_then(serde_json::Value::as_str),
                ) {
                    remember(predicted, real);
                }
            }
        }
        Err(e) => tracing::warn!(action.id = id, error = %e, "control action refused"),
    }
}

/// Carry out `batch` (in order) on the engine this window drives: as one
/// call over its link (`crate::view_client::apply` — a kit's slip edit is a
/// hundred thousand edits, one message), the engine answering with the
/// items it made; or, driving a system with no link (a REAPER), edit by
/// edit through the facade.
#[cfg(feature = "native")]
fn send_batch(runtime: &tokio::runtime::Runtime, batch: Vec<Edit>) {
    if crate::view_client::is_linked() {
        let song = crate::open::current_song().unwrap_or_default();
        let Some(applied) = runtime.block_on(crate::view_client::apply(song, batch)) else {
            return;
        };
        for (predicted, real) in &applied.made {
            remember(predicted, real);
        }
        return;
    }
    let mut items = false;
    for edit in &batch {
        items |= edit.is_item();
        runtime.block_on(apply_reported(edit));
    }
    if items {
        crate::wave_detail::engine_changed();
    }
}

/// The items the window named before the engine made them: its guid for
/// a split's right half or a copy, and the real one. Edits run one at a
/// time, in order (see `Engine::send`), so a name is always remembered
/// before an edit that uses it.
static MADE: std::sync::Mutex<Option<std::collections::HashMap<String, String>>> =
    std::sync::Mutex::new(None);

fn remember(predicted: &str, real: &str) {
    MADE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(predicted.to_owned(), real.to_owned());
}

/// The real guid of an item the window may have named itself: the
/// engine's own name for a piece the window split off and named, until the
/// song is read back — what anything asking the engine about the window's
/// items (a waveform's detail) asks it by.
pub fn made(guid: &str) -> String {
    MADE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(|m| m.get(guid).cloned())
        .unwrap_or_else(|| guid.to_owned())
}

/// Set the stretch markers of the item `guid`'s active take, on this
/// window's own engine — the one place a take's markers are written from
/// here (see `Edit::SetStretchMarkers`). Nothing to do without one.
fn stretch(guid: &str, markers: &[(f64, f64)]) {
    #[cfg(not(feature = "local"))]
    let _ = (guid, markers);
    #[cfg(feature = "local")]
    {
        use daw_proto::StretchMarkers as _;
        let (Some(daw), Some(song)) = (crate::open::local_engine(), crate::open::current_song())
        else {
            return;
        };
        let take = daw_proto::StretchTakeRef::new(
            daw::service::ProjectContext::Project(song),
            daw::service::ItemRef::Guid(guid.to_owned()),
            daw::service::TakeRef::Active,
        );
        let markers = markers
            .iter()
            .map(|&(at, source)| daw_proto::StretchMarker::new(at, source))
            .collect();
        if let Err(error) = daw.set_stretch_markers(take, markers) {
            tracing::warn!(error = %error, "the engine refused the stretch markers");
        }
    }
    #[cfg(not(feature = "native"))]
    let _ = (guid, markers);
}

/// Split `item` at `split_at`, keeping the audio on both sides (see
/// `Edit::SplitItem`).
async fn split(item: &daw::rpc::ItemHandle, split_at: f64, new_guid: &str) -> daw::rpc::Result<()> {
    let secs = daw_proto::primitives::Duration::from_seconds;
    let info = item.info().await?;
    let start = info.position.as_seconds();
    let end = start + info.length.as_seconds();
    if split_at <= start || split_at >= end {
        return Ok(());
    }
    let right = item.duplicate().await?;
    remember(new_guid, right.guid());
    right
        .set_position(daw_proto::primitives::PositionInSeconds::from_seconds(
            split_at,
        ))
        .await?;
    right.set_length(secs(end - split_at)).await?;
    right
        .takes()
        .active()
        .await?
        .set_start_offset(secs(info.start_offset.as_seconds() + (split_at - start)))
        .await?;
    right.set_fade_in(secs(0.0), info.fade_in_shape).await?;
    item.set_fade_out(secs(0.0), info.fade_out_shape).await?;
    item.set_length(secs(split_at - start)).await
}

/// An item edit, applied to this window's own engine directly (its
/// services are synchronous in-process); `false` for anything else, or
/// with no song open here, which then goes through the facade. The same
/// edits the facade path makes, call for call.
#[cfg(feature = "local")]
fn apply_locally(edit: &Edit) -> bool {
    use daw_proto::{ItemRef, Items as _, TakeRef, Takes as _};
    if !edit.is_item() {
        return false;
    }
    let (Some(daw), Some(song)) = (crate::open::local_engine(), crate::open::current_song()) else {
        return false;
    };
    let project = daw_proto::ProjectContext::Project(song);
    let secs = daw_proto::primitives::Duration::from_seconds;
    let at = daw_proto::primitives::PositionInSeconds::from_seconds;
    let outcome = match edit {
        Edit::DeselectAllItems(_) => daw.select_all_items(project, false),
        Edit::SelectAllItems(_) => daw.select_all_items(project, true),
        _ => {
            let guid = made(edit.guid());
            let item = ItemRef::Guid(guid.clone());
            let Some(info) = daw.get_item(project.clone(), item.clone()) else {
                return true;
            };
            match edit {
                Edit::SetFadeIn(_, s, shape) => daw.set_fade_in(project, item, secs(*s), *shape),
                Edit::SetFadeOut(_, s, shape) => daw.set_fade_out(project, item, secs(*s), *shape),
                Edit::SelectItem(_, exclusive) => {
                    if *exclusive {
                        let _ = daw.select_all_items(project.clone(), false);
                    }
                    daw.set_selected(project, item, true)
                }
                Edit::DeselectItem(_) => daw.set_selected(project, item, false),
                Edit::SlipItem(_, offset) => {
                    daw.set_start_offset(project, item, TakeRef::Active, secs(*offset))
                }
                Edit::MoveItem(_, position) => daw.set_position(project, item, at(*position)),
                Edit::TrimItem(_, position, length) => daw
                    .set_position(project.clone(), item.clone(), at(*position))
                    .and_then(|()| daw.set_length(project, item, secs(*length))),
                Edit::SplitItem(_, split_at, new_guid) => {
                    let start = info.position.as_seconds();
                    let end = start + info.length.as_seconds();
                    if *split_at <= start || *split_at >= end {
                        return true;
                    }
                    let Some(right) = daw.duplicate_item(project.clone(), item.clone()) else {
                        return true;
                    };
                    remember(new_guid, &right);
                    let half = ItemRef::Guid(right);
                    daw.set_position(project.clone(), half.clone(), at(*split_at))
                        .and_then(|()| {
                            daw.set_length(project.clone(), half.clone(), secs(end - split_at))
                        })
                        .and_then(|()| {
                            daw.set_start_offset(
                                project.clone(),
                                half.clone(),
                                TakeRef::Active,
                                secs(info.start_offset.as_seconds() + (split_at - start)),
                            )
                        })
                        .and_then(|()| {
                            daw.set_fade_in(project.clone(), half, secs(0.0), info.fade_in_shape)
                        })
                        .and_then(|()| {
                            daw.set_fade_out(
                                project.clone(),
                                item.clone(),
                                secs(0.0),
                                info.fade_out_shape,
                            )
                        })
                        .and_then(|()| daw.set_length(project, item, secs(split_at - start)))
                }
                Edit::CopyItem(_, to, new_guid) => daw
                    .duplicate_item(project.clone(), item)
                    .map_or(Ok(()), |copy| {
                        remember(new_guid, &copy);
                        daw.set_position(project, ItemRef::Guid(copy), at(*to))
                    }),
                Edit::SetItemLane(_, lane) => daw.set_fixed_lane(project, item, *lane),
                Edit::SetItemVolume(_, volume) => {
                    daw_proto::Items::set_volume(daw, project, item, *volume)
                }
                Edit::SetStretchMarkers(_, markers) => {
                    stretch(&guid, markers);
                    Ok(())
                }
                Edit::SetSnapOffset(_, s) => daw.set_snap_offset(project, item, secs(*s)),
                Edit::SetTakeVolume(_, points) => {
                    take_volume_locally(daw, &project, &guid, points);
                    Ok(())
                }
                Edit::DeleteItem(_) => daw.delete_item(project, item),
                _ => return false,
            }
        }
    };
    if let Err(error) = outcome {
        tracing::warn!(error = %error, edit = ?edit, "the engine refused an item edit");
    }
    true
}

/// An automation edit, through the facade: the envelope by its track and
/// reference, its points replaced whole.
async fn apply_automation(project: &daw_control::Project, edit: &Edit) {
    let Ok(Some(track)) = project.tracks().by_guid(edit.guid()).await else {
        return;
    };
    let outcome = match edit {
        Edit::SetEnvelope(_, target, points) => {
            let envelope = track.envelopes().of(target.clone());
            match envelope.clear().await {
                Ok(()) => {
                    let mut done = Ok(());
                    for p in points {
                        if let Err(error) = envelope.add_point(p.time, p.value, p.shape).await {
                            done = Err(error);
                            break;
                        }
                    }
                    done
                }
                Err(error) => Err(error),
            }
        }
        Edit::SetItemCurve(_, target, item, points) => {
            track
                .envelopes()
                .of(target.clone())
                .set_automation_item_points(*item, points.clone())
                .await
        }
        Edit::MoveAutomationItem(_, target, item, at) => {
            let mut params = daw_proto::automation::SetAutomationItemParams::at(*item);
            params.position = Some(daw_proto::primitives::PositionInSeconds::from_seconds(*at));
            track
                .envelopes()
                .of(target.clone())
                .set_automation_item(params)
                .await
        }
        Edit::AddAutomationItem(_, target, (at, length, pool), curve) => {
            let envelope = track.envelopes().of(target.clone());
            let (at, length) = (
                daw_proto::primitives::PositionInSeconds::from_seconds(*at),
                daw_proto::primitives::Duration::from_seconds(*length),
            );
            let made = if *pool >= 0 {
                envelope.add_pooled_automation_item(*pool, at, length).await
            } else {
                envelope.add_automation_item(at, length).await
            };
            let outcome = match made {
                Ok(index) if *pool < 0 && !curve.is_empty() => {
                    envelope
                        .set_automation_item_points(index, curve.clone())
                        .await
                }
                Ok(_) => Ok(()),
                Err(error) => Err(error),
            };
            // The window drew a stand-in: the engine's own, read back.
            #[cfg(feature = "native")]
            crate::song::request_resync();
            outcome
        }
        Edit::SetAutomationItemBounds(_, target, index, (at, length, offset)) => {
            let mut params = daw_proto::automation::SetAutomationItemParams::at(*index);
            params.position = Some(daw_proto::primitives::PositionInSeconds::from_seconds(*at));
            params.length = Some(daw_proto::primitives::Duration::from_seconds(*length));
            params.start_offset = Some(daw_proto::primitives::Duration::from_seconds(*offset));
            track
                .envelopes()
                .of(target.clone())
                .set_automation_item(params)
                .await
        }
        Edit::DeleteAutomationItem(_, target, index) => {
            let outcome = track
                .envelopes()
                .of(target.clone())
                .delete_automation_item(*index)
                .await;
            // The items after it are numbered again.
            #[cfg(feature = "native")]
            crate::song::request_resync();
            outcome
        }
        Edit::ShowEnvelope(_, target, shown) => {
            track
                .envelopes()
                .of(target.clone())
                .set_visible(*shown)
                .await
        }
        Edit::SetEnvelopeMode(_, target, mode) => {
            track
                .envelopes()
                .of(target.clone())
                .set_automation_mode(*mode)
                .await
        }
        Edit::SetLaneHeight(_, target, height) => {
            track
                .envelopes()
                .of(target.clone())
                .show_in_lane(*height)
                .await
        }
        _ => Ok(()),
    };
    if let Err(error) = outcome {
        tracing::warn!(error = %error, "the engine refused an automation edit");
    }
}

/// An item's take volume replaced by `points` in this process's engine —
/// and its waveform, drawn through it, read again.
#[cfg(feature = "local")]
fn take_volume_locally(
    daw: &daw::standalone::Standalone,
    project: &daw_proto::ProjectContext,
    item: &str,
    points: &[daw_proto::automation::EnvelopePoint],
) {
    use daw_proto::automation::{Automation as _, TakeEnvelopeKind, TimeRangeParams};
    use daw_proto::{ItemRef, Takes as _};
    let Some(take) = daw.get_active_take(project.clone(), ItemRef::Guid(item.to_owned())) else {
        return;
    };
    let at = daw_proto::EnvelopeLocation::new(
        daw_proto::TrackRef::Index(0),
        daw_proto::EnvelopeRef::Take {
            item_guid: item.to_owned(),
            take_guid: take.guid,
            kind: TakeEnvelopeKind::Volume,
        },
    );
    daw.delete_points_in_range(
        project.clone(),
        at.clone(),
        TimeRangeParams::new(
            daw_proto::primitives::PositionInSeconds::from_seconds(f64::MIN / 4.0),
            daw_proto::primitives::PositionInSeconds::from_seconds(f64::MAX / 4.0),
        ),
    );
    for p in points {
        daw.add_point(
            project.clone(),
            at.clone(),
            daw_proto::automation::AddPointParams::new(p.time, p.value, p.shape),
        );
    }
    crate::midi::mark_stale([item.to_owned()]);
    crate::song::request_resync();
}

/// The latest ticket handed out ([`ticket`]).
static ISSUED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The latest [`Edit::Done`] applied, and who waits on it.
static DONE: (std::sync::Mutex<u64>, std::sync::Condvar) =
    (std::sync::Mutex::new(0), std::sync::Condvar::new());

/// A ticket for an edit queued for the window: the window sends
/// [`Edit::Done`] with it after the edit's own edits, and [`wait_done`]
/// returns once the engine has applied them all.
#[must_use]
pub fn ticket() -> u64 {
    ISSUED
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
        .saturating_add(1)
}

/// Wait until [`Edit::Done`] with `ticket` (or a later one) has been
/// applied — at most `within`. False if it was not.
pub fn wait_done(ticket: u64, within: std::time::Duration) -> bool {
    let (lock, signal) = &DONE;
    let started = web_time::Instant::now();
    let mut applied = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while *applied < ticket {
        let Some(left) = within.checked_sub(started.elapsed()) else {
            return false;
        };
        applied = signal
            .wait_timeout(applied, left)
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0;
    }
    true
}

/// [`Edit::Done`] with `ticket` applied: everyone waiting on it or an
/// earlier one woken.
fn done(ticket: u64) {
    let (lock, signal) = &DONE;
    let mut applied = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *applied = (*applied).max(ticket);
    signal.notify_all();
}

/// One edit, against the engine, waited for: what a test or a tool that
/// needs the result before going on does instead of the queue. False when
/// there is no runtime to run it on.
#[cfg(feature = "native")]
#[must_use]
pub fn apply_now(edit: &Edit) -> bool {
    crate::open::runtime().is_some_and(|runtime| {
        runtime.block_on(apply_reported(edit));
        true
    })
}

/// One edit, against the engine — then said to the Action List, so
/// whoever watches the app (a model, through `architect-mcp`'s `watch`)
/// hears the session change as it changes.
pub async fn apply_reported(edit: &Edit) {
    apply(edit).await;
    #[cfg(feature = "native")]
    if !matches!(edit, Edit::BeginUndo(_) | Edit::EndUndo(_) | Edit::Done(_)) {
        let topic = if edit.is_ruler() {
            "markers"
        } else if edit.is_item() {
            "items"
        } else {
            "tracks"
        };
        crate::actions::list().changed(topic, edit.guid());
    }
}

/// One edit, against the engine.
async fn apply(edit: &Edit) {
    // An item edit on this window's own engine, straight to it: through
    // the facade each is a dozen round trips (a split is a lookup, a
    // duplicate, a move, a length, an offset and two fades), and a kit's
    // slip edit is a hundred thousand edits — half an hour, through it.
    #[cfg(feature = "local")]
    if apply_locally(edit) {
        return;
    }
    let Some(daw) = daw::rpc::Daw::try_get() else {
        return;
    };
    let Ok(project) = daw.current_project().await else {
        return;
    };
    // The brackets first: they are about the history rather than about
    // anything in the session, so they belong to neither the item path
    // nor the track one.
    match edit {
        Edit::Done(ticket) => {
            done(*ticket);
            return;
        }
        Edit::BeginUndo(label) => {
            if let Err(error) = project.begin_undo_block(label).await {
                tracing::warn!(error = %error, "the engine refused to open an undo step");
            }
            return;
        }
        Edit::EndUndo(label) => {
            if let Err(error) = project.end_undo_block(label).await {
                tracing::warn!(error = %error, "the engine refused to close an undo step");
            }
            return;
        }
        // Run by the window's edit worker, never applied as an edit.
        Edit::Run(id, _) => {
            tracing::warn!(action.id = %id, "a named action reached the edit path unrun");
            return;
        }
        _ => {}
    }
    if edit.is_automation() {
        apply_automation(&project, edit).await;
        return;
    }
    if edit.is_item() {
        if let Edit::DeselectAllItems(_) = edit {
            if let Err(error) = project.items().deselect_all().await {
                tracing::warn!(error = %error, "the engine refused to deselect");
            }
            return;
        }
        if let Edit::SelectAllItems(_) = edit {
            if let Err(error) = project.items().select_all().await {
                tracing::warn!(error = %error, "the engine refused to select all");
            }
            return;
        }
        let Ok(Some(item)) = project.items().by_guid(&made(edit.guid())).await else {
            return;
        };
        let secs = daw_proto::primitives::Duration::from_seconds;
        let at = daw_proto::primitives::PositionInSeconds::from_seconds;
        let outcome = match edit {
            Edit::SetFadeIn(_, s, shape) => item.set_fade_in(secs(*s), *shape).await,
            Edit::SetFadeOut(_, s, shape) => item.set_fade_out(secs(*s), *shape).await,
            Edit::SelectItem(_, exclusive) => {
                if *exclusive && let Err(error) = project.items().deselect_all().await {
                    tracing::warn!(error = %error, "the engine refused to deselect");
                }
                item.select().await
            }
            // The active take's offset into its source. Addressed
            // through the take because that is where the offset lives —
            // `Item::start_offset` is the read side of the same value.
            Edit::SlipItem(_, offset) => match item.takes().active().await {
                Ok(take) => take.set_start_offset(secs(*offset)).await,
                Err(error) => Err(error),
            },
            Edit::DeselectItem(_) => item.deselect().await,
            Edit::MoveItem(_, position) => item.set_position(at(*position)).await,
            Edit::TrimItem(_, position, length) => match item.set_position(at(*position)).await {
                Ok(()) => item.set_length(secs(*length)).await,
                Err(error) => Err(error),
            },
            // The engine has no split of its own: the right half is the
            // item duplicated — source, take, fades and all — moved to
            // the cut and slipped as far into its source, and the left
            // is the item shortened. The window named the right half;
            // the name is remembered against the real one, so a later
            // edit (a quantize moves every slice it cut) finds it.
            Edit::SplitItem(_, split_at, new_guid) => split(&item, *split_at, new_guid).await,
            // The copy the engine makes is its own, with its own guid:
            // the one in the edit was the window's prediction, and it is
            // remembered against the real one for a later edit.
            Edit::CopyItem(_, to, new_guid) => match item.duplicate().await {
                Ok(copy) => {
                    remember(new_guid, copy.guid());
                    copy.set_position(at(*to)).await
                }
                Err(error) => Err(error),
            },
            Edit::SetItemLane(_, lane) => item.set_fixed_lane(*lane).await,
            Edit::SetItemVolume(_, volume) => item.set_volume(*volume).await,
            Edit::SetStretchMarkers(_, markers) => {
                stretch(item.guid(), markers);
                Ok(())
            }
            Edit::DeleteItem(_) => item.delete().await,
            Edit::SetTakeVolume(_, points) => match item.active_take().envelopes().await {
                Ok(envelopes) => {
                    let volume = envelopes.volume();
                    match volume.clear().await {
                        Ok(()) => {
                            for p in points {
                                if let Err(error) = volume.add_point(p.time, p.value, p.shape).await
                                {
                                    tracing::warn!(error = %error, "the engine refused a take volume point");
                                    break;
                                }
                            }
                            Ok(())
                        }
                        Err(error) => Err(error),
                    }
                }
                Err(error) => Err(error),
            },
            _ => Ok(()),
        };
        if let Err(error) = outcome {
            tracing::warn!(error = %error, edit = ?edit, "the engine refused an item edit");
        }
        return;
    }
    if edit.is_ruler() {
        let markers = project.markers();
        let regions = project.regions();
        let outcome = match edit {
            Edit::AddMarker(_, at, lane) => match markers.add(*at, "").await {
                // The lane is a second call because REAPER's add takes
                // no lane. A marker that landed in the wrong lane would
                // be a mark in the sections row, which is a section
                // that is not one.
                Ok(id) => markers.set_lane(id, Some(*lane)).await,
                Err(error) => Err(error),
            },
            Edit::MoveMarker(_, id, at) => markers.move_to(*id, at.max(0.0)).await,
            Edit::RenameMarker(_, id, name) => markers.rename(*id, name).await,
            Edit::RemoveMarker(_, id) => markers.remove(*id).await,
            Edit::AddRegion(_, from, to, lane) => {
                let (from, to) = ordered(*from, *to);
                match regions.add(from, to, "").await {
                    Ok(id) => regions.set_lane(id, Some(*lane)).await,
                    Err(error) => Err(error),
                }
            }
            Edit::SetRegionBounds(_, id, from, to) => {
                let (from, to) = ordered(*from, *to);
                regions.set_bounds(*id, from, to).await
            }
            Edit::RenameRegion(_, id, name) => regions.rename(*id, name).await,
            Edit::RemoveRegion(_, id) => regions.remove(*id).await,
            Edit::SetTempo(_, at, bpm) => set_tempo(&project, *at, *bpm).await,
            _ => Ok(()),
        };
        if let Err(error) = outcome {
            tracing::warn!(error = %error, edit = ?edit, "the engine refused a ruler edit");
        }
        return;
    }
    let Ok(Some(track)) = project.tracks().by_guid(edit.guid()).await else {
        // The track went away between the click and the apply — a
        // project reload, or another client removing it. Nothing to
        // report: the session simply no longer has what was clicked.
        return;
    };
    let outcome = match edit {
        Edit::ToggleMute(_) => track.toggle_mute().await,
        Edit::ToggleSolo(_) => track.toggle_solo().await,
        Edit::ToggleArm(_) => track.toggle_arm().await,
        Edit::Select(_) => track.select_exclusive().await,
        Edit::AddToSelection(_) => track.select().await,
        // r[impl automation.write]
        // Through the automation writer: it sets the fader, and records a
        // point where the envelope's mode (or the track's) says to —
        // Touch while held, Latch from the first touch, Write always —
        // while the song plays.
        Edit::SetVolume(_, v) => match track.envelopes().volume().write(*v).await {
            Ok(()) => Ok(()),
            Err(_) => track.set_volume(*v).await,
        },
        Edit::SetPan(_, p) => match track.envelopes().pan().write(p.mul_add(0.5, 0.5)).await {
            Ok(()) => Ok(()),
            Err(_) => track.set_pan(*p).await,
        },
        Edit::Touch(_, which, held) => {
            let envelope = match which {
                Touched::Volume => track.envelopes().volume(),
                Touched::Pan => track.envelopes().pan(),
            };
            if *held {
                envelope.touch().await
            } else {
                envelope.release().await
            }
        }
        Edit::Rename(_, name) => track.rename(name).await,
        Edit::SetPhase(_, inverted) => track.set_phase_inverted(*inverted).await,
        Edit::SetLanePlays(_, mask) => track.set_lane_play_mask(*mask).await,
        Edit::SetInputMonitor(_, mode) => track.set_input_monitor(*mode).await,
        Edit::SetParentSend(_, enabled) => track.set_parent_send(*enabled).await,
        Edit::SetFxParam(_, fx, index, value) => match track.fx_chain().by_name(fx).await {
            Ok(Some(fx)) => fx.param(*index).set(*value).await,
            other => other.map(|_| ()),
        },
        Edit::SetFxEnabled(_, fx, enabled) => match track.fx_chain().by_name(fx).await {
            Ok(Some(fx)) if *enabled => fx.enable().await,
            Ok(Some(fx)) => fx.disable().await,
            other => other.map(|_| ()),
        },
        // A send is addressed through the track that owns it, by the
        // number REAPER gave it. `by_index` costs a round trip of its
        // own to prove the route is still there — worth it, because
        // the alternative is writing a level into whatever now holds
        // that number after somebody removed the one before it.
        Edit::AddSend(_, dest) => track.sends().add_to(dest).await.map(|_| ()),
        Edit::RemoveRoute(_, at) => {
            on_route(&track, *at, |route| async move { route.remove().await }).await
        }
        Edit::SetRouteVolume(_, at, v) => {
            let v = *v;
            on_route(&track, *at, move |route| async move {
                route.set_volume(v).await
            })
            .await
        }
        Edit::SetRoutePan(_, at, p) => {
            let p = *p;
            on_route(&track, *at, move |route| async move { route.set_pan(p).await }).await
        }
        Edit::SetRouteMute(_, at, muted) => {
            let muted = *muted;
            on_route(&track, *at, move |route| async move {
                if muted { route.mute().await } else { route.unmute().await }
            })
            .await
        }
        Edit::SetSendMode(_, index, mode) => {
            let mode = *mode;
            let at = crate::routes::At::new(daw_proto::routing::RouteType::Send, *index);
            on_route(&track, at, move |route| async move {
                route.set_send_mode(mode).await
            })
            .await
        }
        Edit::SetColor(_, color) => track.set_color(*color).await,
        Edit::SetAutomationMode(_, mode) => track.set_automation_mode(*mode).await,
        Edit::SetRecordInput(_, input) => track.set_record_input(*input).await,
        Edit::SetVisibility(_, tcp, mixer) => track.set_visibility(*tcp, *mixer).await,
        Edit::SetHeight(_, pixels) => track.set_tcp_height(*pixels).await,
        Edit::SetFolderDepth(_, depth) => track.set_folder_depth(*depth).await,
        Edit::SetGroupMembership(_, slot, member) => {
            track.set_group_membership(*slot, *member).await
        }
        Edit::SetGroupFlags(_, slot, family, role) => {
            track.set_group_flags(*slot, *family, *role).await
        }
        Edit::SetGroupModifier(_, slot, modifier, on) => {
            track.set_group_modifier(*slot, *modifier, *on).await
        }
        Edit::SetEnvelope(..)
        | Edit::AddAutomationItem(..)
        | Edit::SetAutomationItemBounds(..)
        | Edit::DeleteAutomationItem(..)
        | Edit::SetItemCurve(..)
        | Edit::MoveAutomationItem(..)
        | Edit::ShowEnvelope(..)
        | Edit::SetEnvelopeMode(..)
        | Edit::SetLaneHeight(..)
        | Edit::SetFadeIn(..)
        | Edit::SetFadeOut(..)
        | Edit::SelectItem(..)
        | Edit::DeselectItem(_)
        | Edit::DeselectAllItems(_)
        | Edit::SelectAllItems(_)
        | Edit::MoveItem(..)
        | Edit::TrimItem(..)
        | Edit::SplitItem(..)
        | Edit::CopyItem(..)
        | Edit::SetItemLane(..)
        | Edit::SetItemVolume(..)
        | Edit::SetStretchMarkers(..)
        | Edit::SetSnapOffset(..)
        | Edit::SetTakeVolume(..)
        | Edit::SlipItem(..)
        | Edit::DeleteItem(_)
        // Already handled above, where they did not need a track.
        | Edit::AddMarker(..)
        | Edit::MoveMarker(..)
        | Edit::RenameMarker(..)
        | Edit::RemoveMarker(..)
        | Edit::AddRegion(..)
        | Edit::SetRegionBounds(..)
        | Edit::RenameRegion(..)
        | Edit::RemoveRegion(..)
        | Edit::SetTempo(..)
        // Already handled above, where they needed no track either.
        | Edit::BeginUndo(_)
        | Edit::EndUndo(_)
        | Edit::Done(_)
        | Edit::Run(..) => Ok(()),
    };
    if let Err(error) = outcome {
        // One line, because a failed edit is a thing the user did that
        // did not happen — silence here is how a mixer starts lying.
        tracing::warn!(error = %error, edit = ?edit, "the engine refused an edit");
    }
}

/// Write a tempo at a time, moving the marker there if one is close
/// enough to be the same one.
///
/// "Close enough" is a millisecond. Tempo mapping puts a marker on a
/// bar line and then corrects it, and a correction that added a second
/// marker a thousandth of a second from the first would leave a tempo
/// map full of pairs — and pairs mean a bar that is two tempos, which
/// is a bar with no tempo at all.
async fn set_tempo(project: &daw_control::Project, at: f64, bpm: f64) -> daw_control::Result<()> {
    const SAME_POINT: f64 = 0.001;
    let map = project.tempo_map();
    let existing = map.points().await.unwrap_or_default();
    let found = existing.iter().position(|point| {
        point
            .position
            .time
            .as_ref()
            .is_some_and(|t| (t.as_seconds() - at).abs() < SAME_POINT)
    });
    match found {
        Some(index) => {
            map.set_tempo_at(u32::try_from(index).unwrap_or(0), bpm)
                .await
        }
        None => map.add_point(at.max(0.0), bpm).await.map(|_| ()),
    }
}

/// Two times, in order, with a floor at zero.
///
/// A region dragged right to left is the region the user drew; a
/// region with its end before its start is one REAPER will refuse.
fn ordered(a: f64, b: f64) -> (f64, f64) {
    let (from, to) = if a <= b { (a, b) } else { (b, a) };
    (from.max(0.0), to.max(0.0))
}

/// Do something to one of a track's routes.
///
/// The resolve is half the cost of a route edit — `by_index` reads the
/// route to prove it exists before handing back a handle — so it is in
/// one place rather than repeated per arm, and a route that has gone is
/// a warning rather than a silent nothing: a level written into a
/// number nobody holds any more is exactly the bug that would otherwise
/// look like the panel not working.
async fn on_route<F, Fut>(
    track: &daw_control::TrackHandle,
    at: crate::routes::At,
    act: F,
) -> daw_control::Result<()>
where
    F: FnOnce(daw_control::RouteHandle) -> Fut,
    Fut: std::future::Future<Output = daw_control::Result<()>>,
{
    use daw_proto::routing::RouteType;
    let found = match at.kind {
        RouteType::Send => track.sends().by_index(at.index).await,
        RouteType::Receive => track.receives().by_index(at.index).await,
        RouteType::HardwareOutput => track.hardware_outputs().by_index(at.index).await,
    };
    match found {
        Ok(Some(route)) => act(route).await,
        Ok(None) => {
            tracing::warn!(
                route.index = at.index,
                route.kind = ?at.kind,
                "the route this edit names is gone"
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// The engine's account of the ROUTING, as it changes.
///
/// A second subscription, and it has to be: what a send is doing —
/// its level, its pan, its mute — travels on the routing stream, and
/// the track stream carries only how MANY there are. A strip that
/// draws a lane does not need the first; a panel that draws the sends
/// needs nothing else.
///
/// Taken from the cross-domain bus rather than a routing stream of its
/// own, because there is no routing stream client: the bus is the only
/// client-side path to a [`RoutingEvent`]. One filter, one domain — the
/// rest are dropped before they reach this channel.
pub struct Bus {
    changes: std::sync::mpsc::Receiver<daw_proto::routing::RoutingEvent>,
    alive: Alive,
}

impl Bus {
    /// Subscribe. `None` if the facade is not up.
    #[must_use]
    pub fn start() -> Option<Self> {
        let runtime = crate::open::runtime()?;
        let (tx, rx) = std::sync::mpsc::channel();
        let alive = Alive::new();
        let mine = alive.clone();
        std::thread::Builder::new()
            .name("session-engine-routing".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let _end = scopeguard(move || mine.ended());
                    let Some(daw) = daw::rpc::Daw::try_get() else {
                        return;
                    };
                    let filter = daw_proto::event_bus::BusFilter {
                        routing: true,
                        ..daw_proto::event_bus::BusFilter::default()
                    };
                    let Ok(mut stream) = daw.events().subscribe(filter).await else {
                        return;
                    };
                    while let Ok(Some(event)) = stream.recv().await {
                        let daw_proto::event_bus::DawEvent::Routing(routing) = event.get() else {
                            continue;
                        };
                        if tx.send(routing.clone()).is_err() {
                            break;
                        }
                    }
                });
            })
            .ok()?;
        Some(Self { changes: rx, alive })
    }

    /// Everything that has happened since the last frame.
    pub fn drain(&self) -> impl Iterator<Item = daw_proto::routing::RoutingEvent> + '_ {
        self.changes.try_iter()
    }

    /// Is this subscription still connected?
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.alive.is_live()
    }
}

/// Which tracks' FX chains changed in the engine, by track GUID.
///
/// A parameter moved, an FX added, removed, moved or switched: what the FX
/// row reads again ([`crate::tone::Store::sync`]). Whoever made the change:
/// the row itself, an action over MCP, the mix chains at prepare.
#[cfg(feature = "native")]
pub struct FxWatch {
    changes: std::sync::mpsc::Receiver<String>,
    alive: Alive,
}

#[cfg(feature = "native")]
impl FxWatch {
    /// Subscribe. `None` if the facade is not up.
    #[must_use]
    pub fn start() -> Option<Self> {
        let runtime = crate::open::runtime()?;
        let (tx, rx) = std::sync::mpsc::channel();
        let alive = Alive::new();
        let mine = alive.clone();
        std::thread::Builder::new()
            .name("session-engine-fx".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let _end = scopeguard(move || mine.ended());
                    let Some(daw) = daw::rpc::Daw::try_get() else {
                        return;
                    };
                    let filter = daw_proto::event_bus::BusFilter {
                        fx: true,
                        ..daw_proto::event_bus::BusFilter::default()
                    };
                    let Ok(mut stream) = daw.events().subscribe(filter).await else {
                        return;
                    };
                    while let Ok(Some(event)) = stream.recv().await {
                        let daw_proto::event_bus::DawEvent::Fx(fx) = event.get() else {
                            continue;
                        };
                        if let daw_proto::fx::FxChainContext::Track(track) = fx_context(&fx.event)
                            && tx.send(track.clone()).is_err()
                        {
                            break;
                        }
                    }
                });
            })
            .ok()?;
        Some(Self { changes: rx, alive })
    }

    /// The tracks whose chains changed since the last frame.
    pub fn drain(&self) -> impl Iterator<Item = String> + '_ {
        self.changes.try_iter()
    }

    /// Is this subscription still connected?
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.alive.is_live()
    }
}

/// The chain an FX event is about.
#[cfg(feature = "native")]
const fn fx_context(event: &daw_proto::fx::FxEvent) -> &daw_proto::fx::FxChainContext {
    use daw_proto::fx::FxEvent as E;
    match event {
        E::Added { context, .. }
        | E::Removed { context, .. }
        | E::EnabledChanged { context, .. }
        | E::Moved { context, .. }
        | E::ParameterChanged { context, .. }
        | E::PresetChanged { context, .. }
        | E::WindowChanged { context, .. }
        | E::ContainerCreated { context, .. }
        | E::ContainerRemoved { context, .. }
        | E::RoutingModeChanged { context, .. }
        | E::MovedToContainer { context, .. }
        | E::ContainerRenamed { context, .. }
        | E::TreeStructureChanged { context } => context,
    }
}

/// The engine's own account of the tracks, as it changes.
///
/// The window predicts an edit so the control moves under the hand —
/// see `apply_locally` — and this is what turns that prediction into a
/// correction. It is also the only way a change made ANYWHERE ELSE
/// reaches the window: another client, a control surface, a REAPER on
/// the other end of daw-sync. A UI that only ever saw its own edits
/// would be a UI that silently disagrees with everyone.
///
/// Events rather than polling, because a change IS an event: missing
/// one leaves the window wrong until something else happens to it,
/// where missing a transport position is covered by the next one a few
/// milliseconds later. That asymmetry is why `Transport` polls and this
/// does not.
pub struct Watch {
    changes: std::sync::mpsc::Receiver<daw_proto::track::TrackEvent>,
    alive: Alive,
}

/// What is woken when a track event arrives, by key: a pane that draws
/// at its own pace (an Architect process view's, every 16.7 ms) draws as
/// the event lands instead, so an edit made in another pane shows in the
/// same frame as there.
static ON_EVENT: std::sync::Mutex<Vec<(u64, Wake)>> = std::sync::Mutex::new(Vec::new());

/// What [`on_track_event`] wakes.
pub type Wake = std::sync::Arc<dyn Fn() + Send + Sync>;

/// The last key [`on_track_event`] gave.
static ON_EVENT_KEYS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Wake `wake` (from the watch's thread) whenever a track event arrives,
/// for as long as the returned guard is held.
#[must_use]
pub fn on_track_event(wake: Wake) -> OnTrackEvent {
    let key = ON_EVENT_KEYS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    ON_EVENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((key, wake));
    OnTrackEvent(key)
}

/// Held while [`on_track_event`]'s wake is: dropping it stops the wakes.
#[derive(Debug)]
pub struct OnTrackEvent(u64);

impl Drop for OnTrackEvent {
    fn drop(&mut self) {
        ON_EVENT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|(key, _)| *key != self.0);
    }
}

/// Wake whoever [`on_track_event`] asked to be.
#[cfg(feature = "native")]
#[cfg(feature = "native")]
fn track_event_arrived() {
    let wakes: Vec<_> = ON_EVENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|(_, wake)| std::sync::Arc::clone(wake))
        .collect();
    for wake in wakes {
        wake();
    }
}

impl Watch {
    /// Subscribe. `None` if the facade is not up.
    ///
    /// The browser: no thread to subscribe on, so a task on the page's
    /// event loop, as [`Meters::start`] does.
    #[cfg(not(feature = "native"))]
    #[must_use]
    pub fn start() -> Option<Self> {
        let daw = daw::rpc::Daw::try_get()?;
        let (tx, rx) = std::sync::mpsc::channel();
        let alive = Alive::new();
        let mine = alive.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _end = scopeguard(move || mine.ended());
            let Ok(project) = daw.current_project().await else {
                return;
            };
            let Ok(mut stream) = project.tracks().subscribe().await else {
                return;
            };
            while let Ok(Some(event)) = stream.recv().await {
                if tx.send(event.get().event.clone()).is_err() {
                    break;
                }
            }
        });
        Some(Self { changes: rx, alive })
    }

    /// Subscribe. `None` if the facade is not up.
    #[cfg(feature = "native")]
    #[must_use]
    pub fn start() -> Option<Self> {
        let runtime = crate::open::runtime()?;
        let (tx, rx) = std::sync::mpsc::channel();
        let alive = Alive::new();
        let mine = alive.clone();
        std::thread::Builder::new()
            .name("session-engine-watch".into())
            .spawn(move || {
                runtime.block_on(async move {
                    // Every exit from here marks the handle dead,
                    // including the ones that never got a stream at
                    // all: a subscribe that failed is a window with no
                    // corrections coming, which looks exactly like a
                    // project where nothing is happening.
                    let _end = scopeguard(move || mine.ended());
                    let Some(daw) = daw::rpc::Daw::try_get() else {
                        return;
                    };
                    let Ok(project) = daw.current_project().await else {
                        return;
                    };
                    let Ok(mut stream) = project.tracks().subscribe().await else {
                        return;
                    };
                    while let Ok(Some(event)) = stream.recv().await {
                        // A closed channel means the window has gone;
                        // there is nothing left to correct.
                        if tx.send(event.get().event.clone()).is_err() {
                            break;
                        }
                        track_event_arrived();
                    }
                });
            })
            .ok()?;
        Some(Self { changes: rx, alive })
    }

    /// Everything that has happened since the last frame.
    ///
    /// Drained rather than waited on: the window asks once per frame
    /// and applies whatever arrived. A frame that finds nothing has
    /// nothing to do, which is most of them.
    pub fn drain(&self) -> impl Iterator<Item = daw_proto::track::TrackEvent> + '_ {
        self.changes.try_iter()
    }

    /// Is this subscription still connected?
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.alive.is_live()
    }
}

/// Apply one engine event to the window's copy.
///
/// The engine is the authority, so this overwrites rather than merges —
/// including over a prediction that turned out wrong. A fader the user
/// is still dragging is the one exception a caller may want to make,
/// and it is the caller's to make because only it knows what is being
/// dragged.
/// The track a CONTINUOUS event is about — a volume or a pan.
///
/// These are the only edits a hand can still be making when the
/// engine's answer to an earlier one arrives, so they are the only
/// ones a caller may want to hold back while a drag is in flight.
/// Everything else is a toggle or a name: by the time the engine
/// answers, the gesture that caused it is over, and there is nothing
/// for its answer to fight with.
#[must_use]
pub fn continuous_for(event: &daw_proto::track::TrackEvent) -> Option<&str> {
    use daw_proto::track::TrackEvent as E;
    match event {
        E::VolumeChanged { guid, .. } | E::PanChanged { guid, .. } => Some(guid),
        _ => None,
    }
}

/// What an event changed.
///
/// Worth distinguishing because the two cost different things. A field
/// is a poke: the row it belongs to redraws and nothing else moves. The
/// LIST changing invalidates every row map, every offset and the scene
/// resolved against it, so the caller has to rebuild — and it needs
/// telling, because it cannot see inside this function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// One track's field. Redraw it.
    Field,
    /// The track list itself. Rebuild the rows.
    Structure,
    /// Nothing this window keeps. Most often an event for a field the
    /// window does not draw.
    Nothing,
}

/// Apply one event to the window's track list.
///
/// Takes the `Vec` rather than a slice, and that is the whole reason
/// structure events used to be dropped here: a slice cannot grow, so
/// `Added` had nowhere to go and was quietly ignored. A track created
/// in REAPER did not appear until the window was restarted, and nothing
/// said so.
pub fn apply_event(
    tracks: &mut Vec<daw_proto::Track>,
    event: &daw_proto::track::TrackEvent,
) -> Applied {
    use daw_proto::track::TrackEvent as E;
    let find = |tracks: &mut [daw_proto::Track], guid: &str| -> Option<usize> {
        tracks.iter().position(|t| t.guid == guid)
    };
    match event {
        E::Renamed { guid, name } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].name.clone_from(name);
            }
        }
        E::MuteChanged { guid, muted } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].muted = *muted;
            }
        }
        E::SoloChanged { guid, soloed } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].soloed = *soloed;
            }
        }
        E::ArmChanged { guid, armed } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].armed = *armed;
            }
        }
        E::SelectionChanged { guid, selected } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].selected = *selected;
            }
        }
        E::VolumeChanged { guid, volume } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].volume = *volume;
            }
        }
        E::PanChanged { guid, pan } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].pan = *pan;
            }
        }
        E::ColorChanged { guid, color } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].color = *color;
            }
        }
        E::PhaseInvertedChanged { guid, inverted } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].phase_inverted = *inverted;
            }
        }
        E::RouteCountsChanged {
            guid,
            send_count,
            receive_count,
        } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].send_count = *send_count;
                tracks[i].receive_count = *receive_count;
            }
        }
        E::FxCountChanged {
            guid,
            fx_count,
            input_fx_count,
        } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].fx_count = *fx_count;
                tracks[i].input_fx_count = *input_fx_count;
            }
        }
        // The fields this window WRITES and then never heard back
        // about. A prediction that is never confirmed is only right
        // while nothing rejects it: a parent send refused by the engine,
        // or toggled by somebody else in REAPER, stayed wrong here for
        // the life of the window with nothing to say so.
        E::ParentSendChanged { guid, enabled } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].parent_send = *enabled;
            }
        }
        E::InputMonitorChanged { guid, monitor } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].input_monitor = *monitor;
            }
        }
        E::RecordInputChanged { guid, input } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].record_input = *input;
            }
        }
        E::AutomationModeChanged { guid, mode } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].automation_mode = *mode;
            }
        }
        E::GroupingChanged { guid, grouping } => {
            if let Some(i) = find(tracks, guid) {
                tracks[i].grouping = grouping.clone();
            }
        }
        E::HeightChanged { guid, height } => {
            let Some(i) = find(tracks, guid) else {
                return Applied::Nothing;
            };
            tracks[i].height = *height;
            // A row's height is the LAYOUT, so every row below it moves
            // — the same reason a track appearing is structural. A
            // window that repainted only this row would leave every
            // other one drawn at its old offset.
            return Applied::Structure;
        }
        E::LanesChanged {
            guid,
            lane_count,
            lane_play_mask,
            lane_names,
            lane_display,
        } => {
            let Some(i) = find(tracks, guid) else {
                return Applied::Nothing;
            };
            let track = &mut tracks[i];
            track.lane_count = *lane_count;
            track.lane_play_mask = *lane_play_mask;
            track.lane_names.clone_from(lane_names);
            track.lane_display = *lane_display;
            // Lanes are rows too: turning them on gives a track three
            // times the height it had.
            return Applied::Structure;
        }
        E::FolderDepthChanged { guid, folder_depth } => {
            let Some(i) = find(tracks, guid) else {
                return Applied::Nothing;
            };
            tracks[i].folder_depth = *folder_depth;
            tracks[i].is_folder = *folder_depth > 0;
            // Depth is RELATIVE, so this one track's change re-parents
            // every track below it, and the parent guids this window
            // holds are now wrong for tracks it was told nothing about.
            // Only a re-read can fix that, which is what Structure asks
            // for — patching the tree from here would be guessing.
            return Applied::Structure;
        }
        // Visibility is the list as drawn rather than the list as
        // stored, so it re-derives the rows: hiding a track in REAPER
        // and leaving its row on screen is the same failure as leaving
        // a removed one there.
        E::TcpVisibilityChanged { guid, visible } => {
            let Some(i) = find(tracks, guid) else {
                return Applied::Nothing;
            };
            tracks[i].visible_in_tcp = *visible;
            return Applied::Structure;
        }
        E::MixerVisibilityChanged { guid, visible } => {
            let Some(i) = find(tracks, guid) else {
                return Applied::Nothing;
            };
            tracks[i].visible_in_mixer = *visible;
            return Applied::Structure;
        }
        // ── the list itself ──────────────────────────────────────
        E::Added(track) => {
            // Inserted at the track's own index rather than pushed:
            // REAPER numbers tracks by position, so a track added in the
            // middle would otherwise sit at the end and every row below
            // it would name the wrong track.
            let at = usize::try_from(track.index).unwrap_or(tracks.len());
            let at = at.min(tracks.len());
            tracks.insert(at, track.clone());
            reindex(tracks);
            return Applied::Structure;
        }
        E::Removed(guid) => {
            let Some(i) = find(tracks, guid) else {
                return Applied::Nothing;
            };
            tracks.remove(i);
            reindex(tracks);
            return Applied::Structure;
        }
        E::Moved {
            guid, new_index, ..
        } => {
            let Some(from) = find(tracks, guid) else {
                return Applied::Nothing;
            };
            let to = usize::try_from(*new_index)
                .unwrap_or(from)
                .min(tracks.len().saturating_sub(1));
            let track = tracks.remove(from);
            tracks.insert(to, track);
            reindex(tracks);
            return Applied::Structure;
        } // Deliberately exhaustive. The catch-all that used to sit here
          // meant a field added to the event upstream was dropped in
          // silence, which is exactly how five of the arms above went
          // missing for months. A new variant should break this build.
    }
    Applied::Field
}

/// Renumber after the list changed.
///
/// The index is a track's position, so every track below an insertion
/// or a removal has a new one. Leaving them stale is worse than it
/// sounds: the scene engine resolves rows by index, so one stale number
/// shows the wrong track at the right place — which reads as a
/// rendering bug rather than a bookkeeping one.
fn reindex(tracks: &mut [daw_proto::Track]) {
    for (at, track) in tracks.iter_mut().enumerate() {
        track.index = u32::try_from(at).unwrap_or(track.index);
    }
}

/// The engine's live meter levels.
///
/// A separate subscription from [`Watch`] because it is a different
/// KIND of fact. A track event is a change the window must not miss —
/// a mute it never heard about leaves the strip wrong until something
/// else happens. A meter frame is a measurement, and the next one is
/// thirty milliseconds away: missing one costs nothing, and queueing
/// them costs a backlog of readings that were true a second ago.
///
/// So this keeps the LATEST frame and drops the rest, where `Watch`
/// keeps every event in order. Same transport, opposite policy, and the
/// policy is the reason they are not one type.
pub struct Meters {
    latest: std::sync::Arc<std::sync::Mutex<Vec<daw_proto::TrackLevels>>>,
    alive: Alive,
}

impl Meters {
    /// Subscribe. `None` if the facade is not up, in which case the
    /// meters stay at rest rather than the page failing to open.
    ///
    /// The browser: no thread to subscribe on, so a task on the page's
    /// event loop, fed by the engine's meter pump (which the web audio
    /// loop's renders fill).
    #[cfg(not(feature = "native"))]
    #[must_use]
    pub fn start() -> Option<Self> {
        let daw = daw::rpc::Daw::try_get()?;
        let latest = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let into_task = std::sync::Arc::clone(&latest);
        let alive = Alive::new();
        let mine = alive.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _end = scopeguard(move || mine.ended());
            let Ok(project) = daw.current_project().await else {
                return;
            };
            let mut stream = project.meter_events();
            while let Ok(Some(frame)) = stream.recv().await {
                let frame = frame.get();
                let Ok(mut slot) = into_task.lock() else {
                    break;
                };
                slot.clear();
                slot.extend_from_slice(&frame.tracks);
            }
        });
        Some(Self { latest, alive })
    }

    /// Subscribe. `None` if the facade is not up, in which case the
    /// meters stay at rest rather than the window failing to open.
    #[cfg(feature = "native")]
    #[must_use]
    pub fn start() -> Option<Self> {
        let runtime = crate::open::runtime()?;
        let latest = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let into_thread = std::sync::Arc::clone(&latest);
        let alive = Alive::new();
        let mine = alive.clone();
        std::thread::Builder::new()
            .name("session-engine-meters".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let _end = scopeguard(move || mine.ended());
                    let Some(daw) = daw::rpc::Daw::try_get() else {
                        return;
                    };
                    // Every song's frames: the song on screen's drawn, the
                    // song changing under a running subscription (a tab
                    // picked, the set rolling on in Live).
                    let mut stream = daw.meter_events();
                    while let Ok(Some(frame)) = stream.recv().await {
                        let frame = frame.get();
                        if crate::open::current_song()
                            .is_some_and(|song| song != frame.project_guid)
                        {
                            continue;
                        }
                        // Overwrite rather than append: the only frame
                        // worth drawing is the one that just arrived.
                        let Ok(mut slot) = into_thread.lock() else {
                            break;
                        };
                        slot.clear();
                        slot.extend_from_slice(&frame.tracks);
                    }
                });
            })
            .ok()?;
        Some(Self { latest, alive })
    }

    /// Is this subscription still connected?
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.alive.is_live()
    }

    /// The most recent frame, copied out for this redraw.
    ///
    /// Copied rather than borrowed because the lock must not be held
    /// across a frame: the pump publishes at 30 Hz and a renderer
    /// holding its mutex would stall the engine's thread, which is the
    /// one thread in this program that has a deadline.
    #[must_use]
    pub fn levels(&self) -> Vec<daw_proto::TrackLevels> {
        self.latest
            .lock()
            .map(|frame| frame.clone())
            .unwrap_or_default()
    }
}

/// The tracks an engine publishes values for: their meters
/// ([`meter_value`]) and their mute and solo ([`track_value`]).
///
/// The first this many of the project, by index. Seven values a track
/// (five a meter, two the buttons), inside a shared value table's 1024
/// slots (`architect_scene::engine::MAX_SLOTS`); a track past them keeps
/// its meter and its buttons drawn into the panes' pictures.
pub const VALUE_TRACKS: usize = 146;

/// The tracks whose meters an engine publishes as values ([`meter_value`]):
/// [`VALUE_TRACKS`].
pub const METER_TRACKS: usize = VALUE_TRACKS;

/// The engine value a host lights track `index`'s `part` button by: 1
/// lit, 0 not.
///
/// `part` is one of [`TRACK_PARTS`]. The `daw` extension writes them as
/// the engine's track events say, in the engine's own process, so every
/// pane that draws the button from it (the arrangement's track panel and
/// the mixer, as Architect panes) lights it in the same frame.
#[must_use]
pub fn track_value(index: usize, part: &str) -> String {
    format!("engine.track.{index}.{part}")
}

/// The buttons [`track_value`] names.
pub const TRACK_PARTS: [&str; 2] = ["mute", "solo"];

/// The engine value a host moves track `index`'s meter by.
///
/// `part` is `pl` and `pr` (the channels' peaks), `hl` and `hr` (their holds), and `pk`
/// (the louder peak, which lights the scale), each [`meter_fraction`] of
/// its level. The `daw` extension writes them; a mixer pane in an
/// Architect host draws its meters from them (`studio.mixer`), so a
/// playing meter costs no frame.
#[must_use]
pub fn meter_value(index: usize, part: &str) -> String {
    format!("engine.meter.{index}.{part}")
}

/// The parts of a meter [`meter_value`] names.
pub const METER_PARTS: [&str; 5] = ["pl", "pr", "hl", "hr", "pk"];

/// A meter's height, from a linear peak.
///
/// The meter is drawn from a fraction of its own height, so the level
/// has to be mapped the way an ear reads it rather than the way the
/// sample buffer holds it: linear `0..1` puts −20 dBFS — a perfectly
/// ordinary track — at two per cent of the well, which reads as
/// silence.
///
/// The same scale the fader uses, so a track sitting at unity with a
/// hot signal lights the meter to about where its own cap sits. That
/// correspondence is what makes a mixer readable at a glance.
#[must_use]
pub fn meter_fraction(peak: f32) -> f64 {
    let db = 20.0 * f64::from(peak.max(1e-6)).log10();
    // The METER's scale, which runs past unity: a signal over 0 dBFS
    // has to land above the 0 mark rather than pinning to the same full
    // bar a clean −1 draws.
    crate::scale::meter_norm(db)
}

/// Run a closure when the scope ends, however it ends.
///
/// Every stream loop here has several ways out — no facade, no project,
/// a refused subscribe, a closed stream — and each of them has to mark
/// the handle dead. Writing that at four exits is writing it at three
/// and forgetting the fourth, which in this file means a window that
/// believes it is connected.
fn scopeguard<F: FnOnce()>(f: F) -> impl Drop {
    struct Guard<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for Guard<F> {
        fn drop(&mut self) {
            if let Some(f) = self.0.take() {
                f();
            }
        }
    }
    Guard(Some(f))
}

/// Whether a subscription is still connected.
///
/// Every stream in this file ends the same way: REAPER quits, the
/// socket closes, `recv` returns `None` and the thread falls out of its
/// loop. Nothing used to notice. The window kept its handle, the
/// handle's restart check only fires when the handle is *missing*, and
/// so a REAPER quit and reopened left a window drawing the last thing
/// it heard — connected in appearance and dead in fact, which is the
/// failure a mirror must never have.
///
/// Shared rather than returned, because the thread is the only one that
/// knows and the window is the only one that can act.
#[derive(Clone)]
pub struct Alive(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Alive {
    fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            true,
        )))
    }

    fn ended(&self) {
        self.0.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// Is the stream behind this handle still delivering?
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Everything in the project that is not a track.
///
/// Items, takes, markers, regions and the tempo map are all read as a
/// snapshot and drawn from it, and until now nothing told the window
/// they had moved. An item dragged in REAPER stayed where it used to be
/// until some unrelated track was added or removed and the resulting
/// re-read happened to bring it along — which is a mirror that is right
/// only by accident.
///
/// It is a FLAG rather than a queue of events, and that is the whole
/// design. The window does not keep items, takes, markers or the tempo
/// map as anything it could patch: they are derived, in one read, from
/// the project. So the useful thing to know is "the snapshot is old",
/// and knowing it twice is the same as knowing it once — which is
/// exactly what a flag says and a queue does not. A drag in REAPER
/// emits an event per frame and this coalesces every one of them into
/// a single re-read.
///
/// Tracks are the exception and keep their own [`Watch`], because the
/// window DOES hold them and can correct a single field without paying
/// for a whole read.
pub struct Refresh {
    stale: std::sync::Arc<std::sync::atomic::AtomicBool>,
    alive: Alive,
}

impl Refresh {
    /// Subscribe. `None` if the facade is not up.
    #[must_use]
    pub fn start() -> Option<Self> {
        use daw_proto::event_bus::BusFilter;

        let runtime = crate::open::runtime()?;
        let stale = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer = std::sync::Arc::clone(&stale);
        let alive = Alive::new();
        let mine = alive.clone();
        std::thread::Builder::new()
            .name("session-engine-refresh".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let _end = scopeguard(move || mine.ended());
                    let Some(daw) = daw::rpc::Daw::try_get() else {
                        return;
                    };
                    // Named field by field rather than `all()` minus a
                    // couple, because every domain switched on here
                    // costs a whole project read. Tracks are out
                    // because `Watch` has them and patches them in
                    // place. The position tick and the play state are
                    // out because they change constantly and change
                    // nothing that is drawn from the snapshot. FX and
                    // routing are out because nothing here draws
                    // them — an automated parameter would otherwise
                    // re-read the session at audio rate.
                    let filter = BusFilter {
                        items: true,
                        takes: true,
                        markers: true,
                        regions: true,
                        tempo_map: true,
                        projects: true,
                        tracks: false,
                        fx: false,
                        routing: false,
                        transport_state: false,
                        transport_position: false,
                        project_guid: None,
                    };
                    let Ok(mut stream) = daw.events().subscribe(filter).await else {
                        return;
                    };
                    while let Ok(Some(_)) = stream.recv().await {
                        writer.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                });
            })
            .ok()?;
        Some(Self { stale, alive })
    }

    /// Is this subscription still connected?
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.alive.is_live()
    }

    /// Is the snapshot old?
    ///
    /// Asking does not clear it. Reading and clearing in one step reads
    /// better and is wrong here: a caller that cannot act yet — because
    /// the last read is still in flight — would throw away the news
    /// that arrived while it was reading, and the last edit of a drag
    /// is exactly the one that would go missing.
    #[must_use]
    pub fn pending(&self) -> bool {
        self.stale.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Say the snapshot has been re-read. Call this only when a read
    /// actually started.
    pub fn settled(&self) {
        self.stale
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
