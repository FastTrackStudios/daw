//! Form components for the example feature, built on `architect::form`
//! (`use_field` + `validate`). Dumb: they own their field state and emit
//! the validated `(name, description)` pair; the caller decides what to do
//! with it (drive an optimistic mutation, call the client, stub in a test).

mod create;
mod edit;

pub use create::ExampleCreateForm;
pub use edit::ExampleEditForm;
