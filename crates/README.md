# DAW Cells

This directory contains the DAW (Digital Audio Workstation) protocol and implementations for roam.

## Architecture

The DAW system is split into multiple crates following clean separation of concerns:

```
daw/
├── daw-proto/      # Pure protocol definitions (services + types)
├── daw-control/    # Ergonomic Rust API wrapper (reaper-rs style)
├── daw-reaper/     # REAPER DAW implementation
└── daw-standalone/ # Standalone/mock implementation
```

## Crates

### `daw-proto` - Protocol Definitions

**Pure, functional protocol layer** - no global state, no opinions.

Contains:
- Service trait definitions (`TransportService`, `TrackService`, `ProjectService`, etc.)
- Data types (`Transport`, `Track`, `Position`, `Tempo`, etc.)
- Update types for streaming (`TransportUpdate`, `TrackUpdate`, etc.)

Used by:
- ✅ Server implementations (`daw-reaper`, `daw-standalone`)
- ✅ Client wrapper (`daw-control`)
- ✅ Code generation (TypeScript, Swift, etc.)
- ✅ Anyone needing the raw protocol

Example:
```rust
use daw_proto::{TransportService, TransportServiceClient};

// Direct usage of protocol (verbose but pure)
let client = TransportServiceClient::new(handle);
client.play(Some(project_id)).await?;
```

### `daw-control` - Ergonomic API Wrapper

**Reaper-RS style hierarchical API** for Rust consumers.

Provides:
- Global singleton pattern (`Daw::init()`, like `Reaper::get()`)
- Lightweight handles (`Project`, `Transport`, `Track`)
- Beautiful hierarchical navigation
- Zero-cost abstractions (handles are just IDs)

Used by:
- ✅ Desktop app (Dioxus host)
- ✅ CLI tools
- ✅ Test utilities
- ✅ Any Rust code consuming the DAW

Example:
```rust
use daw_control::Daw;

// Initialize once at startup
Daw::init(handle)?;

// Beautiful API (like reaper-rs!)
let project = Daw::current_project().await?;
project.transport().play().await?;

let track = project.tracks().get("track-123").await?;
track.set_volume(0.8).await?;
```

### `daw-reaper` - REAPER Implementation

**Server-side implementation** for REAPER DAW.

Contains:
- Service implementations (`TransportServiceImpl`, `TrackServiceImpl`, etc.)
- REAPER callback handlers (`IReaperControlSurface`)
- State management and broadcasting
- Integration with `reaper-rs`

Uses:
- `daw-proto` for service definitions
- `reaper-rs` for REAPER API access
- `tokio::sync::broadcast` for reactive updates

### `daw-standalone` - Standalone Implementation

**Mock/test implementation** for development without REAPER.

Contains:
- Simulated DAW state
- Mock implementations of all services
- Useful for testing and development

## Design Philosophy

### Separation of Concerns

**Protocol (daw-proto):**
- 100% pure definitions
- No global state
- No implementation opinions
- Language-agnostic (can be code-generated)

**Control (daw-control):**
- Ergonomic Rust wrapper
- Adds convenience, not functionality
- Optional - can use `daw-proto` directly
- Follows reaper-rs patterns

**Implementations (daw-reaper, daw-standalone):**
- Implement the protocol
- Handle DAW-specific details
- Manage internal state

### Reaper-RS Style API

Inspired by `reaper-rs`, `daw-control` provides:

1. **Global Singleton** - `Daw::init()` / `Daw::current_project()`
2. **Lightweight Handles** - Just IDs, no connections stored
3. **Every Method Accesses Singleton** - `DawConnection::get()`
4. **Hierarchical Navigation** - `project.transport().play()`

Compare to reaper-rs:
```rust
// reaper-rs
let project = Reaper::get().current_project();
project.tracks().next().unwrap().set_name("Lead");

// daw-control (same pattern!)
let project = Daw::current_project().await?;
project.tracks().get("track-1").await?.set_volume(0.8).await?;
```

### Reactive Streaming

All services support streaming for real-time updates:

```rust
// Subscribe to transport updates (60fps playhead)
let mut updates = project.transport().subscribe().await?;
while let Some(update) = updates.next().await {
    println!("Position: {:?}", update.state.playhead_position);
}
```

Implementation uses:
- `tokio::sync::broadcast` for efficient multi-client streaming
- `Arc<T>` for zero-copy message sharing
- Optional project IDs (None = current project)

## Usage Examples

### Desktop App (Host)

```rust
use daw_control::Daw;
use dioxus::prelude::*;

#[tokio::main]
async fn main() {
    // Connect to REAPER extension via shared memory
    let handle = roam_shm::connect("/tmp/reaper-daw.shm").await.unwrap();
    Daw::init(handle).unwrap();
    
    dioxus::launch(App);
}

fn App() -> Element {
    let mut transport_state = use_signal(|| None);
    
    use_effect(move || async move {
        let project = Daw::current_project().await.ok()?;
        let mut updates = project.transport().subscribe().await.ok()?;
        
        while let Some(update) = updates.next().await {
            transport_state.set(Some(update.state));
        }
    });
    
    rsx! {
        if let Some(state) = transport_state() {
            div { "Playing: {state.is_playing()}" }
            div { "Position: {state.playhead_position:?}" }
        }
    }
}
```

### CLI Tool

```rust
use daw_control::Daw;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // Connect to DAW
    let handle = roam::connect("unix:///tmp/fts-daw.sock").await?;
    Daw::init(handle)?;
    
    // Control transport
    let project = Daw::current_project().await?;
    
    println!("Playing...");
    project.transport().play().await?;
    
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    
    println!("Stopped");
    project.transport().stop().await?;
    
    Ok(())
}
```

### REAPER Extension (Server)

```rust
use daw_proto::{TransportService, Transport};
use roam::{Context, Tx};

pub struct ReaperTransportService {
    state: Arc<RwLock<Transport>>,
    broadcast_tx: broadcast::Sender<Arc<TransportUpdate>>,
}

impl TransportService for ReaperTransportService {
    async fn play(&self, _cx: &Context, project_id: Option<String>) {
        let project = match project_id {
            Some(id) => find_project(&id)?,
            None => Reaper::get().current_project(),
        };
        
        project.play();
    }
    
    async fn subscribe(&self, _cx: &Context, project_id: Option<String>, updates: Tx<TransportUpdate>) {
        let mut rx = self.broadcast_tx.subscribe();
        
        loop {
            match rx.recv().await {
                Ok(update) => {
                    if updates.send(&*update).await.is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    }
}
```

## Future Work

- [ ] Add `TrackService` and track handles
- [ ] Add `FxService` for plugin control
- [ ] Add `ItemService` for media items
- [ ] Add `EnvelopeService` for automation
- [ ] Add `MarkerService` for markers/regions
- [ ] Generate TypeScript client bindings
- [ ] Generate Swift client bindings
- [ ] Add bidirectional streaming support
- [ ] Add batch operations for efficiency

## See Also

- [reaper-rs](https://github.com/helgoboss/reaper-rs) - Inspiration for API design
- [roam](../../reference/roam) - RPC framework
- [dodeca](../../reference/dodeca) - Example of multi-service architecture