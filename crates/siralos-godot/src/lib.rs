//! Godot domain — in-repo Plugin crate behind `siralos:domain-abi@1.0.0`.
//!
//! Stage 4.1+ extraction per `decisions/34-stage4-1-generic-runtime-and-godot-plugin-extraction.md`
//! and `decisions/37-godot-crate-extraction-entry-review.md`.
//! Sources have been duplicated into `siralos-godot/src/godot` (38 files)
//! with rewired imports (`crate::identity` → `siralos_core::identity`); the
//! canonical `siralos-core/src/godot` remains until consumers flip imports.
//! `pub mod godot` here is the extraction owner; `siralos_core::godot` is the
//! deprecated shim during migration. Both compile to 72 tests in this slice.

#![forbid(unsafe_code)]

pub mod godot;
