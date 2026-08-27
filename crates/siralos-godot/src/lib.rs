//! Godot domain — in-repo Plugin crate behind `siralos:domain-abi@1.0.0`.
//!
//! Stage 4.1+ extraction per `decisions/34-stage4-1-generic-runtime-and-godot-plugin-extraction.md`
//! (shim `0996b38`; sources still in `siralos-core/src/godot`, `siralos-godot`
//! is an empty crate that compiles to 0 tests). The crate is a placeholder
//! that reserves the workspace name and `siralos-godot → siralos-core`
//! direction (`Cargo.toml`, `check:rust`) until the 6+3 verbatim move
//! (decision 37) is implemented. No Godot domain is re-exported yet; imports
//! must still use `siralos_core::godot`.

#![forbid(unsafe_code)]
