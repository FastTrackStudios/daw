//! Field value descriptors for metadata fields.
//!
//! This module allows each metadata field value (e.g., "Out", "In", "Hi Hat", "Ride")
//! to have its own patterns and negative patterns, making them act more like separate groups.
//!
//! # Purpose
//!
//! When extracting metadata values like `MultiMic`, simple pattern matching may not be
//! sufficient. Field value descriptors allow fine-grained control over:
//!
//! - Which patterns indicate a specific value
//! - Which patterns should exclude a value
//!
//! # Example
//!
//! ```ignore
//! use monarchy::FieldValueDescriptor;
//!
//! let descriptors = vec![
//!     FieldValueDescriptor::builder("In")
//!         .patterns(["in"])
//!         .exclude(["inside", "input"])
//!         .build(),
//!     FieldValueDescriptor::builder("Out")
//!         .patterns(["out"])
//!         .exclude(["outside", "output"])
//!         .build(),
//! ];
//!
//! // Use with a group
//! Group::builder("Kick")
//!     .field_value_descriptors(MyField::MultiMic, descriptors)
//!     .build()
//! ```

use facet::Facet;
use serde::{Deserialize, Serialize};

/// Helper trait to convert various input types into a `Vec<String>` for patterns.
///
/// This trait enables flexible APIs where pattern methods accept
/// single strings, arrays, or vectors.
pub trait IntoPatterns {
    /// Convert self into a `Vec<String>`.
    fn into_patterns(self) -> Vec<String>;
}

// Implementation for single &str
impl IntoPatterns for &str {
    fn into_patterns(self) -> Vec<String> {
        vec![self.to_string()]
    }
}

// Implementation for String
impl IntoPatterns for String {
    fn into_patterns(self) -> Vec<String> {
        vec![self]
    }
}

// Implementation for Vec<String>
impl IntoPatterns for Vec<String> {
    fn into_patterns(self) -> Vec<String> {
        self
    }
}

// Implementation for Vec<&str>
impl IntoPatterns for Vec<&str> {
    fn into_patterns(self) -> Vec<String> {
        self.into_iter().map(|s| s.to_string()).collect()
    }
}

// Implementation for arrays [&str; N]
impl<const N: usize> IntoPatterns for [&str; N] {
    fn into_patterns(self) -> Vec<String> {
        self.into_iter().map(|s| s.to_string()).collect()
    }
}

// Implementation for slices &[&str]
impl IntoPatterns for &[&str] {
    fn into_patterns(self) -> Vec<String> {
        self.iter().map(|s| (*s).to_string()).collect()
    }
}

/// Describes a specific value for a metadata field with its own patterns
///
/// This allows values like "Out", "Hi Hat", "Ride" to have their own matching rules
/// instead of just using simple string matching.
///
/// # Example
///
/// ```
/// use monarchy::FieldValueDescriptor;
///
/// let descriptor = FieldValueDescriptor::new("Out")
///     .patterns(vec!["out".to_string(), "ouut".to_string()])
///     .exclude(vec!["outside".to_string(), "outdoor".to_string()]);
///
/// assert!(descriptor.matches("kick out"));
/// assert!(!descriptor.matches("outside"));
/// ```
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Facet)]
pub struct FieldValueDescriptor {
    /// The value name (e.g., "Out", "Hi Hat", "Ride")
    pub value: String,

    /// Patterns that match this value (any of these will match)
    pub patterns: Vec<String>,

    /// Negative patterns to exclude (none of these should match)
    pub negative_patterns: Vec<String>,
}

impl FieldValueDescriptor {
    /// Create a new field value descriptor
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            patterns: Vec::new(),
            negative_patterns: Vec::new(),
        }
    }

    /// Add patterns to match this value
    pub fn patterns<P>(mut self, patterns: P) -> Self
    where
        P: Into<Vec<String>>,
    {
        self.patterns = patterns.into();
        self
    }

    /// Add negative patterns to exclude
    pub fn exclude<P>(mut self, patterns: P) -> Self
    where
        P: Into<Vec<String>>,
    {
        self.negative_patterns = patterns.into();
        self
    }

    /// Check if a string matches this value descriptor
    pub fn matches(&self, text: &str) -> bool {
        // Check negative patterns first
        for pattern in &self.negative_patterns {
            if crate::utils::contains_word(text, pattern) {
                return false;
            }
        }

        // If no patterns specified, match the value name directly as a word
        if self.patterns.is_empty() {
            return crate::utils::contains_word(text, &self.value);
        }

        // Check positive patterns
        for pattern in &self.patterns {
            if crate::utils::contains_word(text, pattern) {
                return true;
            }
        }

        false
    }
}

/// Builder for field value descriptors
#[derive(Clone, Debug, Facet)]
pub struct FieldValueDescriptorBuilder {
    descriptor: FieldValueDescriptor,
}

impl FieldValueDescriptorBuilder {
    /// Create a new builder
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            descriptor: FieldValueDescriptor::new(value),
        }
    }

    /// Add patterns to match this value
    pub fn patterns<P>(mut self, patterns: P) -> Self
    where
        P: IntoPatterns,
    {
        self.descriptor.patterns = patterns.into_patterns();
        self
    }

    /// Add negative patterns to exclude
    pub fn exclude<P>(mut self, patterns: P) -> Self
    where
        P: IntoPatterns,
    {
        self.descriptor.negative_patterns = patterns.into_patterns();
        self
    }

    /// Build the descriptor
    pub fn build(self) -> FieldValueDescriptor {
        self.descriptor
    }
}

impl FieldValueDescriptor {
    /// Start building a new field value descriptor
    pub fn builder(value: impl Into<String>) -> FieldValueDescriptorBuilder {
        FieldValueDescriptorBuilder::new(value)
    }
}
