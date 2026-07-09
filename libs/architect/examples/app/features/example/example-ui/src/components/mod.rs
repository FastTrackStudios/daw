//! Dumb, presentational components for the example feature — props in,
//! events out, no store/client/socket. They're composed by the wired
//! `views` and reusable on their own (the SSR component
//! tests render these directly).

mod example_card;
mod example_list;
mod example_row;
pub mod forms;
mod search_bar;

pub use example_card::ExampleCard;
pub use example_list::ExampleList;
pub use example_row::ExampleRow;
pub use forms::{ExampleCreateForm, ExampleEditForm};
pub use search_bar::SearchBar;
