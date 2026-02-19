//! Universal input processing with trie-based key dispatch and modal editing.
//!
//! This crate provides the core input processing pipeline: key chord matching,
//! modal editing with a mode stack, multi-key sequences via a trie, and
//! operator+motion composition.

#![deny(unsafe_code)]

pub mod command;
pub mod config;
pub mod context;
pub mod event;
pub mod key;
pub mod macros;
pub mod mode;
pub mod mouse;
pub mod processor;
pub mod scroll;
pub mod sequence;
pub mod trie;

pub use command::{ActionId, InputArgs, InputCommand};
pub use config::{ConfigError, KeymapConfig};
pub use context::{ActionContext, WhenExpr};
pub use event::{InputEvent, KeyEvent, MouseAction, MouseButton, MouseEvent, ScrollEvent};
pub use key::{KeyChord, KeyCode, Modifiers};
pub use macros::MacroRecorder;
pub use mode::{ModeDefinition, ModeId, ModeStack};
pub use mouse::{MouseBindingTable, MousePattern};
pub use processor::InputProcessor;
pub use scroll::{ScrollAxis, ScrollBindingTable, ScrollPattern};
pub use sequence::{SequenceResult, SequenceState};
pub use trie::{KeyTrie, LeafAction, TrieNode};
