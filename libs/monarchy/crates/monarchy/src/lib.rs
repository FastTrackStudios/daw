//! # Monarchy: A Hierarchical Sorting and Organization Library
//!
//! Monarchy is a powerful, type-safe library for organizing string inputs into hierarchical
//! tree structures based on pattern matching and metadata extraction. Originally designed
//! for organizing audio track names in DAW (Digital Audio Workstation) projects, it can be
//! used for any domain that requires flexible categorization of named items.
//!
//! ## Key Features
//!
//! - **Pattern-based matching**: Define groups with patterns that match input strings
//! - **Hierarchical organization**: Automatically organize items into nested tree structures
//! - **Metadata extraction**: Extract structured data from input strings (performer, arrangement, etc.)
//! - **Configurable fallback**: Control how unmatched items are handled
//! - **Visitor pattern**: Transform trees with composable visitors (collapse, cleanup, etc.)
//! - **Scoped sorting**: Re-sort items within specific group contexts
//! - **Derive macros**: Automatically implement `Metadata` trait for your types
//!
//! ## Core Concepts
//!
//! - **[`Config`]**: Configuration defining groups, parser rules, and fallback strategy
//! - **[`Group`]**: Pattern-based category with optional nesting, prefixes, and metadata fields
//! - **[`Item`]**: A parsed input with extracted metadata and matched group hierarchy
//! - **[`Structure`]**: The resulting hierarchical tree of organized items
//! - **[`Metadata`]**: Trait for types that hold extracted metadata from inputs
//!
//! ## Quick Start
//!
//! ```ignore
//! use monarchy::{monarchy_sort, Config, Group, Metadata};
//!
//! // Define your metadata struct with the Metadata derive macro
//! #[derive(Metadata, Default, Clone, Debug)]
//! struct TrackMetadata {
//!     performer: Option<String>,
//!     multi_mic: Option<Vec<String>>,
//! }
//!
//! // Create a configuration with groups and patterns
//! let config = Config::builder()
//!     .group(Group::builder("Drums")
//!         .patterns(["kick", "snare", "hi-hat"])
//!         .prefix("D")
//!         .group(Group::builder("Kick").patterns(["kick"]).build())
//!         .group(Group::builder("Snare").patterns(["snare"]).build())
//!         .build())
//!     .group(Group::builder("Guitars")
//!         .patterns(["guitar", "gtr"])
//!         .prefix("GTR")
//!         .build())
//!     .build();
//!
//! // Sort input strings into a hierarchical structure
//! let inputs = vec!["Kick In.wav", "Kick Out.wav", "Snare Top.wav", "Guitar DI.wav"];
//! let structure = monarchy_sort(inputs, config)?;
//!
//! // The structure now contains:
//! // root
//! // ├── Drums
//! // │   ├── Kick: [Kick In.wav, Kick Out.wav]
//! // │   └── Snare: [Snare Top.wav]
//! // └── Guitars: [Guitar DI.wav]
//! ```
//!
//! ## Advanced Usage
//!
//! ### Metadata Extraction
//!
//! Groups can define metadata fields that are extracted from matched inputs:
//!
//! ```ignore
//! Group::builder("Electric Guitar")
//!     .patterns(["electric", "elec"])
//!     .field(TrackMetadataField::Performer)
//!     .field_value_descriptors(
//!         TrackMetadataField::MultiMic,
//!         vec![
//!             FieldValueDescriptor::builder("In").patterns(["in"]).build(),
//!             FieldValueDescriptor::builder("Out").patterns(["out"]).build(),
//!         ]
//!     )
//!     .build()
//! ```
//!
//! ### Visitors for Tree Transformation
//!
//! Apply visitors to transform the structure after sorting:
//!
//! ```ignore
//! use monarchy::{Visitable, CollapseHierarchy, cleanup_display_names};
//!
//! let mut structure = monarchy_sort(inputs, config.clone())?;
//!
//! // Collapse unnecessary intermediate levels
//! structure.accept(&mut CollapseHierarchy::new(&config));
//!
//! // Clean up display names
//! cleanup_display_names(&mut structure);
//! ```
//!
//! ### Scoped Re-sorting
//!
//! Re-sort items within a specific group context for iterative workflows:
//!
//! ```ignore
//! use monarchy::monarchy_sort_scoped;
//!
//! // Re-sort unsorted items within the "Electric Guitar" group
//! let result = monarchy_sort_scoped(unsorted_items, config, "Electric Guitar")?;
//! ```
//!
//! ## Module Overview
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`config`] | Configuration types: `Config`, `ParserRules`, `FallbackStrategy` |
//! | [`group`] | Group definition and builder: `Group`, `GroupBuilder` |
//! | [`structure`] | Hierarchical tree structure: `Structure` |
//! | [`metadata`] | Metadata trait and field name trait |
//! | [`parser`] | Input string parser: `Parser` |
//! | [`organizer`] | Structure builder from parsed items: `Organizer` |
//! | [`visitor`] | Tree transformation visitors |
//! | [`field`] | Metadata field infrastructure |
//! | [`field_value`] | Field value descriptors for pattern matching |
//! | [`error`] | Error types and result alias |

