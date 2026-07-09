//! Scoped sorting and re-sorting functionality.
//!
//! This module provides functions for scoped and iterative sorting workflows,
//! enabling a two-pass sorting approach where items can be re-sorted after
//! user intervention or additional context is available.
//!
//! # Use Cases
//!
//! - **Iterative refinement**: Sort items, review unsorted, then re-sort with more context
//! - **User-guided sorting**: Let users assign items to groups, then organize by metadata
//! - **Incremental updates**: Add new items to an existing sorted structure
//!
//! # Available Functions
//!
//! | Function | Purpose |
//! |----------|---------|
//! | [`monarchy_sort_scoped`] | Sort items within a specific group context |
//! | [`assign_to_group`] | Explicitly assign items to a group |
//! | [`move_unsorted_to_group`] | Move items from Unsorted and re-sort |
//! | [`sort_and_merge`] | Sort and merge into existing structure |
//! | [`reapply_collapse`] | Re-collapse hierarchy after merging |
//!
//! # Example Workflow
//!
//! ```ignore
//! use monarchy::{monarchy_sort, monarchy_sort_scoped, assign_to_group};
//!
//! // Step 1: Initial sort
//! let mut structure = monarchy_sort(inputs, config.clone())?;
//!
//! // Step 2: Find items that ended up in Unsorted
//! let unsorted_items = structure.extract_items_from_child("Unsorted");
//!
//! // Step 3: User says "these are electric guitars"
//! let result = assign_to_group(unsorted_items, &config, "Electric Guitar")?;
//!
//! // Step 4: Merge the sorted items back
//! structure.merge_at_path(&["Guitars"], result.sorted);
//!
//! // Step 5: Re-collapse to clean up the hierarchy
//! reapply_collapse(&mut structure, &config);
//! ```

use crate::metadata::FieldName;
use crate::{
    CollapseHierarchy, Config, Group, Item, Metadata, MonarchyError, Organizer, Parser, Result,
    Structure, config::FallbackStrategy,
};
use facet::Facet;

// region:    --- ScopedSortResult

/// Result of a scoped sorting operation.
///
/// Contains both the successfully sorted items (in a [`Structure`]) and
/// any items that couldn't be sorted within the specified scope.
///
/// # Example
///
/// ```ignore
/// let result = monarchy_sort_scoped(items, config, "Electric Guitar")?;
///
/// println!("Sorted {} items", result.sorted.total_items());
/// println!("Unsorted: {} items", result.unsorted.len());
///
/// // Merge sorted into main structure
/// main_structure.merge_at_path(&["Guitars"], result.sorted);
///
/// // Handle remaining unsorted
/// if !result.unsorted.is_empty() {
///     // Add back to Unsorted folder or handle differently
/// }
/// ```
#[derive(Clone, Debug, Facet)]
pub struct ScopedSortResult<M: Metadata> {
    /// Items that were successfully sorted into a hierarchical structure.
    pub sorted: Structure<M>,

    /// Items that couldn't be sorted within the scope.
    ///
    /// These items didn't match any subgroup of the specified scope
    /// and should be handled by the caller (e.g., placed back in Unsorted).
    pub unsorted: Vec<Item<M>>,
}

// endregion: --- ScopedSortResult

