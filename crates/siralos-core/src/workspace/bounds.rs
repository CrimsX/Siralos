//! Workspace operation bounds (R4, reference `WORKSPACE_LIMITS`).
//!
//! Every bounded result exposes its truncation disposition; an exact
//! read never derives whole-file identity from truncated returned text
//! (the SHA-256 always covers the complete allowed file bytes).

/// Bounded workspace operation limits mirroring the TypeScript
/// reference `WORKSPACE_LIMITS` (packages/adapters workspace limits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceLimits {
    /// Maximum directory entries returned by one list operation.
    pub max_directory_entries: usize,
    /// Maximum file bytes accepted by an exact read.
    pub max_read_file_size_bytes: usize,
    /// Maximum content characters returned by an exact read.
    pub max_read_content_chars: usize,
    /// Maximum file bytes considered by search.
    pub max_search_file_size_bytes: usize,
    /// Maximum files scanned by one search operation.
    pub max_search_files: usize,
    /// Maximum matches returned by one search operation.
    pub max_search_matches: usize,
    /// Maximum match text characters returned per line.
    pub max_search_line_length_chars: usize,
    /// Maximum complete text-file bytes (mutation contract).
    pub max_text_file_size_bytes: usize,
    /// Maximum created-file content bytes (mutation contract).
    pub max_created_content_bytes: usize,
    /// Maximum sequential exact replacements (mutation contract).
    pub max_replacements: usize,
    /// Maximum replacement text bytes (mutation contract).
    pub max_replacement_text_bytes: usize,
    /// Maximum complete preview diff bytes.
    pub max_complete_diff_bytes: usize,
    /// Maximum preview diff lines.
    pub max_diff_lines: usize,
    /// Maximum directories visited by one search traversal.
    pub max_search_directories: usize,
    /// Maximum entries examined by one search traversal.
    pub max_search_entries: usize,
    /// Maximum files considered (lstat) by one search traversal.
    pub max_search_files_considered: usize,
    /// Maximum input bytes read by one search traversal.
    pub max_search_input_bytes: usize,
    /// Maximum output match bytes emitted by one search.
    pub max_search_output_bytes: usize,
    /// Wall-clock budget for one search (adapter wall clock only).
    pub max_search_duration_ms: u64,
    /// Maximum directory depth for recursive search (root counts).
    pub max_search_depth: usize,
}

/// The reference workspace limits (adapter execution defaults).
pub const WORKSPACE_LIMITS: WorkspaceLimits = WorkspaceLimits {
    max_directory_entries: 200,
    max_read_file_size_bytes: 512 * 1024,
    max_read_content_chars: 64_000,
    max_search_file_size_bytes: 512 * 1024,
    max_search_files: 500,
    max_search_matches: 100,
    max_search_line_length_chars: 400,
    max_text_file_size_bytes: 1024 * 1024,
    max_created_content_bytes: 512 * 1024,
    max_replacements: 32,
    max_replacement_text_bytes: 64 * 1024,
    max_complete_diff_bytes: 256 * 1024,
    max_diff_lines: 10_000,
    max_search_directories: 2_000,
    max_search_entries: 25_000,
    max_search_files_considered: 2_000,
    max_search_input_bytes: 64 * 1024 * 1024,
    max_search_output_bytes: 200_000,
    max_search_duration_ms: 10_000,
    max_search_depth: 64,
};

#[cfg(test)]
mod tests {
    use super::WORKSPACE_LIMITS;

    #[test]
    fn reference_limits_are_positive_and_ordered() {
        // Constant assertions are evaluated at compile time; the values
        const _: () = {
            assert!(WORKSPACE_LIMITS.max_directory_entries > 0);
            assert!(WORKSPACE_LIMITS.max_read_file_size_bytes > 0);
            assert!(WORKSPACE_LIMITS.max_read_content_chars > 0);
            assert!(WORKSPACE_LIMITS.max_search_matches > 0);
            assert!(WORKSPACE_LIMITS.max_search_files > 0);
            assert!(WORKSPACE_LIMITS.max_search_duration_ms > 0);
            assert!(WORKSPACE_LIMITS.max_search_depth > 0);
            assert!(
                WORKSPACE_LIMITS.max_read_content_chars
                    < WORKSPACE_LIMITS.max_read_file_size_bytes
            );
        };
    }
}
