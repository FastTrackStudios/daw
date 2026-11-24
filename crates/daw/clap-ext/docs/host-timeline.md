# `com.fasttrackstudio.host-timeline/1`

A CLAP host extension: the plugin reads its own track's **whole** MIDI
content and the tempo map, ahead of the playhead. The C side is
`include/fts/clap-ext.h` (`fts_host_timeline_t`), the Rust side is
`daw_clap_ext::{timeline, host::HostTimeline, plugin::HostTimelineClient}`.

What it is for: an orchestral sampler whose legato transitions take 100–330
ms to arrive (and whose shorts have a pre-delay) has to start each sample
*before* the note's time for it to be heard *on* the note's time. With the
track's timeline in hand it can do that for every note, each by its own
lead, on every channel, with no negative track delay and with **zero
reported latency**: the lead comes from reading ahead, not from delaying
everything else.

## The calls

| call | thread | what |
|---|---|---|
| `revision(host) -> u64` | any (lock-free) | The revision of what `read` returns. 0 while the plugin has no track. Moves on every change of the timeline. |
| `read(host, out) -> bool` | main | The whole timeline into `out`: the header, the notes, the controllers, the tempo map (below). `false` while there is none. |
| `set_playback_claimed(host, claimed) -> bool` | main | While claimed, the host stops delivering the timeline's own notes and controllers as `process()` events. `false`: the host cannot, and keeps delivering them. |

Change notification rides `host-actions/1`: a plugin that subscribes to the
topic **`timeline`** gets `{"revision": N}` on its `host-events/1` after
every change (Studio: from its 50 ms main-thread pump, after the edit that
changed it). The revision is also cheap to poll.

A plugin re-reads on the main thread, rebuilds what it plays from (the
sampler: annotates a new `Schedule`) off the audio thread, and swaps it in
lock-free at a block boundary.

## The stream

`fts_timeline_header_t`, then `note_count` × `fts_timeline_note_t`,
`ctrl_count` × `fts_timeline_ctrl_t`, `tempo_count` ×
`fts_timeline_tempo_t`, each array sorted by time. Native byte order (the
stream never leaves the process). Each element is `*_size` bytes: a later
minor version may append fields, and a reader steps by the size it was
given and reads the fields it knows (`Timeline::decode` does).

- **Notes**: `start_qn`, `end_qn`, `channel` 0–15, `key`, `velocity` 1–127,
  `release_velocity` (64 when the source has none), `port`.
- **Controllers**: CC (`number` = controller), pitch bend (14-bit, 8192
  centre), channel pressure, poly pressure (`number` = key), program.
- **Tempo**: each point's `qn`, its `seconds` (the host's own time of that
  QN), `bpm`, time signature, and `FTS_TIMELINE_TEMPO_RAMP` if the tempo
  ramps linearly (in QN) to the next point; otherwise it steps there. The
  first point is at QN 0; an empty map is 120 bpm 4/4.

## Time: the document epoch

Every position is **project quarter notes from project time zero**, as the
host *plays* the track:

