[PRD]
# PRD: DAW Real-Time Event Streaming

## Overview

FastTrackStudio needs real-time change streams from all DAW domains to support a sync engine that keeps multiple REAPER instances in lockstep. The `daw` workspace already has extensive query/mutation APIs and event types defined for every domain, but only Transport, FX, Project, Marker, and Region have working `subscribe` methods. Track, Item, Routing, and Tempo Map need subscribe methods added to their service traits, implemented in daw-reaper with poll-and-broadcast change detection, wired into the reaper-extension timer, wrapped in daw-control, and stubbed in daw-standalone.

## Goals

- Add `subscribe` methods to TrackService, ItemService, TakeService, RoutingService, and TempoMapService in daw-proto
- Implement poll-and-broadcast change detection in daw-reaper for each new domain, following the established transport.rs/fx.rs patterns
- Wire all new poll functions into the reaper-extension timer callback
- Add client-side `subscribe_*()` wrappers in daw-control
- Add no-op/stub implementations in daw-standalone so it compiles
- Verify that existing Marker and Region subscriptions work end-to-end; fix if broken

## Quality Gates

These commands must pass for every user story:
- `cargo check -p daw-proto` - Proto crate compiles
- `cargo check -p daw-reaper` - REAPER impl compiles
- `cargo check -p daw-control` - Client wrappers compile
- `cargo check -p daw-standalone` - Standalone backend compiles
- `cargo check -p reaper-extension` - Extension entry point compiles (run from main FastTrackStudio workspace: `cd /Users/codywright/Documents/Development/FastTrackStudio/FastTrackStudio && cargo check -p reaper-extension`)

## Reference Patterns

The implementing agent MUST read these files before writing any code — they contain the exact patterns to follow:

### Transport (broadcast + cache + polling)
- **daw-proto**: `crates/daw-proto/src/transport/transport.rs` — `subscribe_state()` and `subscribe_all_projects()` method signatures
- **daw-reaper**: `crates/daw-reaper/src/transport.rs` — `BROADCASTER` OnceLock, `CACHED_STATE`, `init_transport_broadcaster()`, `poll_and_broadcast()` function, `subscribe_state` impl that spawns a forwarding task from broadcast rx → roam Tx
- **daw-control**: `crates/daw-control/src/transport.rs` — `subscribe_state()` creates roam channel, calls service method, returns Rx
- **reaper-extension**: `apps/reaper-extension/src/lib.rs` (in main workspace at `/Users/codywright/Documents/Development/FastTrackStudio/FastTrackStudio/`) — `init_transport_broadcaster()` called at startup, `poll_and_broadcast()` called in `timer_callback()`

### FX (monitored-set + per-chain cache)
- **daw-reaper**: `crates/daw-reaper/src/fx.rs` — `FX_BROADCASTER` OnceLock, `FX_MONITORED_CHAINS` (only polls chains with active subscribers), `FX_CHAIN_CACHE` per-chain state, `init_fx_broadcaster()`, `poll_and_broadcast_fx()`

### Marker/Region (spawn-per-subscriber polling)
- **daw-reaper**: `crates/daw-reaper/src/marker.rs` — `subscribe()` spawns a per-subscriber polling task (different pattern from transport/fx broadcast approach)
- **daw-reaper**: `crates/daw-reaper/src/region.rs` — same spawn-per-subscriber pattern

### Key architectural rule
- `daw-reaper/src/lib.rs` re-exports `init_*_broadcaster` and `poll_and_broadcast_*` functions — new domains must be re-exported the same way

## User Stories

### US-001: Add `subscribe_tracks` to TrackService (daw-proto)

**Description:** As a sync engine developer, I want a `subscribe_tracks` method on TrackService so that I can receive real-time track change events.

**Acceptance Criteria:**
- [ ] `TrackService` in `crates/daw-proto/src/track/service.rs` has method: `async fn subscribe_tracks(&self, project: ProjectContext, tx: Tx<TrackEvent>);`
- [ ] The file imports `Tx` from roam and `TrackEvent` from the event module
- [ ] `cargo check -p daw-proto` passes

### US-002: Implement track change detection in daw-reaper

