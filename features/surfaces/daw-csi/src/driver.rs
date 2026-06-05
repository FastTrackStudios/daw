//! The surface driver: event-bus feedback out, surface gestures in.
//!
//! Unlike CSI (which polls REAPER ~50×/sec because REAPER has no push
//! API), this driver is event-driven: the daw event bus pushes every
//! state change, the driver diffs against the [`Shadow`] and sends
//! only real updates to the surface.
//!
//! Dispatch is **zone-driven** (see [`crate::zones`]): a gesture
//! resolves through the active zone's bindings to an [`Action`],
//! which produces [`Intent`]s (pure, unit-testable) that the async
//! edge executes against the `daw-control` services. Feedback walks
//! the same bindings in reverse — a fader bound to `@TrackVolume`
//! gets volume motor feedback, an LCD row bound to `@TrackName`
//! shows the name.

use daw_control::Daw;
use daw_proto::event_bus::{BusFilter, DawEvent};
use daw_proto::track::TrackEvent;
use daw_proto::transport::TransportEvent;
use daw_proto::{PlayState, Track};

use crate::action::Action;
use crate::mcu::{self, Button, RingMode, StripColor, SurfaceInput};
use crate::midi::SurfacePort;
use crate::navigator::{NavMode, Navigator};
use crate::shadow::Shadow;
use crate::taper;
use crate::zones::{ALT, CONTROL, GlobalWidget, Modifiers, OPTION, SHIFT, StripWidget, ZoneSet};

/// Surface driver configuration.
#[derive(Debug, Clone)]
pub struct CsiConfig {
    /// Case-insensitive substring matched against MIDI port names.
    pub device_match: String,
}

impl Default for CsiConfig {
    fn default() -> Self {
        Self {
            device_match: "x-touch".into(),
        }
    }
}

/// What a decoded gesture wants done. Pure data so gesture handling
/// is testable without services or hardware.
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    SetVolume {
        guid: String,
        volume: f64,
    },
    SetMasterVolume {
        volume: f64,
    },
    SetPan {
        guid: String,
        pan: f64,
    },
    SetMuted {
        guid: String,
        muted: bool,
    },
    SetSoloed {
        guid: String,
        soloed: bool,
    },
    SetArmed {
        guid: String,
        armed: bool,
    },
    SelectExclusive {
        guid: String,
    },
    Play,
    Stop,
    Record,
    StopRecording,
    ToggleLoop,
    NudgePosition {
        seconds: f64,
    },
    /// Navigation already applied to the navigator / zone state —
    /// re-resolve strips and refresh the surface.
    Refresh,
}

/// The analog payload of a gesture, passed to [`Action`] handling.
#[derive(Debug, Clone, Copy)]
enum Gesture {
    /// Button-like: press (releases are filtered before dispatch).
    Press,
    /// Fader move, 14-bit position.
    Fader(u16),
    /// Relative encoder, signed ticks.
    Delta(i8),
}

/// Everything the gesture/feedback logic reads and mutates. No I/O —
/// the async edge owns the port and services.
pub struct DriverState {
    pub nav: Navigator,
    pub shadow: Shadow,
    pub zones: ZoneSet,
    /// Name of the active zone (always resolvable in `zones`).
    pub active_zone: String,
    /// Flat track cache, index == project order.
    pub tracks: Vec<Track>,
    /// Current strip → track-cache index mapping.
    strips: Vec<Option<usize>>,
    pub master_guid: String,
    pub master_volume: f64,
    pub play_state: PlayState,
    pub looping: bool,
    /// Fader touch sensors — while touched, motor feedback for that
    /// strip is suppressed (no fader fights).
    touched: [bool; 9],
    /// Live modifier-key mask (Shift/Option/Control/Alt).
    modifiers: Modifiers,
}

impl DriverState {
    pub fn new(
        zones: ZoneSet,
        tracks: Vec<Track>,
        master_guid: String,
        master_volume: f64,
    ) -> Self {
        let active_zone = zones.home.clone();
        let mut s = Self {
            nav: Navigator::default(),
            shadow: Shadow::default(),
            zones,
            active_zone,
            tracks,
            strips: vec![None; mcu::STRIPS],
            master_guid,
            master_volume,
            play_state: PlayState::Stopped,
            looping: false,
            touched: [false; 9],
            modifiers: 0,
        };
        s.enter_zone(&s.zones.home.clone());
        s.rebind_strips();
        s
    }

    /// Convenience for tests / embedded default behavior.
    pub fn with_builtin_zones(tracks: Vec<Track>, master_guid: String, master_volume: f64) -> Self {
        Self::new(ZoneSet::builtin(), tracks, master_guid, master_volume)
    }

