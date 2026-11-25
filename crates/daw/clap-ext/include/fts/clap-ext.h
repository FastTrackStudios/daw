/*
 * FastTrackStudio's CLAP extensions (signal#26).
 *
 *   com.fasttrackstudio.host-actions/1  the plugin calls the host: its actions
 *                                       (JSON in, JSON out), events, files
 *   com.fasttrackstudio.host-events/1   the plugin's half: where events arrive
 *   com.fasttrackstudio.signal/1        the host drives the plugin: Signal's
 *                                       actions, the chain as data, stable
 *                                       parameter ids, modulation sources
 *   com.fasttrackstudio.host-automation/1
 *                                       the plugin reads and writes the host's
 *                                       automation lanes of its OWN parameters
 *
 * Every string is UTF-8 and NUL-terminated. JSON goes out through a
 * clap_ostream, with no size limit and no buffer negotiation. Everything is
 * [main-thread] except fts_plugin_signal.revision ([thread-safe]) and
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

/* ── Host extension: the plugin's own parameters' automation ──────── */
/*
 * CLAP gives a plugin no way to write host automation. This does, for the
 * plugin's own parameters only: every call names a clap_id of the CALLING
 * plugin, and a host refuses any other. What it is for: a block that analyses
 * a whole track once (a vocal leveler, an aligner) writes its result as
 * ordinary parameter automation, which the host then shows, plays, edits,
 * undoes and moves with edits like any other lane.
 *
 * Times are seconds on the project's timeline (the song's time, from its
 * start, after tempo: what the host's transport calls 0 s). The host converts
 * to its own units (beats, ticks, samples). A range is [from, to); `to` may
 * be INFINITY for "to the end".
 *
 * Values are normalized 0..1, exactly as clap_plugin_params' value_to_text
 * reads them after the plugin's own mapping.
 *
 * Points are JSON:
 *   {"t": 12.5, "value": 0.42, "shape": "linear", "tension": 0.0, "by": "plugin"}
 *   shape:   "linear" | "square" | "slow" | "fast" | "bezier" (the curve from
 *            this point to the next; "square" holds until the next)
 *   tension: -1..1, for "bezier" only (optional)
 *   by:      who last wrote the point: "plugin" (through this extension) or
 *            "user" (drawn, dragged, recorded). The host keeps this per
 *            point; `read` returns it, `write` ignores it.
 *
 * Every function is [main-thread]. A host must not hold, while it answers
 * one, any lock that a call into this plugin would need: the plugin may be
 * inside one of the host's calls into it (its lock held by the host).
 */
static CLAP_CONSTEXPR const char FTS_EXT_HOST_AUTOMATION[] = "com.fasttrackstudio.host-automation/1";

enum {
   // Make the parameter's lane when it has none (otherwise `write` fails).
   FTS_AUTOMATION_CREATE_LANE = 1 << 0,
   // Show the lane in the host's arrangement.
   FTS_AUTOMATION_SHOW_LANE = 1 << 1,
   // Never replace a point whose `by` is "user": it stays, and the points
   // written around it go in beside it. `write` reports each kept point.
   FTS_AUTOMATION_KEEP_USER_EDITS = 1 << 2,
};

typedef struct fts_host_automation {
   // The plugin's parameters that have lanes in the host:
   //   [{"param_id": 1407, "mode": "read"|"touch"|"latch"|"write"|"trim"|"off",
   //     "shown": true, "points": 312, "user_points": 4}, ...]
   bool(CLAP_ABI *lanes)(const clap_host_t *host, const clap_ostream_t *out_json);

   // The points of `param_id`'s lane in [from, to), in time order:
   //   {"param_id": 1407, "points": [<point>, ...]}
   // Returns false (and {"error": ...}) when the parameter has no lane or is
   // not the calling plugin's.
   bool(CLAP_ABI *read)(const clap_host_t *host, clap_id param_id, double from, double to,
                        const clap_ostream_t *out_json);

   // Replace the points of `param_id`'s lane in [from, to) with `points_json`
   // (a JSON array of points, each `t` inside the range). `flags` is the
   // FTS_AUTOMATION_* bits. Answers:
   //   {"written": 120, "kept": [<point>, ...], "created": false}
   // `kept`: the user's points left in place (FTS_AUTOMATION_KEEP_USER_EDITS),
   // with what the plugin wrote at their times omitted.
   bool(CLAP_ABI *write)(const clap_host_t *host, clap_id param_id, double from, double to,
                         const char *points_json, uint32_t flags,
                         const clap_ostream_t *out_json);

   // Delete `param_id`'s points in [from, to). The lane stays.
   bool(CLAP_ABI *clear)(const clap_host_t *host, clap_id param_id, double from, double to);

   // Group every write and clear until end_edit into one undo step in the
   // host, named `label`. Not nested: a second begin_edit before end_edit
   // returns false.
   bool(CLAP_ABI *begin_edit)(const clap_host_t *host, const char *label);
   bool(CLAP_ABI *end_edit)(const clap_host_t *host);
} fts_host_automation_t;

/* The user changed one of the plugin's lanes: delivered on topic
 * "automation" (subscribed through host-actions/1's `subscribe`, received by
 * fts_plugin_host_events.on_event) as
 *   {"param_id": 1407, "from": 12.0, "to": 14.5, "by": "user"}
 * A plugin's own writes through this extension are not reported. */
static CLAP_CONSTEXPR const char FTS_AUTOMATION_TOPIC[] = "automation";

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
