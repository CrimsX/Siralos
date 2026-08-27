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
    /// Bounded editor plugin descriptor size (256 KiB).
    pub max_plugin_descriptor_bytes: usize,
    /// Bounded GDExtension descriptor size (1 MiB).
    pub max_gdextension_descriptor_bytes: usize,
    /// Maximum project files scanned by language and content inventory.
    pub max_project_files_scanned: usize,
    /// Maximum project directories visited during static traversal (root counts).
    pub max_project_directories_visited: usize,
    /// Maximum project directory depth during static traversal (root counts).
    pub max_project_scan_depth: usize,
    /// Maximum readdir entries examined (excluded and non-regular entries count).
    pub max_project_entries_examined: usize,
    /// Maximum files surfaced in a static scan result.
    pub max_project_files_surfaced: usize,
    /// Maximum editor plugin directories enumerated in addons/.
    pub max_project_plugin_directories: usize,
    /// Maximum plugin.cfg + .gdextension descriptors parsed per inspection.
    pub max_project_descriptors_parsed: usize,
    /// Maximum executable-content inventory output items (scripts + plugins + descriptors + autoloads).
    pub max_project_inventory_items: usize,
    /// Maximum total raw file bytes read during content inventory (128 MiB).
    pub max_project_total_read_bytes: usize,
    /// Maximum source bytes inspected by content inventory (64 MiB).
    pub max_source_bytes_inspected: usize,
    /// Maximum tool-script head bytes scanned for `@tool` markers.
    pub max_tool_script_head_bytes: usize,
    /// Maximum enabled editor plugin entries declared in project.godot.
    pub max_project_plugins: usize,
    /// Maximum autoload declarations retained per project.
    pub max_project_autoloads: usize,
    /// Maximum input-action declarations retained per project.
    pub max_project_input_actions: usize,
    /// Maximum event types retained per input action.
    pub max_input_action_event_types: usize,
    /// Maximum GDExtension library targets assessed per descriptor.
    pub max_gdextension_targets_per_descriptor: usize,
    /// Maximum UTF-8 byte length of a project-provided res:// path reference.
    pub max_res_reference_path_bytes: usize,
    /// Maximum UTF-8 byte length of a descriptor field value.
    pub max_project_descriptor_value_bytes: usize,
    /// Maximum configured installation entries.
    pub max_configured_installations: usize,
    /// Maximum installation id length.
    pub max_installation_id_length: usize,
    /// Version probe timeout.
    pub version_probe_timeout_ms: u64,
    /// Help probe timeout.
    pub help_probe_timeout_ms: u64,
    /// Extension API dump probe timeout.
    pub api_dump_timeout_ms: u64,
    /// Static project scan timeout (checked during traversal).
    pub static_project_scan_timeout_ms: u64,
    /// Recovery-probe risk refresh deadline (static inspection + hashing).
    pub risk_refresh_timeout_ms: u64,
    /// Disposable project mirror preparation deadline.
    pub mirror_prepare_timeout_ms: u64,
    /// Recovery-mode editor startup probe deadline.
    pub recovery_probe_timeout_ms: u64,
    /// Bounded editor iteration count for `--quit-after`.
    pub recovery_quit_after_iterations: u64,
    /// Post-probe `.godot` inspection deadline.
    pub post_probe_inspection_timeout_ms: u64,
    /// Probe-directory cleanup deadline.
    pub cleanup_timeout_ms: u64,
    /// Maximum mirror source files copied.
    pub max_mirror_files: usize,
    /// Maximum mirror total bytes (4 GiB).
    pub max_mirror_bytes: u64,
    /// Maximum single mirror file bytes (512 MiB).
    pub max_mirror_single_file_bytes: usize,
    /// Maximum mirror relative path length in UTF-8 bytes.
    pub max_mirror_relative_path_bytes: usize,
    /// Maximum mirror directory depth.
    pub max_mirror_depth: usize,
    /// Per-stream recovery probe output capture (1 MiB).
    pub max_recovery_stream_bytes: usize,
    /// Maximum normalized recovery diagnostics retained per severity.
    pub max_recovery_diagnostics: usize,
    /// Maximum raw recovery diagnostic lines retained for display.
    pub max_raw_diagnostic_lines: usize,
    /// Maximum `.godot` generated files inspected after the probe.
    pub max_generated_godot_files: usize,
    /// Maximum `.godot` generated bytes inspected after the probe.
    pub max_generated_godot_bytes: usize,
    /// Maximum authored files covered by a workspace integrity baseline.
    pub max_baseline_manifest_files: usize,
    /// Maximum authored bytes covered by a workspace integrity baseline.
    pub max_baseline_manifest_bytes: u64,
    /// Maximum simultaneously prepared recovery probes (bounded prepared state).
    pub max_prepared_probes: usize,
    /// Maximum aggregate serialized bytes of all prepared probe plans.
    pub max_prepared_probe_state_bytes: usize,
    /// Maximum lifetime of a prepared probe before it expires.
    pub prepared_probe_ttl_ms: u64,
    /// Maximum accepted `--dump-extension-api-with-docs` output file (256 MiB).
    pub max_api_dump_with_docs_bytes: usize,
    /// Maximum indexed API classes (native + built-in).
    pub max_api_classes: usize,
    /// Maximum indexed API symbols (classes, members, globals, utilities).
    pub max_api_symbols: usize,
    /// Maximum retained description length for one API symbol (256 KiB).
    pub max_api_description_bytes: usize,
    /// Maximum search-result snippet length (2 KiB).
    pub max_api_summary_bytes: usize,
    /// Maximum API search results returned to the provider or CLI.
    pub max_api_search_results: usize,
    /// Maximum serialized API lookup result bytes (512 KiB).
    pub max_api_lookup_result_bytes: usize,
    /// Knowledge profile schema version (immutable; mismatch rebuilds the cache).
    pub knowledge_schema_version: u32,
    /// With-docs API dump probe timeout (fixed Siralos probe).
    pub api_docs_dump_timeout_ms: u64,
    /// Maximum checked GDScript file size (4 MiB).
    pub max_gdscript_file_bytes: usize,
    /// Maximum scripts enumerated per project-wide check.
    pub max_gdscript_files_per_project: usize,
    /// Maximum aggregate GDScript bytes per project-wide check.
    pub max_gdscript_total_bytes: usize,
    /// Maximum normalized diagnostics retained for one script.
    pub max_diagnostics_per_script: usize,
    /// Maximum normalized diagnostics retained for one check run.
    pub max_diagnostics_per_run: usize,
    /// Maximum retained length of one normalized diagnostic message (8 KiB).
    pub max_diagnostic_message_bytes: usize,
    /// Single check-only invocation timeout (30 seconds).
    pub gdscript_check_timeout_ms: u64,
    /// Total budget for one project-wide diagnostic run (10 minutes).
    pub project_diagnostics_budget_ms: u64,
    /// Maximum raw output bytes captured per check stream.
    pub max_check_stream_bytes: usize,
    /// Maximum simultaneously prepared GDScript checks.
    pub max_prepared_checks: usize,
    /// Maximum aggregate serialized bytes of all prepared check plans.
    pub max_prepared_check_state_bytes: usize,
    /// Maximum lifetime of a prepared check before it expires.
    pub prepared_check_ttl_ms: u64,
    /// Maximum incoming LSP message body (16 MiB).
    pub lsp_message_body_bytes: usize,
    /// Maximum LSP header block (32 KiB).
    pub lsp_header_bytes: usize,
    /// Maximum concurrent pending JSON RPC requests.
    pub lsp_max_pending_requests: usize,
    /// Maximum simultaneously open LSP documents.
    pub lsp_max_open_documents: usize,
    /// Maximum normalized diagnostics retained per document.
    pub lsp_max_diagnostics_per_document: usize,
    /// Maximum completion items returned per query.
    pub lsp_max_completion_items: usize,
    /// Maximum retained hover content bytes (512 KiB).
    pub lsp_max_hover_bytes: usize,
    /// Maximum definition locations returned per query.
    pub lsp_max_definition_locations: usize,
    /// LSP session startup timeout (30 seconds).
    pub lsp_startup_timeout_ms: u64,
    /// LSP request timeout (15 seconds).
    pub lsp_request_timeout_ms: u64,
    /// LSP session idle timeout (10 minutes).
    pub lsp_idle_timeout_ms: u64,
    /// Maximum LSP session lifetime (30 minutes).
    pub lsp_max_session_lifetime_ms: u64,
    /// Bounded shutdown wait after LSP shutdown (5 seconds).
    pub lsp_shutdown_timeout_ms: u64,
    /// LSP session policy version (binds every approval).
    pub lsp_policy_version: u32,
    /// Maximum simultaneously prepared LSP sessions.
    pub max_prepared_lsp_sessions: usize,
    /// Maximum aggregate serialized bytes of all prepared LSP session plans.
    pub max_prepared_lsp_session_state_bytes: usize,
    /// Maximum lifetime of a prepared LSP session before it expires.
    pub prepared_lsp_session_ttl_ms: u64,
}

