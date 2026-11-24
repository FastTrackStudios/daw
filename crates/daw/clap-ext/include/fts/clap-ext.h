/*
 * FastTrackStudio's CLAP extensions (signal#26).
 *
 *   com.fasttrackstudio.host-actions/1  the plugin calls the host: its actions
 *                                       (JSON in, JSON out), events, files
 *   com.fasttrackstudio.host-events/1   the plugin's half: where events arrive
 *   com.fasttrackstudio.signal/1        the host drives the plugin: Signal's
 *                                       actions, the chain as data, stable
 *                                       parameter ids, modulation sources
 *   com.fasttrackstudio.host-timeline/1 the plugin reads its track's whole
 *                                       MIDI and the tempo map, ahead of the
 *                                       playhead, and may claim its playback
 *
 * Every string is UTF-8 and NUL-terminated. JSON goes out through a
 * clap_ostream, with no size limit and no buffer negotiation. Everything is
 * [main-thread] except fts_plugin_signal.revision and
 * fts_host_timeline.revision ([thread-safe]) and
 * fts_plugin_signal.mod_source_value ([audio-thread]).
 *
 * The Rust side of this header is the crate it ships in, daw-clap-ext.
 * GPL-3.0-or-later.
 */
#pragma once
#include <clap/clap.h>

/* ── Host extension: the plugin calls the host ─────────────────────── */
static CLAP_CONSTEXPR const char FTS_EXT_HOST_ACTIONS[] = "com.fasttrackstudio.host-actions/1";

typedef struct fts_host_actions {
   // The host's actions as JSON: [{"id","description","args_schema"?}, ...].
   // Returns false when the host has none to give.
   // [main-thread]
   bool(CLAP_ABI *list)(const clap_host_t *host, const clap_ostream_t *out_json);

   // Describe one action (the same object as `list` gives, one entry).
   // [main-thread]
   bool(CLAP_ABI *describe)(const clap_host_t *host, const char *action_id,
                            const clap_ostream_t *out_json);

   // Run an action: JSON arguments in, its JSON result out, synchronously.
   // An action that takes long returns {"job": <id>} and reports completion
   // as a `job` event (below). On failure the result is {"error": "..."}
   // and the call returns false.
   // [main-thread]
   bool(CLAP_ABI *run)(const clap_host_t *host, const char *action_id,
                       const char *args_json, const clap_ostream_t *out_json);

   // Ask for events on a topic ("hits:Drums/Kick", "tempo", "transport",
   // "job"), delivered through the plugin's fts_plugin_host_events. Returns
   // a subscription id, 0 if the topic is unknown.
   // [main-thread]
   uint64_t(CLAP_ABI *subscribe)(const clap_host_t *host, const char *topic);
   void(CLAP_ABI *unsubscribe)(const clap_host_t *host, uint64_t subscription);

   // A file the plugin needs by reference ("rig-library:guitar/models/x.nam"),
   // resolved by the host: what SignalHost::load_file asks for. Captures are
   // private: the host may refuse, and returns false.
   // [main-thread]
   bool(CLAP_ABI *load_file)(const clap_host_t *host, const char *reference,
                             const char *blake3_or_null, const clap_ostream_t *out_bytes);
} fts_host_actions_t;

/* The plugin's half of host-actions: where subscribed events arrive. */
static CLAP_CONSTEXPR const char FTS_EXT_HOST_EVENTS[] = "com.fasttrackstudio.host-events/1";

typedef struct fts_plugin_host_events {
   // [main-thread]
   void(CLAP_ABI *on_event)(const clap_plugin_t *plugin, uint64_t subscription,
                            const char *topic, const char *event_json);
} fts_plugin_host_events_t;

/* ── Plugin extension: the host drives Signal (and a block's CLAP) ─── */
static CLAP_CONSTEXPR const char FTS_EXT_SIGNAL[] = "com.fasttrackstudio.signal/1";

typedef struct fts_signal_mod_source_info {
   char id[CLAP_NAME_SIZE];   // stable id: "gain_reduction"
   char name[CLAP_NAME_SIZE]; // "Gain Reduction"
   char unit[16];             // "dB", "%", "Hz", ""
} fts_signal_mod_source_info_t;