    /// Re-resolve which track sits on each strip (after banking,
    /// folder navigation, or track list changes).
    pub fn rebind_strips(&mut self) {
        self.strips = self.nav.visible(&self.tracks, mcu::STRIPS);
    }

    fn strip_track(&self, strip: u8) -> Option<&Track> {
        self.strips
            .get(strip as usize)
            .copied()
            .flatten()
            .and_then(|i| self.tracks.get(i))
    }

    /// Activate a zone: pin its navigator mode and rebind.
    fn enter_zone(&mut self, name: &str) {
        let Some(zone) = self.zones.zone(name) else {
            tracing::warn!("csi: @GoZone to unknown zone '{name}'");
            return;
        };
        let nav_mode = zone.navigator;
        self.active_zone = name.to_string();
        if let Some(mode) = nav_mode {
            self.nav.set_mode(mode);
        }
        self.rebind_strips();
    }

    // ── Gestures: surface → intents ─────────────────────────────────

    /// Decode raw MIDI from the surface, resolve it through the
    /// active zone, and return the intents to execute. Navigation
    /// actions mutate local state here and yield `Refresh`.
    pub fn handle_midi(&mut self, raw: &[u8]) -> Vec<Intent> {
        let Some(input) = mcu::decode(raw) else {
            return Vec::new();
        };
        match input {
            SurfaceInput::Fader { strip, pos } => {
                if strip == mcu::MASTER {
                    self.dispatch_global(GlobalWidget::MasterFader, Gesture::Fader(pos))
                } else {
                    self.dispatch_strip(strip, StripWidget::Fader, Gesture::Fader(pos))
                }
            }
            SurfaceInput::FaderTouch { strip, touched } => {
                // Touch isn't bindable — it's motor-feedback
                // suppression infrastructure.
                if let Some(cell) = self.touched.get_mut(strip as usize) {
                    *cell = touched;
                }
                Vec::new()
            }
            SurfaceInput::VPot { strip, delta } => {
                self.dispatch_strip(strip, StripWidget::VPot, Gesture::Delta(delta))
            }
            SurfaceInput::VPotPress { strip, pressed } => {
                if !pressed {
                    return Vec::new();
                }
                self.dispatch_strip(strip, StripWidget::VPotPress, Gesture::Press)
            }
            SurfaceInput::Jog { delta } => {
                self.dispatch_global(GlobalWidget::Jog, Gesture::Delta(delta))
            }
            SurfaceInput::Button { button, pressed } => {
                // Modifier keys update the mask and are never bindable.
                let modifier = match button {
                    Button::Shift => Some(SHIFT),
                    Button::Option => Some(OPTION),
                    Button::Control => Some(CONTROL),
                    Button::Alt => Some(ALT),
                    _ => None,
                };
                if let Some(bit) = modifier {
                    if pressed {
                        self.modifiers |= bit;
                    } else {
                        self.modifiers &= !bit;
                    }
                    return Vec::new();
                }
                if !pressed {
                    return Vec::new();
                }
                match button {
                    Button::Rec(s) => self.dispatch_strip(s, StripWidget::Rec, Gesture::Press),
                    Button::Solo(s) => self.dispatch_strip(s, StripWidget::Solo, Gesture::Press),
                    Button::Mute(s) => self.dispatch_strip(s, StripWidget::Mute, Gesture::Press),
                    Button::Select(s) => {
                        self.dispatch_strip(s, StripWidget::Select, Gesture::Press)
                    }
                    other => self.dispatch_global(GlobalWidget::Button(other), Gesture::Press),
                }
            }
        }
    }

    fn dispatch_strip(&mut self, strip: u8, widget: StripWidget, gesture: Gesture) -> Vec<Intent> {
        let Some(zone) = self.zones.zone(&self.active_zone) else {
            return Vec::new();
        };
        let Some(action) = zone.strip_action(self.modifiers, widget).cloned() else {
            return Vec::new();
        };
        self.apply_action(&action, Some(strip), gesture)
    }

    fn dispatch_global(&mut self, widget: GlobalWidget, gesture: Gesture) -> Vec<Intent> {
        let Some(zone) = self.zones.zone(&self.active_zone) else {
            return Vec::new();
        };
        let Some(action) = zone.global_action(self.modifiers, widget).cloned() else {
            return Vec::new();
        };
        self.apply_action(&action, None, gesture)
    }

