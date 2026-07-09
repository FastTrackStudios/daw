# Monarchy Specification

**A metadata-based hierarchical sorter with type-safe, config-compatible organization.**

## Overview

Monarchy is a Rust crate that provides a generic, type-safe system for organizing any type of data into hierarchical structures based on metadata. It uses a macro-based approach to ensure type safety while maintaining compatibility with configuration formats like JSON and TOML.

## Core Concepts

### 1. **Metadata** - The Schema
Defines what properties/attributes items can have. Users implement the `Metadata` trait to specify:
- **Fields**: The keys/properties that can be extracted
- **Values**: The types of values those fields can hold
- **Accessors**: How to get/set field values

### 2. **Group** - Pattern Definition
Defines how to match and categorize items:
- **Patterns**: What strings to match (e.g., "kick", "snare")
- **Negative Patterns**: What to exclude
- **Metadata Fields**: Which metadata keys this group cares about
- **Subgroups**: Nested group definitions
- **Priority**: Order of pattern matching

### 3. **Config** - Collection of Groups
Combines groups with parsing rules:
- **Groups**: All pattern definitions
- **Parser Rules**: How to extract metadata from strings
- **Fallback Strategy**: What to do with unmatched items

### 4. **Item** - Parsed Data
The result of parsing input:
- **ID**: Unique identifier
- **Original**: The input string
- **Metadata**: Extracted metadata values
- **Matched Group**: Which group pattern it matched

### 5. **Structure** - Output Hierarchy
The final organized tree:
- **Name**: Label for this level
- **Items**: Items at this level
- **Children**: Nested structures

## Architecture Flow

```
Input (Strings)
    ↓
Parser (uses Config to extract metadata)
    ↓
Items (with populated Metadata)
    ↓
Organizer (uses Groups to build hierarchy)
    ↓
Structure (final hierarchical result)
```

## Type Safety

The crate uses a macro (`define_metadata!`) to generate type-safe metadata structures:

```rust
define_metadata! {
    MyMetadata {
        MyField,      // Field enum name
        MyValue,      // Value enum name
        {
            FieldA => String,
            FieldB => i32,
            FieldC => bool,
        }
    }
}
```

This generates:
- `MyField` enum with variants for each field
- `MyValue` enum with typed variants
- `MyMetadata` struct with optional fields
- `Metadata` trait implementation

## Main API

### Core Function
```rust
pub fn monarchy_sort<M>(
    inputs: impl IntoIterator<Item = String>,
    config: Config<M>,
) -> Result<Structure<M>>
where
    M: Metadata
```

### Key Traits
- `Metadata`: Defines the schema for sortable items
- `Target`: For existing containers (optional, for integration)

### Key Types
- `Group<M>`: Pattern definitions
- `Config<M>`: Complete configuration
- `Item<M>`: Parsed items with metadata
- `Structure<M>`: Hierarchical output

## Usage Pattern

1. **Define Metadata** using the macro or implementing the trait
2. **Create Groups** that define patterns and which metadata to extract
3. **Build Config** from groups plus parsing rules
4. **Call `monarchy_sort()`** with input strings and config
5. **Receive `Structure`** containing organized hierarchy

## Example

```rust
// 1. Define metadata
define_metadata! {
    MusicMetadata {
        MusicField,
        MusicValue,
        {
            Instrument => String,
            Performer => String,
            MicPosition => String,
        }
    }
}

// 2. Create groups
let drums = Group {
    name: "Drums",
    patterns: vec!["kick", "snare"],
    metadata_fields: vec![MusicField::Instrument, MusicField::MicPosition],
    // ...
};

// 3. Build config
let config = Config::new(vec![drums, guitar, bass]);

// 4. Sort
let inputs = vec!["Kick In", "Snare Top", "Guitar Amp"];
let structure = monarchy_sort(inputs, config)?;

// 5. Use the organized structure
// Structure will contain:
// - Drums
//   - Kick
//     - Items: ["Kick In"]
//   - Snare  
//     - Items: ["Snare Top"]
// - Guitar
//   - Items: ["Guitar Amp"]
```

## Configuration Compatibility

Groups and Configs can be serialized/deserialized to/from JSON and TOML:

```toml
[[groups]]
name = "Drums"
patterns = ["kick", "snare", "tom"]
metadata_fields = ["instrument", "mic_position"]
priority = 100

[[groups.subgroups]]
name = "Kick"
patterns = ["kick"]
metadata_fields = ["mic_position"]
```

## Extensibility

Users can:
- Define custom metadata types for any domain
- Create domain-specific groups and patterns
- Implement custom parsing logic
- Integrate with existing systems via the `Target` trait

## Benefits

1. **Type Safety**: Compile-time checking of metadata fields
2. **Flexibility**: Works with any data type and domain
3. **Config Support**: JSON/TOML serialization built-in
4. **Minimalist**: No unnecessary abstractions or aliases
5. **Generic**: Not tied to any specific domain (music, files, etc.)

## Module Structure

- `metadata`: Core `Metadata` trait and macro
- `group`: Pattern definition types
- `config`: Configuration types
- `parser`: String parsing into Items
- `organizer`: Building hierarchies from Items
- `structure`: Output hierarchy type
- `error`: Error types

## Design Principles

1. **Separation of Concerns**: Schema (Metadata) vs Instance (Item) vs Pattern (Group)
2. **Generic Implementation**: Parser and Organizer work with any Metadata type
3. **User Simplicity**: Users only define Metadata and Groups
4. **Type Safety First**: Leverage Rust's type system for correctness
5. **Minimalist API**: No unnecessary methods or aliases