pub mod config;
pub mod error;
pub mod field;
pub mod field_value;
pub mod group;
pub mod metadata;
pub mod organizer;
pub mod parser;
pub mod structure;
pub mod test_utils;
pub mod utils;
pub mod visitor;

// Core exports
pub use config::{Config, ConfigBuilder, FallbackStrategy, ParserRules};
pub use error::{MonarchyError, Result};
pub use field::{IntoField, MetadataField};
pub use field_value::FieldValueDescriptor;
pub use group::{FieldGroupingStrategy, Group, GroupBuilder, IntoVec};
pub use metadata::{FieldName, Metadata};
pub use organizer::Organizer;
pub use parser::Parser;
pub use structure::Structure;
pub use test_utils::StructureAssertions;
// Display name generation
// ToDisplayName: trait implemented by metadata types to generate canonical display names
pub use visitor::{
    ApplyTaggedCollections, CleanupDisplayNames, CollapseHierarchy, CollapseSingleChildFolders,
    CollectUnsorted, ExpandItemsToChildren, PromoteSingleChild, RemoveEmptyNodes, StructureVisitor,
    Visitable, cleanup_display_names, cleanup_display_names_with_fallback,
    collapse_single_child_folders, collect_unsorted_to_root, collect_unsorted_to_root_with_name,
    expand_items_to_children,
};
// Note: ToDisplayName is defined in this file, not in visitor module

// Re-export the derive macros
// Note: Metadata derive macro shares the same name as the Metadata trait - this is the standard Rust pattern
// (like serde::Serialize trait and serde::Serialize derive macro)
pub use monarchy_derive::{Metadata, MetadataBuilder};

// Re-export serde traits for derive usage
pub use serde::{Deserialize, Serialize};

// Re-export facet for derive macro usage (so downstream crates don't need direct facet dependency)
pub use facet;

use facet::Facet;
use serde::{Serializer, ser::SerializeStruct};

/// Represents a parsed item with metadata
#[derive(Clone, Debug, Facet)]
pub struct Item<M: Metadata> {
    /// Unique identifier for this item
    pub id: String,

    /// Original input string
    pub original: String,

    /// Parsed metadata
    pub metadata: M,

    /// Full hierarchy of groups that matched, from top-level to most specific.
    ///
    /// Stored as actual Group instances so we can access fields like prefix, transparent, etc.
    /// Example: `[Drums Group, Drum_Kit Group, Kick Group]`
    /// The last element is the most specific group that matched.
    ///
    /// Note: For serialization, this is serialized as group names (`Vec<String>`).
    pub matched_groups: Vec<Group<M>>,
}

impl<M: Metadata> Item<M> {
    /// Derive display name from matched groups and metadata
    ///
    /// This method extracts prefixes and group names from matched_groups,
    /// then delegates to the metadata's `to_display_name()` implementation.
    ///
    /// Returns the original name if metadata doesn't implement ToDisplayName
    /// or if to_display_name returns an empty string.
    pub fn derive_display_name(&self) -> String
    where
        M: ToDisplayName,
    {
        let prefixes: Vec<String> = self
            .matched_groups
            .iter()
            .filter_map(|g| g.prefix.clone())
            .collect();

        let group_names: Vec<String> = self.matched_groups.iter().map(|g| g.name.clone()).collect();

        let display_name = self.metadata.to_display_name(&prefixes, &group_names);

        // Fall back to original if empty
        if display_name.is_empty() {
            self.original.clone()
        } else {
            display_name
        }
    }
}

/// Trait for types that can generate a display name from their data
///
/// This trait is implemented by metadata types to generate canonical display names
/// that include all relevant information from the parsed metadata.
///
/// # Example
/// ```ignore
/// impl ToDisplayName for MyMetadata {
///     fn to_display_name(&self, prefixes: &[String], group_names: &[String]) -> String {
///         let mut parts = Vec::new();
///         
///         // Add prefixes
///         if !prefixes.is_empty() {
///             parts.push(prefixes.join(" "));
///         }
///         
///         // Add last group name
///         if let Some(group) = group_names.last() {
///             parts.push(group.clone());
///         }
///         
///         // Add other metadata fields...
///         
///         parts.join(" ")
///     }
/// }
/// ```
pub trait ToDisplayName {
    /// Generate a full canonical display name
    ///
    /// # Arguments
    /// * `prefixes` - Accumulated prefixes from matched groups (e.g., ["D", "GTR"])
    /// * `group_names` - Names of matched groups in hierarchy order (e.g., ["Drums", "Kick"])
    ///
    /// # Returns
    /// Full display name like "D Kick In" or "GTR E Cody Crunch 2"
    fn to_display_name(&self, prefixes: &[String], group_names: &[String]) -> String;
}

/// Trait for converting various input types into sortable strings
pub trait IntoInputs {
    fn into_inputs(self) -> Vec<String>;
}

// Blanket implementation for any iterator that yields items convertible to String
impl<I, T> IntoInputs for I
where
    I: IntoIterator<Item = T>,
    T: Into<String>,
{
    fn into_inputs(self) -> Vec<String> {
        self.into_iter().map(|item| item.into()).collect()
    }
}

