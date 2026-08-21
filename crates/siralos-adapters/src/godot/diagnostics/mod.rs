//! Godot check-only diagnostic adapters (R8).

pub mod diagnostic_normalizer;
pub mod script_enumeration;
pub mod service;

pub use diagnostic_normalizer::{
    GodotCheckOutputInput, GodotCheckOutputNormalization,
    normalize_godot_check_output, normalize_with_limits,
};
pub use script_enumeration::{
    EnumerationLimits, GodotScriptEnumeration, GodotScriptTarget,
    PROJECT_SCAN_EXCLUDED_DIRECTORIES, enumerate_gdscript_files,
    enumerate_gdscript_files_with_limits, validate_check_script,
};
pub use service::{
    GODOT_CHECK_EXECUTION_UNAVAILABLE_MESSAGE, GodotDiagnosticsCancelled,
    GodotDiagnosticsService,
};
