//! Fail-closed Godot process boundary (R8).
//!
//! Every engine-affecting invocation reports a typed `unavailable`
//! outcome and never spawns an executable, creates a mirror, or mutates
//! the filesystem. Pure parsing of recorded engine output is available.

pub mod godot_check_only_runner;
pub mod godot_knowledge_runner;
pub mod godot_lsp_runner;
pub mod help_capabilities_parser;
pub mod probe_runner;
pub mod recovery_runner;
pub mod version_parser;

pub use godot_check_only_runner::{
    GODOT_CHECK_ONLY_BASE_ARGUMENTS, GODOT_CHECK_ONLY_MIRROR_PATH_MARKER,
    GODOT_CHECK_ONLY_MIRROR_SCRIPT_MARKER,
    GODOT_CHECK_ONLY_UNAVAILABLE_MESSAGE, GodotCheckOnlyRunOutcome,
    create_godot_check_only_runner, godot_check_only_argument_template,
    godot_check_only_arguments,
};
pub use godot_knowledge_runner::{
    GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE, GodotKnowledgeRunOutcome,
    compute_godot_knowledge_command_digest, create_godot_knowledge_runner,
    godot_knowledge_arguments,
};
pub use godot_lsp_runner::{
    GODOT_LSP_BASE_ARGUMENTS, GODOT_LSP_MIRROR_PATH_MARKER,
    GODOT_LSP_PORT_MARKER, GODOT_LSP_UNAVAILABLE_MESSAGE,
    GodotLspSessionCommandDigestParts, GodotLspStartOutcome,
    compute_godot_lsp_session_command_digest, create_godot_lsp_server_runner,
    godot_lsp_argument_template, godot_lsp_arguments,
};
pub use help_capabilities_parser::{
    GodotHelpParseResult, parse_help_capabilities,
};
pub use probe_runner::{
    GODOT_PROBING_UNAVAILABLE_MESSAGE, create_godot_probe_runner,
};
pub use recovery_runner::{
    GODOT_RECOVERY_BASE_ARGUMENTS, GODOT_RECOVERY_MIRROR_PATH_MARKER,
    GODOT_RECOVERY_RUN_UNAVAILABLE_MESSAGE, GodotRecoveryCommandDigestParts,
    GodotRecoveryRunOutcome, compute_godot_recovery_command_digest,
    create_godot_recovery_runner, godot_recovery_argument_template,
    godot_recovery_arguments,
};
pub use version_parser::{
    GodotVersionParseFailure, parse_godot_version_text,
    sanitize_control_characters,
};