/// Main sorting function - now accepts anything that can be converted to inputs
pub fn monarchy_sort<M, I>(inputs: I, config: &Config<M>) -> Result<Structure<M>>
where
    M: Metadata,
    I: IntoInputs,
{
    let input_strings = inputs.into_inputs();
    let parser = Parser::new(config);
    let mut items = Vec::new();

    for input in input_strings {
        items.push(parser.parse(input)?);
    }

    let organizer = Organizer::new(config);
    Ok(organizer.organize(items))
}

/// Result of scoped sorting - contains both sorted structure and items that couldn't be sorted
#[derive(Clone, Debug, Facet)]
pub struct ScopedSortResult<M: Metadata> {
    /// Items that were successfully sorted into the structure
    pub sorted: Structure<M>,
    /// Items that couldn't be sorted (didn't match any subgroup of the scope)
    pub unsorted: Vec<Item<M>>,
}

/// Scoped sorting function - sorts items within a specific group context
///
/// This is useful for iteratively sorting items that couldn't be fully classified
/// in the first pass. By providing a scope (group name), the sorting is constrained
/// to only look at subgroups of that scope.
///
/// # Arguments
/// * `items` - Items to sort (typically from an "Unsorted" folder)
/// * `config` - The full config
/// * `scope` - Name of the group to scope sorting to (e.g., "Guitars")
///
/// # Returns
/// * `ScopedSortResult` containing sorted structure and any remaining unsorted items
///
/// # Example
/// ```ignore
/// // First pass: sort everything
/// let result = monarchy_sort(inputs, config)?;
///
/// // Find unsorted items in Guitars
/// let unsorted_guitars = find_unsorted_in(&result, "Guitars");
///
/// // Re-sort with more context (e.g., user labeled them as Electric)
/// let scoped_result = monarchy_sort_scoped(unsorted_guitars, config, "Electric_Guitar")?;
///
/// // Merge back into original structure
/// merge_into(&mut result, scoped_result.sorted, "Guitars");
/// ```
pub fn monarchy_sort_scoped<M>(
    items: Vec<Item<M>>,
    config: &Config<M>,
    scope: &str,
) -> Result<ScopedSortResult<M>>
where
    M: Metadata,
{
    // Find the scoped group in the config
    fn find_group<'a, M: Metadata>(groups: &'a [Group<M>], name: &str) -> Option<&'a Group<M>> {
        for group in groups {
            if group.name.eq_ignore_ascii_case(name) {
                return Some(group);
            }
            if let Some(found) = find_group(&group.groups, name) {
                return Some(found);
            }
        }
        None
    }

    let scoped_group = find_group(&config.groups, scope).ok_or_else(|| {
        MonarchyError::Parse(format!("Scope group '{}' not found in config", scope))
    })?;

    // Create a scoped config with the full scoped group as the top-level group
    // This preserves the group's patterns so items like "EdCrunch" can match
    // the "Crunch" arrangement pattern defined in Electric Guitar
    let scoped_config = Config {
        groups: vec![scoped_group.clone()],
        fallback_strategy: FallbackStrategy::PlaceAtRoot, // Unsorted items go to root
        parser_rules: config.parser_rules.clone(),
    };

    // Parse and organize with scoped config
    let parser = Parser::new(&scoped_config);
    let mut parsed_items = Vec::new();
    let mut unsorted = Vec::new();

    for item in items {
        // Re-parse the original string with scoped config
        match parser.parse(item.original.clone()) {
            Ok(new_item) => {
                if new_item.matched_groups.is_empty() {
                    // Still couldn't match - keep as unsorted
                    unsorted.push(item);
                } else {
                    parsed_items.push(new_item);
                }
            }
            Err(_) => {
                // Parsing failed - keep original item as unsorted
                unsorted.push(item);
            }
        }
    }

    let organizer = Organizer::new(&scoped_config);
    let sorted = organizer.organize(parsed_items);

    Ok(ScopedSortResult { sorted, unsorted })
}

