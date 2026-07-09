//! Hierarchical tree structure for organized items.
//!
//! The [`Structure`] type represents the output of monarchy's sorting process.
//! It's a tree where each node can contain:
//!
//! - **Items**: The actual sorted items at this level
//! - **Children**: Nested sub-structures for further categorization
//! - **Names**: Both a canonical name and a display name with prefixes
//!
//! # Structure Navigation
//!
//! ```ignore
//! // Find a specific child
//! if let Some(drums) = structure.find_child("Drums") {
//!     println!("Found {} drum items", drums.total_items());
//! }
//!
//! // Navigate by path
//! if let Some(kick) = structure.find_at_path(&["Drums", "Kick"]) {
//!     for item in &kick.items {
//!         println!("  {}", item.original);
//!     }
//! }
//! ```
//!
//! # Structure Manipulation
//!
//! ```ignore
//! // Merge structures
//! structure.merge(other_structure);
//!
//! // Merge at a specific path
//! structure.merge_at_path(&["Guitars", "Electric"], new_items_structure);
//!
//! // Extract items from a child
//! let items = structure.extract_items_from_child("Unsorted");
//! ```
//!
//! # Display
//!
//! ```ignore
//! // Print as a tree
//! structure.print_tree();
//!
//! // Get as string
//! let tree_string = structure.to_tree_string();
//! ```

use crate::{Item, Metadata};
use facet::Facet;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A hierarchical tree structure containing organized items.
///
/// Each `Structure` node represents a level in the organization hierarchy.
/// The root structure typically has name "root" and contains the top-level
/// categories as children.
///
/// # Fields
///
/// - `name`: The canonical name of this node (e.g., "Kick")
/// - `display_name`: The display name with prefixes (e.g., "D Kick")
/// - `items`: Items directly contained at this level
/// - `children`: Nested sub-structures
///
/// # Example
///
/// ```ignore
/// let structure = monarchy_sort(inputs, config)?;
///
/// // Iterate over top-level groups
/// for child in &structure.children {
///     println!("{}: {} items", child.name, child.total_items());
/// }
/// ```
#[derive(Clone, Debug, Serialize, Deserialize, Facet)]
pub struct Structure<M: Metadata> {
    /// Name of this level in the hierarchy (e.g., "Kick", "Snare").
    pub name: String,

    /// Display name with accumulated prefixes (e.g., "D Kick").
    ///
    /// This includes any prefix inheritance from parent groups.
    pub display_name: String,

    /// Items directly contained at this level.
    ///
    /// These are the actual sorted items. A node can have both items
    /// and children (e.g., a folder with items on it and subfolders).
    pub items: Vec<Item<M>>,

    /// Child structures representing nested categories.
    pub children: Vec<Structure<M>>,
}

impl<M: Metadata> Structure<M> {
    /// Create a new structure with the given name
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            display_name: name.clone(),
            name,
            items: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Create a new structure with a name and display name
    pub fn with_display_name(name: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            display_name: display_name.into(),
            items: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Check if this structure is empty (no items or children)
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.children.is_empty()
    }

    /// Get the total count of items (recursive)
    pub fn total_items(&self) -> usize {
        self.items.len() + self.children.iter().map(|c| c.total_items()).sum::<usize>()
    }

    /// Get the depth of the hierarchy from this point
    pub fn depth(&self) -> usize {
        if self.children.is_empty() {
            1
        } else {
            1 + self.children.iter().map(|c| c.depth()).max().unwrap_or(0)
        }
    }

    /// Find a child structure by name
    pub fn find_child(&self, name: &str) -> Option<&Structure<M>> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Find a child structure by name (mutable)
    pub fn find_child_mut(&mut self, name: &str) -> Option<&mut Structure<M>> {
        self.children.iter_mut().find(|c| c.name == name)
    }

    /// Print the structure as a tree to stdout
    pub fn print_tree(&self) {
        // Skip root level if it's just a container
        if self.name == "root" || self.name.is_empty() {
            for child in &self.children {
                child.print_tree_with_prefix("", true);
            }
            // Print root items if any
            if !self.items.is_empty() {
                print!("-Uncategorized: [");
                for (i, item) in self.items.iter().enumerate() {
                    if i > 0 {
                        print!(", ");
                    }
                    print!("{}", item.original);
                }
                println!("]");
            }
        } else {
            self.print_tree_with_prefix("", true);
        }
    }

