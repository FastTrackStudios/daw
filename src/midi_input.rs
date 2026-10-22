//! Hardware MIDI into the tracks armed for it — REAPER's model: a track
//! armed with a MIDI record input hears the keyboard, whatever plays on it
//! (an instrument, a rig's sampler). Session's instrument tracks and the
//! rigs in an engine take MIDI the same way.
//!
//! The source is the process's one MIDI input ([`midi::hub`]), so the
//! engine opens no device clients of its own beside the rigs'.
//!
//! The hub exists only where `midi` has a native input (Linux, macOS). On
//! other targets (iOS) the route is inert: it builds and drops cleanly but
//! delivers nothing, so callers need no cfg of their own.

#[cfg(any(target_os = "linux", target_os = "macos"))]
use daw_proto::RecordInput;

use crate::sync::Standalone;

/// Hardware MIDI routed into a project's armed tracks; dropping it stops
/// the routing.
pub struct MidiInputRoute {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    _subscription: midi::hub::Subscription,
}

impl Standalone {
    /// Route the process's hardware MIDI into `project_guid`'s tracks that
    /// are armed with a MIDI record input — every device, or the channel a
    /// track names. A track picks its source by its record input; this
    /// route only delivers.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[must_use]
    pub fn route_midi_input(&self, project_guid: &str) -> MidiInputRoute {
        let daw = self.clone();
        let project = project_guid.to_string();
        let subscription = midi::hub::hub().subscribe("engine", None, move |timed| {
            let channel = timed.event.channel().map(midi::Channel::index);
            let targets: Vec<String> = daw
                .read_project(&project, |p| {
                    p.tracks
                        .iter()
                        .filter(|t| t.armed)
                        .filter(|t| match t.record_input {
                            RecordInput::Midi { channel: wants, .. } => {
                                wants.is_none() || wants == channel
                            }
                            _ => false,
                        })
                        .map(|t| t.guid.clone())
                        .collect()
                })
                .unwrap_or_default();
            for track in targets {
                daw.push_live_midi(&track, timed.event.clone());
            }
        });
        MidiInputRoute {
            _subscription: subscription,
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl Standalone {
    /// No hardware MIDI hub on this target (iOS): the route delivers
    /// nothing. Same signature as the Linux/macOS route.
    #[must_use]
    pub fn route_midi_input(&self, _project_guid: &str) -> MidiInputRoute {
        MidiInputRoute {}
    }
}
