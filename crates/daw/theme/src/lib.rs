//! `daw-theme`: the contract between a theme pack and the views that draw
//! a DAW (the arrangement and its track panel, the mixer, the MIDI editor).
//!
//! - [`spec`]: every token, part and field a pack can implement, versioned.
//! - [`view`]: what each view declares it draws with ([`view::Uses`]),
//!   held to the spec.
//! - [`pack`]: a pack on disk (`themes/`, an optional `pack.styx`) and its
//!   themes, compiled with Frame.
//! - [`source`]: a pack's link (`owner/repo[@rev]`, a GitHub URL, a
//!   folder), installed with `git` into a cache, pinned by commit.
//! - [`check`]: a theme held to the spec — what it names that the spec
//!   lacks, what it leaves out and what that falls back to.
//! - [`tokens`]: a token's value, with the spec's fallback when a pack
//!   leaves it out.
//! - [`watch`]: noticing a theme's files change, for live reload.
//! - [`library`]: the packs a host installed and the theme it shows.
//!
//! The views implement the spec (Studio's `daw-view-kit`); the packs are
//! `studio-<name>-theme` repositories of Frame documents. Every view loads
//! the same pack, so a mute looks the same on a strip as in the track panel.

pub mod check;
pub mod library;
pub mod pack;
pub mod source;
pub mod spec;
pub mod tokens;
pub mod view;
pub mod watch;

pub use check::{Report, check, check_compiled};
pub use pack::{Pack, ThemeDef};
pub use source::{Installed, Source, install};
pub use tokens::{Resolved, Tokens};
pub use view::Uses;
pub use watch::Watch;
