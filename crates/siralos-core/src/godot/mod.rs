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
pub mod compatibility;
pub mod diagnostics;
pub mod digest;
pub mod engine_profile;
pub mod events;
pub mod gdscript;
pub mod impact;
pub mod inspector;
pub mod installations;
pub mod knowledge;
pub mod limits;
pub mod lsp;
pub mod probe;
pub mod probes;
pub mod project;
pub mod scene;
pub mod scene_mutation;
pub mod selection;
pub mod version;

pub use capabilities::{
    FORBIDDEN_GODOT_PROJECT_ARGUMENTS, GODOT_KNOWN_OPTIONS,
    GodotCapabilityKey, GodotCommandCapabilities, GodotKnownOption,
    empty_godot_command_capabilities,
};
pub use compatibility::{
    CompatibilitySeverity, GodotCompatibilityAssessment,
    GodotCompatibilityStatus, assess_godot_compatibility,
};
pub use diagnostics::{DiagnosticSeverity, SafeDiagnostic};
pub use digest::{canonicalize_json, sha256_hex_str};
pub use engine_profile::{
    GodotEdition, GodotEditionClassification, GodotEditionConfidence,
    GodotEditionEvidence, GodotEditionHint, GodotEngineProfile,
    GodotProbesSucceeded, GodotSupportClassificationInput,
    SiralosGodotSupport, classify_godot_edition, classify_godot_support,
    describe_installation_provenance, is_editor_selection_candidate,
};
pub use events::GodotApplicationEvent;
pub use inspector::{
    GodotDiscoveryConfiguration, GodotDiscoveryResult, GodotDoctorCache,
    GodotDoctorReport, GodotDoctorSandbox, GodotInstallationOverview,
    GodotProbeStatusLine, GodotSelectedInstallation, GodotStatusSnapshot,
};
pub use installations::{
    GodotEditionHint as InstallEditionHint, GodotInstallation,
    GodotInstallationSource,
};

pub use api::{
    GodotApiIndex, GodotApiLookupResult, GodotApiNamedValue,
    GodotApiParameter, GodotApiSearchKind, GodotApiSearchOutcome,
    GodotApiSearchQuery, GodotApiSearchRank, GodotApiSearchResult,
    GodotApiSymbol, GodotApiSymbolDetails, GodotApiSymbolKind, GodotApiType,
    godot_symbol_id,
};
pub use gdscript::{
    GODOT_DIAGNOSTICS_OFFLINE_PROFILE_ID, GdScriptDiagnosticSource,
    GdScriptSeverity, GodotCheckOnlyCommandDigestParts,
    GodotCheckPreparationResult, GodotCheckPreparationStatus,
    GodotDiagnosticPreview, GodotDiagnosticScripts,
    GodotDiagnosticsExecutionContext, GodotDiagnosticsRequest,
    GodotDiagnosticsState, GodotDiagnosticsStatus, GodotDiagnosticsSupport,
    GodotGdScriptDiagnostic, GodotPreparedCheckDigestParts,
    GodotPreparedCheckLimits, GodotProjectCheckResult,
    GodotProjectCheckRunStatus, GodotScriptCheckTarget, PreparedGDScriptCheck,
    compute_godot_check_only_command_digest,
    compute_godot_prepared_check_digest,
};
pub use knowledge::{
    GodotKnowledgeBase, GodotKnowledgeCacheValidation,
    GodotKnowledgeLookupOutcome, GodotKnowledgeProfileV1,
    GodotKnowledgeQueryResult, GodotKnowledgeRefreshResult,
    GodotKnowledgeStatus, GodotKnowledgeSupport, KNOWLEDGE_SCHEMA_VERSION,
    KnowledgeApi, KnowledgeCacheReason, KnowledgeEngine, KnowledgeIndex,
    KnowledgeLookupStatus, KnowledgeQueryStatus, KnowledgeRefreshStatus,
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
pub use probe::{
    GodotAuthoredFileManifest, GodotAutoloadRiskEntry, GodotDiagnostic,
    GodotDiagnosticCategory, GodotFileRiskEntry, GodotGDExtensionRiskEntry,
    GodotImportState, GodotLibraryRiskEntry, GodotPluginRiskEntry,
    GodotPreparedProbeDigestParts, GodotProbeEngineSelection,
    GodotProbeLimits, GodotProbeMirrorEstimate, GodotProbePreview,
    GodotProbeRiskCounts, GodotProbeStatus, GodotProjectRiskManifest,
    GodotProjectTrustState, compute_godot_prepared_probe_digest,
    compute_godot_risk_manifest_digest,
};
pub use probes::{
    GodotApiDumpProbe, GodotApiDumpSummary, GodotHelpProbe, GodotProbeRunner,
    GodotVersionProbe,
};
pub use project::{
    GodotAutoloadSummary, GodotExecutableContentInventory,
    GodotGDExtensionSummary, GodotLanguageProfile, GodotPluginLanguage,
    GodotPluginSummary, GodotProjectProfile, GodotScanTruncationReason,
    create_empty_godot_executable_content_inventory,
    create_empty_godot_project_profile,
};
pub use scene::{
    BalancedScan, DictionaryEntry, ExternalResourceRef, GODOT_SCENE_LIMITS,
    GodotAutoload, GodotDependencyEdge, GodotDependencyResult,
    GodotInputAction, GodotInspectionOutcome, GodotIntelligenceStatus,
    GodotMainSceneReference, GodotParseStatus, GodotProjectRelationshipResult,
    GodotProperty, GodotRawValue, GodotRelationshipEntry,
    GodotRelationshipIndex, GodotRelationshipKind,
    GodotResourceInspectionResult, GodotResourceModel, GodotSceneEvidenceView,
    GodotSceneInspectionResult, GodotSceneIntelligenceSupport,
    GodotSceneLimits, GodotSceneModel, GodotSceneNode, GodotSceneNodeTree,
    GodotSceneTreeNode, GodotSignalConnection, GodotTextDiagnostic,
    GodotTextDocument, GodotTextDocumentKind, GodotVariantValue,
    HeaderAttribute, ResPathResolution, ResourceReference, SceneReference,
    SourceRange, SubResourceRef, VariantParseResult, build_scene_node_tree,
    is_balanced_text, is_comment_line, is_godot_uid, nodes_in_group,
    parse_godot_resource, parse_godot_scene, parse_godot_variant,
    parse_header_attributes, parse_quoted_string, resolve_res_path,
    scan_balanced, split_key_value, split_top_level_arguments,
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
