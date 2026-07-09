//! Test utilities for easier structure verification.
//!
//! This module provides assertion helpers for testing monarchy structures.
//! It uses a fluent builder pattern for readable, chainable assertions.
//!
//! # Basic Usage
//!
//! ```ignore
//! use monarchy::StructureAssertions;
//!
//! let structure = monarchy_sort(inputs, config)?;
//!
//! structure.assert()
//!     .has_total_items(5)
//!     .has_groups(2)
//!     .group("Drums")
//!         .has_items(3)
//!         .contains("Kick In.wav")
//!         .done()
//!     .group("Guitars")
//!         .has_items(2)
//!         .done();
//! ```
//!
//! # Nested Groups
//!
//! ```ignore
//! structure.assert()
//!     .group("Drums")
//!         .group("Kick")
//!             .contains_exactly(&["Kick In.wav", "Kick Out.wav"])
//!             .done()
//!         .group("Snare")
//!             .has_items(1)
//!             .done()
//!         .done();
//! ```
//!
//! # Macros
//!
//! For more complex assertions, use the `assert_structure!` macro:
//!
//! ```ignore
//! assert_structure!(structure, {
//!     total_items: 5,
//!     groups: [
//!         { name: "Drums", items: ["Kick.wav", "Snare.wav"] },
//!         { name: "Guitars", items: ["Guitar.wav"] }
//!     ]
//! });
//! ```

use crate::{Metadata, Structure};
use facet::Facet;

/// Assertion helper for verifying [`Structure`] contents in tests.
///
/// Use [`StructureAssertions::assert`] to create an assertion helper,
/// then chain assertion methods to verify the structure.
///
/// # Example
///
/// ```ignore
/// structure.assert()
///     .has_total_items(5)
///     .group("Drums")
///         .has_items(3)
///         .done();
/// ```
#[derive(Clone, Debug, Facet)]
pub struct StructureAssertion<'a, M: Metadata> {
    structure: &'a Structure<M>,
    path: Vec<String>,
}

impl<'a, M: Metadata> StructureAssertion<'a, M> {
    /// Create a new assertion helper for a structure
    pub fn new(structure: &'a Structure<M>) -> Self {
        Self {
            structure,
            path: vec![],
        }
    }

    /// Assert the total number of items
    pub fn has_total_items(self, count: usize) -> Self {
        assert_eq!(
            self.structure.total_items(),
            count,
            "Expected {} total items, found {}",
            count,
            self.structure.total_items()
        );
        self
    }

    /// Assert the number of root-level items
    pub fn has_root_items(self, count: usize) -> Self {
        assert_eq!(
            self.structure.items.len(),
            count,
            "Expected {} root items, found {}",
            count,
            self.structure.items.len()
        );
        self
    }

    /// Assert the number of child groups
    pub fn has_groups(self, count: usize) -> Self {
        assert_eq!(
            self.structure.children.len(),
            count,
            "Expected {} groups, found {}",
            count,
            self.structure.children.len()
        );
        self
    }

    /// Assert that a specific group exists and enter it for further assertions
    pub fn group(mut self, name: &str) -> GroupAssertion<'a, M> {
        self.path.push(name.to_string());
        let group = self
            .structure
            .find_child(name)
            .unwrap_or_else(|| panic!("Group '{}' not found at path {:?}", name, self.path));

        GroupAssertion {
            group,
            parent: self,
        }
    }

    /// Assert that the structure has a specific depth
    pub fn has_depth(self, depth: usize) -> Self {
        assert_eq!(
            self.structure.depth(),
            depth,
            "Expected depth {}, found {}",
            depth,
            self.structure.depth()
        );
        self
    }

    /// Assert that root contains specific items
    pub fn has_root_item(self, original: &str) -> Self {
        assert!(
            self.structure.items.iter().any(|i| i.original == original),
            "Root item '{}' not found",
            original
        );
        self
    }

    /// Assert that root does NOT contain specific items
    pub fn not_has_root_item(self, original: &str) -> Self {
        assert!(
            !self.structure.items.iter().any(|i| i.original == original),
            "Unexpected root item '{}' found",
            original
        );
        self
    }
}

/// Assertion helper for a specific group
#[derive(Clone, Debug, Facet)]
pub struct GroupAssertion<'a, M: Metadata> {
    group: &'a Structure<M>,
    parent: StructureAssertion<'a, M>,
}