    /// Print the structure as a tree with the given prefix
    fn print_tree_with_prefix(&self, prefix: &str, _is_last: bool) {
        // Print current node
        let node_prefix = format!("{}-", prefix);
        if !self.items.is_empty() {
            // Node with items - show count if many items
            if self.items.len() > 3 {
                print!("{}{}: [", node_prefix, self.display_name);
                for i in 0..2 {
                    if i > 0 {
                        print!(", ");
                    }
                    print!("{}", self.items[i].original);
                }
                println!(", ... and {} more]", self.items.len() - 2);
            } else {
                print!("{}{}: [", node_prefix, self.display_name);
                for (i, item) in self.items.iter().enumerate() {
                    if i > 0 {
                        print!(", ");
                    }
                    print!("{}", item.original);
                }
                println!("]");
            }
        } else if !self.children.is_empty() {
            // Node without items but with children
            println!("{}{}", node_prefix, self.display_name);
        } else {
            // Empty node (shouldn't happen normally but handle it)
            println!("{}{} (empty)", node_prefix, self.display_name);
        }

        // Print children
        for (i, child) in self.children.iter().enumerate() {
            let is_last_child = i == self.children.len() - 1;
            let child_prefix = format!("{}-", prefix);
            child.print_tree_with_prefix(&child_prefix, is_last_child);
        }
    }

    /// Convert the structure to a pretty-printed string tree
    pub fn to_tree_string(&self) -> String {
        let mut result = String::new();

        // Skip root level if it's just a container
        if self.name == "root" || self.name.is_empty() {
            for child in &self.children {
                result.push_str(&child.to_tree_string_with_prefix("", true));
            }
            // Add root items if any
            if !self.items.is_empty() {
                result.push_str("-Uncategorized: [");
                for (i, item) in self.items.iter().enumerate() {
                    if i > 0 {
                        result.push_str(", ");
                    }
                    result.push_str(&item.original);
                }
                result.push_str("]\n");
            }
        } else {
            result.push_str(&self.to_tree_string_with_prefix("", true));
        }

        result
    }

    fn to_tree_string_with_prefix(&self, prefix: &str, _is_last: bool) -> String {
        let mut result = String::new();

        // Build current node string
        let node_prefix = format!("{}-", prefix);
        if !self.items.is_empty() {
            // Node with items - show count if many items
            if self.items.len() > 3 {
                result.push_str(&format!("{}{}: [", node_prefix, self.name));
                for i in 0..2 {
                    if i > 0 {
                        result.push_str(", ");
                    }
                    result.push_str(&self.items[i].original);
                }
                result.push_str(&format!(", ... and {} more]\n", self.items.len() - 2));
            } else {
                result.push_str(&format!("{}{}: [", node_prefix, self.name));
                for (i, item) in self.items.iter().enumerate() {
                    if i > 0 {
                        result.push_str(", ");
                    }
                    result.push_str(&item.original);
                }
                result.push_str("]\n");
            }
        } else if !self.children.is_empty() {
            // Node without items but with children
            result.push_str(&format!("{}{}\n", node_prefix, self.name));
        } else {
            // Empty node
            result.push_str(&format!("{}{} (empty)\n", node_prefix, self.name));
        }

        // Add children
        for (i, child) in self.children.iter().enumerate() {
            let is_last_child = i == self.children.len() - 1;
            let child_prefix = format!("{}-", prefix);
            result.push_str(&child.to_tree_string_with_prefix(&child_prefix, is_last_child));
        }

        result
    }
}

impl<M: Metadata> fmt::Display for Structure<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_tree_string())
    }
}

// ============================================================================
// Structure manipulation methods for scoped sorting workflow
// ============================================================================

impl<M: Metadata> Structure<M> {
    /// Remove and return a child structure by name
    ///
    /// Returns `Some(Structure)` if found and removed, `None` otherwise.
    pub fn remove_child(&mut self, name: &str) -> Option<Structure<M>> {
        if let Some(pos) = self.children.iter().position(|c| c.name == name) {
            Some(self.children.remove(pos))
        } else {
            None
        }
    }

    /// Extract all items from this structure recursively
    ///
    /// Returns all items from this node and all descendants, clearing them from the structure.
    pub fn extract_all_items(&mut self) -> Vec<Item<M>> {
        let mut items = std::mem::take(&mut self.items);
        for child in &mut self.children {
            items.extend(child.extract_all_items());
        }
        items
    }

