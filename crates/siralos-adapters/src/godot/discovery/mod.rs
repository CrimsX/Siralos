//! Godot discovery adapters (R8-6b, TS oracle
//! `packages/adapters/src/godot/discovery/`).

pub mod candidate_names;
pub mod executable_validation;
pub mod macos_bundle;
pub mod path_discovery;

pub use candidate_names::{godot_candidate_names, path_list_separator};
pub use executable_validation::{
    ExecutableIdentity, ValidateExecutableOptions, validate_executable,
};
pub use macos_bundle::{enclosing_app_bundle, resolve_macos_bundle};
pub use path_discovery::{PathDiscoveryOptions, discover_on_path};
