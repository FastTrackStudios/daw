//! midir-backed OS MIDI input backend for `midicore`.
//!
//! The concrete adapter behind `midicore`'s port/selector/event vocabulary:
//! enumerate input ports, open a [`PortSelector`] (one named device, **all**
//! merged, or a virtual port other apps connect to), decode raw bytes into
//! [`midicore_proto::MidiEvent`] with the crate's own codec, and hand each
//! event to a caller-supplied sink.
//!
//! The sink runs directly on midir's OS callback thread — keep it cheap and
//! non-blocking (push to a lock-free ring, don't allocate or lock). This crate
//! stays a leaf: it knows only ports, decoding, and the sink.
//!
//! ```no_run
//! use midicore_midir::MidiInput;
//! use midicore_proto::{MidiEvent, PortSelector, TimedEvent};
//! let _midi = MidiInput::open(PortSelector::All, |t: TimedEvent| {
//!     if let MidiEvent::NoteOn { key, velocity, .. } = t.event {
//!         println!("note {} vel {}", key.get(), velocity.get());
//!     }
//! }).unwrap();
//! ```

use eyre::eyre;
use midir::{MidiInput as MidirInput, MidiInputConnection};

use midicore_proto::{Direction, MidiEvent, PortId, PortInfo, PortSelector, RawShortMessage, TimedEvent};

const CLIENT: &str = "midicore-midir";

/// List the names of all available MIDI input ports (sorted; our own
/// clients excluded).
pub fn input_ports() -> Vec<String> {
    let Ok(midi) = MidirInput::new(&format!("{CLIENT}-enum")) else {
        return Vec::new();
    };
    let mut ports: Vec<String> = midi
        .ports()
        .iter()
        .filter_map(|p| midi.port_name(p).ok())
        // Our own clients (open streams, virtual ports) appear in the
        // enumeration; including them makes hot-plug "did the ports
        // change?" scans observe their own reopens — a feedback loop
        // that leaks an ALSA seq client per scan until queue exhaustion.
        .filter(|name| !name.contains(CLIENT))
        .collect();
    ports.sort();
    ports
}

/// List available MIDI input ports as [`PortInfo`] records (id = port name).
pub fn input_devices() -> Vec<PortInfo> {
    input_ports()
        .into_iter()
        .map(|name| PortInfo {
            id: PortId(name.clone()),
            name,
            direction: Direction::Input,
            virtual_port: false,
        })
        .collect()
}

/// An open set of MIDI input connections, each forwarding decoded events to a
/// sink. Drop to stop and close every port.
pub struct MidiInput {
    // Held for their Drop — closing a connection stops its callback.
    _conns: Vec<MidiInputConnection<()>>,
    /// Names of the ports actually opened (for UI / logs).
    pub opened: Vec<String>,
}

impl MidiInput {
    /// Open `selector` and forward every decoded event to `sink`.
    ///
    /// `sink` is called on midir's OS callback thread; keep it cheap and
    /// non-blocking.
    pub fn open<F>(selector: PortSelector, sink: F) -> eyre::Result<Self>
    where
        F: Fn(TimedEvent) + Send + Clone + 'static,
    {
        match selector {
            PortSelector::Default => {
                let midi = MidirInput::new(CLIENT)?;
                let port = midi
                    .ports()
                    .into_iter()
                    .next()
                    .ok_or_else(|| eyre!("no MIDI input ports available"))?;
                let name = midi.port_name(&port).unwrap_or_default();
                let conn = connect_port(midi, &port, &name, sink)?;
                tracing::info!(port = %name, "midicore-midir: opened input");
                Ok(Self { _conns: vec![conn], opened: vec![name] })
            }
            PortSelector::Id(PortId(id)) => {
                // PortId is the port name for this backend — match exactly.
                let (conn, name) = connect_where(sink, |n| n == id)?;
                tracing::info!(port = %name, "midicore-midir: opened input");
                Ok(Self { _conns: vec![conn], opened: vec![name] })
            }
            PortSelector::NameContains(needle) => {
                let needle = needle.to_lowercase();
                let (conn, name) = connect_where(sink, |n| n.to_lowercase().contains(&needle))?;
                tracing::info!(port = %name, "midicore-midir: opened input");
                Ok(Self { _conns: vec![conn], opened: vec![name] })
            }
            PortSelector::All => {
                let midi = MidirInput::new(CLIENT)?;
                let ports = midi.ports();
                if ports.is_empty() {
                    return Err(eyre!("no MIDI input ports available"));
                }
                let mut conns = Vec::new();
                let mut opened = Vec::new();
                for port in &ports {
                    let name = midi.port_name(port).unwrap_or_else(|_| "unknown".into());
                    // Never open our own clients (see input_ports).
                    if name.contains(CLIENT) {
                        continue;
                    }
                    // Each connection consumes its own MidirInput handle.
                    let Ok(handle) = MidirInput::new(CLIENT) else { continue };
                    match connect_port(handle, port, &name, sink.clone()) {
                        Ok(conn) => {
                            tracing::info!(port = %name, "midicore-midir: opened input");
                            conns.push(conn);
                            opened.push(name);
                        }
                        Err(e) => tracing::warn!(port = %name, "midicore-midir: skip: {e}"),
                    }
                }
                if conns.is_empty() {
                    return Err(eyre!("failed to open any MIDI input port"));
                }
                Ok(Self { _conns: conns, opened })
            }
            PortSelector::Virtual(name) => {
                let conn = connect_virtual(&name, sink)?;
                tracing::info!(port = %name, "midicore-midir: created virtual input");
                Ok(Self { _conns: vec![conn], opened: vec![name] })
            }
        }
    }
}

