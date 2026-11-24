# `signal.chain/1`: a Signal instance's chain, as views read it

`signal/1`'s `chain()` writes this JSON. A host's mixer draws it as if
every block were its own insert, although the track holds one plugin; a
Signal editor in another DAW draws the same data, so the mixer looks and
works the same in Studio, REAPER or Bitwig.

`revision()` (thread-safe) increases on every change to anything below
except `value`/`normalized`/`text` (those are CLAP parameter values, read
and set through the ordinary params extension and its rescan). A host
polls `revision()` (cheap) and re-reads `chain()` when it moves. When the
blocks or their parameters change, the plugin also calls the host's
`clap_host_params.rescan(CLAP_PARAM_RESCAN_INFO | CLAP_PARAM_RESCAN_TEXT)`,
and a host that implements `host-actions/1` gets a `chain` event too.

```jsonc
{
  "schema": "signal.chain/1",
  "revision": 42,
  "session": "unbreakable-drums",       // null: not in a shared session
  "port": "Drums/Kick/Sum/In",          // this instance's track path in the kit, null in single-instance mode
  "mode": "per-track",                  // or "kit" (one multi-channel instance with its internal submixer)
  "midi": {                             // what the note input does (null: no note input)
    "plays": [{ "note": 36, "piece": "Kick", "block": "sample.kick" }],
    "publishes_hits": true              // notes in become the session's hits for that piece (gates elsewhere key on them)
  },
  "phases": [
    {
      "id": "rescue", "name": "Rescue",
      "bypass": false,
      "makeup_db": 1.7,                  // the phase's gain-neutral make-up
      "blocks": [
        {
          "id": "rescue.gate",           // stable across sessions and rebuilds
          "kind": "gate",                // eq | comp | gate | saturate | reverb | delay | nam | sampler | trigger | clap | …
          "name": "Gate",
          "bypass": false,
          "backend": "linked",           // or "clap" (a block's CLAP or a third-party plugin)
          "status": "ok",                // or "unavailable" (bypassed: cannot load here), "error"
          "key": { "from": "Drums/Kick/Sum/Trig", "via": "hits" },   // null; via: "hits" | "midi" | "self" (fell back to its own input)
          "params": [
            {
              "id": "threshold",          // the block's stable parameter id (same compiled in or as its CLAP)
              "clap_id": "b014",         // the slot this parameter is exposed on (CLAP param id string); null if not exposed
              "name": "Threshold",
              "value": -32.0, "normalized": 0.41, "text": "-32.0 dB",
              "unit": "dB", "min": -80.0, "max": 0.0, "default": -40.0,
              "choices": []               // for stepped params: ["Off", "On"]
            }
          ],
          "meters": [                    // live values: signal/1 mod_source_value(index) (audio-thread, lock-free)
            { "id": "gain_reduction", "name": "GR", "unit": "dB", "index": 3 }
          ]
        }
      ]
    }
  ],
  "macros": [ { "id": "macro_1", "name": "Punch", "clap_id": "macro_1", "targets": ["tone.comp.ratio"] } ]
}
```

## Editing from a view

- **A parameter's value**: the CLAP parameter `clap_id`, through the host's
  ordinary param path (an event while processing, `flush` while stopped),
  so automation and undo work as for any plugin.
- **Everything structural** (bypass a block or a phase, add, remove or
  move a block, change a block's kind, a phase's make-up): `run_action`.
  - `SIGNAL_SET {"block": "rescue.gate", "bypass": true}` (or `"phase": "rescue"`, `"makeup_db": 0.5`)
  - `SIGNAL_BLOCK_ADD {"phase": "tone", "kind": "eq", "after": "tone.comp"}`
  - `SIGNAL_BLOCK_REMOVE {"block": "tone.eq"}`, `SIGNAL_BLOCK_MOVE {"block": …, "phase": …, "after": …}`
  Each answers the new `signal.chain/1` and bumps `revision`.
- **The whole kit** (a Signal editor showing every port of the session):
  `run_action("SIGNAL_SESSION", {})` answers
  `{"session": …, "ports": [<signal.chain/1>, …]}`, every instance in the
  process that shares this one's session.
