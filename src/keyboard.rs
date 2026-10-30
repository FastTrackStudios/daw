//! Who has the keyboard: the one switch every global hotkey checks.
//!
//! Typing a name into a box must not also arrange the panels (the dock's
//! hold-W), start the transport (Space) or run whatever a letter is bound
//! to. Anything that takes text — a field with focus, a rename, the chart
//! editor, the command palette — says so here, and every global hotkey
//! asks [`typing`] first and stands aside while it is true.
//!
//! Two ways to say so, both counted, so any number of holders compose:
//!
//! - a [`Claim`], held while a field has focus and dropped when it loses
//!   it (or when the field goes away: dropping releases, so a field that
//!   unmounts mid-edit cannot leave the keyboard taken);
//! - a [`Flag`], a named static set on and off — for state that is polled
//!   each frame (Session's rename) or switched by hand (a "keys off"
//!   toggle). Setting a flag to what it already is changes nothing.
//!
//! Process-wide and lock-free: windows share one keyboard focus at a time,
//! and the check sits on every key press.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// How many claims and set flags hold the keyboard.
static HOLDERS: AtomicUsize = AtomicUsize::new(0);

/// True while text is being typed somewhere: global hotkeys stand aside.
#[must_use]
pub fn typing() -> bool {
    HOLDERS.load(Ordering::Relaxed) > 0
}

/// True while global hotkeys may act ([`typing`]'s opposite).
#[must_use]
pub fn hotkeys_on() -> bool {
    !typing()
}

/// Holds the keyboard until dropped.
#[derive(Debug)]
#[must_use = "the keyboard is released when the claim is dropped"]
pub struct Claim(());

/// Takes the keyboard for text until the returned [`Claim`] is dropped.
pub fn claim() -> Claim {
    HOLDERS.fetch_add(1, Ordering::Relaxed);
    Claim(())
}

impl Drop for Claim {
    fn drop(&mut self) {
        HOLDERS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A named switch that holds the keyboard while it is on.
///
/// ```
/// static RENAMING: utils::keyboard::Flag = utils::keyboard::Flag::new();
/// RENAMING.set(true);
/// assert!(utils::keyboard::typing());
/// RENAMING.set(false);
/// ```
#[derive(Debug)]
pub struct Flag(AtomicBool);

impl Flag {
    #[must_use]
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    /// Turns the flag on or off; the keyboard is held while it is on.
    pub fn set(&self, on: bool) {
        if self.0.swap(on, Ordering::Relaxed) != on {
            if on {
                HOLDERS.fetch_add(1, Ordering::Relaxed);
            } else {
                HOLDERS.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    /// Whether this flag is on.
    #[must_use]
    pub fn get(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

impl Default for Flag {
    fn default() -> Self {
        Self::new()
    }
}

/// Turned on by the user to silence every global hotkey (a "keys off"
/// switch), whatever has focus.
pub static KEYS_OFF: Flag = Flag::new();

#[cfg(test)]
mod tests {
    use super::{Flag, claim, typing};

    // One test: the holder count is process-wide, and parallel tests
    // would see each other's claims.
    #[test]
    fn claims_and_flags_hold_the_keyboard_until_all_let_go() {
        static A: Flag = Flag::new();
        assert!(!typing());
        let field = claim();
        A.set(true);
        A.set(true);
        assert!(typing());
        drop(field);
        assert!(typing(), "the flag still holds it");
        A.set(false);
        A.set(false);
        assert!(!typing(), "setting a flag twice counts it once");
    }
}
