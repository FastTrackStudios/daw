//! Error types for the monarchy crate.
//!
//! This module provides the [`MonarchyError`] enum and the [`Result`] type alias
//! for error handling throughout the crate.
//!
//! # Error Types
//!
//! - [`MonarchyError::Parse`] - Parsing errors during input processing
//! - [`MonarchyError::NoMatch`] - No group matched the input (with Reject fallback)
//! - [`MonarchyError::InvalidField`] - Invalid metadata field configuration
//! - [`MonarchyError::Serialization`] - JSON serialization/deserialization errors
//! - [`MonarchyError::Toml`] - TOML configuration parsing errors
//!
//! # Example
//!
//! ```ignore
//! use monarchy::{monarchy_sort, MonarchyError, FallbackStrategy};
//!
//! let config = Config::builder()
//!     .fallback(FallbackStrategy::Reject)
//!     .build();
//!
//! match monarchy_sort(inputs, config) {
//!     Ok(structure) => println!("Sorted {} items", structure.total_items()),
//!     Err(MonarchyError::NoMatch(input)) => println!("Unknown: {}", input),
//!     Err(e) => eprintln!("Error: {}", e),
//! }
//! ```

use facet::Facet;
use thiserror::Error;

/// Error types for monarchy operations.
///
/// All fallible operations in monarchy return `Result<T, MonarchyError>`.
#[repr(u8)]
#[derive(Error, Debug, Facet)]
pub enum MonarchyError {
    /// A parsing error occurred while processing an input string.
    #[error("Parse error: {0}")]
    Parse(String),

    /// No group matched the input string.
    ///
    /// This error only occurs when the fallback strategy is
    /// [`FallbackStrategy::Reject`](crate::FallbackStrategy::Reject).
    #[error("No matching group found for: {0}")]
    NoMatch(String),

    /// An invalid metadata field was encountered.
    #[error("Invalid metadata field: {0}")]
    InvalidField(String),

    /// A JSON serialization or deserialization error occurred.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// A TOML parsing error occurred (for configuration files).
    #[error("TOML error: {0}")]
    Toml(String),
}

/// Result type alias using [`MonarchyError`].
pub type Result<T> = std::result::Result<T, MonarchyError>;

impl From<serde_json::Error> for MonarchyError {
    fn from(err: serde_json::Error) -> Self {
        MonarchyError::Serialization(err.to_string())
    }
}

impl From<toml::de::Error> for MonarchyError {
    fn from(err: toml::de::Error) -> Self {
        MonarchyError::Toml(err.to_string())
    }
}
