#![allow(clippy::items_after_test_module)]

//! Differential behavioral harness candidate runner (ADR 0033).
//!
//! The checked-in corpus is parsed as a strict, bounded protocol. Each
//! applicable scenario runs against the Rust candidate and produces one
//! canonical outcome record in manifest order.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Component, Path};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

mod tool_loop;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use siralos_adapters::config::MAX_CONFIG_FILE_BYTES;
use siralos_adapters::provider::DeterministicFakeProvider;
use siralos_adapters::tool::WorkspaceReadTool;
use siralos_core::provider::{
    AssistantToolCallInput, CancellationSignal, CancellationToken,
    ConversationItem, LimitClass, ModelProvider, ModelRequest,
    ProtocolFailure, ProviderEvent, ToolDefinition, ToolExecutionResult,
    TurnFailure, TurnOutcome, TurnToolCall, collect_provider_turn,
    detach_bounded_tool_result,
};
use tool_loop::{tool_loop_record, validate_tool_loop_input};

/// Scenario platform value for Windows hosts.
pub const PLATFORM_WINDOWS: &str = "windows";

/// Scenario platform value for POSIX hosts.
pub const PLATFORM_POSIX: &str = "posix";

const SUBJECT_STATE_DIR: &str = "state-dir";
const SUBJECT_VERSION_IDENTITY: &str = "version-identity";
const SUBJECT_TASK_CONTRACT: &str = "task-contract";
const SUBJECT_WORKSPACE_READ: &str = "workspace-read";
const SUBJECT_WORKSPACE_LIST: &str = "workspace-list";
const SUBJECT_WORKSPACE_SEARCH: &str = "workspace-search";
const SUBJECT_WORKSPACE_REVISION: &str = "workspace-revision";
const SUBJECT_WORKSPACE_PREPARE: &str = "workspace-prepare";
const SUBJECT_WORKSPACE_APPLY: &str = "workspace-apply";
const SUBJECT_CHECKPOINT: &str = "checkpoint";
const SUBJECT_GIT_INSPECTION: &str = "git-inspection";
const SUBJECT_LANGUAGE_DIAGNOSTICS: &str = "language-diagnostics";
const SUBJECT_LANGUAGE_STRUCTURE: &str = "language-structure";
const SUBJECT_LANGUAGE_DEFINITION: &str = "language-definition";
const SUBJECT_MODULE_LIFECYCLE: &str = "domain-lifecycle";
const SUBJECT_MODULE_CAPABILITY: &str = "domain-capability";
const SUBJECT_PROVIDER_TURN: &str = "provider-turn";
const SUBJECT_TOOL_LOOP: &str = "tool-loop";
const SUBJECT_CONTEXT_PROJECTION: &str = "context-projection";
const SUBJECT_USER_CONFIG: &str = "user-config";
const SUBJECT_SECURITY_PERMISSIONS: &str = "security-permissions";
const SUBJECT_COMMAND_CATALOG: &str = "command-catalog";
const SUBJECT_CAPABILITY_DOCTOR: &str = "capability-doctor";
const SUBJECT_INSTRUCTIONS_RESOLUTION: &str = "instructions-resolution";
const SUBJECT_KNOWLEDGE_REVISIONS: &str = "knowledge-revisions";
const SUBJECT_REFERENCE_IDENTITY: &str = "reference-identity";
const SUBJECT_RESEARCH_POLICY: &str = "research-policy";
pub(crate) const SUBJECT_PLANNING_RUNTIME: &str = "planning-runtime";
pub(crate) const SUBJECT_EXECUTOR_BRIEF: &str = "executor-brief";
const SUBJECT_GODOT_SCENE_RESOLVE: &str = "godot-scene-resolve";
const SUBJECT_GODOT_DISCOVERY: &str = "godot-discovery";
const SUBJECT_GODOT_KNOWLEDGE: &str = "godot-knowledge";
const SUBJECT_GODOT_DIAGNOSTICS: &str = "godot-diagnostics";
const SUBJECT_GODOT_LSP: &str = "godot-lsp";
const SUBJECT_GODOT_REVIEW_CONTEXT: &str = "godot-review-context";
const SUBJECT_GODOT_MUTATION_PREPARE: &str = "godot-mutation-prepare";
const SUBJECT_GODOT_DEVELOP_PLAN: &str = "godot-develop-plan";
const SUBJECT_GODOT_RUNTIME_LAUNCH: &str = "godot-runtime-launch";
const SUBJECT_GODOT_RUNTIME_EVIDENCE: &str = "godot-runtime-evidence";
/// Fixed task id for review-context records (identical on both sides).
const GODOT_REVIEW_CONTEXT_TASK_ID: &str = "differential-task";
const SUBJECT_CI_ARTIFACT_DIGEST: &str = "content-identity-artifact-digest";
const SUBJECT_CI_CONTRACT_DIGEST: &str = "content-identity-contract-digest";
const SUBJECT_CI_MANIFESTS: &str = "content-identity-manifests";
const SUBJECT_CI_DELTA: &str = "content-identity-delta";
const SUBJECT_DET_REPLAY: &str = "determinism-replay";
const SUBJECT_ICM_PHASE_CONTRACT: &str = "icm.phase-contract";
const SUBJECT_ICM_DEP_MANIFESTS: &str = "icm.dependency-manifests";
const SUBJECT_RECOVERY_TAXONOMY: &str = "recovery-taxonomy";
const SUBJECT_RR_IDENTITY: &str = "runtime-readiness.identity";
const SUBJECT_RR_BUDGETS: &str = "runtime-readiness.budgets";
const SUBJECT_RR_LIFECYCLE: &str = "runtime-readiness.lifecycle";
const SUBJECT_RR_DOCTOR: &str = "runtime-readiness.doctor";
const SUBJECT_RUNTIME_EXECUTION: &str = "runtime-execution";
const SUBJECT_RUNTIME_EVIDENCE: &str = "runtime-evidence";
const SUBJECT_VISUAL_EVIDENCE: &str = "visual-evidence";
const SUBJECT_RUN_INTERACTION: &str = "run-interaction";
const SUBJECT_QA_WORKFLOW: &str = "qa-workflow";
const SUBJECT_RUN_PROFILE: &str = "run-profile";
const SUBJECT_COMPOSITION_PROFILE: &str = "composition-profile";
const SUBJECT_COMPOSITION_EFFECTIVE: &str = "composition-effective";
const SUBJECT_CONTEXT_CONTROLS: &str = "context-controls";
const SUBJECT_COMPOSITION_LOCK: &str = "composition-lock";
const SUBJECT_COMPOSITION_MODULE_SELECTION: &str =
    "composition-plugin-selection";