    /// Turn a bound action + gesture into intents. `strip` is `Some`
    /// for strip-context widgets.
    fn apply_action(
        &mut self,
        action: &Action,
        strip: Option<u8>,
        gesture: Gesture,
    ) -> Vec<Intent> {
        let strip_guid = |s: &Self| strip.and_then(|i| s.strip_track(i)).map(|t| t.guid.clone());
        match action {
            Action::TrackVolume => {
                let Gesture::Fader(pos) = gesture else {
                    return Vec::new();
                };
                let Some(guid) = strip_guid(self) else {
                    return Vec::new();
                };
                let volume = taper::fader_to_volume(pos);
                // Update the cache now so the echo event diffs clean.
                if let Some(t) = self.tracks.iter_mut().find(|t| t.guid == guid) {
                    t.volume = volume;
                }
                vec![Intent::SetVolume { guid, volume }]
            }
            Action::TrackPan => {
                let Gesture::Delta(delta) = gesture else {
                    return Vec::new();
                };
                let Some(guid) = strip_guid(self) else {
                    return Vec::new();
                };
                // Shift = fine adjustment (CSI convention).
                let step = if self.modifiers & SHIFT != 0 {
                    0.005
                } else {
                    0.02
                };
                let current = self
                    .tracks
                    .iter()
                    .find(|t| t.guid == guid)
                    .map(|t| t.pan)
                    .unwrap_or(0.0);
                let pan = (current + delta as f64 * step).clamp(-1.0, 1.0);
                if let Some(t) = self.tracks.iter_mut().find(|t| t.guid == guid) {
                    t.pan = pan;
                }
                vec![Intent::SetPan { guid, pan }]
            }
            Action::TrackPanReset => {
                let Some(guid) = strip_guid(self) else {
                    return Vec::new();
                };
                if let Some(t) = self.tracks.iter_mut().find(|t| t.guid == guid) {
                    t.pan = 0.0;
                }
                vec![Intent::SetPan { guid, pan: 0.0 }]
            }
            Action::TrackMute => self.toggle_flag(
                strip,
                |t| t.muted,
                |guid, v| Intent::SetMuted { guid, muted: v },
            ),
            Action::TrackSolo => self.toggle_flag(
                strip,
                |t| t.soloed,
                |guid, v| Intent::SetSoloed { guid, soloed: v },
            ),
            Action::TrackRecordArm => self.toggle_flag(
                strip,
                |t| t.armed,
                |guid, v| Intent::SetArmed { guid, armed: v },
            ),
            Action::TrackSelect => strip_guid(self)
                .map(|guid| Intent::SelectExclusive { guid })
                .into_iter()
                .collect(),
            Action::FolderSpill => {
                let Some(t) = strip.and_then(|s| self.strip_track(s)).cloned() else {
                    return Vec::new();
                };
                // Folders drill/pop; plain tracks fall through to
                // selection (CSI's ToggleFolderSpill).
                if self.nav.folder_select(&t) {
                    self.rebind_strips();
                    return vec![Intent::Refresh];
                }
                vec![Intent::SelectExclusive { guid: t.guid }]
            }
            Action::MasterVolume => {
                let Gesture::Fader(pos) = gesture else {
                    return Vec::new();
                };
                let volume = taper::fader_to_volume(pos);
                self.master_volume = volume;
                vec![Intent::SetMasterVolume { volume }]
            }
            Action::Play => vec![Intent::Play],
            Action::Stop => vec![Intent::Stop],
            Action::Record => {
                if matches!(self.play_state, PlayState::Recording) {
                    vec![Intent::StopRecording]
                } else {
                    vec![Intent::Record]
                }
            }
            Action::ToggleLoop => vec![Intent::ToggleLoop],
            Action::NudgePosition { seconds } => vec![Intent::NudgePosition { seconds: *seconds }],
            Action::JogPosition { seconds_per_tick } => {
                let Gesture::Delta(delta) = gesture else {
                    return Vec::new();
                };
                let fine = if self.modifiers & SHIFT != 0 {
                    0.1
                } else {
                    1.0
                };
                vec![Intent::NudgePosition {
                    seconds: delta as f64 * seconds_per_tick * fine,
                }]
            }
            Action::Bank { amount } => {
                self.nav.bank(*amount as isize, &self.tracks, mcu::STRIPS);
                self.rebind_strips();
                vec![Intent::Refresh]
            }
            Action::GoZone { zone } => {
                self.enter_zone(&zone.clone());
                vec![Intent::Refresh]
            }
            // Display actions are feedback-only; binding one to an
            // input widget is inert.
            Action::TrackName
            | Action::PanDisplay
            | Action::VolumeDisplay
            | Action::FolderIndicator
            | Action::Fixed { .. }
            | Action::Blank
            | Action::NoAction => Vec::new(),
        }
    }

