//! Standalone dock panel wrappers for DAW components.
//!
//! Each function is a thin wrapper that initializes the FX browser hook
//! (if needed) and renders the underlying component.

use crate::prelude::*;

/// FX parameter browser dock panel — "no-GUI DAW" style parameter list.
///
/// Shows real DAW FX parameters with live bidirectional updates.
/// Reads directly from the DAW connection via global signals.
#[component]
pub fn FxBrowserDockPanel() -> Element {
    crate::hooks::fx_browser::use_fx_browser_subscription();

    rsx! {
        crate::components::fx_parameter_browser::FxParameterBrowser {}
    }
}
