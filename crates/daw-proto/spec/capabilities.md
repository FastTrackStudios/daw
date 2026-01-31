# DAW Capabilities

This document defines optional capabilities that DAWs MAY implement.

## Overview

Not all DAWs have the same feature set. This specification defines optional behaviors that extend the core protocol. Implementations SHOULD document which capabilities they support.

## Optional Requirements

### Transport Capabilities

r[capabilities.transport.pause]
OPTIONAL: The transport MAY support a Paused state distinct from Stopped.

r[capabilities.transport.record]
OPTIONAL: The transport MAY support recording.

r[capabilities.transport.loop]
OPTIONAL: The transport MAY support loop playback.

r[capabilities.transport.punch-in]
OPTIONAL: The transport MAY support punch-in recording.

### Project Capabilities

r[capabilities.project.templates]
OPTIONAL: The project service MAY support project templates.

r[capabilities.project.autosave]
OPTIONAL: The project service MAY support automatic saving.

r[capabilities.project.assets]
OPTIONAL: The project service MAY support saving with external assets.

r[capabilities.project.backup]
OPTIONAL: The project service MAY support automatic backups.

### Track Capabilities

r[capabilities.track.folders]
OPTIONAL: The track service MAY support folder tracks.

r[capabilities.track.routing]
OPTIONAL: The track service MAY support track routing and sends.

r[capabilities.track.freeze]
OPTIONAL: The track service MAY support track freezing/rendering.

r[capabilities.track.comping]
OPTIONAL: The track service MAY support take comping.

### Plugin Capabilities

r[capabilities.plugin.sidechain]
OPTIONAL: The plugin service MAY support sidechain inputs.

r[capabilities.plugin.delay-compensation]
OPTIONAL: The plugin service MAY support delay compensation.

r[capabilities.plugin.oversampling]
OPTIONAL: The plugin service MAY support oversampling.

## Capability Detection

Services SHOULD provide a method to query supported capabilities:

```rust
r[capabilities.query]
async fn get_capabilities(&self) -> Vec<String>;
```

## Implementation Notes

- Optional capabilities MUST NOT be required for core protocol compliance
- Hosts SHOULD gracefully handle unsupported capabilities
- DAWs SHOULD document which capabilities they implement