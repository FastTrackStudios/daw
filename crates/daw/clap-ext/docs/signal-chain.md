# `signal.chain/1`: a Signal instance's chain, as views read it

`signal/1`'s `chain()` writes this JSON. A host's mixer draws it as if
every block were its own insert, although the track holds one plugin; a
Signal editor in another DAW draws the same data, so the mixer looks and
works the same in Studio, REAPER or Bitwig.

`revision()` (thread-safe) increases on every change to anything below
except `value`/`normalized`/`text` (those are CLAP parameter values, read
and set through the ordinary params extension and its rescan), and on
every state load. A host polls `revision()` (cheap) and re-reads `chain()`
when it moves. When the blocks or their parameters change, the plugin also
calls the host's `clap_host_params.rescan(CLAP_PARAM_RESCAN_INFO |
CLAP_PARAM_RESCAN_TEXT)`, and, when the host offers `host-actions/1`, runs
its action `SIGNAL_CHAIN_CHANGED {"port": <port or null>, "revision": n}`
(host-actions/1 has no plugin-to-host event channel; an action is the
event; a host without the action answers an error, which the plugin
ignores).

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
          "id": "rescue.gate",           // `<phase>.<block id>`: stable across sessions and rebuilds; a move to another phase changes the prefix
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
      ],
      "meters": [                        // the phase's output, dB, K-weighted over the last block
        { "id": "level", "name": "Rescue level", "unit": "dB", "index": 7 }
      ]
    }
  ],
  "macros": [ { "id": "macro_1", "name": "Punch", "clap_id": "macro_1", "targets": ["tone.comp.ratio"] } ]
}
```

- **Phases** are the mix method's, in order: `rescue`, `tone`, `polish`,
  `relational`, `depth`, `creative` (Studio's `MixPhase` slugs; Balance
  is the faders and has no blocks). A block outside them, one that makes
  sound rather than processing it (a sampler on a Sample track), is in a
  phase `{"id": "source", "name": "Source"}`, first; its `bypass` and
  `makeup_db` are always `false` and `0`.
- **`key.via`**: `hits`, a trigger's detected hits; `midi`, hit notes (a
  piece with notes ignores its trigger's detection while the notes come);
  `self`, no hit for ten seconds, so a gate keys from its own input.
- **A meter's `index`** is its position in `mod_source_count`/
  `mod_source_info`: blocks' sources and phases' levels share one list.
- **The kit mode** (one instance running several lanes) has `"phases": []`
  and `"lanes": [{"lane": "<path>", "phases": [...]}, ...]`, each lane a
  strip.

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
- `SIGNAL_CHAIN {}` answers the chain (as `chain()`); `SIGNAL_ALGORITHMS
  {"kind": "eq"}` a family's algorithms, `[{"id", "name"}]`, for
  `SIGNAL_SET {"block", "algorithm"}` and `SIGNAL_BLOCK_ADD`'s
  `"algorithm"`.
- **The whole kit** (a Signal editor showing every port of the session):
  `run_action("SIGNAL_SESSION", {})` answers
  `{"session": …, "ports": [<signal.chain/1>, …]}`, every instance in the
  process that shares this one's session.

## What Signal subscribes to (`host-actions/1`)

When the host offers `host-actions/1`, a Signal instance reads its files
through `load_file` (the disk when the host doesn't resolve one) and
subscribes to:

- `transport`: `{"playing": bool, "frame": <song position, samples>}`, the
  clock its hits are stamped with when the plugin API gives no transport;
- `hits:<piece>` for each piece a block of it is keyed to, `<piece>` the
  piece's track path in the kit (`Drums/Kick`, `Drums/Toms/Tom 1`): each
  hit `{"frame": <song position>, "velocity": 0-1}`, the host's hit
  markers, published to the session as notes (they key gates as MIDI
  does).

## Additions (2026-11-24, from the mixer view)

These are part of `signal.chain/1`; a reader treats each as optional until
every Signal build sends it.

1. **A block's running algorithm**: `"algorithm": "<id>"` on every block
   whose family has algorithms (the id `SIGNAL_ALGORITHMS` lists).
2. **Values move a second counter.** `values_revision()` is not in the C
   header yet; until it is, the chain carries `"values_revision": n`, bumped
   whenever any parameter value changes (a set, automation, a macro), and
   the host's live feed carries it beside `revision`. A view re-reads only
   `value`/`normalized`/`text` when it moves. A chain read always has
   current values.
3. **Each meter says how to draw it**: `"kind": "reduction" | "level" |
   "state"` (`reduction`: dB of gain taken away, 0 = none, drawn downwards;
   `level`: dBFS or LU, drawn upwards; `state`: 0 or 1, a gate open), with
   `"min"` and `"max"` in its unit.
4. **Kit mode addressing**: block ids are unique within a lane only. Every
   structural action (`SIGNAL_SET`, `SIGNAL_BLOCK_ADD`, `SIGNAL_BLOCK_REMOVE`,
   `SIGNAL_BLOCK_MOVE`) takes an optional `"lane": "<path>"`, required in
   kit mode.
5. **`key` says what it wanted and what it uses**: `"from"` is the source
   configured (the Trig track, or `null`), `"via"` what keys it now; when
   it fell back, `"fallback": "no hits for 10 s"` (a short reason).
6. **Host side (Studio's `FX_SET`)**: a parameter is named by its CLAP id as
   `"@<clap_id>"` (`"@b014"`), as well as by name or `#index`.
7. **`clap_id` is the CLAP parameter id** (`clap_param_info.id`, a
   `uint32`) written as a decimal string (`"1407"`), exactly what the
   host's params extension reports, never a nice-plug string id or a slot
   label. Studio's `FX_SET "@1407"` matches it first. (A slot label such as
   `b014` is the parameter's `id` field's business, not `clap_id`'s: a label
   of hex digits would read as a number.)
