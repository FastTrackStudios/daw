//! Field types and traits for metadata fields.
//!
//! This module provides infrastructure for treating metadata fields as
//! [`Group`]-like entities, enabling rich pattern matching capabilities
//! for each field.
//!
//! # Purpose
//!
//! When a group defines metadata fields (like `MultiMic` or `Performer`),
//! this module allows those fields to have their own:
//!
//! - Patterns for matching
//! - Negative patterns for exclusion
//! - Nested sub-values
//! - Grouping strategies
//!
//! # Example
//!
//! ```ignore
//! use monarchy::{Group, MetadataField};
//!
//! // Using metadata_field builder method
//! let group = Group::builder("Kick")
//!     .patterns(["kick"])
//!     .metadata_field(
//!         MyField::MultiMic,
//!         Group::builder("MultiMic")
//!             .patterns(["in", "out", "sub"])
//!             .build()
//!     )
//!     .build();
//! ```

use crate::{Group, Metadata};
use facet::Facet;

/// Trait for types that can be converted into a metadata field configuration.
///
/// This allows flexible APIs where methods can accept either:
/// - A [`Group`] directly
/// - A [`MetadataField`] wrapper
/// - Any other type implementing this trait
pub trait IntoField<M: Metadata> {
    /// Convert this type into a [`Group`] representing a metadata field.
    fn into_field(self) -> Group<M>;
}

/// A metadata field represented as a [`Group`].
///
/// This wrapper associates a metadata field enum variant with a group
/// configuration, allowing the field to have its own patterns, negative
/// patterns, and other group-like properties.
///
/// # Example
///
/// ```ignore
/// let multi_mic_field = MetadataField::from_group(
///     MyField::MultiMic,
///     Group::builder("MultiMic")
///         .patterns(["in", "out", "sub"])
///         .build()
/// );
/// ```
#[derive(Clone, Debug, Facet)]
pub struct MetadataField<M: Metadata> {
    /// The field enum variant this represents.
    pub field: M::Field,

    /// The group configuration for this field.
    pub group: Group<M>,
}

impl<M: Metadata> MetadataField<M> {
    /// Create a new metadata field with default configuration.
    ///
    /// The group is created with the field name and the field itself
    /// registered as a metadata field.
    pub fn new(field: M::Field) -> Self {
        Self {
            field: field.clone(),
            group: Group::builder(format!("{:?}", field)).field(field).build(),
        }
    }

    /// Create a metadata field from an existing [`Group`] configuration.
    ///
    /// This allows full customization of patterns, negative patterns,
    /// and other group properties for the field.
    ///
    /// # Example
    ///
    /// ```ignore
    /// MetadataField::from_group(
    ///     MyField::MultiMic,
    ///     Group::builder("MultiMic")
    ///         .patterns(["in", "out"])
    ///         .exclude(["inside"])
    ///         .build()
    /// )
    /// ```
    pub fn from_group(field: M::Field, group: Group<M>) -> Self {
        Self { field, group }
    }
}

impl<M: Metadata> IntoField<M> for MetadataField<M> {
    fn into_field(self) -> Group<M> {
        self.group
    }
}

impl<M: Metadata> IntoField<M> for Group<M> {
    fn into_field(self) -> Group<M> {
        self
    }
}
