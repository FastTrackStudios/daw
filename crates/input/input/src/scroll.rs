//! Scroll pattern matching and per-mode binding tables.

use crate::command::ActionId;
use crate::context::{ActionContext, WhenExpr};
use crate::event::ScrollEvent;
use crate::key::Modifiers;

/// Scroll axis selector for pattern matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    Any,
    Horizontal,
    Vertical,
}

/// A scroll gesture pattern used for binding lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollPattern {
    pub axis: ScrollAxis,
    pub modifiers: Modifiers,
}

impl ScrollPattern {
    pub fn new(axis: ScrollAxis, modifiers: Modifiers) -> Self {
        Self { axis, modifiers }
    }

    pub fn matches(&self, event: &ScrollEvent) -> bool {
        if self.modifiers != event.modifiers {
            return false;
        }

        match self.axis {
            ScrollAxis::Any => true,
            ScrollAxis::Horizontal => event.delta_x.abs() > event.delta_y.abs(),
            ScrollAxis::Vertical => event.delta_y.abs() >= event.delta_x.abs(),
        }
    }
}

/// Ordered set of scroll bindings for a mode.
///
/// First matching entry wins.
#[derive(Debug, Clone, Default)]
pub struct ScrollBindingTable {
    bindings: Vec<(ScrollPattern, WhenExpr, ActionId)>,
}

impl ScrollBindingTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, pattern: ScrollPattern, when: WhenExpr, action: ActionId) {
        self.bindings.push((pattern, when, action));
    }

    /// Read-only access to the raw binding entries.
    pub fn bindings(&self) -> &[(ScrollPattern, WhenExpr, ActionId)] {
        &self.bindings
    }

    pub fn match_event(&self, event: &ScrollEvent, ctx: &ActionContext) -> Option<ActionId> {
        for (pattern, when, action) in &self.bindings {
            if pattern.matches(event) && when.evaluate(ctx) {
                return Some(action.clone());
            }
        }
        None
    }
}
