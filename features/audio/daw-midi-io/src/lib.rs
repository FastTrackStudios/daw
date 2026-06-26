//! Hardware MIDI input for the DAW engine.
//!
//! The MIDI counterpart to `daw-audio-io`: enumerate input devices, open a
//! [`MidiSelection`] (one named device, **all** devices merged, or a
//! REAPER-style **virtual** port other apps can connect to), parse raw bytes
//! into the engine's canonical [`MidiMessage`], and hand each event to a
//! caller-supplied sink.
//!
//! The sink runs directly on midir's OS callback thread, so the cheapest useful
//! sink pushes straight into the engine's lock-free live-MIDI ring (signal's
//! rig forwards into `Standalone::push_live_midi`). This crate stays a leaf: it
//! knows nothing about the engine — only devices, parsing, and the sink.
//!
//! ```no_run
//! use daw_midi_io::{MidiInput, MidiSelection, MidiMessage};
//! // Forward every note from all keyboards to a closure.
//! let _midi = MidiInput::open(MidiSelection::All, |msg: MidiMessage| {
//!     if let MidiMessage::NoteOn { note, velocity, .. } = msg {
//!         println!("note {note} vel {velocity}");
//!     }
//! }).unwrap();
//! // Held alive by `_midi`; drop to stop and close the ports.
//! ```

use eyre::eyre;
use midir::{MidiInput as MidirInput, MidiInputConnection};

pub use daw_proto::MidiInputDevice;
pub use daw_proto::MidiMessage;

const CLIENT: &str = "daw-midi-io";

/// Which MIDI input(s) to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MidiSelection {
    /// First input port whose name contains this substring (case-insensitive).
    Port(String),
    /// Every available input port, merged into one event stream.
    All,
    /// Create a virtual input port with this name (Unix only) that other apps
    /// can connect to — the way REAPER exposes a virtual MIDI keyboard target.
    Virtual(String),
}

/// List the names of all available MIDI input ports.
pub fn input_ports() -> Vec<String> {
    let Ok(midi) = MidirInput::new(&format!("{CLIENT}-enum")) else {
        return Vec::new();
    };
    midi.ports()
        .iter()
        .filter_map(|p| midi.port_name(p).ok())
        .collect()
}

/// List available MIDI input devices as [`MidiInputDevice`] records (id = port
/// index). Available + not-yet-open; the engine fills `is_open`/`is_connected`.
pub fn input_devices() -> Vec<MidiInputDevice> {
    input_ports()
        .into_iter()
        .enumerate()
        .map(|(i, name)| MidiInputDevice {
            id: i as u32,
            name,
            is_available: true,
            is_open: false,
            is_connected: false,
        })
        .collect()
}

/// An open set of MIDI input connections, each forwarding parsed messages to a
/// sink. Drop to stop and close every port.
pub struct MidiInput {
    // Held for their Drop — closing a connection stops its callback.
    _conns: Vec<MidiInputConnection<()>>,
    /// Names of the ports actually opened (for UI / logs).
    pub opened: Vec<String>,
}

impl MidiInput {
    /// Open `selection` and forward every parsed message to `sink`.
    ///
    /// `sink` is called on midir's OS callback thread; keep it cheap and
    /// non-blocking (push to a lock-free ring, don't allocate or lock).
    pub fn open<F>(selection: MidiSelection, sink: F) -> eyre::Result<Self>
    where
        F: Fn(MidiMessage) + Send + Clone + 'static,
    {
        match selection {
            MidiSelection::Port(needle) => {
                let (conn, name) = connect_matching(&needle, sink)?;
                tracing::info!(port = %name, "daw-midi-io: opened input");
                Ok(Self {
                    _conns: vec![conn],
                    opened: vec![name],
                })
            }
            MidiSelection::All => {
                let midi = MidirInput::new(CLIENT)?;
                let ports = midi.ports();
                if ports.is_empty() {
                    return Err(eyre!("no MIDI input ports available"));
                }
                let mut conns = Vec::new();
                let mut opened = Vec::new();
                for port in &ports {
                    let name = midi.port_name(port).unwrap_or_else(|_| "unknown".into());
                    // Each connection consumes its own MidirInput handle.
                    let Ok(handle) = MidirInput::new(CLIENT) else {
                        continue;
                    };
                    match connect_port(handle, port, &name, sink.clone()) {
                        Ok(conn) => {
                            tracing::info!(port = %name, "daw-midi-io: opened input");
                            conns.push(conn);
                            opened.push(name);
                        }
                        Err(e) => tracing::warn!(port = %name, "daw-midi-io: skip: {e}"),
                    }
                }
                if conns.is_empty() {
                    return Err(eyre!("failed to open any MIDI input port"));
                }
                Ok(Self {
                    _conns: conns,
                    opened,
                })
            }
            MidiSelection::Virtual(name) => {
                let conn = connect_virtual(&name, sink)?;
                tracing::info!(port = %name, "daw-midi-io: created virtual input");
                Ok(Self {
                    _conns: vec![conn],
                    opened: vec![name],
                })
            }
        }
    }
}

