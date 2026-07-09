//! Organizer for building hierarchical structures from parsed items.
//!
//! The organizer takes a collection of parsed [`Item`]s and builds a
//! hierarchical [`Structure`] based on the configuration's group definitions.
//!
//! # Responsibilities
//!
//! - Building the tree structure from matched groups
//! - Grouping items by metadata field values
//! - Handling unmatched items according to the fallback strategy
//! - Applying tagged collections
//! - Applying hierarchy collapse transformations
//!
//! # Usage
//!
//! The organizer is typically used internally by [`monarchy_sort`](crate::monarchy_sort),
//! but can be used directly for custom workflows:
//!
//! ```ignore
//! use monarchy::{Parser, Organizer, Config};
//!
//! let config = Config::builder()
//!     .group(drums_group)
//!     .build();
//!
//! let parser = Parser::new(config.clone());
//! let items: Vec<_> = inputs.iter()
//!     .map(|i| parser.parse(i.clone()).unwrap())
//!     .collect();
//!
//! let organizer = Organizer::new(config);
//! let structure = organizer.organize(items);
//! ```

use crate::{CollapseHierarchy, Config, Item, Metadata, Structure};

/// Organizes parsed [`Item`]s into a hierarchical [`Structure`].
///
/// The organizer builds a tree structure by:
/// 1. Creating nodes for each matched group
/// 2. Grouping items by metadata field values within groups
/// 3. Applying the fallback strategy for unmatched items
/// 4. Collapsing unnecessary intermediate hierarchy levels
///
/// # Example
///
/// ```ignore
/// let organizer = Organizer::new(config);
/// let structure = organizer.organize(items);
///
/// // The structure now contains a tree of groups and items
/// structure.print_tree();
/// ```
#[derive(Clone, Debug)]
pub struct Organizer<'a, M: Metadata> {
    config: &'a Config<M>,
}

impl<'a, M: Metadata> Organizer<'a, M> {
    /// Create a new organizer with the given configuration.
    pub fn new(config: &'a Config<M>) -> Self {
        Self { config }
    }

    /// Organize items into a hierarchical structure.
    ///
    /// This method:
    /// 1. Builds the tree structure from the config's groups
    /// 2. Places items in their matched group locations
    /// 3. Groups items by metadata field values
    /// 4. Handles unmatched items according to the fallback strategy
    /// 5. Applies collapse transformations to simplify the hierarchy
    ///
    /// # Example
    ///
    /// ```ignore
    /// let structure = organizer.organize(items);
    /// assert!(!structure.is_empty());
    /// ```
    pub fn organize(&self, items: Vec<Item<M>>) -> Structure<M> {
        let mut root = Structure::new("root");

        // Build structure from groups (skip metadata_only groups)
        for group in &self.config.groups {
            if !group.metadata_only {
                let group_structure = self.build_group_structure(group, &items, Vec::new());
                if !group_structure.is_empty() {
                    root.children.push(group_structure);
                }
            }
        }

        // Handle unmatched items based on fallback strategy
        let unmatched: Vec<Item<M>> = items
            .into_iter()
            .filter(|item| item.matched_groups.is_empty())
            .collect();

        if !unmatched.is_empty() {
            match self.config.fallback_strategy {
                crate::config::FallbackStrategy::CreateMisc => {
                    let mut unsorted = Structure::new("Unsorted");
                    unsorted.items = unmatched;
                    root.children.push(unsorted);
                }
                crate::config::FallbackStrategy::PlaceAtRoot => {
                    root.items = unmatched;
                }
                crate::config::FallbackStrategy::Reject => {
                    // Items were already rejected during parsing
                }
            }
        }

        // Apply enhanced collapse algorithm using the two-pass system
        // Pass 1: Collapse intermediate groups between top-level and deepest groups
        // Pass 2: Collapse top-level groups that have only a single child
        let collapse = CollapseHierarchy::new(self.config);
        collapse.apply(&mut root);

        root
    }

