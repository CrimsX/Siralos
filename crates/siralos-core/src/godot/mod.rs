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

pub mod api;
pub mod capabilities;
pub mod engine_profile;
pub mod gdscript;
pub mod installations;
pub mod knowledge;
pub mod limits;
pub mod lsp;
pub mod project;
pub mod selection;
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

pub use api::{
    GodotApiParameter, GodotApiSearchKind, GodotApiSearchQuery,
    GodotApiSearchRank, GodotApiSearchResult, GodotApiSymbol,
    GodotApiSymbolDetails, GodotApiSymbolKind, GodotApiType, godot_symbol_id,
};
pub use gdscript::{
    GdScriptDiagnosticSource, GdScriptSeverity, GodotDiagnosticPreview,
    GodotDiagnosticScripts, GodotGdScriptDiagnostic, GodotScriptCheckTarget,
};
pub use knowledge::{
    GodotKnowledgeCacheValidation, GodotKnowledgeProfileV1,
    GodotKnowledgeStatus, GodotKnowledgeSupport, KNOWLEDGE_SCHEMA_VERSION,
    KnowledgeApi, KnowledgeCacheReason, KnowledgeEngine, KnowledgeIndex,
    KnowledgeState, KnowledgeSupportState, classify_godot_manual_channel,
    validate_godot_knowledge_cache,
};
pub use limits::{GODOT_LIMITS, GodotLimits};
pub use lsp::{
    EMPTY_GDSCRIPT_LSP_CAPABILITIES, GdScriptCompletionItem,
    GdScriptCompletionResult, GdScriptDefinitionLocation,
    GdScriptDefinitionResult, GdScriptDiagnosticResult,
    GdScriptDocumentRequest, GdScriptHoverResult, GdScriptHoverSection,
    GdScriptLspCapabilities, GdScriptNetworkIsolation, GdScriptPosition,
    GdScriptPositionRequest, GdScriptQueryOutcome, GdScriptSessionState,
    GdScriptSessionStatus, GdScriptSourceRange,
};
pub use project::{
    GodotAutoloadSummary, GodotExecutableContentInventory,
    GodotGDExtensionSummary, GodotLanguageProfile, GodotPluginLanguage,
    GodotPluginSummary, GodotProjectProfile, GodotScanTruncationReason,
    create_empty_godot_executable_content_inventory,
    create_empty_godot_project_profile,
};
pub use selection::{
    GodotRankedCandidate, GodotSelectionOutcome, GodotSelectionPreference,
    godot_selection_ranks, rank_candidate, rank_godot_candidates,
};
pub use version::{
    GodotDeclaredVersion, GodotReleaseChannel, GodotVersion,
    GodotVersionStatus, classify_godot_release_channel,
    parse_declared_version,
};
