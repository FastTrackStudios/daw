# Plugin Service

The Plugin service manages audio/MIDI plugins and their parameters.

## Overview

The Plugin service is responsible for:
- Scanning available plugins
- Instantiating plugins on tracks
- Controlling plugin parameters
- Managing plugin presets

All DAWs MUST implement the core Plugin service behaviors.

## Requirements

### Plugin Management

r[plugin.scan]
The service MUST support scanning for available plugins.

r[plugin.list]
The service MUST provide a list of available plugins.

r[plugin.instantiate]
The service MUST support instantiating plugins on tracks.

r[plugin.remove]
The service MUST support removing plugins from tracks.

r[plugin.bypass]
The service MUST support bypassing plugins.

### Plugin Parameters

r[plugin.param.list]
The service MUST provide a list of plugin parameters.

r[plugin.param.get]
The service MUST support getting parameter values.

r[plugin.param.set]
The service MUST support setting parameter values.

r[plugin.param.automation]
The service SHOULD support parameter automation.

### Plugin Presets

r[plugin.preset.load]
The service SHOULD support loading plugin presets.

r[plugin.preset.save]
The service SHOULD support saving plugin presets.

## Type Definitions

pub struct PluginId(pub u64);

pub struct PluginInfo {
    pub id: PluginId,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub format: String, // VST3, AU, CLAP, etc.
}

pub struct PluginParameter {
    pub id: String,
    pub name: String,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}