/// Assign items to a specific group and organize by metadata fields
///
/// This is used when the user explicitly tells us "these items belong to X group".
/// Unlike `monarchy_sort_scoped`, this function:
/// 1. **Assumes** all items belong to the target group (skips pattern matching)
/// 2. Extracts metadata using the group's field patterns (Performer, Arrangement, etc.)
/// 3. Organizes items by those metadata fields
///
/// # Arguments
/// * `items` - Items to assign to the group
/// * `config` - The full config (used to find the target group and its hierarchy)
/// * `target_group` - Name of the group items belong to (e.g., "Electric Guitar")
///
/// # Returns
/// * `ScopedSortResult` where `sorted` contains items organized by metadata fields
///   and `unsorted` is always empty (all items are assigned)
///
/// # Example
/// ```ignore
/// // User says: "These Ed/Johny tracks are electric guitars"
/// let result = assign_to_group(unsorted_items, &config, "Electric Guitar")?;
/// // Result: items organized by Performer (Ed, Johny) then Arrangement (Crunch, Lead, etc.)
/// ```
pub fn assign_to_group<M>(
    items: Vec<Item<M>>,
    config: &Config<M>,
    target_group: &str,
) -> Result<ScopedSortResult<M>>
where
    M: Metadata,
{
    // Find the target group and its hierarchy path in the config
    fn find_group_with_path<'a, M: Metadata>(
        groups: &'a [Group<M>],
        name: &str,
        path: Vec<&'a Group<M>>,
    ) -> Option<(Vec<&'a Group<M>>, &'a Group<M>)> {
        for group in groups {
            let mut current_path = path.clone();
            current_path.push(group);

            if group.name.eq_ignore_ascii_case(name) {
                return Some((current_path, group));
            }
            if let Some(found) = find_group_with_path(&group.groups, name, current_path) {
                return Some(found);
            }
        }
        None
    }

    let (group_path, target) = find_group_with_path(&config.groups, target_group, Vec::new())
        .ok_or_else(|| {
            MonarchyError::Parse(format!("Group '{}' not found in config", target_group))
        })?;

    // Build the matched_groups chain (cloned groups in the path)
    let matched_groups: Vec<Group<M>> = group_path.iter().map(|g| (*g).clone()).collect();

    // Process each item: assign to group and extract metadata
    let mut parsed_items = Vec::new();
    let mut unsorted_items = Vec::new();

    // Primary fields are the ones that actually indicate group membership
    // (Performer, Arrangement, Section) - not auxiliary fields like Layers, Channel, Playlist
    let primary_field_names: std::collections::HashSet<&str> =
        ["Performer", "Arrangement", "Section", "MultiMic"]
            .into_iter()
            .collect();

    for item in items {
        // Extract metadata from the original string using the target group's patterns
        let mut metadata = M::default();
        let mut has_primary_metadata = false;

        // Extract metadata fields defined in the target group
        for field in &target.metadata_fields {
            if let Some(value) = extract_field_value(&item.original, field, target, config) {
                metadata.set(field.clone(), value);
                // Check if this is a primary field
                let field_name = format!("{:?}", field);
                if primary_field_names.contains(field_name.as_str()) {
                    has_primary_metadata = true;
                }
            }
        }

        // Only assign items that have at least one PRIMARY metadata field extracted
        // Fields like Layers ("1", "2") and Playlist (".1", ".2") are too generic
        // to indicate group membership - we need Performer, Arrangement, or Section
        if has_primary_metadata {
            let new_item = Item {
                id: item.id,
                original: item.original,
                metadata,
                matched_groups: matched_groups.clone(),
            };
            parsed_items.push(new_item);
        } else {
            // No primary metadata extracted - this item doesn't belong to the target group
            unsorted_items.push(item);
        }
    }

    // Create config for organizing - use target group
    let org_config = Config {
        groups: vec![target.clone()],
        fallback_strategy: FallbackStrategy::PlaceAtRoot,
        parser_rules: config.parser_rules.clone(),
    };

    let organizer = Organizer::new(&org_config);
    let sorted = organizer.organize(parsed_items);

    Ok(ScopedSortResult {
        sorted,
        unsorted: unsorted_items,
    })
}

/// Extract a field value from input text using a group's patterns
fn extract_field_value<M: Metadata>(
    input: &str,
    field: &M::Field,
    group: &Group<M>,
    config: &Config<M>,
) -> Option<M::Value> {
    let field_name = format!("{:?}", field);

    // First check field_value_descriptors
    if let Some(descriptors) = group.field_value_descriptors.get(&field_name) {
        for descriptor in descriptors {
            if descriptor.matches(input) {
                return M::create_string_value(field, descriptor.value.clone());
            }
        }
    }

    // Then check nested groups that match this field
    for nested_group in &group.groups {
        let group_name_lower = nested_group
            .name
            .to_lowercase()
            .replace("_", "")
            .replace(" ", "");
        let field_name_lower = field_name.to_lowercase().replace("_", "").replace(" ", "");

        if group_name_lower == field_name_lower
            || nested_group.name.eq_ignore_ascii_case(&field_name)
        {
            // This nested group is for this field - check its patterns
            for pattern in &nested_group.patterns {
                if crate::utils::contains_word(input, pattern) {
                    return M::create_string_value(field, pattern.clone());
                }
            }
        }
    }

    // Finally check global metadata patterns from config
    for config_group in &config.groups {
        if config_group.metadata_only {
            for nested in &config_group.groups {
                let nested_name_lower =
                    nested.name.to_lowercase().replace("_", "").replace(" ", "");
                let field_name_lower = field_name.to_lowercase().replace("_", "").replace(" ", "");

                if nested_name_lower == field_name_lower {
                    for pattern in &nested.patterns {
                        if crate::utils::contains_word(input, pattern) {
                            return M::create_string_value(field, pattern.clone());
                        }
                    }
                }
            }
        }
    }

    None
}

