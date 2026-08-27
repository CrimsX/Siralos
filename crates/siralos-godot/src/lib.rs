//! Godot domain — in-repo Plugin crate behind `siralos:domain-abi@1.0.0`.
//!
//! Stage 4.1+ extraction per `decisions/34-stage4-1-generic-runtime-and-godot-plugin-extraction.md`.
//! Moves exactly the 6+3 R8/R9 surfaces out of `siralos-core`/`siralos-adapters`
//! without changing observable behavior: `discovery`/`profiling`, recovery contracts,
//! API knowledge, GDScript check-only, bounded LSP, scene/resource intelligence
//! (R8) + review/impact, `scene_mutation` prepare, unified `/develop` core (R9).
//!
//! Dependency direction: `siralos-godot → siralos-core`; `siralos-adapters` must
//! not depend on this crate and `siralos-core` must not import it (enforced by
//! `npm run check:rust`). External `github.com/CrimsX/siralos-godot` is FUTURE
//! until `siralos.toml → siralos.lock` portable locking is proven (Stage 5).

#![forbid(unsafe_code)]

//! Thin shim: `siralos-core::godot` remains the authoritative host-owned
//! domain until the sources are moved into this crate. New code should
//! import `siralos_godot`; direct `siralos_core::godot` imports are
//! deprecated but still build during the migration.