const SUBJECT_COMPOSITION_SKILLS: &str = "composition-skills";
const SUBJECT_COMPOSITION_MODULE_ACTIVATION: &str =
    "composition-plugin-activation";
const SUBJECT_COMPOSITION_CONTEXT_CONTROL: &str =
    "composition-context-control";
const SUBJECT_COMPOSITION_LOCK_VERIFY: &str = "composition-lock-verify";
const SUBJECT_COMPOSITION_SKILL_CONSUMPTION: &str =
    "composition-skill-consumption";
const SUBJECT_EVOLVE_CORPUS: &str = "evolve-corpus";
const SUBJECT_EVOLVE_WORKFLOW: &str = "evolve-workflow";
const SUBJECT_EVOLVE_PROPOSAL: &str = "evolve-proposal";
const SUBJECT_PROVIDER_GENERIC: &str = "provider-generic";
const SUBJECT_PROVIDER_REPLAY: &str = "provider-replay";
const SUBJECT_SESSION_REPLAY: &str = "session-replay";
const SUBJECT_REPLAY_STORE: &str = "replay-store";
const SUBJECT_CONTEXT_GRAPH: &str = "context-graph";
const SUBJECT_CONTEXT_REPRESENTATION: &str = "context-representation";
const SUBJECT_CONTEXT_SCHEDULER: &str = "context-scheduler";
const SUBJECT_CONTEXT_TOOL: &str = "context-tool";
const SUBJECT_CONTEXT_SCAN: &str = "context-scan";
const SUBJECT_CONTEXT_BENCHMARK: &str = "context-benchmark";
const SUBJECT_CONTEXT_SESSION: &str = "context-session";
/// T4 (decision 108): the pinned TUI render-model subject — four
/// `TestBackend` frame snapshots over the production draw path.
const SUBJECT_TUI_RENDER: &str = "tui-render";
/// Hermetic endpoint pinned by the harness for provider subjects: an
/// unreachable loopback address, so the executed provider call never
/// performs live network I/O and the `reqwest` refusal is deterministic
/// (offline-stable, no real credential is ever transmitted).
const HERMETIC_PROVIDER_ENDPOINT: &str = "http://127.0.0.1:1/invalid";
const SUBJECT_EVOLVE_PACKAGING: &str = "evolve-packaging";
const SUBJECT_CLI_SESSION: &str = "cli-session";
const CORPUS_SCHEMA_VERSION: u64 = 3;
const CORPUS_VERSION: u64 = 83;
const MAX_LANGUAGE_INPUT_BYTES: usize = 64 * 1024;
const MAX_MODULE_INPUT_BYTES: usize = 64 * 1024;
const MAX_PROVIDER_INPUT_BYTES: usize = 64 * 1024;
const MAX_TOOL_LOOP_INPUT_BYTES: usize = 64 * 1024;
const MAX_CONTEXT_PROJECTION_INPUT_BYTES: usize = 64 * 1024;
const MAX_USER_CONFIG_INPUT_BYTES: usize = 64 * 1024;
const MAX_R13_AUTHORITY_INPUT_BYTES: usize = 64 * 1024;
const MAX_R13_GUIDANCE_INPUT_BYTES: usize = 64 * 1024;
const MAX_R13_EXTERNAL_KNOWLEDGE_INPUT_BYTES: usize = 64 * 1024;
const MAX_GODOT_INPUT_BYTES: usize = 64 * 1024;
const MAX_TASK_INPUT_BYTES: usize = 8 * 1024;
const MAX_WORKSPACE_INPUT_BYTES: usize = 64 * 1024;
const MAX_CLI_SESSION_INPUT_BYTES: usize = 64 * 1024;
const RUNNER_PROTOCOL_SCHEMA_VERSION: u64 = 1;
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_SCENARIO_BYTES: usize = 16 * 1024;
const MAX_SCENARIOS: usize = 384;
const MAX_IDENTIFIER_BYTES: usize = 128;
const MAX_FILE_NAME_BYTES: usize = 160;
const MAX_ENV_ENTRIES: usize = 16;
const MAX_ENV_KEY_BYTES: usize = 64;
const MAX_ENV_VALUE_BYTES: usize = 4 * 1024;
const MAX_PROBE_OUTPUT_BYTES: usize = 16 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_POLL_INTERVAL: Duration = Duration::from_millis(5);
const PROBE_ERROR_MARKER: &[u8] = b"ERR";

