//! Immutable Godot milestone limits (R8).
//!
//! Mirrors `packages/core/src/godot/limits.ts`. Provider input cannot raise
//! them and user configuration cannot disable them. Truncation is always
//! explicit; every bound is enforced by the adapter during discovery,
//! probing, and scanning.

/// Godot milestone limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotLimits {
    /// Maximum discovery candidates retained after validation.
    pub max_candidates: usize,
    /// Maximum accepted executable size (512 MiB).
    pub max_executable_bytes: usize,
    /// Bounded `--version` output (64 KiB).
    pub max_version_output_bytes: usize,
    /// Bounded `--help` output (2 MiB).
    pub max_help_output_bytes: usize,
    /// Bounded API dump file (128 MiB).
    pub max_api_dump_bytes: usize,
    /// Bounded `project.godot` size (4 MiB).
    pub max_project_file_bytes: usize,
    /// Maximum editor plugin descriptors enumerated.
    pub max_project_descriptors_parsed: usize,
    /// Maximum scan depth.
    pub max_project_scan_depth: usize,
}

/// Canonical Godot limits matching the TypeScript oracle.
pub const GODOT_LIMITS: GodotLimits = GodotLimits {
    max_candidates: 16,
    max_executable_bytes: 512 * 1024 * 1024,
    max_version_output_bytes: 64 * 1024,
    max_help_output_bytes: 2 * 1024 * 1024,
    max_api_dump_bytes: 128 * 1024 * 1024,
    max_project_file_bytes: 4 * 1024 * 1024,
    max_project_descriptors_parsed: 512,
    max_project_scan_depth: 64,
};

#[cfg(test)]
mod tests {
    use super::GODOT_LIMITS;

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn limits_are_sane() {
        assert_eq!(GODOT_LIMITS.max_candidates, 16);
        assert!(GODOT_LIMITS.max_executable_bytes > 4 * 1024 * 1024);
        assert!(GODOT_LIMITS.max_project_file_bytes == 4 * 1024 * 1024);
    }
}