    /// Build a structure for a specific group with prefix inheritance
    fn build_group_structure(
        &self,
        group: &crate::Group<M>,
        all_items: &[Item<M>],
        parent_prefixes: Vec<String>,
    ) -> Structure<M> {
        // Calculate the accumulated prefixes for this group
        let mut current_prefixes = parent_prefixes.clone();

        // Add this group's prefix if it exists and should be inherited
        if let Some(ref prefix) = group.prefix {
            // Check if this prefix should be added based on inheritance rules
            if group.inherit_prefix {
                // Filter out blocked prefixes
                let filtered_prefixes: Vec<String> = current_prefixes
                    .into_iter()
                    .filter(|p| !group.blocked_prefixes.contains(p))
                    .collect();
                current_prefixes = filtered_prefixes;
            } else {
                // Don't inherit any prefixes if inherit_prefix is false
                current_prefixes.clear();
            }

            // Always add our own prefix
            current_prefixes.push(prefix.clone());
        } else if !group.inherit_prefix {
            // If we don't have a prefix but don't inherit, clear parent prefixes
            current_prefixes.clear();
        } else {
            // Filter out blocked prefixes even if we don't have our own prefix
            current_prefixes.retain(|p| !group.blocked_prefixes.contains(p));
        }

        // Create the display name with prefixes
        let display_name = if current_prefixes.is_empty() {
            group.name.clone()
        } else {
            format!("{} {}", current_prefixes.join(" "), group.name)
        };

        let mut structure = Structure::with_display_name(&group.name, display_name);

        // Find items that matched this group
        // Check if the item's matched_groups path ends with this group
        // The parser ensures each item only matches one group path, so we can safely
        // check if the last group matches this group's name
        let group_items: Vec<Item<M>> = all_items
            .iter()
            .filter(|item| {
                // Check if the last group in the matched path matches this group's name
                item.matched_groups.last().map(|g| &g.name) == Some(&group.name)
            })
            .cloned()
            .collect();

        // Handle tagged collections if specified (pattern-based grouping)
        if let Some(ref tagged_collection_group) = group.tagged_collection {
            // Check which items match the tagged collection patterns
            let mut matching_items = Vec::new();
            let mut non_matching_items = Vec::new();

            for item in group_items {
                if tagged_collection_group.matches(&item.original) {
                    matching_items.push(item);
                } else {
                    non_matching_items.push(item);
                }
            }

            // If we have both matching and non-matching items, create a subfolder for matches
            if !matching_items.is_empty() && !non_matching_items.is_empty() {
                // Create sub-structure for tagged collection
                let mut collection_structure = Structure::new(&tagged_collection_group.name);
                // Group matching items by metadata fields if needed
                let grouped_matching = self.group_by_metadata_fields_recursive(
                    matching_items,
                    group,
                    &group.metadata_fields,
                );
                if grouped_matching.children.is_empty() && grouped_matching.items.is_empty() {
                    // If grouping didn't create structure, use the items from the grouped result
                    collection_structure.items = grouped_matching.items;
                } else {
                    collection_structure.children = grouped_matching.children;
                    collection_structure.items = grouped_matching.items;
                }
                structure.children.push(collection_structure);

                // Group non-matching items by metadata fields if needed
                let grouped_non_matching = self.group_by_metadata_fields_recursive(
                    non_matching_items,
                    group,
                    &group.metadata_fields,
                );
                structure.children.extend(grouped_non_matching.children);
                structure.items = grouped_non_matching.items;
            } else if !matching_items.is_empty() {
                // All items match - group them by metadata fields if needed
                let grouped = self.group_by_metadata_fields_recursive(
                    matching_items,
                    group,
                    &group.metadata_fields,
                );
                structure.children = grouped.children;
                structure.items = grouped.items;
            } else {
                // No items match - group non-matching items by metadata fields if needed
                let grouped = self.group_by_metadata_fields_recursive(
                    non_matching_items,
                    group,
                    &group.metadata_fields,
                );
                structure.children = grouped.children;
                structure.items = grouped.items;
            }
        } else if let Some(ref variant_field) = group.variants {
            // Handle variants - keep items together but group them by variant value
            // Variants are kept in the same structure but sorted/grouped by variant value
            // so the client can handle them (e.g., different lanes on the same track)
            let mut variant_groups: std::collections::HashMap<String, Vec<Item<M>>> =
                std::collections::HashMap::new();

            for item in group_items {
                // Get the variant field value for this item
                let variant_key = if let Some(value) = item.metadata.get(variant_field) {
                    // Convert the value to a string for grouping
                    format!("{:?}", value)
                } else {
                    String::from("default")
                };

                variant_groups
                    .entry(variant_key.clone())
                    .or_default()
                    .push(item);
            }

            // Sort items by variant value and add them to the structure
            // Items are kept together but grouped by variant for client-side handling
            let mut sorted_items = Vec::new();
            let mut variant_keys: Vec<String> = variant_groups.keys().cloned().collect();
            variant_keys.sort();

            for variant_key in variant_keys {
                if let Some(variant_items) = variant_groups.remove(&variant_key) {
                    sorted_items.extend(variant_items);
                }
            }

            structure.items = sorted_items;
        } else {
            // Check if we should group items by metadata field values
            // If multiple items have different values for metadata fields, create sub-structures
            if !group_items.is_empty() && !group.metadata_fields.is_empty() {
                // Use recursive grouping that respects field priority order and strategies
                let grouped = self.group_by_metadata_fields_recursive(
                    group_items,
                    group,
                    &group.metadata_fields,
                );
                structure.items = grouped.items;
                structure.children = grouped.children;
            } else {
                // No metadata fields to group by - add items directly to this level
                structure.items = group_items;
            }
        }

        // Recursively build nested groups with accumulated prefixes
        for nested_group in &group.groups {
            // Skip metadata-only groups - they don't create structure nodes
            if nested_group.metadata_only {
                continue;
            }

            // Filter items to only include those whose matched_groups path includes this parent group
            // This ensures items matched via "Drum Kit" -> "Kick" don't also get placed in "Electronic Kit" -> "Kick"
            let filtered_items: Vec<Item<M>> = all_items
                .iter()
                .filter(|item| {
                    // Check if the item's matched_groups path includes this parent group
                    // before the nested group. This ensures correct hierarchy matching.
                    let matched_names: Vec<&str> = item
                        .matched_groups
                        .iter()
                        .map(|g| g.name.as_str())
                        .collect();
                    // Check if the nested group name appears in the path
                    if let Some(nested_pos) =
                        matched_names.iter().position(|&n| n == nested_group.name)
                    {
                        // Found the nested group, check if this parent group appears before it
                        matched_names[..nested_pos].contains(&group.name.as_str())
                    } else {
                        // Nested group not in path, so this item doesn't belong here
                        false
                    }
                })
                .cloned()
                .collect();

            let child =
                self.build_group_structure(nested_group, &filtered_items, current_prefixes.clone());
            if !child.is_empty() {
                structure.children.push(child);
            }
        }

        structure
    }

