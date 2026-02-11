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

## Notes
- `rpp_perf` reports `peak_rss_mb` via `getrusage(RUSAGE_SELF)`.
- macOS reports bytes directly, Linux reports KiB and is converted to bytes.
- Keep hardware and build profile consistent when comparing runs.
