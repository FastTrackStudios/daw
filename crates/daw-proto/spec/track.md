# Track Service

The Track service manages audio/MIDI tracks within a project.

## Overview

The Track service is responsible for:
- Creating and deleting tracks
- Managing track order
- Controlling track properties (volume, pan, mute, solo)
- Managing track plugins

All DAWs MUST implement the core Track service behaviors.

## Requirements

### Track Management

r[track.create]
The service MUST support creating new tracks.

r[track.delete]
The service MUST support deleting existing tracks.

r[track.reorder]
The service MUST support reordering tracks within the project.

r[track.list]
The service MUST provide a list of all tracks with their IDs and names.

### Track Properties

r[track.name]
Each track MUST have a name that can be retrieved and set.

r[track.volume]
Each track MUST support volume control (0.0 to 1.0, or dB scale).

r[track.pan]
Each track MUST support pan control (-1.0 left to 1.0 right).

r[track.mute]
Each track MUST support mute toggle.

r[track.solo]
Each track MUST support solo toggle.

### Track State

r[track.state.recording]
Each track MUST support recording arm toggle.

r[track.state.monitoring]
Each track SHOULD support input monitoring toggle.

## Type Definitions

pub struct TrackId(pub u64);

pub struct TrackInfo {
    pub id: TrackId,
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
    pub soloed: bool,
    pub record_armed: bool,
}