/// Move items from the Unsorted folder to a target group and re-sort
///
/// This is the main function for the scoped re-sorting workflow:
/// 1. Extracts items from the Unsorted folder (or a subfolder within it)
/// 2. Re-parses and sorts them within the scope of a target group
/// 3. Merges the sorted result into the target group's existing structure
/// 4. Returns any items that still couldn't be sorted
///
/// # Arguments
/// * `root` - The root structure (modified in place)
/// * `config` - The full config for re-parsing
/// * `unsorted_path` - Path to items within Unsorted (e.g., &["Unsorted"] or &["Unsorted", "John"])
/// * `target_group` - Name of the group to sort items into (e.g., "Electric Guitar")
/// * `target_path` - Path where the target group exists (e.g., &["Guitars"])
///
/// # Returns
/// * Number of items that were successfully sorted
/// * Items that couldn't be sorted are put back in Unsorted
///
/// # Example
/// ```ignore
/// // Move John's items from Unsorted to Guitars/Electric Guitar
/// let sorted_count = move_unsorted_to_group(
///     &mut structure,
///     &config,
///     &["Unsorted", "John"],  // Extract items from Unsorted/John
///     "Electric Guitar",      // Sort within Electric Guitar's subgroups
///     &["Guitars"],           // Merge result into Guitars folder
/// )?;
/// ```
pub fn move_unsorted_to_group<M: Metadata>(
    root: &mut Structure<M>,
    config: &Config<M>,
    unsorted_path: &[&str],
    target_group: &str,
    target_path: &[&str],
) -> Result<usize> {
    // Step 1: Extract items from the unsorted path
    let items = if unsorted_path.is_empty() {
        // Extract from root
        root.extract_all_items()
    } else if unsorted_path.len() == 1 {
        // Extract from a direct child of root
        root.extract_items_from_child(unsorted_path[0])
    } else {
        // Extract from a nested path
        let parent_path = &unsorted_path[..unsorted_path.len() - 1];
        let child_name = unsorted_path[unsorted_path.len() - 1];

        if let Some(parent) = root.find_at_path_mut(parent_path) {
            parent.extract_items_from_child(child_name)
        } else {
            return Ok(0); // Path not found
        }
    };

    if items.is_empty() {
        return Ok(0);
    }

    let _total_items = items.len();

    // Step 2: Assign items to target group and organize by metadata fields
    // This trusts that the user knows these items belong to the target group
    let scoped_result = assign_to_group(items, config, target_group)?;

    let sorted_count = scoped_result.sorted.total_items();

    // Step 3: Merge sorted result into target path
    if !scoped_result.sorted.is_empty() {
        root.merge_at_path(target_path, scoped_result.sorted);
    }

    // Step 4: Put unsorted items back in Unsorted folder
    if !scoped_result.unsorted.is_empty() {
        // Find or create Unsorted folder
        let unsorted_folder = if let Some(folder) = root.find_child_mut("Unsorted") {
            folder
        } else {
            root.children.push(Structure::new("Unsorted"));
            root.children.last_mut().unwrap()
        };
        unsorted_folder.items.extend(scoped_result.unsorted);
    }

    // Step 5: Clean up empty Unsorted folder if needed
    if let Some(unsorted) = root.find_child("Unsorted")
        && unsorted.is_empty()
    {
        root.remove_child("Unsorted");
    }

    Ok(sorted_count)
}

/// Move a specific child node to a target group
///
/// Unlike `move_unsorted_to_group` which moves ALL items from a path, this function
/// moves a specific named child (and all its contents) from a source path to a target group.
///
/// This is useful for manual sorting where you want to move specific performer groups
/// (e.g., "Chad", "Lou") from Unsorted to BGVs, rather than moving everything at once.
///
/// # Arguments
/// * `root` - The root structure (modified in place)
/// * `config` - The full config for re-parsing
/// * `child_name` - Name of the child to move (e.g., "Chad")
/// * `source_path` - Path where the child currently lives (e.g., &["Unsorted"])
/// * `target_group` - Name of the group to sort items into (e.g., "BGVs")
/// * `target_path` - Path where the sorted result should be merged (e.g., &["Vocals"])
///
/// # Returns
/// * `Ok(usize)` - Number of items sorted into the target group
///
/// # Example
///
/// ```ignore
/// // Move "Chad" from Unsorted to BGVs under Vocals
/// move_child_to_group(
///     &mut structure,
///     &config,
///     "Chad",           // Child to move
///     &["Unsorted"],    // Source path
///     "BGVs",           // Target group
///     &["Vocals"],      // Target path
/// )?;
/// ```
pub fn move_child_to_group<M: Metadata>(
    root: &mut Structure<M>,
    config: &Config<M>,
    child_name: &str,
    source_path: &[&str],
    target_group: &str,
    target_path: &[&str],
) -> Result<usize> {
    // Find the source parent and extract the specific child
    let items = if source_path.is_empty() {
        // Source is root - extract child directly
        if let Some(mut child) = root.remove_child(child_name) {
            child.extract_all_items()
        } else {
            return Ok(0); // Child not found
        }
    } else {
        // Navigate to source path and extract child
        if let Some(parent) = root.find_at_path_mut(source_path) {
            if let Some(mut child) = parent.remove_child(child_name) {
                child.extract_all_items()
            } else {
                return Ok(0); // Child not found
            }
        } else {
            return Ok(0); // Source path not found
        }
    };

    if items.is_empty() {
        return Ok(0);
    }

    // Assign items to target group and organize by metadata fields
    let scoped_result = assign_to_group(items, config, target_group)?;
    let sorted_count = scoped_result.sorted.total_items();

    // Merge sorted result into target path
    if !scoped_result.sorted.is_empty() {
        // Before merging, check if we need to "uncollapse" the target
        // This handles the case where Vocals had Lead collapsed into it,
        // but now we're adding BGVs as a sibling - Lead needs to be restored
        uncollapse_for_sibling(root, target_path, target_group, config);

        root.merge_at_path(target_path, scoped_result.sorted);
    }

    // Put unsorted items back in Unsorted folder
    if !scoped_result.unsorted.is_empty() {
        let unsorted_folder = if let Some(folder) = root.find_child_mut("Unsorted") {
            folder
        } else {
            root.children.push(Structure::new("Unsorted"));
            root.children.last_mut().unwrap()
        };
        unsorted_folder.items.extend(scoped_result.unsorted);
    }

    // Clean up empty nodes in source path
    cleanup_empty_nodes(root, source_path);

    Ok(sorted_count)
}