    fn toggle_flag(
        &self,
        strip: Option<u8>,
        read: impl Fn(&Track) -> bool,
        make: impl Fn(String, bool) -> Intent,
    ) -> Vec<Intent> {
        strip
            .and_then(|s| self.strip_track(s))
            .map(|t| make(t.guid.clone(), !read(t)))
            .into_iter()
            .collect()
    }

    // ── Feedback: events → surface messages ─────────────────────────

    /// Apply one track event to the cache. Returns `true` when the
    /// track LIST changed shape (added/removed/moved) — the caller
    /// must refetch the list and do a full refresh.
    pub fn apply_track_event(&mut self, event: &TrackEvent) -> bool {
        let by_guid = |tracks: &mut Vec<Track>, guid: &str| -> Option<usize> {
            tracks.iter().position(|t| t.guid == guid)
        };
        match event {
            TrackEvent::Added(_) | TrackEvent::Removed(_) | TrackEvent::Moved { .. } => {
                return true;
            }
            TrackEvent::VolumeChanged { guid, volume } => {
                if guid == &self.master_guid {
                    self.master_volume = *volume;
                } else if let Some(i) = by_guid(&mut self.tracks, guid) {
                    self.tracks[i].volume = *volume;
                }
            }
            TrackEvent::PanChanged { guid, pan } => {
                if let Some(i) = by_guid(&mut self.tracks, guid) {
                    self.tracks[i].pan = *pan;
                }
            }
            TrackEvent::MuteChanged { guid, muted } => {
                if let Some(i) = by_guid(&mut self.tracks, guid) {
                    self.tracks[i].muted = *muted;
                }
            }
            TrackEvent::SoloChanged { guid, soloed } => {
                if let Some(i) = by_guid(&mut self.tracks, guid) {
                    self.tracks[i].soloed = *soloed;
                }
            }
            TrackEvent::ArmChanged { guid, armed } => {
                if let Some(i) = by_guid(&mut self.tracks, guid) {
                    self.tracks[i].armed = *armed;
                }
            }
            TrackEvent::SelectionChanged { guid, selected } => {
                if let Some(i) = by_guid(&mut self.tracks, guid) {
                    self.tracks[i].selected = *selected;
                }
            }
            TrackEvent::Renamed { guid, name } => {
                if let Some(i) = by_guid(&mut self.tracks, guid) {
                    self.tracks[i].name = name.clone();
                }
            }
            TrackEvent::ColorChanged { guid, color } => {
                if let Some(i) = by_guid(&mut self.tracks, guid) {
                    self.tracks[i].color = *color;
                }
            }
            _ => {}
        }
        false
    }

    pub fn apply_transport_event(&mut self, event: &TransportEvent) {
        match event {
            TransportEvent::PlayStateChanged { play_state, .. } => {
                self.play_state = *play_state;
            }
            TransportEvent::LoopingChanged { looping, .. } => {
                self.looping = *looping;
            }
            _ => {}
        }
    }

    /// The feedback text for a display action on a strip.
    fn display_text(&self, action: &Action, track: Option<&Track>) -> String {
        let Some(t) = track else {
            return match action {
                Action::Fixed { text } => text.clone(),
                _ => String::new(),
            };
        };
        match action {
            Action::TrackName => t.name.clone(),
            Action::PanDisplay => pan_label(t.pan),
            Action::VolumeDisplay => volume_label(t.volume),
            Action::FolderIndicator => {
                if t.is_folder && self.nav.mode == NavMode::Folder {
                    format!("FOLDR {}", ">".repeat(self.nav.depth().min(1)))
                } else {
                    pan_label(t.pan)
                }
            }
            Action::Fixed { text } => text.clone(),
            _ => String::new(),
        }
    }

    /// The LED state for an action bound to a lit button.
    fn led_state(&self, action: &Action, track: Option<&Track>) -> bool {
        match action {
            Action::TrackMute => track.map(|t| t.muted).unwrap_or(false),
            Action::TrackSolo => track.map(|t| t.soloed).unwrap_or(false),
            Action::TrackRecordArm => track.map(|t| t.armed).unwrap_or(false),
            Action::TrackSelect | Action::FolderSpill => track.map(|t| t.selected).unwrap_or(false),
            Action::Play => matches!(self.play_state, PlayState::Playing | PlayState::Recording),
            Action::Record => matches!(self.play_state, PlayState::Recording),
            Action::Stop => matches!(self.play_state, PlayState::Stopped | PlayState::Paused),
            Action::ToggleLoop => self.looping,
            Action::GoZone { zone } => &self.active_zone == zone,
            _ => false,
        }
    }

