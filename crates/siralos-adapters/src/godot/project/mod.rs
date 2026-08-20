//! Minimal bounded static Godot project scan (R8-6b).
//!
//! Proves Available without any probe: TraversalBudget plus scan_project_file
//! plus read_project_file under GODOT_LIMITS.

pub mod files;
pub mod scanner;
pub mod traversal_limits;

pub use files::read_project_file;
pub use scanner::{
    GodotProjectScanResult, ScannedProjectProperty, scan_project_file,
};
pub use traversal_limits::TraversalBudget;