/// Connect to the first input port matching `needle` (case-insensitive).
fn connect_matching<F>(needle: &str, sink: F) -> eyre::Result<(MidiInputConnection<()>, String)>
where
    F: Fn(MidiMessage) + Send + 'static,
{
    let needle = needle.to_lowercase();
    let midi = MidirInput::new(CLIENT)?;
    let port = midi
        .ports()
        .into_iter()
        .find(|p| {
            midi.port_name(p)
                .map(|n| n.to_lowercase().contains(&needle))
                .unwrap_or(false)
        })
        .ok_or_else(|| eyre!("no MIDI input matching '{needle}'"))?;
    let name = midi.port_name(&port).unwrap_or_default();
    let conn = connect_port(midi, &port, &name, sink)?;
    Ok((conn, name))
}

/// Wire a single port's midir callback to `sink`.
fn connect_port<F>(
    handle: MidirInput,
    port: &midir::MidiInputPort,
    name: &str,
    sink: F,
) -> eyre::Result<MidiInputConnection<()>>
where
    F: Fn(MidiMessage) + Send + 'static,
{
    handle
        .connect(
            port,
            CLIENT,
            move |_ts, bytes, _| {
                if let Some(msg) = parse(bytes) {
                    sink(msg);
                }
            },
            (),
        )
        .map_err(|e| eyre!("connect MIDI input '{name}': {e}"))
}

#[cfg(unix)]
fn connect_virtual<F>(name: &str, sink: F) -> eyre::Result<MidiInputConnection<()>>
where
    F: Fn(MidiMessage) + Send + 'static,
{
    use midir::os::unix::VirtualInput;
    let midi = MidirInput::new(CLIENT)?;
    midi.create_virtual(
        name,
        move |_ts, bytes, _| {
            if let Some(msg) = parse(bytes) {
                sink(msg);
            }
        },
        (),
    )
    .map_err(|e| eyre!("create virtual MIDI input '{name}': {e}"))
}

#[cfg(not(unix))]
fn connect_virtual<F>(_name: &str, _sink: F) -> eyre::Result<MidiInputConnection<()>>
where
    F: Fn(MidiMessage) + Send + 'static,
{
    Err(eyre!("virtual MIDI ports are only supported on Unix"))
}

/// A raw 3-byte MIDI event with the classic status/note/velocity layout —
/// ergonomic for UI loops that poll [`MidiStream::drain`] and only care about
/// notes + CCs. (For full message fidelity use [`MidiInput`] with a sink.)
#[derive(Clone, Copy, Debug)]
pub struct MidiEvent {
    /// Status byte: `0x90 | ch` note-on, `0x80 | ch` note-off, `0xB0 | ch` CC.
    pub status: u8,
    /// Note number, or — for a CC — the controller number.
    pub note: u8,
    /// Velocity, or — for a CC — the controller value.
    pub velocity: u8,
}

impl MidiEvent {
    pub fn is_note_on(self) -> bool {
        (self.status & 0xF0) == 0x90 && self.velocity > 0
    }
    pub fn is_note_off(self) -> bool {
        (self.status & 0xF0) == 0x80 || ((self.status & 0xF0) == 0x90 && self.velocity == 0)
    }
    /// Control Change (status `0xB0`). For CC, `note` is the controller and
    /// `velocity` is its value (e.g. CC64 = sustain pedal).
    pub fn is_cc(self) -> bool {
        (self.status & 0xF0) == 0xB0
    }
    /// Controller number for a CC message.
    pub fn controller(self) -> u8 {
        self.note
    }
    /// Controller value for a CC message.
    pub fn cc_value(self) -> u8 {
        self.velocity
    }
}