**Description:** As a sync engine developer, I want daw-reaper to detect and broadcast track state changes so subscribers receive granular `TrackEvent`s.

**Acceptance Criteria:**
- [ ] Static `TRACK_BROADCASTER: OnceLock<broadcast::Sender<TrackEvent>>` in `crates/daw-reaper/src/track.rs`
- [ ] Static `TRACK_CACHE: OnceLock<Mutex<HashMap<String, CachedTrackState>>>` keyed by project GUID, storing per-track cached state
- [ ] `CachedTrackState` struct caches: guid, name, muted, soloed, armed, selected, volume, pan, color, visible_in_tcp, visible_in_mixer, index, folder_depth, fx_count, input_fx_count
- [ ] `init_track_broadcaster()` public function that initializes the OnceLock
- [ ] `poll_and_broadcast_tracks()` public function that:
  - Iterates all open projects
  - For each project, gets all tracks via safe_wrappers
  - Diffs against cache → emits granular TrackEvent variants
  - Uses thresholds: volume ±0.0001, pan ±0.0001; discrete changes always emit
  - Emits `TrackEvent::Added` for new tracks, `TrackEvent::Removed` for missing tracks
- [ ] `subscribe_tracks` impl on ReaperTrack spawns a forwarding task from broadcast rx → roam Tx
- [ ] `init_track_broadcaster` and `poll_and_broadcast_tracks` are re-exported from `crates/daw-reaper/src/lib.rs`
- [ ] `cargo check -p daw-reaper` passes

### US-003: Add `subscribe_items` and `subscribe_takes` to ItemService/TakeService (daw-proto)

**Description:** As a sync engine developer, I want subscribe methods on ItemService and TakeService so I can receive item/take change events.

**Acceptance Criteria:**
- [ ] `ItemService` in `crates/daw-proto/src/item/service.rs` has method: `async fn subscribe_items(&self, project: ProjectContext, tx: Tx<ItemEvent>);`
- [ ] `TakeService` in `crates/daw-proto/src/item/service.rs` has method: `async fn subscribe_takes(&self, project: ProjectContext, tx: Tx<TakeEvent>);`
- [ ] Appropriate imports added (Tx from roam, event types)
- [ ] `cargo check -p daw-proto` passes

### US-004: Implement item/take change detection in daw-reaper

**Description:** As a sync engine developer, I want daw-reaper to detect and broadcast item and take changes.

**Acceptance Criteria:**
- [ ] Static `ITEM_BROADCASTER: OnceLock<broadcast::Sender<ItemEvent>>` in `crates/daw-reaper/src/item.rs`
- [ ] Static `TAKE_BROADCASTER: OnceLock<broadcast::Sender<TakeEvent>>`
- [ ] Cache strategy: per-project item count + lightweight hash of (guid, position, length, track_guid) per item; on count change or hash mismatch, do full diff
- [ ] Thresholds: position ±0.001s, length ±0.001s for float comparisons
- [ ] `init_item_broadcaster()` and `poll_and_broadcast_items()` public functions
- [ ] `subscribe_items` and `subscribe_takes` impls spawn forwarding tasks
- [ ] Re-exported from `crates/daw-reaper/src/lib.rs`
- [ ] `cargo check -p daw-reaper` passes

### US-005: Add `subscribe_routing` to RoutingService (daw-proto)

**Description:** As a sync engine developer, I want a subscribe method on RoutingService for routing change events.

**Acceptance Criteria:**
- [ ] `RoutingService` in `crates/daw-proto/src/routing/service.rs` has method: `async fn subscribe_routing(&self, project: ProjectContext, tx: Tx<RoutingEvent>);`
- [ ] Appropriate imports added
- [ ] `cargo check -p daw-proto` passes

### US-006: Implement routing change detection in daw-reaper

**Description:** As a sync engine developer, I want daw-reaper to detect and broadcast routing changes.

**Acceptance Criteria:**
- [ ] Static `ROUTING_BROADCASTER: OnceLock<broadcast::Sender<RoutingEvent>>` in `crates/daw-reaper/src/routing.rs`
- [ ] Cache: send/receive/hw-output counts and basic params (volume, pan, muted) per track
- [ ] `init_routing_broadcaster()` and `poll_and_broadcast_routing()` public functions
- [ ] `subscribe_routing` impl spawns forwarding task
- [ ] Re-exported from `crates/daw-reaper/src/lib.rs`
- [ ] `cargo check -p daw-reaper` passes

