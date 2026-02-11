# dawfile-reaper Performance Baseline

## Fixtures
- Tempo-heavy fixture (tracked): `/Users/codywright/Documents/Development/Rust/roam-test/modules/dawfile/dawfile-reaper/tests/fixtures/tempo-map-advanced.RPP`
- Large real-world fixture (local, git-ignored): `/Users/codywright/Documents/Development/Rust/roam-test/modules/dawfile/dawfile-reaper/tests/fixtures/local/Goodness of God.RPP`

## Commands
### CLI timing utility
```bash
cargo run -p dawfile-reaper --bin rpp_perf -- \
  --fixture modules/dawfile/dawfile-reaper/tests/fixtures/tempo-map-advanced.RPP \
  --fixture "modules/dawfile/dawfile-reaper/tests/fixtures/local/Goodness of God.RPP" \
  --warmup 1 \
  --repeat 3
```

### Criterion benchmark
```bash
cargo bench -p dawfile-reaper --bench parse_perf
```

Optional:
```bash
RPP_LARGE_FIXTURE="/absolute/path/to/large.RPP" \
  cargo bench -p dawfile-reaper --bench parse_perf
```

## Baseline (2026-02-10)
From `/Users/codywright/Documents/Development/Rust/roam-test/modules/dawfile/dawfile-reaper/tests/full_project_audit.rs` against `Goodness of God.RPP`:
- fixture size: `418.82 MB`
- `parse_rpp_file`: `40.276s` (~`10.40 MB/s`)
- typed conversion: `0.527s`

## Current (2026-02-11)
From `rpp_perf` against `Goodness of God.RPP` (`--repeat 3`, release):
- fixture size: `418.82 MB`
- parse avg: `0.3274s` (~`1279.21 MB/s`)
- typed avg (full mode): `0.0062s`
- peak RSS: `1254.86 MB`

## Allocation Telemetry (2026-02-11)
From `rpp_perf` with allocator counters enabled (`--repeat 3`, release):
- parse alloc calls avg: `4,175,570`
- parse allocated MB avg: `760.77`
- typed alloc calls avg: `231,507`
- typed allocated MB avg: `11.75`

Notes:
- Allocation telemetry mode adds overhead and should be used for relative allocation comparisons between commits, not absolute throughput comparisons.

## Hotspots Addressed
- Replaced recursive/nom-first project parsing with fast streaming stack parser (with compatibility fallback).
- Added fast token classification path to avoid generic parser overhead on common unquoted lines.
- Added specialized block-header parsing for common `<NAME ...>` headers to avoid full token parser in hot path.
- Added parallel typed decode for independent domains with deterministic merge.
- Reduced temporary allocations in fast tokenization (removed per-line `Vec<&str>` split collection and unconditional float normalization allocation).

## Notes
- `rpp_perf` reports `peak_rss_mb` via `getrusage(RUSAGE_SELF)`.
- macOS reports bytes directly, Linux reports KiB and is converted to bytes.
- Keep hardware and build profile consistent when comparing runs.