/// Stable category for an exit-2 harness failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessErrorKind {
    /// Corpus bytes or structure violate the versioned protocol.
    Corpus,
    /// A required repository input could not be read or decoded.
    Input,
    /// The probe process could not be created.
    ProbeSpawn,
    /// The probe exceeded its deterministic deadline.
    ProbeTimeout,
    /// The probe exited unsuccessfully or was terminated.
    ProbeExit,
    /// The probe emitted an invalid or over-limit outcome.
    ProbeProtocol,
}

/// A harness failure, distinct from a behavioral parity deviation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessError {
    kind: HarnessErrorKind,
    code: String,
    detail: String,
}

impl HarnessError {
    /// Stable machine-branchable error category.
    pub fn kind(&self) -> HarnessErrorKind {
        self.kind
    }

    /// Stable code used by the runner-process protocol.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Stable category separating fixture integrity from runner failures.
    pub fn category(&self) -> &'static str {
        match self.kind {
            HarnessErrorKind::Corpus => "CORPUS_INTEGRITY_FAILURE",
            _ => "HARNESS_INTERNAL_FAILURE",
        }
    }

    fn new(kind: HarnessErrorKind, detail: impl Into<String>) -> Self {
        let code = match kind {
            HarnessErrorKind::Corpus => "MALFORMED_CORPUS",
            HarnessErrorKind::Input => "REPOSITORY_INPUT_FAILURE",
            HarnessErrorKind::ProbeSpawn => "PROBE_SPAWN_FAILURE",
            HarnessErrorKind::ProbeTimeout => "PROBE_TIMEOUT",
            HarnessErrorKind::ProbeExit => "PROBE_PROCESS_CRASHED",
            HarnessErrorKind::ProbeProtocol => "PROBE_PROTOCOL_ERROR",
        };
        Self { kind, code: code.to_owned(), detail: detail.into() }
    }

    fn with_code(
        kind: HarnessErrorKind,
        code: &str,
        detail: impl Into<String>,
    ) -> Self {
        Self { kind, code: code.to_owned(), detail: detail.into() }
    }

    pub(crate) fn corpus(detail: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::Corpus, detail)
    }

    fn input(detail: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::Input, detail)
    }
}

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for HarnessError {}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u64,
    #[serde(rename = "corpusVersion")]
    corpus_version: u64,
    #[serde(rename = "corpusSha256")]
    corpus_sha256: Option<String>,
    scenarios: Vec<ManifestEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestEntry {
    file: String,
    sha256: Option<String>,
}