    /// Extract items from a specific child by name
    ///
    /// Removes the child and returns all its items (recursively).
    /// Returns empty Vec if child not found.
    pub fn extract_items_from_child(&mut self, name: &str) -> Vec<Item<M>> {
        if let Some(mut child) = self.remove_child(name) {
            child.extract_all_items()
        } else {
            Vec::new()
        }
    }

    /// Merge another structure into this one
    ///
    /// The incoming structure's children are merged with existing children by name.
    /// If a child with the same name exists, items and grandchildren are merged recursively.
    /// If no matching child exists, the incoming child is added as a new child.
    pub fn merge(&mut self, other: Structure<M>) {
        // Merge items at this level
        self.items.extend(other.items);

        // Merge children
        for other_child in other.children {
            if let Some(existing_child) = self.find_child_mut(&other_child.name) {
                // Recursively merge into existing child
                existing_child.merge(other_child);
            } else {
                // Add as new child
                self.children.push(other_child);
            }
        }
    }

    /// Merge a structure into a specific child path
    ///
    /// The path is a list of child names to traverse (e.g., ["Guitars", "Electric Guitar"]).
    /// Creates intermediate nodes if they don't exist.
    /// The structure is merged at the final path location.
    ///
    /// # Example
    /// ```ignore
    /// // Merge into root.Guitars.Electric Guitar
    /// root.merge_at_path(&["Guitars", "Electric Guitar"], sorted_structure);
    /// ```
    pub fn merge_at_path(&mut self, path: &[&str], structure: Structure<M>) {
        if path.is_empty() {
            // Merge directly into this node
            self.merge(structure);
            return;
        }

        let target_name = path[0];
        let remaining_path = &path[1..];

        // Find or create the target child
        let target = if let Some(pos) = self.children.iter().position(|c| c.name == target_name) {
            &mut self.children[pos]
        } else {
            // Create new intermediate node
            self.children.push(Structure::new(target_name));
            self.children.last_mut().unwrap()
        };

        // Recursively merge at remaining path
        target.merge_at_path(remaining_path, structure);
    }

    /// Find a child structure by path (recursive)
    ///
    /// Returns a reference to the structure at the given path, or None if not found.
    pub fn find_at_path(&self, path: &[&str]) -> Option<&Structure<M>> {
        if path.is_empty() {
            return Some(self);
        }

        let target_name = path[0];
        let remaining_path = &path[1..];

        self.find_child(target_name)
            .and_then(|child| child.find_at_path(remaining_path))
    }

    /// Find a child structure by path (mutable, recursive)
    pub fn find_at_path_mut(&mut self, path: &[&str]) -> Option<&mut Structure<M>> {
        if path.is_empty() {
            return Some(self);
        }

        let target_name = path[0];
        let remaining_path = &path[1..];

        self.find_child_mut(target_name)
            .and_then(|child| child.find_at_path_mut(remaining_path))
    }

    /// Get all items at a specific path
    ///
    /// Returns references to items at the given path location.
    pub fn items_at_path(&self, path: &[&str]) -> Option<&[Item<M>]> {
        self.find_at_path(path).map(|s| s.items.as_slice())
    }
}

// ============================================================================
// Closure-based transform methods
// ============================================================================

impl<M: Metadata> Structure<M> {
    /// Recursively transform `name` and `display_name` on every node.
    ///
    /// The closure receives the current name and should return the transformed name.
    /// If the closure returns an empty string, the original name is kept (guard against
    /// accidental erasure from overly broad regex replacements).
    ///
    /// # Example
    /// ```ignore
    /// // Strip a song name prefix from all structure nodes
    /// structure.map_names(&|name| name.replace("MySong_", ""));
    /// ```
    pub fn map_names(&mut self, f: &impl Fn(&str) -> String) {
        let new_name = f(&self.name);
        if !new_name.is_empty() {
            self.name = new_name;
        }

        let new_display = f(&self.display_name);
        if !new_display.is_empty() {
            self.display_name = new_display;
        }

        for child in &mut self.children {
            child.map_names(f);
        }
    }

