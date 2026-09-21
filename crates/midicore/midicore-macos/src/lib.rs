//! Native CoreMIDI input backend for `midicore` — **one client, one input
//! port**, every selected source connected to it.
//!
//! # Why not midir
//!
//! midir opens one CoreMIDI client *per connection*: that is its API. CoreMIDI
//! itself is built the other way round — a client owns an input port and any
//! number of sources are *connected* to that port with
//! `MIDIPortConnectSource`. That is exactly the shape the PipeWire backend
//! builds by linking sources into one node, so this backend is the same idea
//! in CoreMIDI's own terms: selection is connect/disconnect on one port, and
//! nothing is torn down to change which keyboards feed the rig.
//!
//! # Port names
//!
//! A source is named by its `kMIDIPropertyDisplayName` — byte-identical to
//! what midir reported on macOS — so a port name stored in a rig preset keeps
//! selecting the same device after the swap.
//!
//! # Hot-plug
//!
//! CoreMIDI only delivers its setup-changed notifications to a thread that is
//! running a CFRunLoop. Rather than keep a run loop alive for that, the owning
//! thread re-reads the source list every [`RESCAN`] — the same cadence as the
//! PipeWire backend's reconcile timer. Unlike midir-over-JACK, enumerating
//! CoreMIDI sources creates nothing, so polling costs no client churn.
//!
//! # Threading
//!
//! The client, its port and any virtual destinations live on one thread,
//! which the handle drives over a channel. `sink` is called on CoreMIDI's own
//! high-priority receive thread — keep it cheap and non-blocking.
#![cfg(target_os = "macos")]

use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use coremidi::{Client, InputPort, PacketList, Source, Sources, VirtualDestination};
use midicore_proto::{
    decode_all, BackendError, Direction, InputBackend, InputConfig, MaybeSend, PortId, PortInfo,
    PortSelector, TimedEvent,
};

/// How often the owning thread re-reads the source list.
pub const RESCAN: Duration = Duration::from_millis(200);

/// Every CoreMIDI source right now: unique id → display name.
///
/// The unique id is what survives a rename or a reorder, so it is what the
/// connection bookkeeping keys on; the name is what selectors match.
fn scan() -> BTreeMap<u32, String> {
    Sources
        .into_iter()
        .filter_map(|s| {
            let id = s.unique_id()?;
            let name = s.display_name().or_else(|| s.name())?;
            Some((id, name))
        })
        .collect()
}

fn source_info(name: String, virtual_port: bool) -> PortInfo {
    PortInfo {
        id: PortId(name.clone()),
        name,
        direction: Direction::Input,
        virtual_port,
    }
}

enum Cmd {
    Select(Vec<PortSelector>),
    Quit,
}

/// An open CoreMIDI input. Drop to disconnect every source and dispose the
/// port.
pub struct CoreMidiInput {
    tx: mpsc::Sender<Cmd>,
    connected: Arc<RwLock<Vec<PortInfo>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// The sink, shared by the input port and every virtual destination. Behind a
/// mutex because the caller's sink is only `Send`; every callback runs on
/// CoreMIDI's single receive thread, so the lock is never contended.
type Sink = Arc<Mutex<dyn Fn(TimedEvent) + Send>>;

fn deliver(sink: &Sink, started: Instant, packets: &PacketList) {
    let timestamp_us = started.elapsed().as_micros() as u64;
    let sink = sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for packet in packets.iter() {
        decode_all(packet.data(), |event| {
            sink(TimedEvent {
                timestamp_us,
                event,
            });
        });
    }
}

impl InputBackend for CoreMidiInput {
    const NAME: &'static str = "coremidi";

    fn sources() -> Vec<PortInfo> {
        let mut names: Vec<String> = scan().into_values().collect();
        names.sort();
        names.into_iter().map(|n| source_info(n, false)).collect()
    }

    fn open<F>(config: InputConfig, sink: F) -> Result<Self, BackendError>
    where
        F: Fn(TimedEvent) + MaybeSend + 'static,
    {
        let sink: Sink = Arc::new(Mutex::new(sink));
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), BackendError>>(1);
        let connected = Arc::new(RwLock::new(Vec::new()));

