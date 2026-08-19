//! Optional Godot Stage-2 parity — typed models and pure selection (R8).
//!
//! This module is the Rust counterpart of `packages/core/src/godot/`.
//! It owns only provider-neutral typed models and pure, host-owned
//! selection semantics. It performs no filesystem, path/canonicalization,
//! subprocess, or network operation; adapters own every governed effect.
//!
//! Domain isolation: the Rust guard in `scripts/check-rust-architecture.mjs`
//! now tolerates `src/godot/**` in `siralos-core` (6a77885, R8 entry-review)
//! while the rest of the crate remains domain-neutral. A type in this
//! module may name Godot concepts, but no type outside this module may
//! depend on them.

pub mod capabilities;
pub mod engine_profile;
pub mod installations;
pub mod version;

pub use capabilities::{
    GodotCommandCapabilities, empty_godot_command_capabilities,
};
pub use engine_profile::{
    GodotEdition, GodotEditionClassification, GodotEditionConfidence,
    GodotEditionEvidence, GodotEditionHint, GodotEngineProfile,
    GodotProbesSucceeded, GodotSupportClassificationInput,
    SiralosGodotSupport, classify_godot_edition, classify_godot_support,
    describe_installation_provenance, is_editor_selection_candidate,
};
pub use installations::{
    GodotEditionHint as InstallEditionHint, GodotInstallation,
    GodotInstallationSource,
};

pub use version::{
    GodotDeclaredVersion, GodotReleaseChannel, GodotVersion,
    GodotVersionStatus, classify_godot_release_channel,
    parse_declared_version,
};