// region:    --- Scoped Sorting Functions

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
    let available_groups: Vec<String> = collect_all_group_names(&config.groups);
    let scoped_group = find_group(&config.groups, scope).ok_or_else(|| {
        MonarchyError::group_not_found_with_available(scope, "scoped sorting", available_groups)
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
///   and `unsorted` contains items that couldn't extract any primary metadata
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
    let available_groups: Vec<String> = collect_all_group_names(&config.groups);
    let (group_path, target) = find_group_with_path(&config.groups, target_group, Vec::new())
        .ok_or_else(|| {
            MonarchyError::group_not_found_with_available(
                target_group,
                "group assignment",
                available_groups,
            )
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
                let field_name = field.name();
                if primary_field_names.contains(field_name) {
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
/// * `unsorted_path` - Path to items within Unsorted (e.g., `&["Unsorted"]` or `&["Unsorted", "John"]`)
/// * `target_group` - Name of the group to sort items into (e.g., "Electric Guitar")
/// * `target_path` - Path where the target group exists (e.g., `&["Guitars"]`)
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
    // -- Extract items from the unsorted path
    let items = if unsorted_path.is_empty() {
        root.extract_all_items()
    } else if unsorted_path.len() == 1 {
        root.extract_items_from_child(unsorted_path[0])
    } else {
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

    // -- Assign items to target group and organize by metadata fields
    let scoped_result = assign_to_group(items, config, target_group)?;
    let sorted_count = scoped_result.sorted.total_items();

    // -- Merge sorted result into target path
    if !scoped_result.sorted.is_empty() {
        root.merge_at_path(target_path, scoped_result.sorted);
    }

    // -- Put unsorted items back in Unsorted folder
    if !scoped_result.unsorted.is_empty() {
        let unsorted_folder = if let Some(folder) = root.find_child_mut("Unsorted") {
            folder
        } else {
            root.children.push(Structure::new("Unsorted"));
            root.children.last_mut().unwrap()
        };
        unsorted_folder.items.extend(scoped_result.unsorted);
    }

    // -- Clean up empty Unsorted folder if needed
    if let Some(unsorted) = root.find_child("Unsorted") {
        if unsorted.is_empty() {
            root.remove_child("Unsorted");
        }
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

    // Clean up empty source path if needed
    if !source_path.is_empty() {
        // Check if source parent is now empty and remove it
        if let Some(parent) = root.find_at_path_mut(&source_path[..source_path.len() - 1]) {
            let source_name = source_path[source_path.len() - 1];
            if let Some(source) = parent.find_child(source_name) {
                if source.is_empty() {
                    parent.remove_child(source_name);
                }
            }
        } else if source_path.len() == 1 {
            // Source is direct child of root
            if let Some(source) = root.find_child(source_path[0]) {
                if source.is_empty() {
                    root.remove_child(source_path[0]);
                }
            }
        }
    }

    Ok(sorted_count)
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
    use crate::visitor::Visitable;
    let mut collapse_visitor = CollapseHierarchy::new(config);
    for child in &mut root.children {
        child.accept(&mut collapse_visitor);
    }
}

// endregion: --- Scoped Sorting Functions

// region:    --- Support Functions

/// Collect all group names from a list of groups (recursively)
fn collect_all_group_names<M: Metadata>(groups: &[Group<M>]) -> Vec<String> {
    let mut names = Vec::new();
    for group in groups {
        if !group.metadata_only {
            names.push(group.name.clone());
        }
        names.extend(collect_all_group_names(&group.groups));
    }
    names
}

/// Find a group by name in a list of groups (recursively)
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

/// Find a group by name and return the path to it
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

/// Extract a field value from input text using a group's patterns
fn extract_field_value<M: Metadata>(
    input: &str,
    field: &M::Field,
    group: &Group<M>,
    config: &Config<M>,
) -> Option<M::Value> {
    let field_name = field.name();

    // First check field_value_descriptors
    if let Some(descriptors) = group.field_value_descriptors.get(field_name)
        && let Some(descriptor) = descriptors.iter().find(|d| d.matches(input))
    {
        return M::create_string_value(field, descriptor.value.clone());
    }

    // Then check nested groups that match this field
    for nested_group in &group.groups {
        let group_name_lower = nested_group
            .name
            .to_lowercase()
            .replace('_', "")
            .replace(' ', "");
        let field_name_lower = field_name.to_lowercase().replace('_', "").replace(' ', "");

        if group_name_lower == field_name_lower
            || nested_group.name.eq_ignore_ascii_case(field_name)
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
                    nested.name.to_lowercase().replace('_', "").replace(' ', "");
                let field_name_lower = field_name.to_lowercase().replace('_', "").replace(' ', "");

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

// endregion: --- Support Functions
