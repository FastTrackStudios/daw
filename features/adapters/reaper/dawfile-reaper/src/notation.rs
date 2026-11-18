//! REAPER's MIDI notation events, in and out of item chunks.
//!
//! Inside a `<SOURCE MIDI>` block, notation lives in `<X>` blocks whose
//! payload is base64:
//!
//! ```text
//! <X 480 0 4294966816 -1
//!   /w9UUkFDIGR5bmFtaWMgcHA=
//! >
//! ```
//!
//! Decoded that is `FF 0F` — a sequencer-specific meta event — followed
//! by ASCII beginning `TRAC`:
//!
//! ```text
//! TRAC dynamic pp
//! TRAC text "Something Here"
//! ```
//!
//! This is the editable half of REAPER's notation. Unlike the project's
//! `<KEYSIG>` block (see [`crate::keysig`]), item chunks *can* be
//! round-tripped on a live project — `GetItemStateChunk` /
//! `SetItemStateChunk` both exist — so anything expressible here can be
//! written by a tool.
//!
//! ## What is not here
//!
//! Key signatures. Two real projects' worth of evidence — one of them
//! built specifically to exercise key signatures, with ten changes — put
//! every key signature in the project-level `<KEYSIG>` block and none in
//! any item. No standard MIDI key-signature meta (`FF 59`) either. If
//! REAPER can write a per-track key signature (its MIDI editor has a
//! "Key signature changes affect all tracks" toggle, action 41616), that
//! would produce a file shape not represented in either project, and
//! guessing its bytes would be inventing a format rather than reading
//! one.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

/// A REAPER notation event attached to a position in a take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotationEvent {
    /// Position within the take, in the source's PPQ.
    pub ppq: u64,
    /// The command after the `TRAC` prefix, e.g. `dynamic pp` or
    /// `text "Something Here"`.
    pub command: String,
}

impl NotationEvent {
    pub fn new(ppq: u64, command: impl Into<String>) -> Self {
        Self {
            ppq,
            command: command.into(),
        }
    }

    /// The event's kind — the first word of the command (`dynamic`,
    /// `text`, …).
    pub fn kind(&self) -> &str {
        self.command.split_whitespace().next().unwrap_or("")
    }

    /// Encode to the base64 payload REAPER stores.
    pub fn encode(&self) -> String {
        let mut bytes = vec![0xFF, 0x0F];
        bytes.extend_from_slice(b"TRAC ");
        bytes.extend_from_slice(self.command.as_bytes());
        B64.encode(bytes)
    }

    /// Decode one payload, or `None` if it isn't a `TRAC` notation event.
    pub fn decode(ppq: u64, payload: &str) -> Option<Self> {
        let raw = B64.decode(payload.trim()).ok()?;
        let rest = raw.strip_prefix(&[0xFF, 0x0F])?;
        let text = std::str::from_utf8(rest).ok()?;
        let command = text.strip_prefix("TRAC ")?;
        Some(Self::new(ppq, command))
    }
}

/// Every notation event in a chunk, in file order.
///
/// Works on an item chunk, a take, or a whole project — it scans for
/// `<X>` blocks and ignores everything else.
pub fn parse(chunk: &str) -> Vec<NotationEvent> {
    let mut out = Vec::new();
    let mut lines = chunk.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("<X ") else {
            continue;
        };
        let Some(ppq) = rest
            .split_whitespace()
            .next()
            .and_then(|t| t.parse::<u64>().ok())
        else {
            continue;
        };
        // The payload is the next non-empty line before the closing `>`.
        if let Some(payload) = lines.peek().map(|l| l.trim().to_string()) {
            if payload != ">" {
                if let Some(event) = NotationEvent::decode(ppq, &payload) {
                    out.push(event);
                }
            }
        }
    }
    out
}

/// Render an event as the `<X>` block REAPER writes.
///
/// The three numbers after the position are flags REAPER uses for its own
/// bookkeeping; `0 0 -1` is what it accepts for a freshly authored event.
pub fn render_block(event: &NotationEvent, indent: &str) -> String {
    format!(
        "{indent}<X {} 0 0 -1\n{indent}  {}\n{indent}>\n",
        event.ppq,
        event.encode()
    )
}
