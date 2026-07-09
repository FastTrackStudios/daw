//! Reaper stream source for [`daw_proto::event_bus::EventBus`].
//!
//! The cross-domain bus is an architect `#[subscribe]` stream served
//! from the central hub's `PubSub<DawEvent>` (see
//! [`crate::event_hub`]). Every `publish_*` on [`DawEventHub`] wraps
//! its event in [`DawEvent`] and publishes onto the bus hub alongside
//! its own domain channel, so this file is just the source wiring —
//! the per-subscriber `select!` forwarder of the old Tx-parameter
//! subscribe is gone. `BusFilter` moved client-side
//! (`daw_control::Events`); the wire carries everything.
//!
//! [`DawEventHub`]: crate::event_hub::DawEventHub

use daw_proto::event_bus::{DawEvent, EventBus, EventBusStreamSource};

// The base `EventBus` trait is empty after the `#[subscribe]` port —
// only the stream sibling carries surface.
impl EventBus for crate::Reaper {}

impl EventBusStreamSource for crate::Reaper {
    fn events_hub(&self) -> &architect::PubSub<DawEvent> {
        crate::event_hub::hub().bus_hub()
    }
}
