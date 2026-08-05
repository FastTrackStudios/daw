//! Record control — restart-recording and monitor-mode toggles.
//!
//! Contract only; the implementation lives in `daw-actions`
//! (`daw_actions::record`).

pub mod record_control_service {
    use crate::DawError;

    #[architect::rpc]
    pub trait RecordControlService {
        /// Stop the current recording (DELETE all recorded media this
        /// pass) and immediately start a fresh recording pass. One
        /// undo block.
        async fn restart_recording(&self) -> Result<(), DawError>;

        /// Toggle record monitor on selected tracks between **on (1)**
        /// and **off (0)** — skips auto/tape.
        async fn toggle_monitor_on_off(&self) -> Result<(), DawError>;

        /// Toggle record monitor on selected tracks between
        /// **auto/tape (2)** and **off (0)**.
        async fn toggle_monitor_tape_off(&self) -> Result<(), DawError>;
    }
}

#[cfg(feature = "vox")]
pub use record_control_service::RecordControlServiceClient;
pub use record_control_service::{
    RecordControlService, RecordControlServiceDispatcher, Service as RecordControlServiceLayer,
    layer as record_control_service_layer, record_control_service_rpc_service_descriptor,
    record_control_service_service_descriptor, serve as serve_record_control_service,
};