/// One validated corpus scenario (public for the nightly fuzz crate).
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    id: String,
    subject: String,
    platforms: Vec<String>,
    parity: String,
    env: BTreeMap<String, String>,
    /// Subject-specific inputs (task-contract scenarios only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input: Option<Value>,
}

struct LoadedScenario {
    scenario: Scenario,
    applicable: bool,
}

/// Current-host platform value used by scenario applicability.
pub fn platform_name() -> &'static str {
    if cfg!(windows) { PLATFORM_WINDOWS } else { PLATFORM_POSIX }
}

use siralos_adapters::reference::{
    FakeRepositoryBackend, FakeRepositoryFixtureEntry, LocalDirectoryResolver,
    ReferenceMaterializer, RepositoryResolver,
};
use siralos_adapters::research::{
    BuildResearchDocumentOptions, FakeGodotDocsSource,
    FakeRepositoryFileFixture, FakeRepositoryResearchFixture,
    FakeRepositorySource, GodotDocsFallback, GodotDocsFixture,
    GodotDocsPageFixture, TRUNCATION_MARKER, build_research_document,
    classify_content_type, normalize_json_to_sections,
    normalize_markdown_to_sections, normalize_plain_to_sections,
};
use siralos_adapters::workspace::checkpoint::{
    open_checkpoint_store, reconcile_checkpoints,
};
use siralos_adapters::workspace::effects::{
    ApplicationOutcome, MutationTool, PreparationOutcome, apply_mutation,
    prepare_mutation,
};
use siralos_adapters::workspace::git::git_inspection_disposition;
use siralos_adapters::workspace::list::{ListOutcome, list_directory};
use siralos_adapters::workspace::read::{
    ReadInput, ReadMode, ReadOutcome, parse_read_input, read_file,
};
use siralos_adapters::workspace::search::{
    SearchOutcome, parse_search_input, search,
};
use siralos_core::reference::{
    MaterializationOutcome, Reference, ReferenceDeclaration, ReferenceKind,
    ReferenceLimits, ReferenceMaterializerPort, ReferenceRefreshResult,
    ReferenceRegistryOptions, ReferenceResolutionOutcome,
    ReferenceResolverPort, ReferenceSource, RepositoryRef,
    ResolvedReferenceIdentity, TrustForFn, create_reference_id,
    create_reference_registry, is_path_within, normalize_repository_origin,
    parse_reference_declaration, parse_reference_declarations_section,
    validate_reference_alias,
};
use siralos_core::research::{
    ResearchBounds, ResearchContentType, ResearchDocument, ResearchEvidence,
    ResearchFetchResult, ResearchOutcome, ResearchRequest, ResearchService,
    ResearchServiceOptions, ResearchSourceKind, ResearchSourcePort,
    ResearchTaskBinding, compute_research_document_content_digest,
    compute_research_document_id, default_research_bounds,
};
use siralos_core::security::{
    CapabilityPolicy, PermissionDecision, SandboxProfile,
    create_default_policy, evaluate_permission, get_built_in_profile,
};
use siralos_core::task::contract::{
    AcceptanceCriterion, ConstraintKind, CreateTaskContractInput, PausePolicy,
    ReviseContext, ReviseTaskContractInput, TaskConstraint, TaskContract,
    VerificationKind,
};
use siralos_core::task::evidence::FindingInput;
use siralos_core::task::model::{
    ActivityEvent, ApprovalDecision, DispositionSource, EvidenceKind,
    EvidenceSource, EvidenceVerification, FindingSeverity, TaskPhase,
    TaskReviewStatus, TaskStepKind, TaskStepSpec, TaskValidationStatus,
    VerificationOutcome, WorkflowDisposition,
};
use siralos_core::task::progress::HostObservation;
use siralos_core::task::runtime::{
    AttachResult, CompletionResult, CreateTaskInput, CriterionResult,
    StepOpResult, TaskRuntime,
};
use siralos_core::tool::permission::PermissionRule;
use siralos_godot::godot::scene::models::{
    DictionaryEntry, GodotRawValue, GodotVariantValue,
};
use siralos_godot::godot::scene_mutation::{
    MutationOperation, MutationProperty, SemanticExpectation,
};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use siralos_core::workspace::bounds::WORKSPACE_LIMITS;
use siralos_core::workspace::checkpoint::{
    CheckpointState, FileCheckpoint, WorkspaceFileState, plan_undo,
};

