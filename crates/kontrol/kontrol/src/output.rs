//! Thin MIDI **output** layer over `midir` — used only by the falsification
//! probe (proving note-ons on Main/DAW do NOT light the guide) and as the seam
//! for future NIHIA DAW-port integration.
//!
//! `midicore-midir` only exposes MIDI *input* today, so this crate opens its
//! own `midir::MidiOutput` connections. Everything here is native-only
//! hardware I/O; there is no wasm/no_std path.

use eyre::{eyre, Result};
use midir::{MidiOutput as MidirOutput, MidiOutputConnection};

use crate::LightColor;

/// The three MIDI ports the S88 MK3 exposes, matched by a case-insensitive
/// substring of the ALSA port name.
pub mod ports {
    /// The keybed / performance MIDI port.
    pub const MAIN: &str = "Main";
    /// The DAW-integration port (where NIHIA host-integration traffic flows).
    pub const DAW: &str = "DAW";
    /// The secondary / "external" MIDI port.
    pub const EXT: &str = "Ext";
    /// Substring identifying any S88 MK3 port (device name prefix).
    pub const DEVICE: &str = "KONTROL S88 MK3";
}

/// A candidate MIDI encoding for a Light-Guide note-on, used by the probe to
/// falsify the "maybe it's just MIDI" hypotheses. None of these is expected to
/// work on MK3 (the guide is USB, not MIDI) — see `docs/protocol.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// Note On, channel 1 (index 0), `key = note`, `velocity = palette byte`.
    NoteOnCh1,
    /// Note On, channel 16 (index 15) — NIHIA control channel.
    NoteOnCh16,
}

impl Encoding {
    /// Serialize one key's light state to raw MIDI bytes.
    pub fn encode(self, note: u8, color: LightColor) -> [u8; 3] {
        let ch = match self {
            Encoding::NoteOnCh1 => 0x00,
            Encoding::NoteOnCh16 => 0x0f,
        };
        [0x90 | ch, note & 0x7f, color.byte()]
    }
}

const CLIENT: &str = "kontrol";

/// A single open MIDI output port.
pub struct MidiOut {
    conn: MidiOutputConnection,
    /// The port name actually opened (for logs / the probe).
    pub name: String,
}

impl MidiOut {
    /// Open the first output port whose name contains `needle` (case-insensitive).
    pub fn open_contains(needle: &str) -> Result<Self> {
        let needle_lc = needle.to_lowercase();
        let midi = MidirOutput::new(&format!("{CLIENT}-out"))?;
        let port = midi
            .ports()
            .into_iter()
            .find(|p| {
                midi.port_name(p)
                    .map(|n| n.to_lowercase().contains(&needle_lc))
                    .unwrap_or(false)
            })
            .ok_or_else(|| eyre!("no MIDI output port matching {needle:?}"))?;
        let name = midi.port_name(&port).unwrap_or_default();
        let conn = midi
            .connect(&port, CLIENT)
            .map_err(|e| eyre!("connect MIDI output {name:?}: {e}"))?;
        tracing::info!(port = %name, "kontrol: opened MIDI output");
        Ok(Self { conn, name })
    }

    /// Send raw MIDI bytes to the port.
    pub fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.conn
            .send(bytes)
            .map_err(|e| eyre!("MIDI send to {:?}: {e}", self.name))
    }
}

/// List the names of all available MIDI **output** ports (our own clients excluded).
pub fn output_ports() -> Vec<String> {
    let Ok(midi) = MidirOutput::new(&format!("{CLIENT}-enum")) else {
        return Vec::new();
    };
    let mut ports: Vec<String> = midi
        .ports()
        .iter()
        .filter_map(|p| midi.port_name(p).ok())
        .filter(|n| !n.contains(CLIENT))
        .collect();
    ports.sort();
    ports
}