/// A [`MidiInput`] paired with a lock-free channel — open once, then `drain()`
/// pending [`MidiEvent`]s from a UI loop each frame. The drop-in replacement for
/// signal-audio's retired `MidiInput`.
///
/// Cloning shares the receiver + keeps the underlying connection alive (the
/// input lives behind an `Arc`), so it can be stored in Dioxus context and
/// cloned freely — exactly like the type it replaces.
#[derive(Clone)]
pub struct MidiStream {
    _input: std::sync::Arc<MidiInput>,
    rx: crossbeam_channel::Receiver<MidiEvent>,
    /// Ports actually opened (for UI / logs).
    pub opened: Vec<String>,
}

impl MidiStream {
    /// Open `selection` into a bounded channel. Events arrive on midir's OS
    /// thread; `drain()` consumes them on the UI thread.
    pub fn open(selection: MidiSelection) -> eyre::Result<Self> {
        let (tx, rx) = crossbeam_channel::bounded::<MidiEvent>(256);
        let input = MidiInput::open(selection, move |msg| {
            if let Some((status, note, velocity)) = msg.to_raw_bytes() {
                let _ = tx.try_send(MidiEvent {
                    status,
                    note,
                    velocity,
                });
            }
        })?;
        let opened = input.opened.clone();
        Ok(Self {
            _input: std::sync::Arc::new(input),
            rx,
            opened,
        })
    }

    /// Open every available MIDI input port. `None` if none are available
    /// (matches signal-audio's old `MidiInput::open_all` ergonomics).
    pub fn open_all() -> Option<Self> {
        Self::open(MidiSelection::All).ok()
    }

    /// Non-blocking drain of all pending events.
    pub fn drain(&self) -> impl Iterator<Item = MidiEvent> + '_ {
        self.rx.try_iter()
    }
}

/// Parse one complete MIDI message (status byte + data) into a [`MidiMessage`].
/// Returns `None` for empty input or a leading data byte (running status — not
/// emitted by midir, which delivers complete messages).
fn parse(bytes: &[u8]) -> Option<MidiMessage> {
    let status = *bytes.first()?;
    if status < 0x80 {
        return None;
    }
    let channel = status & 0x0F;
    let d1 = bytes.get(1).copied().unwrap_or(0);
    let d2 = bytes.get(2).copied().unwrap_or(0);
    Some(match status & 0xF0 {
        // Note-on with velocity 0 is a note-off by convention.
        0x90 if d2 > 0 => MidiMessage::note_on(channel, d1, d2),
        0x90 | 0x80 => MidiMessage::note_off(channel, d1, d2),
        0xA0 => MidiMessage::PolyPressure {
            channel,
            note: d1,
            pressure: d2,
        },
        0xB0 => MidiMessage::control_change(channel, d1, d2),
        0xC0 => MidiMessage::program_change(channel, d1),
        0xD0 => MidiMessage::ChannelPressure {
            channel,
            pressure: d1,
        },
        0xE0 => MidiMessage::pitch_bend(channel, (((d2 as i16) << 7) | d1 as i16) - 8192),
        0xF0 => MidiMessage::SysEx(bytes.to_vec()),
        _ => MidiMessage::Raw(bytes.to_vec()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_note_on() {
        let m = parse(&[0x92, 60, 100]).unwrap();
        assert!(matches!(
            m,
            MidiMessage::NoteOn {
                channel: 2,
                note: 60,
                velocity: 100
            }
        ));
    }

    #[test]
    fn note_on_zero_velocity_is_note_off() {
        let m = parse(&[0x90, 60, 0]).unwrap();
        assert!(m.is_note_off());
    }

    #[test]
    fn parses_cc_and_pitch_bend() {
        assert!(matches!(
            parse(&[0xB0, 1, 64]).unwrap(),
            MidiMessage::ControlChange {
                controller: 1,
                value: 64,
                ..
            }
        ));
        // 14-bit center (LSB 0, MSB 64 => 8192) maps to 0.
        assert!(matches!(
            parse(&[0xE0, 0, 64]).unwrap(),
            MidiMessage::PitchBend { value: 0, .. }
        ));
    }

    #[test]
    fn rejects_running_status() {
        assert!(parse(&[60, 100]).is_none());
        assert!(parse(&[]).is_none());
    }
}