typedef struct fts_plugin_signal {
   /* Actions: SIGNAL_CHAIN, SIGNAL_SET, SIGNAL_PRESET_LOAD, SIGNAL_BLOCK_DEV, …
    * The same list and JSON as the linked core answers, so Studio's MCP and
    * Luau proxy them unchanged ({"track": "Kick/Sum"} picks the instance). */
   // [main-thread]
   bool(CLAP_ABI *list_actions)(const clap_plugin_t *plugin, const clap_ostream_t *out_json);
   // [main-thread]
   bool(CLAP_ABI *run_action)(const clap_plugin_t *plugin, const char *action_id,
                              const char *args_json, const clap_ostream_t *out_json);

   /* The chain as data for the views: the preset and the build report
    * (each block's backend and status). `revision` increases on every
    * change, so a view polls it cheaply. */
   // [main-thread]
   bool(CLAP_ABI *chain)(const clap_plugin_t *plugin, const clap_ostream_t *out_json);
   // [thread-safe]
   uint64_t(CLAP_ABI *revision)(const clap_plugin_t *plugin);

   /* Stable parameter ids: a CLAP parameter's Signal id ("high_gain"), the
    * same as the compiled-in block's, so a preset and its macros address the
    * block the same way on either backend. */
   // [main-thread]
   bool(CLAP_ABI *param_stable_id)(const clap_plugin_t *plugin, clap_id param_id,
                                   char *out, uint32_t out_capacity);

   /* Modulation sources the plugin publishes (a block's envelope, its gain
    * reduction). Values are read after each process() of the block. */
   // [main-thread]
   uint32_t(CLAP_ABI *mod_source_count)(const clap_plugin_t *plugin);
   // [main-thread]
   bool(CLAP_ABI *mod_source_info)(const clap_plugin_t *plugin, uint32_t index,
                                   fts_signal_mod_source_info_t *info);
   // [audio-thread]: lock-free, between process() calls
   double(CLAP_ABI *mod_source_value)(const clap_plugin_t *plugin, uint32_t index);

   /* Per-track mode: which session and port this instance is. `session_id`
    * is what instances in one process meet by; NULL leaves the session. */
   // [main-thread]
   bool(CLAP_ABI *set_session)(const clap_plugin_t *plugin, const char *session_id,
                               const char *port);
} fts_plugin_signal_t;

/* ── Host extension: the track's timeline, ahead of the playhead ───── */
/*
 * com.fasttrackstudio.host-timeline/1 — the plugin reads its own track's
 * whole MIDI content and the tempo map, so it can act BEFORE a note's
 * time: an orchestral sampler fires each legato transition early by its
 * own measured lead, so the transition is HEARD on the grid, without a
 * negative track delay and without reporting latency.
 *
 * The full contract (time base, transport, loops, count-in, channels,
 * live input) is crates/daw/clap-ext/docs/host-timeline.md. In short:
 *
 *  - TIME. Every position is in quarter notes (QN) of the PROJECT, from
 *    project time zero, as the host plays the track: item positions,
 *    take offsets, item loops and mutes already applied, overlapping
 *    items merged. QN 0 = second 0 = CLAP transport song position 0
 *    (song_pos_beats 0, song_pos_seconds 0): the "document epoch". The
 *    tempo map gives each point's seconds too, so a plugin never
 *    integrates the map itself to find where the host's seconds are.
 *  - PLAYHEAD. The process call's clap_event_transport is the authority
 *    for where a block is (song_pos_seconds of its first frame); the
 *    timeline says what is there. A host that offers this extension
 *    starts a new process call at every discontinuity (a seek, a loop
 *    wrap), so a block's first frame is always exactly its position.
 *  - REVISION. Moves on every change of what `read` returns (an edit of
 *    the track's MIDI, its items, or the tempo map). Thread-safe, so the
 *    audio thread can notice a stale schedule. The host also sends a
 *    "timeline" event, {"revision": N}, to every plugin that subscribed
 *    to the topic "timeline" through host-actions/1.
 *  - PLAYBACK. While a plugin claims playback (set_playback_claimed), the
 *    host no longer delivers the timeline's own notes and controllers as
 *    process() events — the plugin plays them from what it read. Live
 *    input (a keyboard, a MIDI input routed to the track) still arrives
 *    as events. Unclaimed, the host delivers everything as usual.
 */