    /// Recursively group items by metadata fields in priority order
    ///
    /// Processes fields in the order they appear in `remaining_fields`, applying
    /// the appropriate grouping strategy for each field.
    fn group_by_metadata_fields_recursive(
        &self,
        items: Vec<Item<M>>,
        group: &crate::Group<M>,
        remaining_fields: &[M::Field],
    ) -> Structure<M> {
        let mut result = Structure::new("temp");

        if items.is_empty() || remaining_fields.is_empty() {
            result.items = items;
            return result;
        }

        // Get the first field (highest priority)
        let current_field = &remaining_fields[0];
        let remaining_fields = &remaining_fields[1..];

        let field_name = format!("{:?}", current_field);
        let strategy = group
            .field_grouping_strategies
            .get(&field_name)
            .cloned()
            .unwrap_or(crate::FieldGroupingStrategy::Default);

        // Group items by this field's value
        let mut field_groups: Vec<(String, Vec<Item<M>>)> = Vec::new();
        let mut items_without_field = Vec::new();

        // Check if there's a default value for this field
        let default_value = group.field_default_values.get(&field_name).cloned();

        for item in items {
            if let Some(value) = item.metadata.get(current_field) {
                let value_str = self.format_metadata_value_for_grouping(&value);
                if let Some(entry) = field_groups.iter_mut().find(|(key, _)| key == &value_str) {
                    entry.1.push(item);
                } else {
                    field_groups.push((value_str, vec![item]));
                }
            } else if let Some(ref default) = default_value {
                // Item doesn't have the field, but there's a default value
                // Treat it as if it has the field with the default value
                if let Some(entry) = field_groups.iter_mut().find(|(key, _)| key == default) {
                    entry.1.push(item);
                } else {
                    field_groups.push((default.clone(), vec![item]));
                }
            } else {
                items_without_field.push(item);
            }
        }

        // Sort field_groups by descriptor order if available, otherwise by nested group pattern order
        // Default value should always come first if present
        if let Some(descriptors) = group.field_value_descriptors.get(&field_name) {
            let descriptor_order: Vec<String> =
                descriptors.iter().map(|d| d.value.clone()).collect();
            field_groups.sort_by(|a, b| {
                // Check if either is the default value - default comes first
                let a_is_default = default_value
                    .as_ref()
                    .is_some_and(|d| a.0 == *d || a.0.to_lowercase() == d.to_lowercase());
                let b_is_default = default_value
                    .as_ref()
                    .is_some_and(|d| b.0 == *d || b.0.to_lowercase() == d.to_lowercase());
                match (a_is_default, b_is_default) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => {
                        // Neither or both are default - use descriptor order
                        let a_idx = descriptor_order
                            .iter()
                            .position(|v| a.0 == *v || a.0.to_lowercase() == v.to_lowercase());
                        let b_idx = descriptor_order
                            .iter()
                            .position(|v| b.0 == *v || b.0.to_lowercase() == v.to_lowercase());
                        match (a_idx, b_idx) {
                            (Some(a_pos), Some(b_pos)) => a_pos.cmp(&b_pos),
                            (Some(_), _) => std::cmp::Ordering::Less,
                            (_, Some(_)) => std::cmp::Ordering::Greater,
                            (_, _) => a.0.cmp(&b.0),
                        }
                    }
                }
            });
        } else {
            // Fall back to nested group pattern order if available
            // Find nested group that matches this field name
            if let Some(nested_group) = group.groups.iter().find(|g| {
                let group_name_lower = g.name.to_lowercase().replace("_", "").replace(" ", "");
                let field_name_lower = field_name.to_lowercase().replace("_", "").replace(" ", "");
                group_name_lower == field_name_lower || g.name.eq_ignore_ascii_case(&field_name)
            }) {
                // Use the nested group's patterns as the sort order
                let pattern_order: Vec<String> = nested_group.patterns.clone();
                field_groups.sort_by(|a, b| {
                    // Check if either is the default value - default comes first
                    let a_is_default = default_value
                        .as_ref()
                        .is_some_and(|d| a.0 == *d || a.0.to_lowercase() == d.to_lowercase());
                    let b_is_default = default_value
                        .as_ref()
                        .is_some_and(|d| b.0 == *d || b.0.to_lowercase() == d.to_lowercase());
                    match (a_is_default, b_is_default) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => {
                            // Neither or both are default - use pattern order
                            let a_idx = pattern_order.iter().position(|v| {
                                a.0.eq_ignore_ascii_case(v)
                                    || a.0.to_lowercase() == v.to_lowercase()
                            });
                            let b_idx = pattern_order.iter().position(|v| {
                                b.0.eq_ignore_ascii_case(v)
                                    || b.0.to_lowercase() == v.to_lowercase()
                            });
                            match (a_idx, b_idx) {
                                (Some(a_pos), Some(b_pos)) => a_pos.cmp(&b_pos),
                                (Some(_), _) => std::cmp::Ordering::Less,
                                (_, Some(_)) => std::cmp::Ordering::Greater,
                                (_, _) => a.0.cmp(&b.0),
                            }
                        }
                    }
                });
            }
        }

        // Apply the grouping strategy
        match strategy {
            crate::FieldGroupingStrategy::Default => {
                // Default: items with field values become children, items without stay at current level
                if field_groups.len() > 1 {
                    // Multiple values - create sub-structures
                    for (field_key, items) in field_groups {
                        let mut sub_structure =
                            self.group_by_metadata_fields_recursive(items, group, remaining_fields);
                        sub_structure.name = field_key;
                        sub_structure.display_name = sub_structure.name.clone();
                        result.children.push(sub_structure);
                    }
                    // Items without this field stay at current level, but process remaining fields
                    if !items_without_field.is_empty() {
                        let without_field_structure = self.group_by_metadata_fields_recursive(
                            items_without_field,
                            group,
                            remaining_fields,
                        );
                        result.items = without_field_structure.items;
                        result.children.extend(without_field_structure.children);
                    }
                } else if field_groups.len() == 1 {
                    // Only one value - check if we need to create sub-structure
                    // If there are remaining fields or items without field, create sub-structure
                    // Otherwise, keep at current level
                    let (field_key, items) = field_groups.remove(0);
                    let processed =
                        self.group_by_metadata_fields_recursive(items, group, remaining_fields);

                    // If there are remaining fields to process or children were created, we need a sub-structure
                    // Also create sub-structure if there are items without this field (they'll be at current level)
                    if !remaining_fields.is_empty()
                        || !processed.children.is_empty()
                        || !items_without_field.is_empty()
                    {
                        let mut sub_structure = processed;
                        sub_structure.name = field_key;
                        sub_structure.display_name = sub_structure.name.clone();
                        result.children.push(sub_structure);
                    } else {
                        // No remaining fields and no children - keep at current level
                        result.items = processed.items;
                        result.children = processed.children;
                    }

                    // Also process items without field
                    if !items_without_field.is_empty() {
                        let without_field_structure = self.group_by_metadata_fields_recursive(
                            items_without_field,
                            group,
                            remaining_fields,
                        );
                        result.items.extend(without_field_structure.items);
                        result.children.extend(without_field_structure.children);
                    }
                } else {
                    // No field values - process remaining fields
                    let processed = self.group_by_metadata_fields_recursive(
                        items_without_field,
                        group,
                        remaining_fields,
                    );
                    result.items = processed.items;
                    result.children = processed.children;
                }
            }
            crate::FieldGroupingStrategy::MainOnContainer => {
                // MainOnContainer: items WITHOUT field go on folder track, items WITH field become children
                // When MultiMic is the last field, items with MultiMic become tracks (not folders)
                // The key: items without MultiMic go ON the folder track itself, items with MultiMic become child tracks
                if !field_groups.is_empty() {
                    // Items with field values become children (tracks when remaining_fields is empty)
                    for (field_key, items) in field_groups {
                        let sub_structure =
                            self.group_by_metadata_fields_recursive(items, group, remaining_fields);
                        // If remaining_fields is empty, this should be a structure with only items (will become a track)
                        // If remaining_fields is not empty, it might have children (will become a folder)
                        let mut sub_structure = sub_structure;
                        sub_structure.name = field_key.clone();
                        sub_structure.display_name = field_key;
                        // Always add as child - Structure-to-Track conversion will handle making it a track vs folder
                        result.children.push(sub_structure);
                    }
                    // Items without field go on the folder track (current level)
                    if !items_without_field.is_empty() {
                        let without_field_structure = self.group_by_metadata_fields_recursive(
                            items_without_field,
                            group,
                            remaining_fields,
                        );
                        result.items = without_field_structure.items;
                        result.children.extend(without_field_structure.children);
                    }
                } else {
                    // No items with field - all items go on folder track
                    let processed = self.group_by_metadata_fields_recursive(
                        items_without_field,
                        group,
                        remaining_fields,
                    );
                    result.items = processed.items;
                    result.children = processed.children;
                }
            }
        }

        result
    }

    /// Format a metadata value for grouping purposes
    ///
    /// This converts the metadata value to a string that can be used as a group key.
    /// Uses the format_metadata_value helper from lib.rs which handles Vec<String> properly.
    fn format_metadata_value_for_grouping(&self, value: &M::Value) -> String {
        crate::format_metadata_value::<M>(value)
    }
}
