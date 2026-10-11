//! Production Component Model / WIT host boundary (ADR 0034, Stage 3R
//! R6).
//!
//! `siralos-adapters` owns the executable Module boundary: the
//! versioned WIT world (`wit/module-abi.wit`), component
//! loading/instantiation, exact-byte package verification, guest-call
//! and resource enforcement, and the host-mediated effect adapters.
//! The generic lifecycle/capability semantics live in
//! `siralos-core::module`; nothing here reimplements them.
//!
//! The component receives no ambient authority: the linker grants
//! exactly the `host-effects` import the world declares, and every
//! effect request is validated against the active capability grant
//! with the Host's bounds applied.

mod effects;
mod host;
pub mod manifest;

pub use effects::{
    EffectMediation, EffectMediationBounds, MAX_EFFECT_COMMAND_BYTES,
    MAX_EFFECT_PATH_BYTES, MediatedAnswer,
};
pub use host::{EffectRequest, ModuleHost, ModuleHostBounds, QueryOutcome};
pub use manifest::{
    MAX_COMPONENT_BYTES, MAX_MANIFEST_BYTES, MAX_SIRALOS_TOML_BYTES,
    MODULE_MANIFEST_FILE_NAME, ModuleManifest, ModuleManifestFailure,
    ModuleRecord, SIRALOS_TOML_FILE_NAME, install_module, load_manifest,
    load_module_records, record_module, verify_component,
};