static CLAP_CONSTEXPR const char FTS_EXT_HOST_TIMELINE[] = "com.fasttrackstudio.host-timeline/1";

/* "FTL1", little-endian. */
#define FTS_TIMELINE_MAGIC 0x314C5446u

/* What a controller event is. */
enum {
   FTS_TIMELINE_CC = 0,               // number = controller, value 0..127
   FTS_TIMELINE_PITCH_BEND = 1,       // value 0..16383, 8192 centre
   FTS_TIMELINE_CHANNEL_PRESSURE = 2, // value 0..127
   FTS_TIMELINE_POLY_PRESSURE = 3,    // number = key, value 0..127
   FTS_TIMELINE_PROGRAM = 4,          // value = program 0..127
};

/* A tempo point's flag: the tempo ramps linearly, in QN, from this point's
 * bpm to the next point's. Without it the tempo steps at the next point. */
#define FTS_TIMELINE_TEMPO_RAMP (1u << 0)

typedef struct fts_timeline_note {
   double start_qn;          // note-on
   double end_qn;            // note-off (> start_qn)
   uint8_t channel;          // 0..15
   uint8_t key;              // 0..127
   uint8_t velocity;         // 1..127
   uint8_t release_velocity; // 0..127, 64 when the source has none
   uint16_t port;            // note port index (0 for the main port)
   uint16_t reserved;        // 0
} fts_timeline_note_t;       // 24 bytes

typedef struct fts_timeline_ctrl {
   double qn;
   uint8_t kind;    // FTS_TIMELINE_*
   uint8_t channel; // 0..15
   uint8_t number;  // the controller (CC) or the key (poly pressure), else 0
   uint8_t reserved;
   uint16_t value;
   uint16_t port;
} fts_timeline_ctrl_t; // 16 bytes

typedef struct fts_timeline_tempo {
   double qn;        // where this tempo starts; the first point is at QN 0
   double seconds;   // the same instant in project seconds, the host's own
   double bpm;       // quarter notes per minute
   uint16_t numerator;   // the time signature from here
   uint16_t denominator;
   uint32_t flags;   // FTS_TIMELINE_TEMPO_*
} fts_timeline_tempo_t; // 32 bytes

/* What `read` writes: this header, then note_count notes, ctrl_count
 * controllers and tempo_count tempo points, each array sorted by time and
 * each element *_size bytes (a later minor version may append fields to an
 * element; a reader steps by *_size and reads the fields it knows). Native
 * byte order: the stream never leaves the process. */
typedef struct fts_timeline_header {
   uint32_t magic;       // FTS_TIMELINE_MAGIC
   uint32_t header_size; // sizeof(fts_timeline_header_t)
   uint64_t revision;    // the revision this snapshot is
   uint32_t note_count;
   uint32_t ctrl_count;
   uint32_t tempo_count;
   uint32_t note_size;   // sizeof(fts_timeline_note_t)
   uint32_t ctrl_size;   // sizeof(fts_timeline_ctrl_t)
   uint32_t tempo_size;  // sizeof(fts_timeline_tempo_t)
   double end_qn;        // where the track's content ends (its last item's end)
} fts_timeline_header_t; // 48 bytes

typedef struct fts_host_timeline {
   // The revision of what `read` returns; 0 while the plugin has no track
   // (before it is placed on one). Lock-free.
   // [thread-safe]
   uint64_t(CLAP_ABI *revision)(const clap_host_t *host);

   // The whole timeline: header + arrays (above) into out. Returns false
   // while there is none (revision 0).
   // [main-thread]
   bool(CLAP_ABI *read)(const clap_host_t *host, const clap_ostream_t *out);

   // Claim (true) or give back (false) the timeline's playback: see
   // PLAYBACK above. Returns false if the host cannot stop delivering
   // the track's events (the plugin must then ignore them itself).
   // [main-thread]
   bool(CLAP_ABI *set_playback_claimed)(const clap_host_t *host, bool claimed);
} fts_host_timeline_t;
