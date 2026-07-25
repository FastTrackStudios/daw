//! The chrome's rendered parts: the frame, the two rails, the bar.

use dioxus::prelude::*;

use crate::icon::{Glyph, Icon};
use crate::{RailItem, StatusItem, use_chrome};

/// Chrome metrics, in one place — the rails and the bar are the app's only
/// fixed furniture, so their sizes are a contract, not a per-view choice.
const RAIL_W: u32 = 48;
const BAR_H: u32 = 40;
const HIT: u32 = 30;

const LINE: &str = "#1c1c21";
const DIM: &str = "#52525b";
const TEXT: &str = "#e4e4e7";
const ACCENT: &str = "#38bdf8";

/// The app frame: bar across the top, rails down the sides, view in the
/// middle. The bar spans the rails (it is also the window's title bar on
/// desktop, where the native decorations are off).
#[component]
pub fn AppFrame(
    /// The left rail — usually [`IconRail`].
    rail: Element,
    /// The one bar — usually [`TopBar`].
    top: Element,
    /// App-level panel flyouts ([`PanelHost`]s) — they sit between the view
    /// and the rail. A view's own panels go inside its layout instead, so they
    /// can share its scroll container.
    #[props(default)]
    panel: Option<Element>,
    /// The right rail — usually [`PanelRail`]. Empty renders nothing.
    #[props(default)]
    right: Option<Element>,
    /// Optional status line pinned to the base (the perform strip, transport).
    #[props(default)]
    footer: Option<Element>,
    children: Element,
) -> Element {
    rsx! {
        div {
            style: "display: flex; flex-direction: column; height: 100vh; min-height: 0; \
                    background: #0a0a0a; color: #e4e4e7; font-family: sans-serif;",
            {top}
            div { style: "display: flex; flex: 1; min-height: 0;",
                {rail}
                div { style: "display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0;",
                    {children}
                }
                if let Some(panel) = panel { {panel} }
                if let Some(right) = right { {right} }
            }
            if let Some(footer) = footer { {footer} }
        }
    }
}

/// The left rail: workspaces, then the level beneath them (the rigs, under
/// Signal), then a foot cluster the app fills (settings, account).
#[component]
pub fn IconRail(
    items: Vec<RailItem>,
    /// Rendered under a divider — the sub-level of the active item.
    #[props(default)]
    sub: Vec<RailItem>,
    /// Pinned to the bottom.
    #[props(default)]
    foot: Option<Element>,
) -> Element {
    rsx! {
        div {
            style: "display: flex; flex-direction: column; align-items: center; gap: 2px; \
                    width: {RAIL_W}px; flex-shrink: 0; padding: 8px 0; \
                    border-right: 1px solid {LINE}; background: #0b0b0e;",
            for item in items.iter() {
                RailButton { key: "{item.id}", item: item.clone() }
            }
            if !sub.is_empty() {
                div { style: "width: 20px; height: 1px; margin: 6px 0; background: {LINE};" }
                for item in sub.iter() {
                    RailButton { key: "sub-{item.id}", item: item.clone() }
                }
            }
            div { style: "flex: 1;" }
            if let Some(foot) = foot {
                div { style: "display: flex; flex-direction: column; align-items: center; gap: 2px;",
                    {foot}
                }
            }
        }
    }
}

#[component]
fn RailButton(item: RailItem) -> Element {
    let on_select = item.on_select;
    rsx! {
        button {
            style: format!(
                "appearance: none; display: flex; align-items: center; justify-content: center; \
                 width: {HIT}px; height: {HIT}px; border: none; border-radius: 9px; cursor: pointer; \
                 background: {}; color: {};",
                if item.active { "#101821" } else { "transparent" },
                if item.active { ACCENT } else { DIM },
            ),
            title: "{item.label}",
            onclick: move |_| on_select.call(()),
            Glyph { icon: item.icon, size: 17 }
        }
    }
}