impl<'a, M: Metadata> GroupAssertion<'a, M> {
    /// Assert the number of items in this group
    pub fn has_items(self, count: usize) -> Self {
        assert_eq!(
            self.group.items.len(),
            count,
            "Group '{}' expected {} items, found {}",
            self.group.name,
            count,
            self.group.items.len()
        );
        self
    }

    /// Assert that the group contains a specific item
    pub fn contains(self, original: &str) -> Self {
        assert!(
            self.group.items.iter().any(|i| i.original == original),
            "Group '{}' does not contain item '{}'",
            self.group.name,
            original
        );
        self
    }

    /// Assert that the group does NOT contain a specific item
    pub fn not_contains(self, original: &str) -> Self {
        assert!(
            !self.group.items.iter().any(|i| i.original == original),
            "Group '{}' unexpectedly contains item '{}'",
            self.group.name,
            original
        );
        self
    }

    /// Assert that the group contains all of these items
    pub fn contains_all(self, items: &[&str]) -> Self {
        for item in items {
            assert!(
                self.group.items.iter().any(|i| i.original == *item),
                "Group '{}' does not contain item '{}'",
                self.group.name,
                item
            );
        }
        self
    }

    /// Assert that the group contains exactly these items (order doesn't matter)
    pub fn contains_exactly(self, items: &[&str]) -> Self {
        assert_eq!(
            self.group.items.len(),
            items.len(),
            "Group '{}' has {} items but expected {}",
            self.group.name,
            self.group.items.len(),
            items.len()
        );

        for item in items {
            assert!(
                self.group.items.iter().any(|i| i.original == *item),
                "Group '{}' does not contain expected item '{}'",
                self.group.name,
                item
            );
        }
        self
    }

    /// Assert the number of nested groups
    pub fn has_groups(self, count: usize) -> Self {
        assert_eq!(
            self.group.children.len(),
            count,
            "Group '{}' expected {} nested groups, found {}",
            self.group.name,
            count,
            self.group.children.len()
        );
        self
    }

    /// Enter a nested group for further assertions
    pub fn group(self, name: &str) -> GroupAssertion<'a, M> {
        let nested_group = self.group.find_child(name).unwrap_or_else(|| {
            panic!(
                "Nested group '{}' not found in group '{}'",
                name, self.group.name
            )
        });

        GroupAssertion {
            group: nested_group,
            parent: self.parent,
        }
    }

    /// Return to the parent structure assertion
    pub fn done(self) -> StructureAssertion<'a, M> {
        self.parent
    }
}

/// Extension trait to add assertion methods to Structure
pub trait StructureAssertions<M: Metadata> {
    /// Create an assertion helper for this structure
    fn assert(&self) -> StructureAssertion<'_, M>;
}

impl<M: Metadata> StructureAssertions<M> for Structure<M> {
    fn assert(&self) -> StructureAssertion<'_, M> {
        StructureAssertion::new(self)
    }
}

/// Macro for declarative structure assertions
#[macro_export]
macro_rules! assert_structure {
    ($structure:expr, {
        total_items: $total:expr,
        groups: [
            $({
                name: $group_name:expr,
                items: [$($item:expr),* $(,)?]
                $(, groups: [$($nested_group:tt)*])?
            }),* $(,)?
        ]
        $(, root_items: [$($root_item:expr),* $(,)?])?
    }) => {{
        let structure = $structure;
        let mut assertion = structure.assert()
            .has_total_items($total)
            .has_groups(count_groups!($($group_name),*));

        // Check root items if specified
        $(
            assertion = assertion.has_root_items(count_items!($($root_item),*));
            $(
                assertion = assertion.has_root_item($root_item);
            )*
        )?

        // Check each group
        $(
            let group_items = vec![$($item),*];
            assertion = assertion
                .group($group_name)
                .contains_exactly(&group_items)
                .done();
        )*

        assertion
    }};
}

// Helper macros for counting
#[macro_export]
macro_rules! count_groups {
    ($($group:expr),*) => {
        {
            let mut count = 0;
            $(
                let _ = $group;
                count += 1;
            )*
            count
        }
    };
}

#[macro_export]
macro_rules! count_items {
    ($($item:expr),*) => {
        {
            let mut count = 0;
            $(
                let _ = $item;
                count += 1;
            )*
            count
        }
    };
}