/// Move items matching ANY of multiple patterns from source to target group
///
/// This is useful for moving multiple related items at once, allowing the organizer
/// to see all items together and create proper grouping (e.g., performer subfolders).
///
/// # Arguments
/// * `root` - The root structure (modified in place)
/// * `config` - The full config for re-parsing
/// * `patterns` - Patterns to match (items matching ANY pattern are moved)
/// * `source_path` - Path to extract items from
/// * `target_group` - Name of the group to sort items into
/// * `target_path` - Path where the sorted result should be merged
///
/// # Example
/// ```ignore
/// // Move all BGV performers from Unsorted to BGVs
/// move_items_matching_any_to_group(
///     &mut structure,
///     &config,
///     &["Chad", "Lou", "Tit"],  // All BGV performers
///     &["Unsorted"],
///     "BGVs",
///     &["Vocals"],
/// )?;
/// ```
pub fn move_items_matching_any_to_group<M: Metadata>(
    root: &mut Structure<M>,
    config: &Config<M>,
    patterns: &[&str],
    source_path: &[&str],
    target_group: &str,
    target_path: &[&str],
) -> Result<usize> {
    // Extract items matching ANY of the patterns from the source path
    let matching_items = {
        let source = if source_path.is_empty() {
            Some(root as &mut Structure<M>)
        } else {
            root.find_at_path_mut(source_path)
        };

        match source {
            Some(node) => {
                let mut items = Vec::new();
                extract_items_matching_any(node, patterns, &mut items);
                items
            }
            None => return Ok(0), // Source path not found
        }
    };

    if matching_items.is_empty() {
        return Ok(0);
    }

    // Assign items to target group and organize by metadata fields
    let scoped_result = assign_to_group(matching_items, config, target_group)?;
    let sorted_count = scoped_result.sorted.total_items();

    // Merge sorted result into target path
    if !scoped_result.sorted.is_empty() {
        // Before merging, check if we need to "uncollapse" the target
        // This handles the case where Vocals had Lead collapsed into it,
        // but now we're adding BGVs as a sibling - Lead needs to be restored
        uncollapse_for_sibling(root, target_path, target_group, config);

        root.merge_at_path(target_path, scoped_result.sorted);
    }

    // Put unsorted items back in Unsorted folder
    if !scoped_result.unsorted.is_empty() {
        let unsorted_folder = if let Some(folder) = root.find_child_mut("Unsorted") {
            folder
        } else {
            root.children.push(Structure::new("Unsorted"));
            root.children.last_mut().unwrap()
        };
        unsorted_folder.items.extend(scoped_result.unsorted);
    }

    // Clean up empty nodes in source path
    cleanup_empty_nodes(root, source_path);

    Ok(sorted_count)
}

/// Helper: Extract items matching ANY of multiple patterns from a structure (recursively)
fn extract_items_matching_any<M: Metadata>(
    node: &mut Structure<M>,
    patterns: &[&str],
    out: &mut Vec<Item<M>>,
) {
    // Extract items matching any pattern from this node
    let mut remaining = Vec::new();
    for item in std::mem::take(&mut node.items) {
        if patterns
            .iter()
            .any(|p| crate::utils::contains_word(&item.original, p))
        {
            out.push(item);
        } else {
            remaining.push(item);
        }
    }
    node.items = remaining;

    // Recursively process children
    for child in &mut node.children {
        extract_items_matching_any(child, patterns, out);
    }

    // Remove empty children
    node.children.retain(|c| !c.is_empty());
}

