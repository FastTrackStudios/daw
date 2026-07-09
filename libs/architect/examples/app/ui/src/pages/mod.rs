//! Route pages — thin handlers that mount feature view-blocks and wire
//! their navigation events to the app's typed [`Route`](crate::Route).
//!
//! This is the layer where a real app combines *multiple* features' views
//! onto one page (a dashboard mounting `ExampleListView` next to a
//! `BillingView`, …); here there's just the example feature, so each page
//! mounts one view and maps its `on_*` events to a route push.

mod collab;
mod detail;
mod edit;
mod home;
mod inspect;
mod not_found;

pub use collab::Collab;
pub use detail::ExampleDetail;
pub use edit::EditExample;
pub use home::Home;
pub use inspect::InspectExample;
pub use not_found::NotFound;