    /// Recursively apply a closure to every [`Item`] in the tree.
    ///
    /// Visits items at every level of the hierarchy (this node's items first,
    /// then children depth-first).
    ///
    /// # Example
    /// ```ignore
    /// // Extract tempo from every item's original name into metadata
    /// structure.map_items(&|item| {
    ///     if let Some(tempo) = extract_tempo(&item.original) {
    ///         item.metadata.set_tempo(tempo);
    ///     }
    /// });
    /// ```
    pub fn map_items(&mut self, f: &impl Fn(&mut Item<M>)) {
        for item in &mut self.items {
            f(item);
        }

        for child in &mut self.children {
            child.map_items(f);
        }
    }

    /// General-purpose depth-first node visitor.
    ///
    /// Applies the closure to this node first, then recursively to each child.
    /// Use this when [`map_names`](Self::map_names) or [`map_items`](Self::map_items)
    /// aren't sufficient (e.g., you need to inspect or mutate children, items,
    /// and names together on the same node).
    ///
    /// # Example
    /// ```ignore
    /// // Log every node in the tree
    /// structure.for_each_node(&|node| {
    ///     println!("{}: {} items, {} children", node.name, node.items.len(), node.children.len());
    /// });
    /// ```
    pub fn for_each_node(&mut self, f: &impl Fn(&mut Structure<M>)) {
        f(self);

        for child in &mut self.children {
            child.for_each_node(f);
        }
    }
}

// ============================================================================
// Read-only traversal and query methods
// ============================================================================

impl<M: Metadata> Structure<M> {
    /// Collect references to all items in the tree (depth-first, non-destructive).
    ///
    /// Returns items from this node first, then recursively from children.
    /// Unlike [`extract_all_items`](Self::extract_all_items), this does not
    /// remove items from the structure.
    ///
    /// # Example
    /// ```ignore
    /// let all_items = structure.flatten_items();
    /// println!("Total items: {}", all_items.len());
    /// for item in &all_items {
    ///     println!("  {}", item.original);
    /// }
    /// ```
    pub fn flatten_items(&self) -> Vec<&Item<M>> {
        let mut items: Vec<&Item<M>> = self.items.iter().collect();
        for child in &self.children {
            items.extend(child.flatten_items());
        }
        items
    }

    /// Read-only depth-first traversal with depth tracking.
    ///
    /// Calls `f` on this node (at the given depth), then recursively on children.
    /// Depth is 0-indexed from the node `walk` is called on.
    ///
    /// For mutable traversal, use [`walk_mut`](Self::walk_mut) or
    /// [`for_each_node`](Self::for_each_node).
    ///
    /// # Example
    /// ```ignore
    /// structure.walk(&|node, depth| {
    ///     let indent = "  ".repeat(depth);
    ///     println!("{}{}: {} items", indent, node.name, node.items.len());
    /// });
    /// ```
    pub fn walk(&self, f: &impl Fn(&Structure<M>, usize)) {
        self.walk_inner(f, 0);
    }

    fn walk_inner(&self, f: &impl Fn(&Structure<M>, usize), depth: usize) {
        f(self, depth);
        for child in &self.children {
            child.walk_inner(f, depth + 1);
        }
    }

    /// Mutable depth-first traversal with depth tracking.
    ///
    /// Like [`for_each_node`](Self::for_each_node) but also provides the current
    /// depth (0-indexed from the node `walk_mut` is called on).
    ///
    /// # Example
    /// ```ignore
    /// // Only strip names below the top-level groups
    /// structure.walk_mut(&|node, depth| {
    ///     if depth > 1 {
    ///         node.name = node.name.replace("prefix_", "");
    ///     }
    /// });
    /// ```
    pub fn walk_mut(&mut self, f: &impl Fn(&mut Structure<M>, usize)) {
        self.walk_mut_inner(f, 0);
    }

    fn walk_mut_inner(&mut self, f: &impl Fn(&mut Structure<M>, usize), depth: usize) {
        f(self, depth);
        for child in &mut self.children {
            child.walk_mut_inner(f, depth + 1);
        }
    }

    /// Read-only depth-first traversal with path tracking.
    ///
    /// Calls `f` on this node with the full path of node names from the root
    /// to the current node (inclusive). This is the pattern needed for
    /// hierarchical lookups like color assignment or path-based routing.
    ///
    /// # Example
    /// ```ignore
    /// // Look up colors based on the group hierarchy path
    /// let mut color_map = HashMap::new();
    /// structure.walk_with_path(&|node, path| {
    ///     if let Some(color) = color_for_path(path) {
    ///         for item in &node.items {
    ///             color_map.insert(item.original.clone(), color);
    ///         }
    ///     }
    /// });
    /// ```
    pub fn walk_with_path(&self, f: &impl Fn(&Structure<M>, &[&str])) {
        self.walk_with_path_inner(f, &mut Vec::new());
    }

