//! Tests for the `define_actions!` macro.

use crate::ActionCategory;

// ── Definitions-only variant (no implementation) ─────────────────────

crate::define_actions! {
    pub test_actions {
        prefix: "fts.test",
        title: "Test",
        PLAY = "play" {
            name: "Play",
            description: "Start playback",
            category: Transport,
        }
        STOP = "stop" {
            name: "Stop",
            description: "Stop playback",
            category: Transport,
            shortcut: "Space",
        }
        LOG_HELLO = "log_hello" {
            name: "Log Hello",
            description: "Log a hello message",
            category: Dev,
            group: "Debug",
            when: "tab:performance",
        }
    }
}

#[test]
fn static_action_id_constants_use_concat() {
    // The macro generates: StaticActionId::new(concat!("fts.test", ".", "play"))
    assert_eq!(test_actions::PLAY.as_str(), "fts.test.play");
    assert_eq!(test_actions::STOP.as_str(), "fts.test.stop");
    assert_eq!(test_actions::LOG_HELLO.as_str(), "fts.test.log_hello");
}

#[test]
fn definitions_returns_correct_action_definitions() {
    let defs = test_actions::definitions();
    assert_eq!(defs.len(), 3);

    let play = &defs[0];
    assert_eq!(play.id.as_str(), "fts.test.play");
    assert_eq!(play.name, "Play");
    assert_eq!(play.description, "Start playback");
    assert_eq!(play.category, ActionCategory::Transport);

    let stop = &defs[1];
    assert_eq!(stop.id.as_str(), "fts.test.stop");
    assert_eq!(stop.shortcut_hint.as_deref(), Some("Space"));

    let log = &defs[2];
    assert_eq!(log.id.as_str(), "fts.test.log_hello");
    assert_eq!(log.category, ActionCategory::Dev);
    assert_eq!(log.when.as_deref(), Some("tab:performance"));
}

#[test]
fn definitions_menu_path_auto_generated_from_title() {
    let defs = test_actions::definitions();

    // No group: menu_path should be "FTS/Test"
    let play = &defs[0];
    assert_eq!(play.menu_path.as_deref(), Some("FTS/Test"));

    // With group "Debug": menu_path should be "FTS/Test/Debug"
    let log = &defs[2];
    assert_eq!(log.menu_path.as_deref(), Some("FTS/Test/Debug"));
}

#[test]
fn title_constant_is_set() {
    assert_eq!(test_actions::TITLE, "Test");
}

// ── LocalActionBinder trait is generated ─────────────────────────────

struct TestBinder;

impl test_actions::LocalActionBinder for TestBinder {
    fn PLAY(&self) -> crate::LocalActionImplementation {
        crate::LocalActionImplementation::Unsupported("test")
    }
    fn STOP(&self) -> crate::LocalActionImplementation {
        crate::LocalActionImplementation::Unsupported("test")
    }
    fn LOG_HELLO(&self) -> crate::LocalActionImplementation {
        crate::LocalActionImplementation::Unsupported("test")
    }
}

#[test]
fn local_action_binder_trait_is_generated() {
    let binder = TestBinder;
    let regs = test_actions::definitions_with_binder(&binder);
    assert_eq!(regs.len(), 3);
    assert_eq!(regs[0].definition.id.as_str(), "fts.test.play");
    // Verify the implementation type
    assert!(matches!(
        regs[0].implementation,
        crate::LocalActionImplementation::Unsupported("test")
    ));
}

// ── Title-less variant defaults to empty string ──────────────────────

crate::define_actions! {
    pub no_title_actions {
        prefix: "fts.bare",
        THING = "thing" {
            name: "Thing",
            description: "A thing",
            category: General,
        }
    }
}

#[test]
fn no_title_defaults_to_empty_and_menu_path_is_fts() {
    assert_eq!(no_title_actions::TITLE, "");
    let defs = no_title_actions::definitions();
    assert_eq!(defs[0].menu_path.as_deref(), Some("FTS"));
}

// ── With explicit title ──────────────────────────────────────────────

crate::define_actions! {
    pub titled_actions {
        prefix: "fts.titled",
        title: "My Section",
        ALPHA = "alpha" {
            name: "Alpha",
            description: "First action",
            category: General,
        }
    }
}

#[test]
fn explicit_title_in_menu_path() {
    let defs = titled_actions::definitions();
    assert_eq!(defs[0].menu_path.as_deref(), Some("FTS/My Section"));
}
