//! Executor briefing foundation (ADR 0022–0024; Stage 3R R13.4).
//!
//! Derived context only: no capability, security, approval, network, or
//! filesystem surface. Documentation selection, scopes, packs, briefs,
//! and acceptance evaluation never grant authority; acceptance is
//! satisfied only by host-observed evidence.

pub mod acceptance;
pub mod brief;
pub mod context;
pub mod contracts;
pub mod milestone;
pub mod new_files;
pub mod scope;

pub use acceptance::{
    AcceptanceEvaluationInput, AcceptanceEvaluator, AcceptanceTaskIdentity,
    MilestoneAcceptanceCounts, MilestoneAcceptanceReport,
    MilestoneRequirementResult,
};
pub use brief::{
    CompileExecutorBriefInput, EXECUTOR_BRIEF_SCHEMA_VERSION, ExecutorBrief,
    ExecutorBriefLimits, compile_executor_brief,
    compute_executor_brief_fingerprint, render_executor_brief,
    render_executor_brief_bounded, sanitize_secrets_only,
    summarize_executor_brief,
};
pub use context::{
    AcceptanceRequirementPackRef, ActiveWorkingSetRef,
    ArchitectureContextEntry, ArchitectureContextRef,
    BuildExecutorContextPackInput, CapabilityAreasSnapshot, CapabilityRef,
    DocumentationEntry, DocumentationKind, DocumentationSelection,
    DocumentationStatus, ExecutorContextPack, ExecutorContextPackLimits,
    InstructionLite, InstructionRef, NewFileRef, ScopeSignalRef,
    SelectDocumentationContextInput, TaskPlanRef, TaskPlanRefId,
    TouchpointRef, WorkspaceScopeRef, build_executor_context_pack,
    select_architecture_context, select_documentation_context,
};
pub use contracts::{
    CreateExecutionContractInput, ExecutionContract, ExecutionContractRef,
    ExecutionRule, ExecutionRuleKind, ReportingRequirement,
    ReviseExecutionContractInput, ValidationProfileRef,
    compute_execution_contract_digest, create_execution_contract,
    execution_contract_ref, revise_execution_contract,
    validate_execution_contract,
};
pub use milestone::{
    AcceptanceRequirement, AcceptanceRequirementInput,
    CreateMilestoneManifestInput, MilestoneDeliverable, MilestoneInvariant,
    MilestoneManifest, MilestoneManifestLimits, MilestoneManifestRef,
    MilestoneRef, MilestoneRequirement, StandardAcceptanceDefinition,
    StandardAcceptanceId, TestRequirement, compute_milestone_manifest_digest,
    create_milestone_manifest, resolve_acceptance_evidence_kinds,
    revise_milestone_manifest,
};
pub use new_files::{
    DetectProliferationSignalsInput, EvaluateScopeDiffInput,
    NewFileDisciplineLimits, NewFileRationale, NewProductionFile,
    ProliferationHeuristics, ProliferationSignal, ScopeDiffClassification,
    ScopeDiffEntry, ScopeDiffReport, create_new_file_rationale,
    detect_proliferation_signals, evaluate_scope_diff, path_matches_pattern,
};
pub use scope::{
    ActiveFile, ActiveFileInput, ActiveWorkingSet, ActiveWorkingSetLimits,
    CreateActiveWorkingSetInput, CreateWorkspaceScopeInput,
    DEFAULT_SOURCE_EXCLUSIONS, DEFAULT_WORKSPACE_CONTEXT_BUDGET,
    EvictLowValueContextInput, EvictionRecord, FileInclusionReason,
    PromotionRequest, ScopePromotionRecord, SourceFileConfidence,
    SourceFileRef, SourceView, WorkspaceContextBudget, WorkspaceScope,
    WorkspaceScopeLimits, add_candidate_file, create_active_working_set,
    create_workspace_scope, evict_low_value_context, is_excluded_source_path,
    promote_candidate_file, set_file_view,
};