    /// Render the full surface state through the shadow, walking the
    /// active zone's bindings. Returns the MIDI messages that
    /// actually need sending.
    pub fn render(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let Some(zone) = self.zones.zone(&self.active_zone).cloned() else {
            return out;
        };
        let mut colors = [StripColor::Off; mcu::STRIPS];

        for strip in 0..mcu::STRIPS as u8 {
            let track = self.strip_track(strip).cloned();

            // Motor fader follows the fader binding's value.
            if !self.touched[strip as usize] {
                let pos = match zone.strip_action(0, StripWidget::Fader) {
                    Some(Action::TrackVolume) => track
                        .as_ref()
                        .map(|t| taper::volume_to_fader(t.volume))
                        .unwrap_or(0),
                    _ => 0,
                };
                self.shadow.fader(&mut out, strip, pos);
            }

            // V-pot ring follows the vpot binding.
            match (zone.strip_action(0, StripWidget::VPot), &track) {
                (Some(Action::TrackPan), Some(t)) => self.shadow.ring(
                    &mut out,
                    strip,
                    RingMode::BoostCut,
                    mcu::pan_to_ring(t.pan),
                    false,
                ),
                _ => self
                    .shadow
                    .ring(&mut out, strip, RingMode::SingleDot, 0, false),
            }

            // Strip button LEDs follow their bound action's state.
            for (widget, button) in [
                (StripWidget::Rec, Button::Rec(strip)),
                (StripWidget::Solo, Button::Solo(strip)),
                (StripWidget::Mute, Button::Mute(strip)),
                (StripWidget::Select, Button::Select(strip)),
            ] {
                let lit = zone
                    .strip_action(0, widget)
                    .map(|a| self.led_state(a, track.as_ref()))
                    .unwrap_or(false);
                self.shadow.led(&mut out, button, lit);
            }

            // LCD rows.
            for (widget, row) in [(StripWidget::LcdTop, 0u8), (StripWidget::LcdBottom, 1u8)] {
                let text = zone
                    .strip_action(0, widget)
                    .map(|a| self.display_text(a, track.as_ref()))
                    .unwrap_or_default();
                self.shadow.lcd(&mut out, strip, row, &text);
            }

            colors[strip as usize] = track
                .as_ref()
                .map(|t| {
                    t.color
                        .map(mcu::rgb_to_strip_color)
                        .unwrap_or(StripColor::White)
                })
                .unwrap_or(StripColor::Off);
        }
        self.shadow.colors(&mut out, colors);

        // Master fader.
        if !self.touched[mcu::MASTER as usize] {
            let pos = match zone.global_action(0, GlobalWidget::MasterFader) {
                Some(Action::MasterVolume) => taper::volume_to_fader(self.master_volume),
                _ => 0,
            };
            self.shadow.fader(&mut out, mcu::MASTER, pos);
        }

        // Master-section button LEDs follow their bound actions.
        for ((_, widget), action) in zone.global.iter().map(|(k, v)| (*k, v)) {
            if let GlobalWidget::Button(button) = widget {
                let lit = self.led_state(action, None);
                self.shadow.led(&mut out, button, lit);
            }
        }
        out
    }
}

fn pan_label(pan: f64) -> String {
    if pan.abs() < 0.01 {
        "  C".into()
    } else if pan < 0.0 {
        format!("{:.0}L", pan.abs() * 100.0)
    } else {
        format!("{:.0}R", pan * 100.0)
    }
}

fn volume_label(volume: f64) -> String {
    if volume <= 0.0 {
        "-inf".into()
    } else {
        format!("{:+.1}dB", 20.0 * volume.log10())
    }
}

// ── Async edge: services + hardware ─────────────────────────────────

/// Move the next event out of a vox stream (local copy of the daw
/// facade's `RxExt::next_owned` — daw-csi can't depend on `daw`, the
/// facade depends on us).
async fn next_owned<T>(rx: &mut vox::Rx<T>) -> eyre::Result<Option<T>>
where
    T: facet::Facet<'static> + 'static,
{
    match rx.recv().await {
        Ok(Some(selfref)) => {
            let mut taken: Option<T> = None;
            let _ = selfref.map(|value| {
                taken = Some(value);
            });
            Ok(taken)
        }
        Ok(None) => Ok(None),
        Err(e) => Err(eyre::eyre!("event stream error: {e:?}")),
    }
}

