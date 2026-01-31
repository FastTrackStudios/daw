# Transport Service

The Transport service controls playback state and provides real-time state updates to subscribers.

## Overview

The Transport service is responsible for:
- Starting and stopping playback
- Managing playback position
- Broadcasting state changes to subscribers

All DAWs MUST implement the Transport service with the behaviors defined below.

## State Model

The transport maintains a state machine with the following states:

- **Stopped** - Playback is stopped
- **Playing** - Playback is active
- **Paused** - Playback is paused (optional)
- **Recording** - Recording is active (optional)

## Requirements

### Playback Control

r[transport.play.start]
The transport MUST transition to the Playing state when `play()` is called.

r[transport.play.from-position]
Playback MUST begin from the current cursor position.

r[transport.play.already-playing]
If the transport is already playing, calling `play()` MUST NOT result in an error. The transport SHOULD log a warning.

r[transport.stop]
The transport MUST transition to the Stopped state when `stop()` is called.

r[transport.stop.maintain-position]
The cursor position SHOULD be maintained at the location where playback stopped.

r[transport.stop.already-stopped]
If the transport is already stopped, calling `stop()` MUST NOT result in an error. The transport SHOULD log a warning.

### State Streaming

r[transport.state.broadcast]
The transport MUST broadcast state changes to all active subscribers.

r[transport.state.subscribe]
The transport MUST support subscribing to state updates via the `subscribe_state()` method.

r[transport.state.initial]
When a client subscribes, the transport MUST immediately send the current state.

r[transport.state.streaming]
State updates MUST be delivered via bidirectional streaming (Tx/Rx channels).

### Position and Timing

r[transport.position.time]
The transport MUST maintain the current playback position in seconds.

r[transport.tempo]
The transport MUST maintain the current tempo in beats per minute (BPM).

## Type Definitions

```rust
/// Current state of the transport
pub enum TransportState {
    Stopped,
    Playing,
    Paused,
    Recording,
}

/// Time position in the project
pub struct TimePosition {
    pub seconds: f64,
}

/// Transport state update for streaming
pub struct TransportStateUpdate {
    pub state: TransportState,
    pub position: TimePosition,
    pub tempo: f64,
}
```

## Service Interface

```rust
#[roam::service]
pub trait Transport {
    async fn play(&self);
    async fn stop(&self);
    async fn subscribe_state(&self, updates: Tx<TransportStateUpdate>);
}
```

## Error Handling

The transport service SHOULD handle the following error conditions:
- Invalid state transitions (handled gracefully, not as errors)
- Streaming channel failures (subscriber removed)
- Position calculation errors (logged, not propagated)