- item positions applied (a note's QN is the item's start QN plus the
  note's offset into its take);
- the take's start offset applied, notes cut at the item's ends (a note
  that starts inside the item and runs past its end ends at the item's
  end; a note starting before the item's start is not played);
- a looped item (`loop source`) repeats its source for the item's length;
- muted items and muted notes left out; overlapping items merged into one
  sorted timeline.

QN 0 = project second 0 = the CLAP transport's song position 0
(`song_pos_beats` = 0, `song_pos_seconds` = 0). That instant is the
**document epoch**: a plugin's schedule is in frames from it.

## The transport

The process call's `clap_event_transport` is the authority for **where**
each block is; the timeline says **what** is there.

- **Position.** The block's first frame is at `song_pos_seconds × rate`
  frames from the epoch (nice-plug: `transport.pos_samples()`). A host that
  offers this extension starts a new process call at every discontinuity —
  a locate, a loop wrap — so a block's first frame is always exactly its
  position; a block never straddles a wrap. (Studio's engine splits its
  block plan at both.)
- **Playing.** `CLAP_TRANSPORT_IS_PLAYING` is set only while the song
  position advances with the audio. A host that renders while stopped (to
  play live input through) clears it, so the plugin knows to stay live.
- **Loops.** Nothing special: the host plays to `loop_end`, wraps to
  `loop_start` in a new process call, and the plugin sees a jump. A
  plugin that replays from the timeline (the sampler's voice
  reconstruction) is then exactly where a linear play-through would be at
  `loop_start`, including a legato transition that started before it.
  `loop_start_*`/`loop_end_*` and `IS_LOOP_ACTIVE` are informational.
- **Tempo, ramps.** The host's position already includes the tempo map;
  the plugin maps the timeline's QN to frames with the map's points (their
  `seconds` are exact anchors). In a ramp the tempo is linear in QN; a
  plugin that works in steps subdivides the ramp (the sampler: 1/64 QN
  steps whose durations are the ramp's exact integral, so every step
  boundary lands exactly). If the transport's tempo disagrees with the
  map at the playhead, the plugin's copy is stale: it follows the host's
  position and re-reads.
- **Pre-roll.** Starting at P with a pre-roll R, the host plays from P − R
  with the transport rolling. The plugin simply plays from P − R; every
  prefire between P − R and P happens, so what is heard from P is the
  full render at P.
- **Count-in.** A count-in that moves the song position (from P − bars)
  is a pre-roll. One before project zero runs negative positions:
  nothing in the timeline is before QN 0, and a sample that must start
  before QN 0 to arrive on QN 0 starts at the first block, inside the
  count-in. A count-in that freezes the song position (a metronome only)
  must not set `IS_PLAYING`.
- **Varispeed** (a transport rate ≠ 1) is outside version 1: the position
  then advances by `frames × rate` per block, which a frame-exact schedule
  sees as a jump every block. A plugin may give the claim back and play
  the host's events live under varispeed.

## Channels: per-channel content, per-note pre-starts

Every note keeps its channel, so a plugin can give each channel its own
voice and its own timing. How a library maps them is the plugin's choice;
the Signal sampler's:

- **One channel = one line.** Channel *n* is engine line *n*: a
  monophonic legato chain with its own sounding note, its own transition
  choice and its own prefire per note. Four channels of strings are four
  divisi lines, each pre-starting each note by its own lead (velocity →
  legato delay; a short's pre-delay; a sustain's measured arrival).
- **Articulations by keyswitch, per channel.** CC58 (the Cinematic Studio
  libraries) or keyswitch notes select the articulation of the line
  they're on; the timeline's controllers keep their channel, so channel 0
  can play legato while channel 1 plays spiccato.
- **A chord on one channel** is split into lines top to bottom by the
  sampler's auto-divisi (`TrackDocument::auto_divisi`) before legato is
  inferred.

## Live input beside the timeline

While a plugin claims playback, what still arrives as `process()` events
is live: a keyboard, a MIDI input routed to the track, a programmatic
note. It cannot be pre-fired — it has no future — so the plugin plays it
the way it plays without a timeline (the sampler's strict-live path:
reactive legato, the low-latency curve). The sampler runs it on a second
engine whose output is summed with the timeline's, so playing over a
track never disturbs the timeline's exact playback. With the claim given
back, the host delivers the track's notes again and the plugin is a plain
live instrument.

## Latency

A plugin playing from the timeline reports latency 0. Its lead is the
read-ahead. It must not report the prefire as latency: the host would then
delay every other track to match, which is the problem this removes.

## Hosts without it

A plugin must keep working where `get_extension` returns NULL. The Signal
sampler then plays live, and offers two ways to the lookahead:

1. **A MIDI file in Signal**: the part's `.mid` given to the sampler. It
   builds the same timeline from the file, with the file's tempo map, and
   plays it from the host's transport — the same exact playback, the file
   kept in step with the project by hand.
2. **Latency as an opt-in**: the plugin reports a latency equal to its
   longest prefire (333 ms for the Cinematic Studio strings' slow legato)
   and holds each live event back by that latency minus the note's own
   lead. Every note lands on time in any DAW, from the track's own MIDI.
   The trade-off: the host's delay compensation delays the whole mix by
   that latency while monitoring and recording — unusable for a live
   player — and no lead can exceed the reported latency.