### US-007: Add `subscribe_tempo_map` to TempoMapService (daw-proto)

**Description:** As a sync engine developer, I want a subscribe method on TempoMapService for tempo map change events.

**Acceptance Criteria:**
- [ ] `TempoMapService` in `crates/daw-proto/src/tempo_map/service.rs` has method: `async fn subscribe_tempo_map(&self, project: ProjectContext, tx: Tx<TempoMapEvent>);`
- [ ] Appropriate imports added
- [ ] `cargo check -p daw-proto` passes

### US-008: Implement tempo map change detection in daw-reaper

**Description:** As a sync engine developer, I want daw-reaper to detect and broadcast tempo map changes.

**Acceptance Criteria:**
- [ ] Static `TEMPO_MAP_BROADCASTER: OnceLock<broadcast::Sender<TempoMapEvent>>` in `crates/daw-reaper/src/tempo_map.rs`
- [ ] Cache: tempo point count + values per project (tempo maps are usually small, full comparison is fine)
- [ ] `init_tempo_map_broadcaster()` and `poll_and_broadcast_tempo_map()` public functions
- [ ] Emits `TempoMapEvent::MapChanged` on any diff
- [ ] `subscribe_tempo_map` impl spawns forwarding task
- [ ] Re-exported from `crates/daw-reaper/src/lib.rs`
- [ ] `cargo check -p daw-reaper` passes

### US-009: Wire new poll functions into reaper-extension timer

**Description:** As a sync engine developer, I want all new poll-and-broadcast functions called from the timer callback so change detection runs at ~30Hz.

**Acceptance Criteria:**
- [ ] `apps/reaper-extension/src/lib.rs` (in main FastTrackStudio workspace) calls `init_track_broadcaster()`, `init_item_broadcaster()`, `init_routing_broadcaster()`, `init_tempo_map_broadcaster()` during startup (alongside existing `init_transport_broadcaster()` and `init_fx_broadcaster()`)
- [ ] `timer_callback()` calls `poll_and_broadcast_tracks()`, `poll_and_broadcast_items()`, `poll_and_broadcast_routing()`, `poll_and_broadcast_tempo_map()` alongside existing transport and fx calls
- [ ] `cargo check -p reaper-extension` passes (from main workspace)

**Note:** The reaper-extension lives in the main FastTrackStudio workspace at `apps/reaper-extension/`, not in the daw workspace. It accesses daw-reaper functions via `daw::reaper::*`. The implementing agent should modify this file in the main workspace.

### US-010: Add daw-control client wrappers for new subscriptions

**Description:** As an app developer, I want ergonomic subscribe methods on daw-control handles that return `Rx<Event>`.

**Acceptance Criteria:**
- [ ] `crates/daw-control/src/tracks.rs` has `pub async fn subscribe(&self) -> Result<Rx<TrackEvent>>` that creates a roam channel, calls `self.clients.track.subscribe_tracks(context, tx)`, returns rx
- [ ] `crates/daw-control/src/items.rs` has `pub async fn subscribe(&self) -> Result<Rx<ItemEvent>>` and `pub async fn subscribe_takes(&self) -> Result<Rx<TakeEvent>>`
- [ ] `crates/daw-control/src/routing.rs` has `pub async fn subscribe(&self) -> Result<Rx<RoutingEvent>>`
- [ ] `crates/daw-control/src/tempo_map.rs` has `pub async fn subscribe(&self) -> Result<Rx<TempoMapEvent>>`
- [ ] `cargo check -p daw-control` passes

### US-011: Add daw-standalone stubs for new subscribe methods

**Description:** As a developer, I want daw-standalone to compile with the new subscribe methods by providing no-op or basic stub implementations.

**Acceptance Criteria:**
- [ ] `crates/daw-standalone/src/track.rs` implements `subscribe_tracks` (no-op: immediately drop or log)
- [ ] `crates/daw-standalone/src/item.rs` implements `subscribe_items` and `subscribe_takes`
- [ ] `crates/daw-standalone/src/routing.rs` implements `subscribe_routing`
- [ ] `crates/daw-standalone/src/tempo_map.rs` implements `subscribe_tempo_map`
- [ ] `cargo check -p daw-standalone` passes