/// Run the surface driver until the event stream closes or the
/// surface disconnects. Spawn with `moire::task::spawn`.
pub async fn run(daw: Daw, config: CsiConfig) -> eyre::Result<()> {
    let mut port = SurfacePort::open(&config.device_match)?;
    let zones = ZoneSet::load()?;

    let project = daw.current_project().await?;
    let tracks_api = project.tracks();
    let transport = project.transport();

    let master = tracks_api.master().await?;
    let master_guid = master.guid().to_string();
    let master_volume = master.volume().await.unwrap_or(1.0);
    let tracks = tracks_api.all().await?;

    let mut state = DriverState::new(zones, tracks, master_guid, master_volume);
    state.play_state = transport.get_play_state().await.unwrap_or_default();
    state.looping = transport.is_looping().await.unwrap_or(false);
    state.shadow.invalidate();
    for msg in state.render() {
        port.send(&msg);
    }

    let mut bus = daw
        .events()
        .subscribe(BusFilter {
            tracks: true,
            transport_state: true,
            projects: true,
            ..Default::default()
        })
        .await?;

    loop {
        tokio::select! {
            ev = next_owned(&mut bus) => {
                match ev? {
                    None => return Ok(()), // bus closed — host shutting down
                    Some(DawEvent::Track(te)) => {
                        if state.apply_track_event(&te.event) {
                            // List shape changed — refetch + revalidate.
                            state.tracks = tracks_api.all().await?;
                            state.nav.revalidate(&state.tracks);
                            state.rebind_strips();
                        }
                    }
                    Some(DawEvent::TransportState(te)) => state.apply_transport_event(&te),
                    Some(_) => {}
                }
                for msg in state.render() {
                    port.send(&msg);
                }
            }
            raw = port.rx.recv() => {
                let Some(raw) = raw else {
                    return Err(eyre::eyre!("surface MIDI input closed"));
                };
                let intents = state.handle_midi(&raw);
                for intent in intents {
                    if let Err(e) = execute_intent(&project, &transport, intent).await {
                        tracing::warn!("daw-csi: intent failed: {e}");
                    }
                }
                for msg in state.render() {
                    port.send(&msg);
                }
            }
        }
    }
}

