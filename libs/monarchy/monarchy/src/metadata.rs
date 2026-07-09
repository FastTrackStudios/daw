//! Metadata trait and field name trait for sortable items.
//!
//! This module defines the core [`Metadata`] trait that all metadata types must
//! implement, and the [`FieldName`] trait for stable field name lookups.
//!
//! # The Metadata Trait
//!
//! The `Metadata` trait defines how metadata is stored and accessed for each item.
//! It requires two associated types:
//!
//! - `Field`: An enum representing the available metadata fields
//! - `Value`: An enum representing the possible values for those fields
//!
//! # Using the Derive Macro
//!
//! The easiest way to implement `Metadata` is using the derive macro:
//!
//! ```ignore
//! use monarchy::Metadata;
//!
//! #[derive(Metadata, Default, Clone, Debug)]
//! struct TrackMetadata {
//!     performer: Option<String>,
//!     arrangement: Option<String>,
//!     multi_mic: Option<Vec<String>>,
//! }
//! ```
//!
//! The derive macro automatically generates:
//! - A `TrackMetadataField` enum with variants for each field
//! - A `TrackMetadataValue` enum with variants for each field's type
//! - Implementations of `get`, `set`, and `fields`
//! - Field name lookup implementations
//!
//! # Manual Implementation
//!
//! For custom requirements, you can implement `Metadata` manually:
//!
//! ```ignore
//! #[derive(Clone, Default)]
//! struct MyMetadata {
//!     group: Option<String>,
//!     tags: Option<Vec<String>>,
//! }
//!
//! #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
//! enum MyField { Group, Tags }
//!
//! #[derive(Clone, Debug)]
//! enum MyValue { Group(String), Tags(Vec<String>) }
//!
//! impl Metadata for MyMetadata {
//!     type Field = MyField;
//!     type Value = MyValue;
//!
//!     fn get(&self, field: &Self::Field) -> Option<Self::Value> {
//!         match field {
//!             MyField::Group => self.group.clone().map(MyValue::Group),
//!             MyField::Tags => self.tags.clone().map(MyValue::Tags),
//!         }
//!     }
//!
//!     fn set(&mut self, field: Self::Field, value: Self::Value) {
//!         match (field, value) {
//!             (MyField::Group, MyValue::Group(v)) => self.group = Some(v),
//!             (MyField::Tags, MyValue::Tags(v)) => self.tags = Some(v),
//!             _ => {}
//!         }
//!     }
//!
//!     fn fields() -> Vec<Self::Field> {
//!         vec![MyField::Group, MyField::Tags]
//!     }
//! }
//! ```

use facet::Facet;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Trait for getting a stable string name from a metadata field.
///
/// This trait is used for HashMap key lookups and field matching.
/// The derive macro automatically implements this for field enums.
///
/// # Example
///
/// ```ignore
/// impl FieldName for MyField {
///     fn name(&self) -> &'static str {
///         match self {
///             MyField::Group => "Group",
///             MyField::MultiMic => "MultiMic",
///         }
///     }
/// }
/// ```
pub trait FieldName {
    /// Returns the stable string name of this field.
    ///
    /// This should return a consistent name suitable for use as a HashMap key.
    fn name(&self) -> &'static str;
}

/// Core trait that defines metadata for sortable items.
///
/// This trait is the foundation of monarchy's metadata system. Every metadata
/// type used with monarchy must implement this trait.
///
/// # Associated Types
///
/// - `Field`: An enum type representing the available metadata fields
/// - `Value`: An enum type representing the possible values for those fields
///
/// # Required Methods
///
/// - `get`: Retrieve a value for a field
/// - `set`: Store a value for a field
/// - `fields`: Return all available fields
///
/// # Optional Methods
///
/// - `variant_fields`: Return fields marked as variants
/// - `create_string_value`: Create a Value from a String
/// - `create_vec_string_value`: Create a Value from a `Vec<String>`
///
/// # Derive Macro
///
/// The recommended way to implement this trait is using the derive macro:
///
/// ```ignore
/// #[derive(Metadata, Default, Clone, Debug)]
/// struct TrackMetadata {
///     performer: Option<String>,
///     multi_mic: Option<Vec<String>>,
/// }
/// ```
pub trait Metadata: Clone + Default + Send + Sync + 'static {
    /// The field type used as keys for this metadata
    type Field: Clone
        + Debug
        + PartialEq
        + Serialize
        + for<'de> Deserialize<'de>
        + for<'a> Facet<'a>
        + Send
        + Sync
        + 'static;

    /// The value type that can be stored in this metadata
    type Value: Clone + Debug + for<'a> Facet<'a> + Send + Sync + 'static;

    /// Get a value for a given field
    fn get(&self, field: &Self::Field) -> Option<Self::Value>;

    /// Set a value for a given field
    fn set(&mut self, field: Self::Field, value: Self::Value);

    /// Get all available fields for this metadata type
    fn fields() -> Vec<Self::Field>;

    /// Get fields marked as variants (for grouping items that are identical except for this field)
    /// Variants are kept together in the same structure but grouped by variant value
    /// for client-side handling (e.g., different lanes on the same track)
    fn variant_fields() -> Vec<Self::Field> {
        Vec::new() // Default: no variant fields
    }

    /// Create a Value from a string for a given field
    ///
    /// This is used by the parser to create Value enum variants from extracted strings.
    /// The default implementation returns None - specific Metadata implementations
    /// should override this if they need string-based value creation.
    fn create_string_value(_field: &Self::Field, _value: String) -> Option<Self::Value> {
        None
    }

    /// Create a Value from a vector of strings for a given field.
    ///
    /// This is used by the parser to create Value enum variants for `Vec<String>` fields.
    /// The default implementation returns None - specific Metadata implementations
    /// should override this if they need vector-based value creation.
    fn create_vec_string_value(_field: &Self::Field, _values: Vec<String>) -> Option<Self::Value> {
        None
    }
}
