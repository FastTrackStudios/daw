//! Core traits for monarchy
//!
//! This module contains the foundational traits used throughout monarchy:
//! - [`IntoInputs`] - Convert various types into sortable strings
//! - [`ToDisplayName`] - Generate display names from metadata
//! - [`Target`] - Containers that can hold sorted items

use crate::{Item, Metadata};

// region:    --- IntoInputs

/// Trait for converting various input types into sortable strings
///
/// This trait enables flexible input handling - you can pass a `Vec<String>`,
/// `Vec<&str>`, an iterator, or any other type that can produce strings.
///
/// # Example
/// ```ignore
/// // All of these work:
/// monarchy_sort(vec!["Kick In.wav", "Snare Top.wav"], config);
/// monarchy_sort(["Kick In.wav", "Snare Top.wav"], config);
/// monarchy_sort(items.iter().map(|i| i.name.clone()), config);
/// ```
pub trait IntoInputs {
    /// Convert this type into a vector of input strings
    fn into_inputs(self) -> Vec<String>;
}

/// Blanket implementation for any iterator that yields items convertible to String
impl<I, T> IntoInputs for I
where
    I: IntoIterator<Item = T>,
    T: Into<String>,
{
    fn into_inputs(self) -> Vec<String> {
        self.into_iter().map(|item| item.into()).collect()
    }
}

// endregion: --- IntoInputs

// region:    --- ToDisplayName

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
    /// * `prefixes` - Accumulated prefixes from matched groups (e.g., `["D", "GTR"]`)
    /// * `group_names` - Names of matched groups in hierarchy order (e.g., `["Drums", "Kick"]`)
    ///
    /// # Returns
    /// Full display name like `"D Kick In"` or `"GTR E Cody Crunch 2"`
    fn to_display_name(&self, prefixes: &[String], group_names: &[String]) -> String;
}

// endregion: --- ToDisplayName

// region:    --- Target

/// Trait for containers that can hold sorted items
///
/// This is used when you want to add items to an existing container
/// rather than creating a new structure.
pub trait Target<M: Metadata> {
    /// Get existing items from this container
    fn existing_items(&self) -> Vec<Item<M>>;
}

// endregion: --- Target
