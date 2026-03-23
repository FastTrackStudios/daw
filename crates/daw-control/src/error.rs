//! Error types for daw-control
//!
//! Provides error handling for DAW control operations, including RPC errors from vox.

use std::fmt;

/// Result type for daw-control operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error type for daw-control
#[derive(Debug)]
pub enum Error {
    /// Error from vox RPC layer
    Vox(vox::VoxError),

    /// VoxError with generic payload
    VoxGeneric(String),

    /// Project not found
    ProjectNotFound(String),

    /// No current project
    NoCurrentProject,

    /// Invalid operation
    InvalidOperation(String),

    /// Other errors
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Vox(e) => write!(f, "RPC error: {:#?}", e),
            Error::VoxGeneric(msg) => write!(f, "RPC error: {}", msg),
            Error::ProjectNotFound(guid) => write!(f, "Project not found: {}", guid),
            Error::NoCurrentProject => write!(f, "No current project available"),
            Error::InvalidOperation(msg) => write!(f, "Invalid operation: {}", msg),
            Error::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for Error {}

impl From<vox::VoxError> for Error {
    fn from(err: vox::VoxError) -> Self {
        Error::Vox(err)
    }
}

impl From<vox::VoxError<String>> for Error {
    fn from(err: vox::VoxError<String>) -> Self {
        Error::VoxGeneric(format!("{:?}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::ProjectNotFound("abc123".to_string());
        assert!(err.to_string().contains("Project not found"));
    }
}
