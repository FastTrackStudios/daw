/*
 * FastTrackStudio's CLAP extensions (signal#26).
 *
 *   com.fasttrackstudio.host-actions/1  the plugin calls the host: its actions
 *                                       (JSON in, JSON out), events, files
 *   com.fasttrackstudio.host-events/1   the plugin's half: where events arrive
 *   com.fasttrackstudio.signal/1        the host drives the plugin: Signal's
 *                                       actions, the chain as data, stable
 *                                       parameter ids, modulation sources
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
