//! The MIDI **attach lifecycle** — one helper per pattern, so no rig
//! re-derives it.
//!
//! Every rig that plays from hardware MIDI runs the same dance: map a stored
//! port name to a [`PortSelector`], **drop the old handle before opening the
//! new one**, wire the monitor tap into the live-MIDI sink, and re-run the
//! whole thing when the port set changes (hot-plug). That choreography was
//! copy-pasted across the rigs — with drift — so it lives here now:
//!
//! - [`reattach`] — the full drop→select→open→store lifecycle for a rig that
//!   holds a [`MidiInput`](crate::midir::MidiInput) handle.
//! - [`tap_sink`] / [`tap_sink_transformed`] — the monitor-tap →
//!   live-MIDI-sink wiring handed to `MidiInput::open`.
//! - [`rescan_stream`] — hot-plug reopen for a polled
//!   [`MidiStream`](crate::midir::MidiStream) (drop-first, omni).
//!
//! ## Why drop-before-open is an invariant
//!
//! Opening the replacement connection while the old one is still alive
//! doubles ALSA-seq client usage per swap and can exhaust the kernel's queue
//! pool (seen live as `snd_seq_alloc_named_queue` OOM panics inside midir).
//! Every helper here releases the old OS clients *first*.

// r[impl primitives.midi.attach]

use std::sync::{Arc, Mutex};

use crate::proto::{MidiEvent, PortSelector, TimedEvent};

use crate::monitor::MidiMonitor;
use crate::selector_for;

/// Attach (or re-attach) a rig's MIDI input, encoding the whole lifecycle:
///
/// 1. `drop_old` runs **first**, releasing the previous handle's OS clients
///    (the ALSA-seq queue-exhaustion invariant — see module docs).
/// 2. The stored `port` name maps to a selector via [`selector_for`]
///    (name → `NameContains`, none → `All` omni).
/// 3. `open` performs the rig-specific open (typically
///    `rig.attach_midi(sel)`), under whatever locking the rig needs.
///    `Ok(None)` means "nothing to attach to" (rig not running) — a silent
///    no-op, matching every call site's behavior.
/// 4. `store(handle)` stows the new handle; success/failure is logged
///    uniformly under `rig` (e.g. `"keys rig"`).
///
/// Returns `true` when a new handle was attached.
pub fn reattach<H>(
    rig: &str,
    port: Option<&str>,
    drop_old: impl FnOnce(),
    open: impl FnOnce(PortSelector) -> eyre::Result<Option<H>>,
    store: impl FnOnce(H),
) -> bool {
    // Drop the old handle BEFORE opening the new one (see module docs).
    drop_old();
    let sel = selector_for(port);
    match open(sel) {
        Ok(Some(handle)) => {
            store(handle);
            let which = port
                .filter(|p| !p.is_empty())
                .unwrap_or("omni (all inputs)");
            tracing::info!(port = %which, "{rig}: MIDI attached");
            true
        }
        Ok(None) => false,
        Err(e) => {
            tracing::error!("{rig}: MIDI attach failed: {e}");
            false
        }
    }
}

/// The monitor-tap → live-MIDI-sink wiring: build a `MidiInput::open` sink
/// that records every event into `monitor` (so a UI can confirm MIDI is
/// arriving), then forwards it full-fidelity to `forward` (the rig's
/// live-MIDI entry, e.g. `daw.push_live_midi`).
pub fn tap_sink<F>(monitor: MidiMonitor, forward: F) -> impl Fn(TimedEvent) + Send + Clone + 'static
where
    F: Fn(MidiEvent) + Send + Clone + 'static,
{
    move |t: TimedEvent| {
        monitor.record(&t.event);
        forward(t.event);
    }
}

/// Like [`tap_sink`], but runs every event through `transform` first,
/// forwarding each produced event (this is where a drum-map converter
/// inserts). The monitor taps the *original* (pre-transform) event so a UI
/// shows what the hardware actually sent.
///
/// `transform` is shared behind a mutex (the midir sink is `Fn + Clone`) and
/// runs on the MIDI callback thread — keep it allocation-light and lock-free
/// of the audio thread.
pub fn tap_sink_transformed<T, F>(
    monitor: MidiMonitor,
    transform: T,
    forward: F,
) -> impl Fn(TimedEvent) + Send + Clone + 'static
where
    T: FnMut(MidiEvent) -> Vec<MidiEvent> + Send + 'static,
    F: Fn(MidiEvent) + Send + Clone + 'static,
{
    let transform = Arc::new(Mutex::new(transform));
    move |t: TimedEvent| {
        monitor.record(&t.event);
        let outs = match transform.lock() {
            Ok(mut f) => f(t.event),
            Err(_) => return,
        };
        for ev in outs {
            forward(ev);
        }
    }
}

/// Hot-plug rescan for a polled omni [`MidiStream`](crate::midir::MidiStream):
/// when the stream is missing or the input-port set changed since
/// `known_ports`, drop the old stream **first** (see module docs), then reopen
/// across all ports. Call at hot-plug cadence from a pump loop; a mid-set
/// replug comes back on its own.
pub fn rescan_stream(stream: &mut Option<crate::midir::MidiStream>, known_ports: &mut Vec<String>) {
    let ports = crate::midir::input_ports();
    if stream.is_some() && ports == *known_ports {
        return;
    }
    // Release the old stream's OS clients BEFORE opening anew.
    *stream = None;
    *stream = if ports.is_empty() {
        None
    } else {
        crate::midir::MidiStream::open_all()
    };
    match &stream {
        Some(s) => tracing::info!("MIDI capture active on {:?}", s.opened),
        None if ports.is_empty() && !known_ports.is_empty() => {
            tracing::warn!("all MIDI inputs gone — waiting for replug")
        }
        None => {}
    }
    *known_ports = ports;
}
