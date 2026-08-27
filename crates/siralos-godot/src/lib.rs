//! Godot domain — in-repo Plugin crate behind `siralos:domain-abi@1.0.0`.
//!
//! Stage 4.1+ extraction per `decisions/34-stage4-1-generic-runtime-and-godot-plugin-extraction.md`
//! and `decisions/37-godot-crate-extraction-entry-review.md`: the 6+3 R8/R9
//! host-owned surfaces live here as `siralos_godot::godot`, and `siralos-core`
//! is domain-neutral again (`src/godot` removed). Adapters and the CLI depend
//! on this crate for Godot domain types; every runner stays fail-closed
//! (apply/checkpoints typed `unavailable`).

#![forbid(unsafe_code)]

pub mod godot;

pub use godot::*;