/// Canonical Godot limits matching the TypeScript oracle.
pub const GODOT_LIMITS: GodotLimits = GodotLimits {
    max_candidates: 16,
    max_executable_bytes: 512 * 1024 * 1024,
    max_version_output_bytes: 64 * 1024,
    max_help_output_bytes: 2 * 1024 * 1024,
    max_api_dump_bytes: 128 * 1024 * 1024,
    max_project_file_bytes: 4 * 1024 * 1024,
    max_plugin_descriptor_bytes: 256 * 1024,
    max_gdextension_descriptor_bytes: 1024 * 1024,
    max_project_files_scanned: 50_000,
    max_project_directories_visited: 10_000,
    max_project_scan_depth: 64,
    max_project_entries_examined: 200_000,
    max_project_files_surfaced: 20_000,
    max_project_plugin_directories: 256,
    max_project_descriptors_parsed: 512,
    max_project_inventory_items: 4096,
    max_project_total_read_bytes: 128 * 1024 * 1024,
    max_source_bytes_inspected: 64 * 1024 * 1024,
    max_tool_script_head_bytes: 32 * 1024,
    max_project_plugins: 256,
    max_project_autoloads: 256,
    max_project_input_actions: 512,
    max_input_action_event_types: 16,
    max_gdextension_targets_per_descriptor: 512,
    max_res_reference_path_bytes: 1024,
    max_project_descriptor_value_bytes: 16 * 1024,
    max_configured_installations: 16,
    max_installation_id_length: 64,
    version_probe_timeout_ms: 10_000,
    help_probe_timeout_ms: 15_000,
    api_dump_timeout_ms: 120_000,
    static_project_scan_timeout_ms: 30_000,
    risk_refresh_timeout_ms: 30_000,
    mirror_prepare_timeout_ms: 120_000,
    recovery_probe_timeout_ms: 60_000,
    recovery_quit_after_iterations: 120,
    post_probe_inspection_timeout_ms: 30_000,
    cleanup_timeout_ms: 60_000,
    max_mirror_files: 100_000,
    max_mirror_bytes: 4 * 1024 * 1024 * 1024,
    max_mirror_single_file_bytes: 512 * 1024 * 1024,
    max_mirror_relative_path_bytes: 1024,
    max_mirror_depth: 64,
    max_recovery_stream_bytes: 1024 * 1024,
    max_recovery_diagnostics: 100,
    max_raw_diagnostic_lines: 200,
    max_generated_godot_files: 20_000,
    max_generated_godot_bytes: 512 * 1024 * 1024,
    max_baseline_manifest_files: 100_000,
    max_baseline_manifest_bytes: 4 * 1024 * 1024 * 1024,
    max_prepared_probes: 8,
    max_prepared_probe_state_bytes: 8 * 1024 * 1024,
    prepared_probe_ttl_ms: 600_000,
    max_api_dump_with_docs_bytes: 256 * 1024 * 1024,
    max_api_classes: 20_000,
    max_api_symbols: 500_000,
    max_api_description_bytes: 256 * 1024,
    max_api_summary_bytes: 2 * 1024,
    max_api_search_results: 25,
    max_api_lookup_result_bytes: 512 * 1024,
    knowledge_schema_version: 1,
    api_docs_dump_timeout_ms: 180_000,
    max_gdscript_file_bytes: 4 * 1024 * 1024,
    max_gdscript_files_per_project: 10_000,
    max_gdscript_total_bytes: 256 * 1024 * 1024,
    max_diagnostics_per_script: 500,
    max_diagnostics_per_run: 10_000,
    max_diagnostic_message_bytes: 8 * 1024,
    gdscript_check_timeout_ms: 30_000,
    project_diagnostics_budget_ms: 600_000,
    max_check_stream_bytes: 8 * 1024 * 1024,
    max_prepared_checks: 8,
    max_prepared_check_state_bytes: 8 * 1024 * 1024,
    prepared_check_ttl_ms: 600_000,
    lsp_message_body_bytes: 16 * 1024 * 1024,
    lsp_header_bytes: 32 * 1024,
    lsp_max_pending_requests: 128,
    lsp_max_open_documents: 256,
    lsp_max_diagnostics_per_document: 2000,
    lsp_max_completion_items: 500,
    lsp_max_hover_bytes: 512 * 1024,
    lsp_max_definition_locations: 100,
    lsp_startup_timeout_ms: 30_000,
    lsp_request_timeout_ms: 15_000,
    lsp_idle_timeout_ms: 600_000,
    lsp_max_session_lifetime_ms: 1_800_000,
    lsp_shutdown_timeout_ms: 5_000,
    lsp_policy_version: 1,
    max_prepared_lsp_sessions: 4,
    max_prepared_lsp_session_state_bytes: 8 * 1024 * 1024,
    prepared_lsp_session_ttl_ms: 600_000,
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
        assert_eq!(GODOT_LIMITS.max_plugin_descriptor_bytes, 256 * 1024);
        assert_eq!(GODOT_LIMITS.max_project_files_scanned, 50_000);
        assert_eq!(GODOT_LIMITS.max_project_directories_visited, 10_000);
        assert_eq!(GODOT_LIMITS.lsp_policy_version, 1);
        assert_eq!(GODOT_LIMITS.knowledge_schema_version, 1);
        assert_eq!(GODOT_LIMITS.prepared_probe_ttl_ms, 600_000);
        assert_eq!(
            GODOT_LIMITS.max_api_dump_with_docs_bytes,
            256 * 1024 * 1024
        );
    }
}