### US-012: Verify marker and region subscriptions end-to-end

**Description:** As a sync engine developer, I want confidence that the existing marker and region subscribe implementations work correctly.

**Acceptance Criteria:**
- [ ] Reviewed `crates/daw-reaper/src/marker.rs` `subscribe()` impl — verified it compiles and follows a sound polling pattern
- [ ] Reviewed `crates/daw-reaper/src/region.rs` `subscribe()` impl — same verification
- [ ] Verified daw-control has working marker/region subscribe wrappers (check `crates/daw-control/src/markers.rs` and `crates/daw-control/src/regions.rs`)
- [ ] If any are missing or broken, fix them
- [ ] `cargo check -p daw-reaper && cargo check -p daw-control` passes

## Functional Requirements

- FR-1: All new subscribe methods MUST use `roam::Tx<T>` for streaming (not return types)
- FR-2: Poll functions MUST be safe to call from the main thread timer callback (~30Hz)
- FR-3: Poll functions MUST NOT panic — any errors should be logged and skipped
- FR-4: Cache diffing MUST use thresholds for floating-point values (volume ±0.0001, pan ±0.0001, position ±0.001s, length ±0.001s)
- FR-5: Track change detection MUST emit granular events (per-field changes), not just "track changed"
- FR-6: Item change detection MAY use a two-phase approach: quick count/hash check, then full diff only on mismatch
- FR-7: All `init_*_broadcaster()` functions MUST be idempotent (use OnceLock)
- FR-8: All subscribe implementations MUST spawn a forwarding task that reads from broadcast::Receiver and writes to roam::Tx, ending when the Tx send fails (client disconnected)
- FR-9: Safe wrappers in `daw-reaper/src/safe_wrappers/` MUST be used for all REAPER FFI calls — service files stay 100% safe Rust

## Non-Goals (Out of Scope)

- **Automation streaming** — Deferred. Point-level diffing is expensive and the sync engine will use chunk-based sync instead.
- **Sync engine implementation** — That lives in the main FastTrackStudio workspace's `crates/sync/` crates, not in the daw repo.
- **UI for subscriptions** — No UI work needed.
- **Performance optimization** — Initial implementation prioritizes correctness over minimal CPU usage. Optimization (monitored sets, adaptive polling rates) can come later.
- **Tests** — Unit tests for cache diffing logic are nice-to-have but not required for this PRD. The sync engine will serve as the integration test.

## Technical Considerations

- **Broadcast channel capacity**: Use 256 for tracks/routing/tempo_map (low-frequency changes), 1024 for items (can be numerous). Follow what transport.rs and fx.rs use as reference.
- **Thread safety**: All static state (OnceLock, Mutex) must be safe for main-thread-only access. The poll functions run on the main thread; the subscribe forwarding tasks run on async runtime.
- **`moire::task::spawn`**: Use moire's spawn (not tokio::spawn) for forwarding tasks, per project conventions. Name all spawned tasks.
- **`moire::sync::Mutex`**: Use moire's Mutex for cache state, per project conventions.
- **Import pattern**: `use crate::safe_wrappers::track as track_sw;` (or relevant module) for FFI calls.
- **The `daw` facade crate** (`crates/daw/`): After adding new re-exports to daw-reaper's lib.rs, the facade crate may need updating to re-export the new init/poll functions. Check `crates/daw/src/lib.rs`.

## Success Metrics

- All quality gate commands pass
- Every DAW domain (transport, project, fx, track, item, take, routing, tempo map, marker, region) has a working subscribe method
- The sync engine in the main workspace can subscribe to all domain streams via daw-control

## Open Questions

- Should item polling be throttled to a lower frequency than tracks (e.g., ~10Hz instead of ~30Hz) given the potentially large number of items? (Can be decided during implementation based on observed cost)
- Should routing polling use a monitored-set pattern like FX (only poll tracks with active subscribers)? (Recommended yes, but can start simple)
[/PRD]