    fn walk_with_path_inner<'s>(
        &'s self,
        f: &impl Fn(&Structure<M>, &[&str]),
        path: &mut Vec<&'s str>,
    ) {
        path.push(&self.name);
        f(self, path);
        for child in &self.children {
            child.walk_with_path_inner(f, path);
        }
        path.pop();
    }

    /// Count the total number of nodes in the tree (including this node).
    ///
    /// Unlike [`total_items`](Self::total_items) which counts items,
    /// this counts the structure nodes themselves.
    ///
    /// # Example
    /// ```ignore
    /// let structure = monarchy_sort(inputs, &config)?;
    /// println!("{} groups, {} items", structure.node_count(), structure.total_items());
    /// ```
    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.node_count()).sum::<usize>()
    }
}

// ============================================================================
// Filtering and sorting methods
// ============================================================================

impl<M: Metadata> Structure<M> {
    /// Recursively keep only children where the predicate returns true.
    ///
    /// Applies the filter at every level of the tree — a child is removed if
    /// `f` returns false, and its subtree is not visited further.
    ///
    /// # Example
    /// ```ignore
    /// // Remove all empty nodes from the tree
    /// structure.retain_children(&|child| !child.is_empty());
    ///
    /// // Keep only groups matching a set of names
    /// let keep = HashSet::from(["Drums", "Guitars"]);
    /// structure.retain_children(&|child| keep.contains(child.name.as_str()));
    /// ```
    pub fn retain_children(&mut self, f: &impl Fn(&Structure<M>) -> bool) {
        self.children.retain(|child| f(child));
        for child in &mut self.children {
            child.retain_children(f);
        }
    }

