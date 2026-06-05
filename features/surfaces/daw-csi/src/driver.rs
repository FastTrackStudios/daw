//! The surface driver: event-bus feedback out, surface gestures in.
//!
//! Unlike CSI (which polls REAPER ~50×/sec because REAPER has no push
//! API), this driver is event-driven: the daw event bus pushes every
//! state change, the driver diffs against the [`Shadow`] and sends
//! only real updates to the surface. Gestures decode to [`Intent`]s
//! (pure, unit-testable) which the async edge executes against the
//! `daw-control` services.

use daw_control::Daw;
use daw_proto::event_bus::{BusFilter, DawEvent};
use daw_proto::track::TrackEvent;
use daw_proto::transport::TransportEvent;
use daw_proto::{PlayState, Track};

use crate::mcu::{self, Button, RingMode, StripColor, SurfaceInput};
use crate::midi::SurfacePort;
use crate::navigator::{NavMode, Navigator};
use crate::shadow::Shadow;
use crate::taper;

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
    /// Navigation already applied to the navigator — re-resolve
    /// strips and refresh the surface.
    Refresh,
}

/// Everything the gesture/feedback logic reads and mutates. No I/O —
/// the async edge owns the port and services.
pub struct DriverState {
    pub nav: Navigator,
    pub shadow: Shadow,
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
    shift: bool,
}

