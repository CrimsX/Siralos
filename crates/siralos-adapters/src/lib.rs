//! Infrastructure and adapter ownership for Siralos.
//!
//! `siralos-adapters` implements infrastructure behind the domain-neutral
//! contracts owned by `siralos-core`. R1 establishes only the ownership
//! boundary plus the small domain-neutral pieces that prove it; the
//! TypeScript implementation remains the behavioral migration oracle for
//! everything else, and no stub is preferred over a truthful boundary.
//!
//! Adapters may depend on core; core must never depend on adapters
//! (enforced by `npm run check:rust`).

pub mod config;
pub mod domain;
pub mod godot;
pub mod language;
pub mod lockfile;
pub mod paths;
pub mod process;
pub mod profile_config;
pub mod provider;
pub mod reference;
pub mod research;
pub mod skills_loader;
pub mod tool;
pub mod workspace;

pub use paths::state_dir;