        let thread = {
            let connected = connected.clone();
            std::thread::Builder::new()
                .name("midicore-coremidi".into())
                .spawn(move || run(config, sink, rx, &ready_tx, &connected))
                .map_err(|e| BackendError(format!("spawn CoreMIDI thread: {e}")))?
        };

        // Wait until the client and port exist, so a caller that asks for
        // `connected()` straight away gets a settled answer.
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Err(e)) => Err(e),
            Ok(Ok(())) | Err(_) => Ok(Self {
                tx,
                connected,
                thread: Some(thread),
            }),
        }
    }

    fn select(&self, selectors: Vec<PortSelector>) {
        let _ = self.tx.send(Cmd::Select(selectors));
    }

    fn connected(&self) -> Vec<PortInfo> {
        self.connected
            .read()
            .map(|c| c.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }
}

impl Drop for CoreMidiInput {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Everything the owning thread holds. Dropping it disconnects and disposes.
struct State {
    selectors: Vec<PortSelector>,
    /// The last source list reconciled against, to skip no-change rescans.
    seen: BTreeMap<u32, String>,
    /// Connected sources: unique id → name.
    linked: BTreeMap<u32, String>,
    /// Virtual destinations created for `PortSelector::Virtual`, by name.
    virtuals: HashMap<String, VirtualDestination>,
}

fn run(
    config: InputConfig,
    sink: Sink,
    rx: mpsc::Receiver<Cmd>,
    ready: &mpsc::SyncSender<Result<(), BackendError>>,
    connected: &RwLock<Vec<PortInfo>>,
) {
    let started = Instant::now();
    // NOTE: coremidi 0.9 does not dispose a `Client` on drop, so each open
    // leaves a (cheap, idle) client registered until the process exits. The
    // intended use is one long-lived input per process.
    let client = match Client::new(&config.name) {
        Ok(c) => c,
        Err(status) => {
            let _ = ready.send(Err(BackendError(format!(
                "create CoreMIDI client '{}': OSStatus {status}",
                config.name
            ))));
            return;
        }
    };
    let port = {
        let sink = sink.clone();
        match client.input_port(&config.name, move |packets| deliver(&sink, started, packets)) {
            Ok(p) => p,
            Err(status) => {
                let _ = ready.send(Err(BackendError(format!(
                    "create CoreMIDI input port: OSStatus {status}"
                ))));
                return;
            }
        }
    };

    let mut state = State {
        selectors: config.selectors,
        seen: BTreeMap::new(),
        linked: BTreeMap::new(),
        virtuals: HashMap::new(),
    };
    reconcile(&mut state, scan(), &client, &port, &sink, started, connected);
    let _ = ready.send(Ok(()));

    loop {
        let mut dirty = false;
        match rx.recv_timeout(RESCAN) {
            Ok(Cmd::Select(selectors)) => {
                dirty = state.selectors != selectors;
                state.selectors = selectors;
            }
            Ok(Cmd::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now = scan();
        if dirty || now != state.seen {
            reconcile(&mut state, now, &client, &port, &sink, started, connected);
        }
    }

    for id in state.linked.keys() {
        if let Some(source) = Source::from_unique_id(*id) {
            let _ = port.disconnect_source(&source);
        }
    }
}

/// Connect what the selectors want, disconnect what they no longer do, and
/// create or drop virtual destinations to match.
fn reconcile(
    state: &mut State,
    now: BTreeMap<u32, String>,
    client: &Client,
    port: &InputPort,
    sink: &Sink,
    started: Instant,
    connected: &RwLock<Vec<PortInfo>>,
) {
    let wanted: BTreeMap<u32, String> = now
        .iter()
        .filter(|(_, name)| state.selectors.iter().any(|s| s.matches(name)))
        .map(|(id, name)| (*id, name.clone()))
        .collect();

    // A source that vanished is already gone from CoreMIDI; only one that is
    // still present but no longer wanted needs an explicit disconnect.
    state.linked.retain(|id, name| {
        if wanted.contains_key(id) {
            return true;
        }
        if let Some(source) = Source::from_unique_id(*id) {
            let _ = port.disconnect_source(&source);
            tracing::info!(midi.port = %name, "midicore-coremidi: disconnected");
        }
        false
    });

    for (id, name) in &wanted {
        if state.linked.contains_key(id) {
            continue;
        }
        let Some(source) = Source::from_unique_id(*id) else {
            continue;
        };
        match port.connect_source(&source) {
            Ok(()) => {
                tracing::info!(midi.port = %name, "midicore-coremidi: connected");
                state.linked.insert(*id, name.clone());
            }
            Err(status) => tracing::warn!(
                midi.port = %name,
                midi.os_status = status,
                "midicore-coremidi: connect failed"
            ),
        }
    }

    let virtual_names: Vec<&String> = state
        .selectors
        .iter()
        .filter_map(|s| match s {
            PortSelector::Virtual(name) => Some(name),
            _ => None,
        })
        .collect();
    state
        .virtuals
        .retain(|name, _| virtual_names.contains(&name));
    for name in virtual_names {
        if state.virtuals.contains_key(name) {
            continue;
        }
        let sink = sink.clone();
        match client.virtual_destination(name, move |packets| deliver(&sink, started, packets)) {
            Ok(dest) => {
                tracing::info!(midi.port = %name, "midicore-coremidi: created virtual input");
                state.virtuals.insert(name.clone(), dest);
            }
            Err(status) => tracing::warn!(
                midi.port = %name,
                midi.os_status = status,
                "midicore-coremidi: virtual input failed"
            ),
        }
    }

    let mut infos: Vec<PortInfo> = state
        .linked
        .values()
        .map(|n| source_info(n.clone(), false))
        .chain(state.virtuals.keys().map(|n| source_info(n.clone(), true)))
        .collect();
    infos.sort_by(|a, b| a.name.cmp(&b.name));
    if let Ok(mut c) = connected.write() {
        *c = infos;
    }
    state.seen = now;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Enumeration needs no client and must never fail — on a Mac with no
    /// MIDI hardware it is simply empty.
    #[test]
    fn sources_enumerate_without_opening_anything() {
        let sources = CoreMidiInput::sources();
        assert!(sources.windows(2).all(|w| w[0].name <= w[1].name));
        assert!(sources.iter().all(|p| p.id.0 == p.name && !p.virtual_port));
    }

    /// Opening with nothing selected connects nothing, and a virtual selector
    /// creates a port other apps can see and send to.
    #[test]
    fn a_virtual_selector_creates_a_port_and_dropping_removes_it() {
        let name = format!("midicore-test-{}", std::process::id());
        let input = CoreMidiInput::open(
            InputConfig::new("midicore-test").selecting(vec![PortSelector::Virtual(name.clone())]),
            |_| {},
        )
        .expect("CoreMIDI reachable");
        let connected = input.connected();
        assert_eq!(connected.len(), 1);
        assert_eq!(connected[0].name, name);
        assert!(connected[0].virtual_port);

        input.select(Vec::new());
        std::thread::sleep(RESCAN * 2);
        assert!(input.connected().is_empty());
    }

    /// End to end through CoreMIDI: a virtual *source* we create is selected
    /// by name, and bytes sent on it arrive in the sink decoded.
    #[test]
    fn a_selected_source_delivers_decoded_events() {
        let client = Client::new("midicore-test-sender").expect("client");
        let name = format!("midicore-test-src-{}", std::process::id());
        let source = client.virtual_source(&name).expect("virtual source");

        let (tx, rx) = mpsc::channel();
        let input = CoreMidiInput::open(
            InputConfig::new("midicore-test")
                .selecting(vec![PortSelector::NameContains(name.clone())]),
            move |ev| {
                let _ = tx.send(ev.event);
            },
        )
        .expect("CoreMIDI reachable");
        assert_eq!(input.ports_named(), vec![name.clone()]);

        let packets = coremidi::PacketBuffer::new(0, &[0x90, 60, 100]);
        source.received(&packets).expect("send");
        let event = rx.recv_timeout(Duration::from_secs(2)).expect("event arrives");
        assert!(matches!(event, midicore_proto::MidiEvent::NoteOn { .. }));
    }

    impl CoreMidiInput {
        fn ports_named(&self) -> Vec<String> {
            self.connected().into_iter().map(|p| p.name).collect()
        }
    }
}