/// Execute one [`Intent`] against the daw-control services. Public so
/// headless surfaces / tests can drive the same path as the hardware
/// loop.
pub async fn execute_intent(
    project: &daw_control::Project,
    transport: &daw_control::Transport,
    intent: Intent,
) -> eyre::Result<()> {
    match intent {
        Intent::SetVolume { guid, volume } => {
            if let Some(h) = project.tracks().by_guid(&guid).await? {
                h.set_volume(volume).await?;
            }
        }
        Intent::SetMasterVolume { volume } => {
            project.tracks().master().await?.set_volume(volume).await?;
        }
        Intent::SetPan { guid, pan } => {
            if let Some(h) = project.tracks().by_guid(&guid).await? {
                h.set_pan(pan).await?;
            }
        }
        Intent::SetMuted { guid, muted } => {
            if let Some(h) = project.tracks().by_guid(&guid).await? {
                if muted {
                    h.mute().await?
                } else {
                    h.unmute().await?
                }
            }
        }
        Intent::SetSoloed { guid, soloed } => {
            if let Some(h) = project.tracks().by_guid(&guid).await? {
                if soloed {
                    h.solo().await?
                } else {
                    h.unsolo().await?
                }
            }
        }
        Intent::SetArmed { guid, armed } => {
            if let Some(h) = project.tracks().by_guid(&guid).await? {
                if armed {
                    h.arm().await?
                } else {
                    h.disarm().await?
                }
            }
        }
        Intent::SelectExclusive { guid } => {
            if let Some(h) = project.tracks().by_guid(&guid).await? {
                h.select_exclusive().await?;
            }
        }
        Intent::Play => transport.play().await?,
        Intent::Stop => transport.stop().await?,
        Intent::Record => transport.record().await?,
        Intent::StopRecording => transport.stop_recording().await?,
        Intent::ToggleLoop => transport.toggle_loop().await?,
        Intent::NudgePosition { seconds } => {
            let pos = transport.get_position().await?;
            transport.set_position((pos + seconds).max(0.0)).await?;
        }
        // Navigation already applied; the caller renders after the
        // intent batch.
        Intent::Refresh => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(guid: &str, name: &str, parent: Option<&str>, is_folder: bool) -> Track {
        Track {
            guid: guid.into(),
            name: name.into(),
            volume: 1.0,
            parent_guid: parent.map(Into::into),
            is_folder,
            ..Default::default()
        }
    }

    fn state() -> DriverState {
        DriverState::with_builtin_zones(
            vec![
                track("drums", "DRUMS", None, true),
                track("kick", "Kick", Some("drums"), false),
                track("snare", "Snare", Some("drums"), false),
                track("bass", "Bass", None, false),
            ],
            "master".into(),
            1.0,
        )
    }

    #[test]
    fn fader_gesture_sets_volume_and_caches() {
        let mut s = state();
        let raw = mcu::encode_fader(0, taper::volume_to_fader(1.0));
        let intents = s.handle_midi(&raw);
        assert_eq!(intents.len(), 1);
        let Intent::SetVolume { guid, volume } = &intents[0] else {
            panic!("expected SetVolume, got {intents:?}");
        };
        assert_eq!(guid, "drums");
        assert!((volume - 1.0).abs() < 1e-3);
        // Cache updated immediately → echo event diffs clean.
        assert!((s.tracks[0].volume - 1.0).abs() < 1e-3);
    }

    #[test]
    fn master_fader_routes_to_master() {
        let mut s = state();
        let raw = mcu::encode_fader(mcu::MASTER, 0);
        assert_eq!(
            s.handle_midi(&raw),
            vec![Intent::SetMasterVolume { volume: 0.0 }]
        );
    }

    #[test]
    fn mute_button_toggles_from_cache() {
        let mut s = state();
        let press = [0x90, 0x10, 0x7F]; // Mute strip 0
        assert_eq!(
            s.handle_midi(&press),
            vec![Intent::SetMuted {
                guid: "drums".into(),
                muted: true
            }]
        );
        // Releases are ignored.
        assert!(s.handle_midi(&[0x90, 0x10, 0x00]).is_empty());
        // After the echo event lands, the next press unmutes.
        s.apply_track_event(&TrackEvent::MuteChanged {
            guid: "drums".into(),
            muted: true,
        });
        assert_eq!(
            s.handle_midi(&press),
            vec![Intent::SetMuted {
                guid: "drums".into(),
                muted: false
            }]
        );
    }

    #[test]
    fn zone_hop_changes_select_semantics() {
        let mut s = state();
        assert_eq!(s.active_zone, "home");
        // GlobalView is bound to @GoZone{zone folder} in the builtin set.
        assert_eq!(s.handle_midi(&[0x90, 0x33, 0x7F]), vec![Intent::Refresh]);
        assert_eq!(s.active_zone, "folder");
        assert_eq!(s.nav.mode, NavMode::Folder);
        // Root: [DRUMS, Bass]. Select strip 0 (DRUMS folder) → drill.
        assert_eq!(s.handle_midi(&[0x90, 0x18, 0x7F]), vec![Intent::Refresh]);
        // Spill: [DRUMS, Kick, Snare] — strip 1 is Kick now; selecting
        // it is a plain selection, not navigation.
        assert_eq!(
            s.handle_midi(&[0x90, 0x19, 0x7F]),
            vec![Intent::SelectExclusive {
                guid: "kick".into()
            }]
        );
        // Select DRUMS (strip 0, current spill parent) again → pop.
        assert_eq!(s.handle_midi(&[0x90, 0x18, 0x7F]), vec![Intent::Refresh]);
        assert_eq!(s.nav.depth(), 0);
        // GlobalView in the folder zone goes home again.
        assert_eq!(s.handle_midi(&[0x90, 0x33, 0x7F]), vec![Intent::Refresh]);
        assert_eq!(s.active_zone, "home");
        assert_eq!(s.nav.mode, NavMode::Track);
    }

    #[test]
    fn banking_rebinds_strips() {
        // More tracks than strips, otherwise banking correctly clamps.
        let tracks = (0..12)
            .map(|i| track(&format!("t{i}"), &format!("Track {i}"), None, false))
            .collect();
        let mut s = DriverState::with_builtin_zones(tracks, "master".into(), 1.0);
        assert_eq!(s.handle_midi(&[0x90, 0x31, 0x7F]), vec![Intent::Refresh]); // chan right
        // Strip 0 now shows track 1.
        let raw = mcu::encode_fader(0, 16383);
        let intents = s.handle_midi(&raw);
        let Intent::SetVolume { guid, .. } = &intents[0] else {
            panic!();
        };
        assert_eq!(guid, "t1");
        // Bank right (+8) → strip 0 shows t4 (offset clamped to 12−8).
        assert_eq!(s.handle_midi(&[0x90, 0x2F, 0x7F]), vec![Intent::Refresh]);
        let intents = s.handle_midi(&mcu::encode_fader(0, 16383));
        let Intent::SetVolume { guid, .. } = &intents[0] else {
            panic!();
        };
        assert_eq!(guid, "t4");
    }

    #[test]
    fn touch_suppresses_motor_feedback() {
        let mut s = state();
        let _ = s.render(); // settle shadow
        // Touch strip 0, then a volume event for its track arrives.
        s.handle_midi(&[0x90, 0x68, 0x7F]);
        s.apply_track_event(&TrackEvent::VolumeChanged {
            guid: "drums".into(),
            volume: 0.5,
        });
        let msgs = s.render();
        assert!(
            !msgs.iter().any(|m| m[0] == 0xE0),
            "motor fader moved while touched: {msgs:?}"
        );
        // Release → next render moves the motor.
        s.handle_midi(&[0x90, 0x68, 0x00]);
        let msgs = s.render();
        assert!(msgs.iter().any(|m| m[0] == 0xE0));
    }

    #[test]
    fn render_full_surface_then_diffs_empty() {
        let mut s = state();
        let first = s.render();
        assert!(!first.is_empty());
        // LCD for strip 0 contains the track name.
        assert!(
            first
                .iter()
                .any(|m| m.len() == 15 && m[5] == 0x12 && &m[7..12] == b"DRUMS")
        );
        // Steady state: nothing changed → nothing sent.
        assert!(s.render().is_empty());
        // One event → only that strip's messages.
        s.apply_track_event(&TrackEvent::PanChanged {
            guid: "bass".into(),
            pan: -1.0,
        });
        let diff = s.render();
        assert!(
            diff.iter().all(|m| m[0] != 0xE0),
            "pan change moved a fader"
        );
        assert!(!diff.is_empty());
    }

    #[test]
    fn transport_buttons() {
        let mut s = state();
        assert_eq!(s.handle_midi(&[0x90, 0x5E, 0x7F]), vec![Intent::Play]);
        assert_eq!(s.handle_midi(&[0x90, 0x5D, 0x7F]), vec![Intent::Stop]);
        assert_eq!(s.handle_midi(&[0x90, 0x56, 0x7F]), vec![Intent::ToggleLoop]);
        assert_eq!(s.handle_midi(&[0x90, 0x5F, 0x7F]), vec![Intent::Record]);
        s.play_state = PlayState::Recording;
        assert_eq!(
            s.handle_midi(&[0x90, 0x5F, 0x7F]),
            vec![Intent::StopRecording]
        );
    }

    #[test]
    fn modifier_bindings_dispatch() {
        // Custom zone set: shift+mute = record-arm.
        let zones = ZoneSet::parse(
            r#"
zones {
    home {
        strip {
            mute @TrackMute
            shift+mute @TrackRecordArm
        }
    }
}
"#,
        )
        .unwrap();
        let mut s = DriverState::new(
            zones,
            vec![track("a", "A", None, false)],
            "master".into(),
            1.0,
        );
        // Plain mute.
        assert_eq!(
            s.handle_midi(&[0x90, 0x10, 0x7F]),
            vec![Intent::SetMuted {
                guid: "a".into(),
                muted: true
            }]
        );
        // Hold shift → same button arms instead.
        s.handle_midi(&[0x90, 0x46, 0x7F]); // shift down
        assert_eq!(
            s.handle_midi(&[0x90, 0x10, 0x7F]),
            vec![Intent::SetArmed {
                guid: "a".into(),
                armed: true
            }]
        );
        s.handle_midi(&[0x90, 0x46, 0x00]); // shift up
        assert_eq!(
            s.handle_midi(&[0x90, 0x10, 0x7F]),
            vec![Intent::SetMuted {
                guid: "a".into(),
                muted: true
            }]
        );
    }

    #[test]
    fn unbound_widget_is_inert() {
        // Zone with ONLY a fader binding — buttons do nothing.
        let zones = ZoneSet::parse(
            r#"
zones {
    home {
        strip {fader @TrackVolume}
    }
}
"#,
        )
        .unwrap();
        let mut s = DriverState::new(
            zones,
            vec![track("a", "A", None, false)],
            "master".into(),
            1.0,
        );
        assert!(s.handle_midi(&[0x90, 0x10, 0x7F]).is_empty()); // mute
        assert!(s.handle_midi(&[0x90, 0x5E, 0x7F]).is_empty()); // play
        assert!(!s.handle_midi(&mcu::encode_fader(0, 100)).is_empty());
    }
}
