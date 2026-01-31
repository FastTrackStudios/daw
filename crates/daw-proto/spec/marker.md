# Marker Service

The Marker service manages timeline markers for navigation and organization.

## Overview

The Marker service is responsible for:
- Creating markers at specific positions
- Moving existing markers
- Naming and coloring markers
- Listing all markers

All DAWs MUST implement the core Marker service behaviors.

## Requirements

### Marker Management

r[marker.create]
The service MUST support creating markers at specific time positions.

r[marker.delete]
The service MUST support deleting markers.

r[marker.move]
The service MUST support moving markers to new positions.

r[marker.list]
The service MUST provide a list of all markers.

### Marker Properties

r[marker.name]
Each marker MUST have a name.

r[marker.position]
Each marker MUST have a time position.

r[marker.color]
Each marker SHOULD support a color for visual organization.

## Type Definitions

pub struct MarkerId(pub u64);

r[define.marker]
pub struct Marker {
    pub id: MarkerId,
    pub name: String,
    pub position: TimePosition,
    pub color: Option<String>,
}