impl DriverState {
    pub fn new(tracks: Vec<Track>, master_guid: String, master_volume: f64) -> Self {
        let mut s = Self {
            nav: Navigator::default(),
            shadow: Shadow::default(),
            tracks,
            strips: vec![None; mcu::STRIPS],
            master_guid,
            master_volume,
            play_state: PlayState::Stopped,
            looping: false,
            touched: [false; 9],
            shift: false,
        };
        s.rebind_strips();
        s
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

    // ── Gestures: surface → intents ─────────────────────────────────

    /// Decode raw MIDI from the surface into intents. Navigation
    /// gestures mutate the navigator here and yield `Refresh`.
    pub fn handle_midi(&mut self, raw: &[u8]) -> Vec<Intent> {
        let Some(input) = mcu::decode(raw) else {
            return Vec::new();
        };
        match input {
            SurfaceInput::Fader { strip, pos } => {
                let volume = taper::fader_to_volume(pos);
                if strip == mcu::MASTER {
                    self.master_volume = volume;
                    return vec![Intent::SetMasterVolume { volume }];
                }
                let Some(t) = self.strip_track(strip) else {
                    return Vec::new();
                };
                let guid = t.guid.clone();
                // Update the cache now so the echo event diffs clean.
                if let Some(t) = self.tracks.iter_mut().find(|t| t.guid == guid) {
                    t.volume = volume;
                }
                vec![Intent::SetVolume { guid, volume }]
            }
            SurfaceInput::FaderTouch { strip, touched } => {
                if let Some(cell) = self.touched.get_mut(strip as usize) {
                    *cell = touched;
                }
                Vec::new()
            }
            SurfaceInput::VPot { strip, delta } => {
                let Some(t) = self.strip_track(strip) else {
                    return Vec::new();
                };
                let step = if self.shift { 0.005 } else { 0.02 };
                let pan = (t.pan + delta as f64 * step).clamp(-1.0, 1.0);
                let guid = t.guid.clone();
                if let Some(t) = self.tracks.iter_mut().find(|t| t.guid == guid) {
                    t.pan = pan;
                }
                vec![Intent::SetPan { guid, pan }]
            }
            SurfaceInput::VPotPress { strip, pressed } => {
                // Press recenters pan (standard MCU convention).
                if !pressed {
                    return Vec::new();
                }
                let Some(t) = self.strip_track(strip) else {
                    return Vec::new();
                };
                let guid = t.guid.clone();
                if let Some(t) = self.tracks.iter_mut().find(|t| t.guid == guid) {
                    t.pan = 0.0;
                }
                vec![Intent::SetPan { guid, pan: 0.0 }]
            }
            SurfaceInput::Jog { delta } => vec![Intent::NudgePosition {
                seconds: delta as f64 * if self.shift { 0.1 } else { 1.0 },
            }],
            SurfaceInput::Button { button, pressed } => self.handle_button(button, pressed),
        }
    }

    fn handle_button(&mut self, button: Button, pressed: bool) -> Vec<Intent> {
        if let Button::Shift = button {
            self.shift = pressed;
            return Vec::new();
        }
        if !pressed {
            return Vec::new();
        }
        match button {
            Button::Mute(s) => self
                .strip_track(s)
                .map(|t| Intent::SetMuted {
                    guid: t.guid.clone(),
                    muted: !t.muted,
                })
                .into_iter()
                .collect(),
            Button::Solo(s) => self
                .strip_track(s)
                .map(|t| Intent::SetSoloed {
                    guid: t.guid.clone(),
                    soloed: !t.soloed,
                })
                .into_iter()
                .collect(),
            Button::Rec(s) => self
                .strip_track(s)
                .map(|t| Intent::SetArmed {
                    guid: t.guid.clone(),
                    armed: !t.armed,
                })
                .into_iter()
                .collect(),
            Button::Select(s) => {
                let Some(t) = self.strip_track(s).cloned() else {
                    return Vec::new();
                };
                // Folder mode: select drills/pops; only plain tracks
                // fall through to actual selection.
                if self.nav.folder_select(&t) {
                    self.rebind_strips();
                    return vec![Intent::Refresh];
                }
                vec![Intent::SelectExclusive { guid: t.guid }]
            }
            Button::BankLeft => self.bank(-(mcu::STRIPS as isize)),
            Button::BankRight => self.bank(mcu::STRIPS as isize),
            Button::ChannelLeft => self.bank(-1),
            Button::ChannelRight => self.bank(1),
            Button::GlobalView => {
                self.nav.toggle_mode();
                self.rebind_strips();
                vec![Intent::Refresh]
            }
            Button::Play => vec![Intent::Play],
            Button::Stop => vec![Intent::Stop],
            Button::Record => {
                if matches!(self.play_state, PlayState::Recording) {
                    vec![Intent::StopRecording]
                } else {
                    vec![Intent::Record]
                }
            }
            Button::Cycle => vec![Intent::ToggleLoop],
            Button::Rewind => vec![Intent::NudgePosition { seconds: -5.0 }],
            Button::FastForward => vec![Intent::NudgePosition { seconds: 5.0 }],
            _ => Vec::new(),
        }
    }

    fn bank(&mut self, delta: isize) -> Vec<Intent> {
        self.nav.bank(delta, &self.tracks, mcu::STRIPS);
        self.rebind_strips();
        vec![Intent::Refresh]
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

    /// Render the full surface state through the shadow. Returns the
    /// MIDI messages that actually need sending.
    pub fn render(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut colors = [StripColor::Off; mcu::STRIPS];

        for strip in 0..mcu::STRIPS as u8 {
            // Borrow dance: copy the small bits out first.
            let info = self.strip_track(strip).map(|t| {
                (
                    t.volume,
                    t.pan,
                    t.muted,
                    t.soloed,
                    t.armed,
                    t.selected,
                    t.name.clone(),
                    t.color,
                    t.is_folder,
                )
            });
            match info {
                Some((volume, pan, muted, soloed, armed, selected, name, color, is_folder)) => {
                    if !self.touched[strip as usize] {
                        self.shadow
                            .fader(&mut out, strip, taper::volume_to_fader(volume));
                    }
                    self.shadow.ring(
                        &mut out,
                        strip,
                        RingMode::BoostCut,
                        mcu::pan_to_ring(pan),
                        false,
                    );
                    self.shadow.led(&mut out, Button::Mute(strip), muted);
                    self.shadow.led(&mut out, Button::Solo(strip), soloed);
                    self.shadow.led(&mut out, Button::Rec(strip), armed);
                    self.shadow.led(&mut out, Button::Select(strip), selected);
                    self.shadow.lcd(&mut out, strip, 0, &name);
                    let bottom = if is_folder && self.nav.mode == NavMode::Folder {
                        format!("FOLDR {}", ">".repeat(self.nav.depth().min(1)))
                    } else {
                        pan_label(pan)
                    };
                    self.shadow.lcd(&mut out, strip, 1, &bottom);
                    colors[strip as usize] = color
                        .map(mcu::rgb_to_strip_color)
                        .unwrap_or(StripColor::White);
                }
                None => {
                    if !self.touched[strip as usize] {
                        self.shadow.fader(&mut out, strip, 0);
                    }
                    self.shadow
                        .ring(&mut out, strip, RingMode::SingleDot, 0, false);
                    self.shadow.led(&mut out, Button::Mute(strip), false);
                    self.shadow.led(&mut out, Button::Solo(strip), false);
                    self.shadow.led(&mut out, Button::Rec(strip), false);
                    self.shadow.led(&mut out, Button::Select(strip), false);
                    self.shadow.lcd(&mut out, strip, 0, "");
                    self.shadow.lcd(&mut out, strip, 1, "");
                }
            }
        }
        self.shadow.colors(&mut out, colors);

        // Master fader + transport section.
        if !self.touched[mcu::MASTER as usize] {
            self.shadow.fader(
                &mut out,
                mcu::MASTER,
                taper::volume_to_fader(self.master_volume),
            );
        }
        let playing = matches!(self.play_state, PlayState::Playing | PlayState::Recording);
        let recording = matches!(self.play_state, PlayState::Recording);
        let stopped = matches!(self.play_state, PlayState::Stopped | PlayState::Paused);
        self.shadow.led(&mut out, Button::Play, playing);
        self.shadow.led(&mut out, Button::Record, recording);
        self.shadow.led(&mut out, Button::Stop, stopped);
        self.shadow.led(&mut out, Button::Cycle, self.looping);
        self.shadow.led(
            &mut out,
            Button::GlobalView,
            self.nav.mode == NavMode::Folder,
        );
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

    let project = daw.current_project().await?;
    let tracks_api = project.tracks();
    let transport = project.transport();

    let master = tracks_api.master().await?;
    let master_guid = master.guid().to_string();
    let master_volume = master.volume().await.unwrap_or(1.0);
    let tracks = tracks_api.all().await?;

    let mut state = DriverState::new(tracks, master_guid, master_volume);
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
        // Navigation already applied to the navigator; the caller
        // renders after the intent batch.
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
        DriverState::new(
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
    fn folder_mode_select_drills_then_pops() {
        let mut s = state();
        // GlobalView toggles folder mode.
        assert_eq!(s.handle_midi(&[0x90, 0x33, 0x7F]), vec![Intent::Refresh]);
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
    }

    #[test]
    fn banking_rebinds_strips() {
        // More tracks than strips, otherwise banking correctly clamps.
        let tracks = (0..12)
            .map(|i| track(&format!("t{i}"), &format!("Track {i}"), None, false))
            .collect();
        let mut s = DriverState::new(tracks, "master".into(), 1.0);
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
}
