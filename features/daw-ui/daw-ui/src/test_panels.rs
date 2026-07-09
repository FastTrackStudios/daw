//! FTS UI test panels — Native (Blitz) renderer demos.
//!
//! Two panel definitions plus matching toggle actions:
//!
//! - **`FTS_UI_NATIVE`** — full `fts_ui::showcase::Showcase` rendered through
//!   Blitz. The "real" panel showing the design system in REAPER.
//! - **`FTS_DEMO_NATIVE`** — bare-minimum Dioxus demo (counter + buttons,
//!   no Tailwind, no theme). The renderer-perf baseline — anything slower
//!   in `UiTestPanel` is on `fts-ui`, not the renderer.
//!
//! Designed to live in this repo so any REAPER extension that hosts
//! `daw-reaper-dioxus` can mount the demo by calling `panel_defs()` +
//! `action_defs()`. Originally lived in `fts-extensions`; promoted here so
//! the demo is a first-class part of the daw API surface.
//!
//! ## Note on the Desktop (WebView) renderer
//!
//! Earlier versions of this module also exposed `FTS_UI_DESKTOP` /
//! `FTS_DEMO_DESKTOP` rendering through wry/WebKitGTK. After implementing
//! that path end-to-end (committed at 41f8d53 / 5a88a4d) we found that
//! Linux/Wayland + xwayland-satellite + REAPER's SWELL panels create a
//! cluster of issues — separate WebProcess X surfaces, GBM allocation
//! failures inside the bwrap FHS env, focus theft, `_NET_ACTIVE_WINDOW`
//! corruption — that no amount of GtkPlug / override-redirect / XEMBED
//! config can resolve while WebKitGTK is in the loop. WPE WebKit (#19)
//! would unblock it but requires a full fork of dioxus-desktop's
//! interpreter bridge. See the closing comments on issues #10, #11, #18,
//! #19 for the full rationale. For the foreseeable future, Native is the
//! canonical Linux desktop panel renderer; the Desktop variant is filed
//! away for revival if requirements change.

use daw_module::{ActionDef, DockPosition, PanelComponent, PanelDef, PanelRenderer};
use daw_reaper_dioxus::prelude::*;

const TAILWIND_CSS: &str = include_str!("../assets/tailwind.css");
const FTS_THEME_CSS: &str = include_str!("../assets/fts-theme.css");

const BLITZ_FIXES: &str = r#"
input, textarea, select, button { cursor: auto !important; }
input:disabled, textarea:disabled, button:disabled { cursor: not-allowed !important; }
:root { color-scheme: dark; }
"#;

pub const FTS_UI_NATIVE_PANEL_ID: &str = "FTS_UI_NATIVE";
pub const FTS_UI_NATIVE_ACTION_ID: &str = "fts-ui-native";

pub const FTS_DEMO_NATIVE_PANEL_ID: &str = "FTS_DEMO_NATIVE";
pub const FTS_DEMO_NATIVE_ACTION_ID: &str = "fts-demo-native";

#[component]
pub fn UiTestPanel() -> Element {
    rsx! {
        document::Style { {TAILWIND_CSS} }
        document::Style { {FTS_THEME_CSS} }
        document::Style { {BLITZ_FIXES} }

        fts_ui::showcase::Showcase {}
    }
}

/// Bare-minimum Dioxus demo — counter + buttons, no fts-ui, no
/// Tailwind. The renderer-perf baseline so we can isolate fts-ui costs
/// from Blitz costs when something feels slow.
#[component]
pub fn DemoPanel() -> Element {
    let mut count = use_signal(|| 0i32);
    rsx! {
        document::Style {
            "
            html, body {{ margin: 0; padding: 0; height: 100%; }}
            body {{
                font-family: system-ui, sans-serif;
                background: #0d1117;
                color: #e6edf3;
                display: flex;
                align-items: center;
                justify-content: center;
                flex-direction: column;
                gap: 16px;
            }}
            button {{
                background: #2a6df5;
                color: white;
                border: 0;
                border-radius: 8px;
                padding: 12px 24px;
                font-size: 18px;
                cursor: pointer;
            }}
            button:hover {{ background: #3b7df8; }}
            "
        }
        h1 { "Dioxus demo panel" }
        p { "count = {count}" }
        button {
            onclick: move |_| { count += 1; },
            "increment"
        }
        button {
            onclick: move |_| { count.set(0); },
            "reset"
        }
    }
}

/// Native-renderer panel defs: the Showcase + the bare counter.
pub fn panel_defs() -> [PanelDef; 2] {
    [
        PanelDef {
            id: FTS_UI_NATIVE_PANEL_ID,
            title: "FTS UI Native",
            component: PanelComponent::from_fn_ptr(UiTestPanel as fn() -> _ as *const ()),
            default_dock: DockPosition::Floating,
            renderer: PanelRenderer::Native,
            default_size: (900.0, 700.0),
            toggle_action: Some(FTS_UI_NATIVE_ACTION_ID),
        },
        PanelDef {
            id: FTS_DEMO_NATIVE_PANEL_ID,
            title: "FTS Demo Native",
            component: PanelComponent::from_fn_ptr(DemoPanel as fn() -> _ as *const ()),
            default_dock: DockPosition::Floating,
            renderer: PanelRenderer::Native,
            default_size: (480.0, 320.0),
            toggle_action: Some(FTS_DEMO_NATIVE_ACTION_ID),
        },
    ]
}

/// Action defs that toggle the corresponding panels. Handlers call
/// straight into the dock module so consumers don't need the `daw`
/// facade in scope.
pub fn action_defs() -> [ActionDef; 2] {
    [
        ActionDef::new(FTS_UI_NATIVE_ACTION_ID, "FTS: UI Native", || {
            daw_reaper_dioxus::dock::toggle_panel(FTS_UI_NATIVE_PANEL_ID);
        })
        .in_menu(),
        ActionDef::new(FTS_DEMO_NATIVE_ACTION_ID, "FTS: Demo Native", || {
            daw_reaper_dioxus::dock::toggle_panel(FTS_DEMO_NATIVE_PANEL_ID);
        })
        .in_menu(),
    ]
}