/// Helper: Uncollapse a parent node before adding a sibling child
///
/// When a parent (like Vocals) had a single child (Lead) that was collapsed,
/// and we're now adding a sibling (BGVs), we need to "uncollapse" by wrapping
/// the existing items/children in a folder for the original child.
///
/// Example:
/// - Before: Vocals has items directly (Lead was collapsed)
///   OR Vocals has metadata children like Main, 2 (Lead's metadata children were promoted)
/// - After adding BGVs: Vocals -> [Lead (with original items/children), BGVs (new)]
fn uncollapse_for_sibling<M: Metadata>(
    root: &mut Structure<M>,
    target_path: &[&str],
    new_child_group: &str,
    config: &Config<M>,
) {
    // Find the target node
    let target = if target_path.is_empty() {
        return; // Can't uncollapse root
    } else {
        match root.find_at_path_mut(target_path) {
            Some(t) => t,
            None => return, // Target doesn't exist, will be created by merge
        }
    };

    // Only uncollapse if the target has content that should be in a sibling folder
    // This could be:
    // 1. Items directly on target (collapsed from a child)
    // 2. Metadata-derived children (like Main, 2) that were promoted from a collapsed child
    let has_items = !target.items.is_empty();
    let has_metadata_children = target.children.iter().any(|c| {
        // Check if child name looks like a metadata-derived group (not a config group)
        !is_config_group_name(&c.name, config)
    });

    if !has_items && !has_metadata_children {
        return; // Nothing to uncollapse
    }

    // Find the sibling group name for the new child
    // E.g., if new_child_group is "BGVs", the sibling is "Lead"
    let sibling_name = find_sibling_group_name(target_path, new_child_group, config);

    if let Some(sibling_name) = sibling_name {
        // Check if this sibling folder already exists
        if target.find_child(&sibling_name).is_some() {
            return; // Sibling already exists, no need to uncollapse
        }

        // Create a folder for the sibling and move items/metadata-children into it
        let mut sibling_folder = Structure::new(&sibling_name);
        sibling_folder.items = std::mem::take(&mut target.items);

        // Move metadata-derived children to the sibling, keep config-group children
        let mut remaining_children = Vec::new();
        for child in std::mem::take(&mut target.children) {
            if is_config_group_name(&child.name, config) {
                // Keep config-group children at current level
                remaining_children.push(child);
            } else {
                // Move metadata-derived children to sibling
                sibling_folder.children.push(child);
            }
        }
        target.children = remaining_children;

        // Add the sibling folder as the first child
        target.children.insert(0, sibling_folder);
    }
}

/// Check if a name corresponds to a config-defined group
fn is_config_group_name<M: Metadata>(name: &str, config: &Config<M>) -> bool {
    fn search<M: Metadata>(groups: &[Group<M>], name: &str) -> bool {
        for group in groups {
            if group.name.eq_ignore_ascii_case(name) {
                return true;
            }
            if search(&group.groups, name) {
                return true;
            }
        }
        false
    }
    search(&config.groups, name)
}

/// Find the sibling group name when adding a new child to a parent
///
/// E.g., when adding "BGVs" to "Vocals", the sibling is "Lead"
fn find_sibling_group_name<M: Metadata>(
    parent_path: &[&str],
    new_child: &str,
    config: &Config<M>,
) -> Option<String> {
    // Find the parent group in config
    let parent_name = parent_path.last()?;

    fn find_group<'a, M: Metadata>(groups: &'a [Group<M>], name: &str) -> Option<&'a Group<M>> {
        for group in groups {
            if group.name.eq_ignore_ascii_case(name) {
                return Some(group);
            }
            if let Some(found) = find_group(&group.groups, name) {
                return Some(found);
            }
        }
        None
    }

    let parent_group = find_group(&config.groups, parent_name)?;

    // Find child groups that are NOT the new child and NOT metadata-only
    for child_group in &parent_group.groups {
        if !child_group.metadata_only && !child_group.name.eq_ignore_ascii_case(new_child) {
            return Some(child_group.name.clone());
        }
    }

    None
}

/// Helper: Clean up empty nodes along a path
fn cleanup_empty_nodes<M: Metadata>(root: &mut Structure<M>, path: &[&str]) {
    if path.is_empty() {
        return;
    }

    // Check if the source is empty and remove it
    if path.len() == 1 {
        if let Some(source) = root.find_child(path[0])
            && source.is_empty()
        {
            root.remove_child(path[0]);
        }
    } else {
        // Navigate to parent and check child
        if let Some(parent) = root.find_at_path_mut(&path[..path.len() - 1]) {
            let child_name = path[path.len() - 1];
            if let Some(child) = parent.find_child(child_name)
                && child.is_empty()
            {
                parent.remove_child(child_name);
            }
        }
    }
}

/// Re-sort items and merge them into an existing structure
///
/// This is a simpler version of `move_unsorted_to_group` that takes items directly
/// instead of extracting them from a path.
///
/// # Arguments
/// * `root` - The root structure (modified in place)
/// * `config` - The full config for re-parsing
/// * `items` - Items to sort
/// * `target_group` - Name of the group to sort items into
/// * `target_path` - Path where the sorted result should be merged
///
/// # Returns
/// * `ScopedSortResult` with sorted structure and unsorted items
pub fn sort_and_merge<M: Metadata>(
    root: &mut Structure<M>,
    config: &Config<M>,
    items: Vec<Item<M>>,
    target_group: &str,
    target_path: &[&str],
) -> Result<ScopedSortResult<M>> {
    // Sort items within the target group's scope
    let result = monarchy_sort_scoped(items, config, target_group)?;

    // Merge sorted result into target path
    if !result.sorted.is_empty() {
        root.merge_at_path(target_path, result.sorted.clone());
    }

    Ok(result)
}

/// Re-apply collapse visitor after merging
///
/// After merging new items into a structure, the hierarchy may need to be
/// re-collapsed to remove unnecessary intermediate levels.
pub fn reapply_collapse<M: Metadata>(root: &mut Structure<M>, config: &Config<M>) {
    let collapse = CollapseHierarchy::new(config);
    collapse.apply(root);
}

/// Trait for containers that can hold sorted items
pub trait Target<M: Metadata> {
    fn existing_items(&self) -> Vec<Item<M>>;
}