/// Connect to the first input port whose name satisfies `pred`.
fn connect_where<F, P>(sink: F, pred: P) -> eyre::Result<(MidiInputConnection<()>, String)>
where
    F: Fn(TimedEvent) + Send + 'static,
    P: Fn(&str) -> bool,
{
    let midi = MidirInput::new(CLIENT)?;
    let port = midi
        .ports()
        .into_iter()
        .find(|p| midi.port_name(p).map(|n| pred(&n)).unwrap_or(false))
        .ok_or_else(|| eyre!("no MIDI input port matched"))?;
    let name = midi.port_name(&port).unwrap_or_default();
    let conn = connect_port(midi, &port, &name, sink)?;
    Ok((conn, name))
}

/// Wire a single port's midir callback to `sink`, decoding via midicore's codec.
fn connect_port<F>(
    handle: MidirInput,
    port: &midir::MidiInputPort,
    name: &str,
    sink: F,
) -> eyre::Result<MidiInputConnection<()>>
where
    F: Fn(TimedEvent) + Send + 'static,
{
    handle
        .connect(
            port,
            CLIENT,
            move |ts, bytes, _| {
                if let Ok((event, _)) = MidiEvent::decode(bytes) {
                    sink(TimedEvent { timestamp_us: ts, event });
                }
            },
            (),
        )
        .map_err(|e| eyre!("connect MIDI input '{name}': {e}"))
}

#[cfg(unix)]
fn connect_virtual<F>(name: &str, sink: F) -> eyre::Result<MidiInputConnection<()>>
where
    F: Fn(TimedEvent) + Send + 'static,
{
    use midir::os::unix::VirtualInput;
    let midi = MidirInput::new(CLIENT)?;
    midi.create_virtual(
        name,
        move |ts, bytes, _| {
            if let Ok((event, _)) = MidiEvent::decode(bytes) {
                sink(TimedEvent { timestamp_us: ts, event });
            }
        },
        (),
    )
    .map_err(|e| eyre!("create virtual MIDI input '{name}': {e}"))
}

#[cfg(not(unix))]
fn connect_virtual<F>(_name: &str, _sink: F) -> eyre::Result<MidiInputConnection<()>>
where
    F: Fn(TimedEvent) + Send + 'static,
{
    Err(eyre!("virtual MIDI ports are only supported on Unix"))
}

/// A [`MidiInput`] paired with a lock-free channel — open once, then `drain()`
/// pending [`RawShortMessage`]s from a UI loop each frame. The ergonomic
/// polling front-end for note/CC UIs (full-fidelity consumers use [`MidiInput`]
/// with a sink).
///
/// Cloning shares the receiver + keeps the underlying connection alive (the
/// input lives behind an `Arc`), so it can be stored in UI context and cloned
/// freely.
#[derive(Clone)]
pub struct MidiStream {
    // `MidiInput` isn't Sync (holds midir connection handles), but this field
    // is drop-only — never dereferenced past construction. The Arc exists
    // purely so cloning `MidiStream` shares one connection (closes once every
    // clone drops), not to hand out concurrent access.
    #[allow(clippy::arc_with_non_send_sync)]
    _input: std::sync::Arc<MidiInput>,
    rx: crossbeam_channel::Receiver<RawShortMessage>,
    /// Ports actually opened (for UI / logs).
    pub opened: Vec<String>,
}

impl MidiStream {
    /// Open `selector` into a bounded channel. Events arrive on midir's OS
    /// thread; `drain()` consumes them on the UI thread.
    pub fn open(selector: PortSelector) -> eyre::Result<Self> {
        let (tx, rx) = crossbeam_channel::bounded::<RawShortMessage>(256);
        let input = MidiInput::open(selector, move |t| {
            if let Some(raw) = RawShortMessage::from_event(&t.event) {
                let _ = tx.try_send(raw);
            }
        })?;
        let opened = input.opened.clone();
        // See the field's doc comment: drop-only, never dereferenced.
        #[allow(clippy::arc_with_non_send_sync)]
        let input = std::sync::Arc::new(input);
        Ok(Self { _input: input, rx, opened })
    }

    /// Open every available MIDI input port. `None` if none are available.
    pub fn open_all() -> Option<Self> {
        Self::open(PortSelector::All).ok()
    }

    /// Non-blocking drain of all pending events.
    pub fn drain(&self) -> impl Iterator<Item = RawShortMessage> + '_ {
        self.rx.try_iter()
    }
}
