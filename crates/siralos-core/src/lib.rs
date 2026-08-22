//! Domain-neutral core of the Siralos harness.
//!
//! `siralos-core` owns host semantics and types that are independent of
//! infrastructure implementations and of any optional domain. It must
//! never depend on adapter infrastructure or on a domain implementation;
//! the architecture check (`npm run check:rust`) enforces that boundary
//! mechanically, and the crate compiles with every optional domain
//! absent.
//!
//! The TypeScript implementation is the behavioral migration oracle;
//! see `docs/development/RUST_STYLE.md` and ADR 0032 for the engineering
//! rules this crate follows.

pub mod determinism;
pub mod domain;
pub mod godot;
pub mod identity;
pub mod language;
pub mod projection;
pub mod provider;
pub mod task;
pub mod tool;
pub mod version;
pub mod workspace;

pub use version::Version;
