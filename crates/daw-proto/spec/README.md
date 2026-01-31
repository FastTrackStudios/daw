# DAW Protocol Specification

This document defines the DAW (Digital Audio Workstation) Protocol - a standardized interface for controlling DAWs via the Roam RPC framework.

## Overview

The DAW Protocol enables communication between a host application and various DAW implementations (Reaper, Ableton, Standalone, etc.) through a unified service interface. All DAWs MUST implement the universal behaviors defined in this specification.

## Services

The protocol defines the following services:

- **Transport** - Playback control (play, stop, pause, record)
- **Project** - Project management (create, open, save, close)
- **Track** - Track operations (create, delete, reorder, volume, pan)
- **Marker** - Marker management (create, move, name)
- **Plugin** - Plugin handling (scan, instantiate, parameters)

## Requirements

All requirements in this specification use the `r[]` prefix for traceability.

- **Universal requirements** MUST be implemented by all DAWs
- **Optional requirements** MAY be implemented depending on DAW capabilities

## Implementations

| Implementation | Status |
|----------------|--------|
| daw-standalone | Active |
| daw-reaper | Planned |
| daw-ableton | Planned |

## Architecture

The DAW Protocol uses the Roam RPC framework for inter-process communication:

1. **Host** - Orchestrates cells and routes calls
2. **DAW Cell** - Implements the DAW Protocol services
3. **Other Cells** - Call DAW services through the host

Each DAW cell MUST:
- Implement all universal service interfaces
- Support bidirectional streaming for state updates
- Handle errors gracefully

## Version

Current version: 0.1.0