# Reference implementations — what we read, and what we may use

Two REAPER projects worth reading for the audio editor. Their licences
differ, and the difference decides what we are allowed to do with each,
so it is stated first.

Both are cloned to `.reference/` (gitignored) by:

```bash
mkdir -p .reference && cd .reference
git clone --depth 1 https://github.com/b451c/SneakPeak.git
git clone --depth 1 https://github.com/MichaelPilyavskiy/ReaScripts.git mpl-reascripts
```

| project | licence | what that permits |
|---|---|---|
| [SneakPeak](https://github.com/b451c/SneakPeak) | **MIT** (b451c, 2025–2026) | port code, with attribution |
| [mpl ReaScripts](https://github.com/MichaelPilyavskiy/ReaScripts) | **no LICENSE file** | read only — clean-room |

A repository with no licence is all rights reserved, whatever its
visibility. mpl's scripts get the same treatment MPElodyne already gets
in this tree: read the algorithm, describe it here, design our own. No
lines cross over.

## SneakPeak — the REAPER audio API, done properly

A native C++17 extension, ~31k lines: a dockable waveform item editor
with dynamics, spectral view, multi-item layering and metering. It is
the closest thing to what we are building that exists for REAPER, and
it is doing the same API dance our `AudioSession` does.

### Two things it does that we were not

Both found by reading `waveform_view.cpp::LoadConcatenated`, and both
already fixed:

1. **Item volume has to be applied by hand.** A take audio accessor
   returns *source* audio; REAPER applies item and take gain at
   playback. SneakPeak multiplies by `D_VOL` on the item and again on
   the take after reading.

   This matters for more than looks. Our silence floor — the threshold
   separating a consonant from a gap between phrases — is absolute, so
   an item with the fader down would have had every frame below it and
   produced no sibilant spans at all.

   We apply the item's gain. **Take volume we cannot**: the `daw`
   facade's take service has `set_volume` and no getter. That is a gap
   worth closing, and until it is, a take-level trim is unaccounted for.

2. **Chunk size.** 64 k frames per `GetAudioAccessorSamples` call, which
   we now match. There is no reason to differ from a number already
   proven against real takes.

### Where it gets its format from

SneakPeak asks the `PCM_source` directly — `GetSampleRate()`,
`GetNumChannels()` — rather than probing the accessor, and clamps
channels to two. Our `read_all` probes with a one-sample read instead,
because the facade exposes the accessor and not the source. Asking the
source is the better answer and wants a facade method; the probe is a
workaround, and is marked as one.

### Worth reading before building the chrome

- `waveform_rendering.cpp` — peak + RMS at a dB scale with a
  zero-crossing line. Our backdrop is a plain peak envelope; theirs is
  what a mastering engineer expects to see.
- `spectral_view.cpp` — async FFT spectrogram with channel-pair packing
  (their README claims ~10× on stereo). If the audio editor ever grows
  a spectral view, start here rather than from a textbook.
- `minimap_view.cpp` — the overview strip the manual calls for, already
  solved against the same API.
- `marker_manager.cpp` — REAPER marker/region round-tripping.
- `loop_finder.cpp`, `deess_engine.cpp`, `spectral_repair.cpp` —
  adjacent features, all MIT, all portable if wanted.

## mpl — Align Takes

`Various/mpl_Align takes.lua`, and the forum thread that documents it
([t=179544](https://forum.cockos.com/showthread.php?t=179544)). It is
the free native answer to VocAlign / RevoicePro: pick a **reference**
take, pick a **dub**, and the dub is retimed to match by writing
**stretch markers** into it.

The shape of it, from the thread and the script's own UI:

1. *Get reference* — analyse the reference item's timing.
2. *Get dub* — analyse the item(s) to be matched.
3. Correspondences between the two are turned into stretch markers on
   the dub, so the retiming is non-destructive and stays editable in
   REAPER afterwards.

**Clean-room note.** The above is behaviour, taken from the thread and
the UI. The matching itself we design ourselves — and we are well
placed to, because we already have per-frame f0, RMS and unvoiced spans
from `analyze_take`, which is a richer feature set than a script
working through ReaScript can cheaply get.

### How alignment fits what we already have

Aligning two performances is a correspondence problem between two
analyses, and both halves already exist:

- `Analysis` gives per-frame pitch, level and voiced/unvoiced state for
  a take — that *is* the feature vector to align on. Onsets from the
  unvoiced→voiced transitions are the strongest cue in a vocal, and we
  already compute them.
- The result is a time map: source time → target time. We have one of
  those, `WarpMarker`, and a renderer that honours it
  (`render_world_warped`). So an alignment produces exactly the same
  artefact a timing edit does, and needs no new write path.

That is the whole reason this belongs in this editor rather than beside
it: alignment is the timing feature with its target computed from
another take instead of from a drag.
