//! The universal async-phase chrome — the same `Spinner` / `StatusError`
//! every page reuses for the non-success arms of an `AtomResult` match —
//! plus the app-wide [`NotificationTray`].

use architect::{NoticeLevel, use_notifications};
use dioxus::prelude::*;

/// The waiting arm: a request is in flight and there's nothing to show yet.
/// (Named `Spinner` so it doesn't collide with the `AtomResult::Loading`
/// variant when pages glob-import the phases.)
#[component]
pub fn Spinner() -> Element {
    rsx! {
        p { class: "status", "Loading…" }
    }
}

/// The error / defect arm (string form).
#[component]
pub fn StatusError(message: String) -> Element {
    rsx! {
        p { class: "status error", "{message}" }
    }
}

/// The `Error { error, .. }` arm, taking the typed failure directly —
/// renders via `Display`, with the reconnecting hint for infrastructure
/// failures.
#[component]
pub fn FailurePanel<E: std::fmt::Display + Clone + PartialEq + 'static>(
    error: architect::ClientError<E>,
) -> Element {
    let hint = if error.is_retryable() {
        " — retrying may help"
    } else {
        ""
    };
    rsx! {
        p { class: "status error", "{error}{hint}" }
    }
}

/// The `Defect { defect, .. }` arm.
#[component]
pub fn DefectPanel(defect: String) -> Element {
    rsx! {
        p { class: "status error", "defect: {defect}" }
    }
}

/// App-wide notice strip, rendered once in the shell layout. Optimistic
/// mutations report their rollback failures to the `Notifications` queue
/// automatically — so a delete that navigated away before the server
/// rejected it still tells the user (and the row is already restored).
#[component]
pub fn NotificationTray() -> Element {
    let notices = use_notifications();
    let list = notices.list();
    if list.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "notice-tray",
            for n in list {
                div {
                    key: "{n.id}",
                    class: if n.level == NoticeLevel::Error { "notice notice--error" } else { "notice" },
                    span { class: "notice__message", "{n.message}" }
                    button {
                        class: "notice__dismiss",
                        aria_label: "dismiss",
                        onclick: move |_| notices.dismiss(n.id),
                        "×"
                    }
                }
            }
        }
    }
}
