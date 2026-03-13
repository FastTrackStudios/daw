//! Item and Take module
//!
//! Items are media containers on tracks that hold one or more takes.
//! Takes are alternative recordings or sources within an item.

mod error;
mod event;
#[allow(clippy::module_inception)]
mod item;
mod service;
mod take;

pub use error::{ItemError, TakeError};
pub use event::{ItemEvent, TakeEvent};
pub use item::{FadeShape, Item, ItemRef};
pub use service::{
    ItemService, ItemServiceClient, ItemServiceDispatcher, TakeService, TakeServiceClient,
    TakeServiceDispatcher, item_service_service_descriptor, take_service_service_descriptor,
};
pub use take::{SourceType, Take, TakeRef};