/// Format metadata as a display name using the item's metadata ToDisplayName implementation
///
/// This delegates to `item.derive_display_name()` which:
/// 1. Extracts prefixes and group names from matched_groups
/// 2. Calls `metadata.to_display_name()` to generate the display string
/// 3. Falls back to original input if no meaningful display name is generated
///
/// Example: "808 Snare Top" where:
/// - "808" is the variant
/// - "Snare" is the most specific group name  
/// - "Top" is the multi_mic value
///
/// Note: The `_config` parameter is kept for API compatibility but not used.
/// The display name logic is controlled by the Metadata type's ToDisplayName implementation.
pub fn to_display_name<M: Metadata + ToDisplayName>(item: &Item<M>, _config: &Config<M>) -> String {
    item.derive_display_name()
}

impl<M: Metadata> Serialize for Item<M>
where
    M: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Item", 4)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("original", &self.original)?;
        state.serialize_field("metadata", &self.metadata)?;
        let group_names: Vec<String> = self.matched_groups.iter().map(|g| g.name.clone()).collect();
        state.serialize_field("matched_groups", &group_names)?;
        state.end()
    }
}

impl<'de, M: Metadata> Deserialize<'de> for Item<M>
where
    M: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct ItemVisitor<M: Metadata>(std::marker::PhantomData<M>);

        impl<'de, M: Metadata> Visitor<'de> for ItemVisitor<M>
        where
            M: Deserialize<'de>,
        {
            type Value = Item<M>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Item")
            }

            fn visit_map<V>(self, mut map: V) -> std::result::Result<Item<M>, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut id = None;
                let mut original = None;
                let mut metadata = None;
                let mut matched_group_names: Option<Vec<String>> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        "id" => {
                            if id.is_some() {
                                return Err(de::Error::duplicate_field("id"));
                            }
                            id = Some(map.next_value()?);
                        }
                        "original" => {
                            if original.is_some() {
                                return Err(de::Error::duplicate_field("original"));
                            }
                            original = Some(map.next_value()?);
                        }
                        "metadata" => {
                            if metadata.is_some() {
                                return Err(de::Error::duplicate_field("metadata"));
                            }
                            metadata = Some(map.next_value()?);
                        }
                        "matched_groups" => {
                            if matched_group_names.is_some() {
                                return Err(de::Error::duplicate_field("matched_groups"));
                            }
                            matched_group_names = Some(map.next_value()?);
                        }
                        _ => {
                            let _ = map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }

                let id = id.ok_or_else(|| de::Error::missing_field("id"))?;
                let original = original.ok_or_else(|| de::Error::missing_field("original"))?;
                let metadata = metadata.ok_or_else(|| de::Error::missing_field("metadata"))?;
                let matched_group_names = matched_group_names.unwrap_or_default();

                // When deserializing, we can't reconstruct the full Group instances
                // So we create empty groups with just the names
                // This is a limitation - ideally we'd need the config to reconstruct properly
                let matched_groups = matched_group_names
                    .into_iter()
                    .map(|name| Group::builder(name).build())
                    .collect();

                Ok(Item {
                    id,
                    original,
                    metadata,
                    matched_groups,
                })
            }
        }

        deserializer.deserialize_struct(
            "Item",
            &["id", "original", "metadata", "matched_groups"],
            ItemVisitor(std::marker::PhantomData),
        )
    }
}

/// Format a metadata value as a string for display
pub fn format_metadata_value<M: Metadata>(value: &M::Value) -> String {
    let value_str = format!("{:?}", value);
    // Remove enum variant wrapper if present (e.g., "MultiMic([\"In\"])" -> "In")
    // This is a simple heuristic - we might need a better way
    if let Some(start) = value_str.find('(')
        && let Some(end) = value_str.rfind(')')
    {
        let inner = &value_str[start + 1..end];
        // If it's a Vec format like ["In", "Out"], extract values
        if inner.starts_with('[') && inner.ends_with(']') {
            let content = &inner[1..inner.len() - 1];
            // Split by comma and clean up quotes
            // Note: We use strip_prefix/strip_suffix to only remove the outer
            // Debug formatting quotes, not quotes that are part of the actual value
            // (e.g., "Rack 10\"" should become "Rack 10\"", not "Rack 10")
            let values: Vec<String> = content
                .split(',')
                .map(|s| {
                    let trimmed = s.trim();
                    // Only strip one quote from each end (the Debug formatting quotes)
                    let without_leading = trimmed.strip_prefix('"').unwrap_or(trimmed);
                    let without_trailing =
                        without_leading.strip_suffix('"').unwrap_or(without_leading);
                    // Unescape any escaped quotes in the value
                    without_trailing.replace("\\\"", "\"")
                })
                .filter(|s| !s.is_empty())
                .collect();
            return values.join(" ");
        }
        // Otherwise, just return the inner content without quotes
        // Strip one quote from each end, not all quotes
        let without_leading = inner.strip_prefix('"').unwrap_or(inner);
        let without_trailing = without_leading.strip_suffix('"').unwrap_or(without_leading);
        return without_trailing.replace("\\\"", "\"");
    }
    value_str
}
