//! Error type for the custom [`ExampleService`](crate::ExampleService).
//!
//! A `facet::Facet` enum so it travels over the vox wire, and
//! `thiserror::Error` for ergonomic `?` on the server. `#[repr(u8)]` keeps
//! the wire tag compact. Add variants here as the service grows.

#[derive(Debug, Clone, PartialEq, Eq, facet::Facet, thiserror::Error)]
#[repr(u8)]
pub enum ExampleServiceError {
    #[error("example not found")]
    NotFound,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("internal error: {0}")]
    Internal(String),
}