    /// Recursively sort children at every level using a comparator.
    ///
    /// Sorts this node's direct children, then recurses into each child
    /// to sort their children, and so on.
    ///
    /// # Example
    /// ```ignore
    /// // Sort all children alphabetically by name
    /// structure.sort_children_by(&|a, b| a.name.cmp(&b.name));
    ///
    /// // Sort by item count (most items first)
    /// structure.sort_children_by(&|a, b| b.total_items().cmp(&a.total_items()));
    /// ```
    pub fn sort_children_by(
        &mut self,
        f: &impl Fn(&Structure<M>, &Structure<M>) -> std::cmp::Ordering,
    ) {
        self.children.sort_by(|a, b| f(a, b));
        for child in &mut self.children {
            child.sort_children_by(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal Metadata implementation for testing structure methods.
    // We only need the trait bounds satisfied — no real fields or parsing.
    #[derive(Clone, Default, Debug)]
    struct TestMeta;

    #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, facet::Facet)]
    #[repr(u8)]
    enum TestField {
        _Dummy,
    }

    #[derive(Clone, Debug, facet::Facet)]
    #[repr(u8)]
    enum TestValue {
        _Dummy,
    }

    impl crate::metadata::FieldName for TestField {
        fn name(&self) -> &'static str {
            match self {
                TestField::_Dummy => "_Dummy",
            }
        }
    }

    impl crate::Metadata for TestMeta {
        type Field = TestField;
        type Value = TestValue;

        fn get(&self, _field: &Self::Field) -> Option<Self::Value> {
            None
        }
        fn set(&mut self, _field: Self::Field, _value: Self::Value) {}
        fn fields() -> Vec<Self::Field> {
            vec![]
        }
    }

    fn make_item(name: &str) -> Item<TestMeta> {
        Item {
            id: name.to_string(),
            original: name.to_string(),
            metadata: TestMeta,
            matched_groups: vec![],
        }
    }

    fn nested_structure() -> Structure<TestMeta> {
        let mut root = Structure::new("SongName_Drums");
        root.display_name = "SongName_Drums".to_string();
        root.items.push(make_item("overhead"));

        let mut kick = Structure::new("SongName_Kick");
        kick.display_name = "SongName_Kick".to_string();
        kick.items.push(make_item("kick_in"));
        kick.items.push(make_item("kick_out"));

        let mut snare = Structure::new("SongName_Snare");
        snare.display_name = "SongName_Snare".to_string();
        snare.items.push(make_item("snare_top"));

        root.children.push(kick);
        root.children.push(snare);
        root
    }

    // -- map_names tests --

    #[test]
    fn map_names_transforms_all_nodes() {
        let mut s = nested_structure();
        s.map_names(&|name| name.replace("SongName_", ""));

        assert_eq!(s.name, "Drums");
        assert_eq!(s.display_name, "Drums");
        assert_eq!(s.children[0].name, "Kick");
        assert_eq!(s.children[0].display_name, "Kick");
        assert_eq!(s.children[1].name, "Snare");
        assert_eq!(s.children[1].display_name, "Snare");
    }

    #[test]
    fn map_names_guards_against_empty_result() {
        let mut s = Structure::<TestMeta>::new("Drums");
        // Closure that would erase the name entirely
        s.map_names(&|name| name.replace("Drums", ""));

        // Original name is preserved because closure returned ""
        assert_eq!(s.name, "Drums");
        assert_eq!(s.display_name, "Drums");
    }

    #[test]
    fn map_names_on_empty_structure() {
        let mut s = Structure::<TestMeta>::new("root");
        s.map_names(&|name| format!("prefix_{}", name));
        assert_eq!(s.name, "prefix_root");
    }

    // -- map_items tests --

    #[test]
    fn map_items_visits_all_items_recursively() {
        let mut s = nested_structure();
        let counter = std::cell::Cell::new(0u32);
        s.map_items(&|_item| {
            counter.set(counter.get() + 1);
        });
        let count = counter.get();

        // root has 1 item, kick has 2, snare has 1 = 4 total
        assert_eq!(count, 4);
    }

    #[test]
    fn map_items_can_mutate_items() {
        let mut s = nested_structure();
        s.map_items(&|item| {
            item.original = item.original.to_uppercase();
        });

        assert_eq!(s.items[0].original, "OVERHEAD");
        assert_eq!(s.children[0].items[0].original, "KICK_IN");
        assert_eq!(s.children[0].items[1].original, "KICK_OUT");
        assert_eq!(s.children[1].items[0].original, "SNARE_TOP");
    }

    #[test]
    fn map_items_on_empty_structure() {
        let mut s = Structure::<TestMeta>::new("empty");
        let counter = std::cell::Cell::new(0u32);
        s.map_items(&|_| {
            counter.set(counter.get() + 1);
        });
        assert_eq!(counter.get(), 0);
    }

    // -- for_each_node tests --

    #[test]
    fn for_each_node_visits_all_nodes_depth_first() {
        let mut s = nested_structure();
        let visited = std::cell::RefCell::new(Vec::new());
        s.for_each_node(&|node| {
            visited.borrow_mut().push(node.name.clone());
        });

        let names = visited.into_inner();
        // Depth-first: root first, then first child, then second child
        assert_eq!(
            names,
            vec!["SongName_Drums", "SongName_Kick", "SongName_Snare"]
        );
    }

    #[test]
    fn for_each_node_can_mutate_nodes() {
        let mut s = nested_structure();
        s.for_each_node(&|node| {
            node.name = node.name.to_uppercase();
        });

        assert_eq!(s.name, "SONGNAME_DRUMS");
        assert_eq!(s.children[0].name, "SONGNAME_KICK");
        assert_eq!(s.children[1].name, "SONGNAME_SNARE");
    }

    #[test]
    fn for_each_node_on_empty_structure() {
        let mut s = Structure::<TestMeta>::new("lonely");
        let counter = std::cell::Cell::new(0u32);
        s.for_each_node(&|_| {
            counter.set(counter.get() + 1);
        });
        assert_eq!(counter.get(), 1); // visits the node itself
    }

    // -- flatten_items tests --

    #[test]
    fn flatten_items_collects_all_items() {
        let s = nested_structure();
        let items = s.flatten_items();
        let names: Vec<&str> = items.iter().map(|i| i.original.as_str()).collect();
        // depth-first: root items, then kick items, then snare items
        assert_eq!(names, vec!["overhead", "kick_in", "kick_out", "snare_top"]);
    }

    #[test]
    fn flatten_items_on_empty_structure() {
        let s = Structure::<TestMeta>::new("empty");
        assert!(s.flatten_items().is_empty());
    }

    #[test]
    fn flatten_items_is_non_destructive() {
        let s = nested_structure();
        let _items = s.flatten_items();
        // Structure still has all its items
        assert_eq!(s.total_items(), 4);
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.children[0].items.len(), 2);
    }

    // -- walk tests --

    #[test]
    fn walk_visits_with_correct_depths() {
        let s = nested_structure();
        let visited = std::cell::RefCell::new(Vec::new());
        s.walk(&|node, depth| {
            visited.borrow_mut().push((node.name.clone(), depth));
        });

        let result = visited.into_inner();
        assert_eq!(
            result,
            vec![
                ("SongName_Drums".to_string(), 0),
                ("SongName_Kick".to_string(), 1),
                ("SongName_Snare".to_string(), 1),
            ]
        );
    }

    #[test]
    fn walk_on_deeper_tree() {
        let mut root = Structure::<TestMeta>::new("root");
        let mut child = Structure::<TestMeta>::new("child");
        let grandchild = Structure::<TestMeta>::new("grandchild");
        child.children.push(grandchild);
        root.children.push(child);

        let visited = std::cell::RefCell::new(Vec::new());
        root.walk(&|node, depth| {
            visited.borrow_mut().push((node.name.clone(), depth));
        });

        let result = visited.into_inner();
        assert_eq!(
            result,
            vec![
                ("root".to_string(), 0),
                ("child".to_string(), 1),
                ("grandchild".to_string(), 2),
            ]
        );
    }

    #[test]
    fn walk_on_leaf_node() {
        let s = Structure::<TestMeta>::new("leaf");
        let counter = std::cell::Cell::new(0u32);
        s.walk(&|_, depth| {
            assert_eq!(depth, 0);
            counter.set(counter.get() + 1);
        });
        assert_eq!(counter.get(), 1);
    }

    // -- walk_mut tests --

    #[test]
    fn walk_mut_provides_depth_and_allows_mutation() {
        let mut s = nested_structure();
        s.walk_mut(&|node, depth| {
            node.name = format!("d{}_{}", depth, node.name);
        });

        assert_eq!(s.name, "d0_SongName_Drums");
        assert_eq!(s.children[0].name, "d1_SongName_Kick");
        assert_eq!(s.children[1].name, "d1_SongName_Snare");
    }

    #[test]
    fn walk_mut_skip_root_pattern() {
        let mut s = nested_structure();
        // Common pattern: only transform nodes below root
        s.walk_mut(&|node, depth| {
            if depth > 0 {
                node.name = node.name.to_uppercase();
            }
        });

        // Root unchanged
        assert_eq!(s.name, "SongName_Drums");
        // Children transformed
        assert_eq!(s.children[0].name, "SONGNAME_KICK");
        assert_eq!(s.children[1].name, "SONGNAME_SNARE");
    }

    // -- retain_children tests --

    #[test]
    fn retain_children_filters_by_predicate() {
        let mut s = nested_structure();
        // Keep only children with "Kick" in the name
        s.retain_children(&|child| child.name.contains("Kick"));

        assert_eq!(s.children.len(), 1);
        assert_eq!(s.children[0].name, "SongName_Kick");
    }

    #[test]
    fn retain_children_applies_recursively() {
        let mut root = Structure::<TestMeta>::new("root");
        let mut a = Structure::<TestMeta>::new("A");
        a.children.push(Structure::new("keep"));
        a.children.push(Structure::new("remove"));
        root.children.push(a);
        root.children.push(Structure::new("B"));

        // Remove nodes named "remove" at any depth
        root.retain_children(&|child| child.name != "remove");

        assert_eq!(root.children.len(), 2); // A and B both kept
        assert_eq!(root.children[0].children.len(), 1); // only "keep" remains inside A
        assert_eq!(root.children[0].children[0].name, "keep");
    }

    #[test]
    fn retain_children_on_leaf_node() {
        let mut s = Structure::<TestMeta>::new("leaf");
        s.items.push(make_item("item"));
        s.retain_children(&|_| false); // no-op, no children to remove
        assert_eq!(s.items.len(), 1); // items untouched
    }

    #[test]
    fn retain_children_remove_empty() {
        let mut root = nested_structure();
        // Add an empty child
        root.children.push(Structure::new("Empty"));

        assert_eq!(root.children.len(), 3);
        root.retain_children(&|child| !child.is_empty());
        assert_eq!(root.children.len(), 2); // "Empty" removed
    }

    // -- sort_children_by tests --

    #[test]
    fn sort_children_by_alphabetical() {
        let mut root = Structure::<TestMeta>::new("root");
        root.children.push(Structure::new("Zebra"));
        root.children.push(Structure::new("Apple"));
        root.children.push(Structure::new("Mango"));

        root.sort_children_by(&|a, b| a.name.cmp(&b.name));

        let names: Vec<&str> = root.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Apple", "Mango", "Zebra"]);
    }

    #[test]
    fn sort_children_by_applies_recursively() {
        let mut root = Structure::<TestMeta>::new("root");
        let mut parent = Structure::<TestMeta>::new("parent");
        parent.children.push(Structure::new("Z"));
        parent.children.push(Structure::new("A"));
        root.children.push(parent);

        root.sort_children_by(&|a, b| a.name.cmp(&b.name));

        // Nested children are also sorted
        let nested_names: Vec<&str> = root.children[0]
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(nested_names, vec!["A", "Z"]);
    }

    #[test]
    fn sort_children_by_item_count() {
        let mut root = Structure::<TestMeta>::new("root");
        let mut few = Structure::new("few");
        few.items.push(make_item("a"));
        let mut many = Structure::new("many");
        many.items.push(make_item("a"));
        many.items.push(make_item("b"));
        many.items.push(make_item("c"));
        root.children.push(few);
        root.children.push(many);

        // Sort by item count descending
        root.sort_children_by(&|a, b| b.total_items().cmp(&a.total_items()));

        assert_eq!(root.children[0].name, "many");
        assert_eq!(root.children[1].name, "few");
    }

    // -- walk_with_path tests --

    #[test]
    fn walk_with_path_tracks_hierarchy() {
        let s = nested_structure();
        let visited = std::cell::RefCell::new(Vec::new());
        s.walk_with_path(&|_node, path| {
            let owned: Vec<String> = path.iter().map(|s| s.to_string()).collect();
            visited.borrow_mut().push(owned);
        });

        let result = visited.into_inner();
        assert_eq!(
            result,
            vec![
                vec!["SongName_Drums"],
                vec!["SongName_Drums", "SongName_Kick"],
                vec!["SongName_Drums", "SongName_Snare"],
            ]
        );
    }

    #[test]
    fn walk_with_path_deeper_tree() {
        let mut root = Structure::<TestMeta>::new("root");
        let mut child = Structure::<TestMeta>::new("A");
        let grandchild = Structure::<TestMeta>::new("B");
        child.children.push(grandchild);
        root.children.push(child);

        let visited = std::cell::RefCell::new(Vec::new());
        root.walk_with_path(&|_node, path| {
            let owned: Vec<String> = path.iter().map(|s| s.to_string()).collect();
            visited.borrow_mut().push(owned);
        });

        let result = visited.into_inner();
        assert_eq!(
            result,
            vec![vec!["root"], vec!["root", "A"], vec!["root", "A", "B"],]
        );
    }

    #[test]
    fn walk_with_path_sibling_paths_are_independent() {
        let mut root = Structure::<TestMeta>::new("root");
        root.children.push(Structure::new("X"));
        root.children.push(Structure::new("Y"));

        let visited = std::cell::RefCell::new(Vec::new());
        root.walk_with_path(&|_node, path| {
            let owned: Vec<String> = path.iter().map(|s| s.to_string()).collect();
            visited.borrow_mut().push(owned);
        });

        let result = visited.into_inner();
        // X's path doesn't leak into Y's path
        assert_eq!(result[1], vec!["root", "X"]);
        assert_eq!(result[2], vec!["root", "Y"]);
    }

    // -- node_count tests --

    #[test]
    fn node_count_nested_structure() {
        let s = nested_structure();
        // root + kick + snare = 3
        assert_eq!(s.node_count(), 3);
    }

    #[test]
    fn node_count_single_node() {
        let s = Structure::<TestMeta>::new("leaf");
        assert_eq!(s.node_count(), 1);
    }

    #[test]
    fn node_count_deeper_tree() {
        let mut root = Structure::<TestMeta>::new("root");
        let mut child = Structure::<TestMeta>::new("child");
        child.children.push(Structure::new("grandchild"));
        root.children.push(child);
        root.children.push(Structure::new("sibling"));
        // root + child + grandchild + sibling = 4
        assert_eq!(root.node_count(), 4);
    }
}