/// The one bar: crumb trail · page tabs · drag surface · status · app cluster.
#[component]
pub fn TopBar(
    /// Far left, before the trail — the wordmark, a rail toggle.
    #[props(default)]
    leading: Option<Element>,
    /// Far right, after the status readouts — settings, window controls.
    #[props(default)]
    trailing: Option<Element>,
    /// Dragging the bar's empty space moves the window (frameless desktop).
    #[props(default)]
    on_drag: Option<EventHandler<()>>,
    /// Double-click the same space.
    #[props(default)]
    on_expand: Option<EventHandler<()>>,
) -> Element {
    let chrome = use_chrome();
    let crumbs = chrome.trail();
    let tabs = chrome.active_tabs();
    let status = chrome.active_status();
    // Which crumb's sibling menu is open, by index.
    let mut menu = use_signal(|| None::<usize>);

    rsx! {
        div {
            style: "position: relative; display: flex; align-items: center; gap: 12px; \
                    height: {BAR_H}px; flex-shrink: 0; padding: 0 8px 0 10px; \
                    border-bottom: 1px solid {LINE}; background: #0b0b0e; user-select: none;",
            if let Some(leading) = leading { {leading} }

            // ── the trail: where you are, and the way back ──────────────
            div { style: "display: flex; align-items: center; gap: 6px; min-width: 0;",
                for (i, crumb) in crumbs.iter().enumerate() {
                    {
                        let last = i + 1 == crumbs.len();
                        let has_menu = !crumb.menu.is_empty();
                        let on_click = crumb.on_click;
                        let items = crumb.menu.clone();
                        let label = crumb.label.clone();
                        rsx! {
                            // Separator lives inside the crumb's own box: the
                            // key has to sit on the first node of the block.
                            div { key: "crumb-{i}", style: "position: relative; display: flex; align-items: center; gap: 6px;",
                                if i > 0 {
                                    span { style: "color: #3f3f46; font-size: 11px;", "▸" }
                                }
                                button {
                                    style: format!(
                                        "appearance: none; border: none; background: transparent; \
                                         padding: 4px 6px; border-radius: 6px; white-space: nowrap; \
                                         font-size: {}; font-weight: {}; color: {}; cursor: {};",
                                        if last { "13px" } else { "12px" },
                                        if last { "700" } else { "500" },
                                        if last { TEXT } else { "#a1a1aa" },
                                        if on_click.is_some() || has_menu { "pointer" } else { "default" },
                                    ),
                                    onclick: move |_| {
                                        if has_menu {
                                            let open = menu() == Some(i);
                                            menu.set(if open { None } else { Some(i) });
                                        } else if let Some(cb) = on_click {
                                            cb.call(());
                                        }
                                    },
                                    "{label}"
                                    if has_menu {
                                        span { style: "margin-left: 5px; font-size: 8px; color: {DIM};", "▼" }
                                    }
                                }
                                if menu() == Some(i) {
                                    div {
                                        style: "position: absolute; top: 100%; left: 0; z-index: 200; \
                                                margin-top: 4px; min-width: 180px; padding: 4px; \
                                                display: flex; flex-direction: column; gap: 1px; \
                                                border: 1px solid #2b2b31; border-radius: 10px; \
                                                background: #0d0d10; box-shadow: 0 12px 32px #000c;",
                                        for (n, (name, current, pick)) in items.iter().enumerate() {
                                            button {
                                                key: "{n}",
                                                style: format!(
                                                    "appearance: none; text-align: left; border: none; \
                                                     border-radius: 6px; padding: 6px 9px; font-size: 11px; \
                                                     cursor: pointer; background: {}; color: {};",
                                                    if *current { "#101821" } else { "transparent" },
                                                    if *current { ACCENT } else { "#d4d4d8" },
                                                ),
                                                onclick: {
                                                    let pick = *pick;
                                                    move |_| {
                                                        menu.set(None);
                                                        pick.call(());
                                                    }
                                                },
                                                "{name}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ── the current view's pages ────────────────────────────────
            if !tabs.is_empty() {
                div {
                    style: "display: flex; gap: 4px; padding: 3px; border-radius: 8px; \
                            border: 1px solid {LINE}; background: #08080a;",
                    for tab in tabs.iter() {
                        {
                            let on_select = tab.on_select;
                            rsx! {
                                button {
                                    key: "{tab.id}",
                                    style: format!(
                                        "appearance: none; border: none; border-radius: 6px; cursor: pointer; \
                                         padding: 4px 12px; font-size: 11px; font-weight: 600; \
                                         background: {}; color: {};",
                                        if tab.active { "#101821" } else { "transparent" },
                                        if tab.active { ACCENT } else { DIM },
                                    ),
                                    onclick: move |_| on_select.call(()),
                                    "{tab.label}"
                                }
                            }
                        }
                    }
                }
            }

            // Drag surface — the bar's slack is the window handle.
            div {
                style: "flex: 1; align-self: stretch; min-width: 12px;",
                onmousedown: move |_| { if let Some(cb) = &on_drag { cb.call(()) } },
                ondoubleclick: move |_| { if let Some(cb) = &on_expand { cb.call(()) } },
            }

            // ── the current view's readouts ─────────────────────────────
            if !status.is_empty() {
                div { style: "display: flex; align-items: center; gap: 10px;",
                    for (i, item) in status.iter().enumerate() {
                        StatusView { key: "{i}", item: item.clone() }
                    }
                }
            }
            if let Some(trailing) = trailing { {trailing} }
        }
    }
}

#[component]
fn StatusView(item: StatusItem) -> Element {
    match item {
        StatusItem::Pill { text, fg, bg } => rsx! {
            span {
                style: "padding: 3px 10px; border-radius: 999px; font-size: 11px; font-weight: 700; \
                        letter-spacing: 0.04em; white-space: nowrap; background: {bg}; color: {fg};",
                "{text}"
            }
        },
        StatusItem::Text { text } => rsx! {
            span { style: "font-size: 10px; color: {DIM}; white-space: nowrap;", "{text}" }
        },
        StatusItem::Dot { on, color } => rsx! {
            span {
                style: format!(
                    "width: 7px; height: 7px; border-radius: 999px; background: {}; box-shadow: 0 0 6px {};",
                    if on { color.clone() } else { "#3f3f46".to_string() },
                    if on { format!("{color}88") } else { "transparent".to_string() },
                ),
            }
        },
        StatusItem::Meter { level, color, width_px } => {
            let pct = (level.clamp(0.0, 1.0) * 100.0) as u32;
            rsx! {
                div {
                    style: "width: {width_px}px; height: 8px; border-radius: 3px; \
                            background: #18181b; overflow: hidden;",
                    div { style: "height: 100%; width: {pct}%; background: {color};" }
                }
            }
        }
    }
}

/// The right rail: one button per panel the current view offers. The panel's
/// *content* belongs to that view — see [`PanelHost`].
#[component]
pub fn PanelRail() -> Element {
    let chrome = use_chrome();
    let panels = chrome.all_panels();
    if panels.is_empty() {
        return rsx! {};
    }
    rsx! {
        div {
            style: "display: flex; flex-direction: column; align-items: center; gap: 2px; \
                    width: {RAIL_W}px; flex-shrink: 0; padding: 8px 0; \
                    border-left: 1px solid {LINE}; background: #0b0b0e;",
            for panel in panels.iter() {
                {
                    let open = chrome.panel_open(&panel.id);
                    let id = panel.id.clone();
                    rsx! {
                        button {
                            key: "{panel.id}",
                            style: format!(
                                "appearance: none; display: flex; align-items: center; justify-content: center; \
                                 width: {HIT}px; height: {HIT}px; border: none; border-radius: 9px; \
                                 cursor: pointer; background: {}; color: {};",
                                if open { "#101821" } else { "transparent" },
                                if open { ACCENT } else { DIM },
                            ),
                            title: "{panel.title}",
                            onclick: move |_| chrome.toggle_panel(&id),
                            Glyph { icon: panel.icon, size: 17 }
                        }
                    }
                }
            }
        }
    }
}

/// The flyout for a panel, rendered by the view that owns it — put it at the
/// right edge of the view's own row and it lands flush against the rail.
/// Renders nothing unless `id` is the open panel.
#[component]
pub fn PanelHost(id: String, children: Element) -> Element {
    let chrome = use_chrome();
    if !chrome.panel_open(&id) {
        return rsx! {};
    }
    let spec = chrome.all_panels().into_iter().find(|p| p.id == id);
    let Some(spec) = spec else { return rsx! {} };
    rsx! {
        div {
            style: "display: flex; flex-direction: column; min-height: 0; flex-shrink: 0; \
                    width: {spec.width_px}px; border-left: 1px solid {LINE}; background: #0a0a0d;",
            div {
                style: "display: flex; align-items: center; gap: 8px; padding: 10px 12px; \
                        border-bottom: 1px solid {LINE};",
                span {
                    style: "flex: 1; font-size: 10px; font-weight: 700; letter-spacing: 0.1em; \
                            text-transform: uppercase; color: #a1a1aa;",
                    "{spec.title}"
                }
                button {
                    style: "appearance: none; border: none; background: transparent; color: {DIM}; \
                            cursor: pointer; padding: 0; line-height: 0;",
                    title: "Close",
                    onclick: move |_| chrome.close_panel(),
                    Glyph { icon: Icon::Close, size: 14 }
                }
            }
            div { style: "flex: 1; min-height: 0; overflow: auto;", {children} }
        }
    }
}

/// A square bar button — the app cluster's settings gear and window controls.
#[component]
pub fn WindowButton(
    icon: Icon,
    title: String,
    #[props(default = false)] active: bool,
    /// Close gets a red hover; everything else stays quiet.
    #[props(default = false)] danger: bool,
    on_click: EventHandler<()>,
) -> Element {
    rsx! {
        button {
            style: format!(
                "appearance: none; display: flex; align-items: center; justify-content: center; \
                 width: 28px; height: 28px; border: none; border-radius: 7px; cursor: pointer; \
                 background: {}; color: {};",
                if active { "#101821" } else { "transparent" },
                if active {
                    ACCENT
                } else if danger {
                    "#a1a1aa"
                } else {
                    DIM
                },
            ),
            title: "{title}",
            onclick: move |_| on_click.call(()),
            Glyph { icon, size: 15 }
        }
    }
}