use siralos_core::workspace::revision::{
    ObservedReadMode, WorkspaceRevisionRegistry,
    WorkspaceRevisionRegistryOptions, compute_workspace_revision_handle,
};

use siralos_core::workspace::is_protected_behavioral_config_path;

use siralos_adapters::language::uri::map_uri_to_workspace_relative;
use siralos_core::language::definition::{
    DefinitionLimits, RawDefinitionEntry, normalize_definition_locations,
};
use siralos_core::language::diagnostic::{
    Diagnostic, RawDiagnostic, RawDiagnosticCode,
    normalize_diagnostic_payload, normalize_diagnostic_set,
};
use siralos_core::language::limits::LANGUAGE_LIMITS;
use siralos_core::language::position::{RawPosition, RawRange};
use siralos_core::language::structure::{
    StructuralDeclaration, StructuralDocument, StructuralIssue,
    StructuralKind, StructureOptions, StructureStatus, SummaryOptions,
    build_structural_summary, normalize_structural_document,
};
use siralos_core::module::capability::{
    CapabilityRequest, GrantDecision, HostAuthority, decide_grant,
};
use siralos_core::module::failure::ModuleFailure;
use siralos_core::module::lifecycle::{
    ActivationRequest, ModuleLifecycle, RuntimeCheckResult,
    classify_workspace_file, workspace_module_scan,
};
use siralos_core::module::package::{
    ModuleAbi, ModulePackage, PackageDigest, verify_package_digest,
};
use std::path::PathBuf;

mod dispatch;
mod loader;
mod subjects;

// Shared name surface for the split submodules.
#[cfg(test)]
pub(in crate::harness) use self::dispatch::{
    canonical_records_text, cargo_workspace_version, execute_bounded_child,
    state_dir_record,
};
pub use self::dispatch::{probe_state_dir_bytes, run_corpus};
pub use self::loader::fuzz_scenario_bytes;
pub(in crate::harness) use self::loader::*;
pub(in crate::harness) use self::subjects::composition::*;
pub(in crate::harness) use self::subjects::context_session::*;
pub(in crate::harness) use self::subjects::godot::*;
pub(in crate::harness) use self::subjects::godot_records::*;
pub(in crate::harness) use self::subjects::language_domain::*;
pub(in crate::harness) use self::subjects::provider_config::*;
pub(in crate::harness) use self::subjects::provider_turn::*;
pub(in crate::harness) use self::subjects::r13_authority::*;
pub(in crate::harness) use self::subjects::r13_reference::*;
pub(in crate::harness) use self::subjects::runtime_records::*;
pub(in crate::harness) use self::subjects::task_contract::*;
pub(crate) use self::subjects::task_contract::{
    scenario_clock, store_scenario_now,
};
pub(in crate::harness) use self::subjects::workspace_ops::*;

#[cfg(test)]
mod audit_remediation_harness_gate;
#[cfg(test)]
mod godot_tests;
#[cfg(test)]
mod tests;
