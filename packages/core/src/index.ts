export type { ConversationItem } from "./domain/conversation.js";
export { validateConversationItems } from "./domain/conversation.js";
export { isCancellationError } from "./domain/cancellation.js";
export type { JsonObject, JsonPrimitive, JsonValue } from "./domain/json.js";
export type { ModelEvent, ModelProvider, ModelRequest } from "./ports/provider.js";
export {
  createToolRegistry,
  type RegisteredToolInfo,
  type ToolRegistry,
} from "./tools/tool-registry.js";
export type {
  Tool,
  ToolDefinition,
  ToolExecutionContext,
  ToolExecutionResult,
} from "./tools/tool.js";
export {
  createPreparedMutation,
  isPreparedMutationTool,
  isPreparedCommandTool,
  toolCapability,
  type PreparedMutation,
  type PreparedMutationTool,
  type RegisteredTool,
  type ToolPreparationResult,
} from "./tools/prepared-mutation-tool.js";
export {
  isPreparedProbeTool,
  type GodotProbeToolPreparationResult,
  type PreparedProjectProbeTool,
} from "./tools/prepared-probe-tool.js";
export { PROCESS_RUN_TOOL_NAME } from "./commands/command-tool.js";
export type {
  CommandToolExecutionContext,
  CommandToolPreparationResult,
  PreparedCommandTool,
  ProcessRunInput,
} from "./commands/command-tool.js";
export type { CommandAuditRecord, CommandApplicationEvent } from "./commands/command-events.js";
export { MAX_RETAINED_COMMAND_AUDIT_RECORDS } from "./commands/command-events.js";
export type { CommandDigestParts, CommandDigestService } from "./commands/command-digest.js";
export { canonicalizeCommandDigest } from "./commands/command-digest.js";
export { COMMAND_LIMITS } from "./commands/command-limits.js";
export {
  createCommandRunnerRegistry,
  type CommandRunnerRegistry,
} from "./commands/command-registry.js";
export {
  createPreparedCommand,
  type CommandExecutionContext,
  type CommandExecutionRequest,
  type CommandExecutionRequestResult,
  type CommandPreparationContext,
  type CommandPreparationResult,
  type CommandPreview,
  type CommandRunner,
  type CommandRunnerDefinition,
  type CommandRunPaths,
  type PreparedCommand,
} from "./commands/command-runners.js";
export {
  createSiralosApplication,
  DEFAULT_MAX_TOOL_ROUNDS,
  PROVIDER_TURN_LIMITS,
  type ApplicationEvent,
  type SessionStatus,
  type SiralosApplication,
  type SiralosApplicationDependencies,
} from "./application/application.js";
export {
  createScriptedProvider,
  toolCall,
  type ScriptedProvider,
} from "./application/test-support.js";
export { canonicalizeJson, sha256Hex } from "./godot/digest.js";
export type { Capability, CapabilityPolicy, PermissionRule } from "./security/capability.js";
export { createDefaultPolicy } from "./security/default-policy.js";
export type {
  GitBranchStatus,
  GitChangeEntry,
  GitConflictEntry,
  GitDiffFileSummary,
  GitDiffResult,
  GitDiffScope,
  GitFileStatus,
  GitStatusKind,
  GitStatusResult,
  GitWorkspaceStatus,
} from "./git/git-models.js";
export { GitError, describeGitError, type GitErrorCode } from "./git/git-errors.js";
export type { GitInspector, GitStatusRequest, GitDiffRequest } from "./git/git-inspector.js";
export type {
  AppliedCheckpointResult,
  CheckpointFileState,
  CheckpointListQuery,
  CheckpointOperation,
  CheckpointPreimage,
  CheckpointState,
  CheckpointTerminalState,
  FileCheckpoint,
  PreparedCheckpoint,
} from "./checkpoints/checkpoint-model.js";
export type { CheckpointStore } from "./checkpoints/checkpoint-store.js";
export {
  planUndo,
  type UndoPlanDecision,
  type WorkspaceFileState,
} from "./checkpoints/undo-plan.js";
export type { UndoOutcome, UndoService } from "./checkpoints/undo-service.js";
export {
  isPreparedDiagnosticTool,
  type GodotDiagnosticToolPreparationResult,
  type PreparedDiagnosticTool,
} from "./tools/prepared-diagnostic-tool.js";
export {
  isPreparedLSPSessionTool,
  type GDScriptLSPSessionToolPreparationResult,
  type PreparedLSPSessionTool,
} from "./tools/prepared-lsp-session-tool.js";
export type { ApprovalDecision, ApprovalRequest, ApprovalReviewer } from "./security/approval.js";
export { DEFAULT_MAX_PENDING_APPROVAL_MS } from "./security/approval.js";
export type {
  GodotDiagnosticApprovalRequest,
  GodotLSPSessionApprovalRequest,
  GodotProjectProbeApprovalRequest,
  ProcessExecutionApprovalRequest,
  TaskPlanApprovalRequest,
  WorkspaceWriteApprovalRequest,
} from "./security/approval.js";
export type {
  ChangePreview,
  FileChangeOperation,
  FileChangePreview,
} from "./security/change-preview.js";
export {
  DEVELOP_OFFLINE_PROFILE,
  GODOT_DIAGNOSTICS_OFFLINE_PROFILE,
  GODOT_LSP_LOCAL_PROFILE,
  GODOT_PROBE_OFFLINE_PROFILE,
  GODOT_RECOVERY_PROBE_OFFLINE_PROFILE,
  INSPECT_PROFILE,
  VALIDATION_OFFLINE_PROFILE,
  getBuiltInProfile,
  type SandboxProfile,
  type SandboxProfileId,
  type WorkspaceAccess,
} from "./security/profile.js";
export type { PermissionEvaluation } from "./security/permission-evaluator.js";
export { evaluatePermission } from "./security/permission-evaluator.js";
export type {
  SandboxBackend,
  SandboxBackendStatus,
  SandboxedProcessRequest,
  SandboxedProcessResult,
  SandboxedProcessStatus,
  SandboxViolation,
  ProcessOutputEvent,
} from "./security/sandbox-backend.js";
export {
  normalizeSandboxError,
  SandboxError,
  type SandboxErrorCode,
} from "./security/sandbox-error.js";
export type { SandboxEvent } from "./security/sandbox-events.js";
export {
  createSiralosSecurity,
  type SiralosSecurity,
  type SiralosSecurityDependencies,
} from "./security/sandbox-service.js";
export { GODOT_LIMITS } from "./godot/limits.js";
export type { SafeDiagnostic } from "./godot/diagnostics.js";
export {
  classifyGodotReleaseChannel,
  parseDeclaredVersion,
  type GodotDeclaredVersion,
  type GodotReleaseChannel,
  type GodotVersion,
  type GodotVersionStatus,
} from "./godot/version.js";
export {
  FORBIDDEN_GODOT_PROJECT_ARGUMENTS,
  GODOT_KNOWN_OPTIONS,
  createEmptyGodotCommandCapabilities,
  type GodotCommandCapabilities,
} from "./godot/capabilities.js";
export type {
  GodotEditionHint,
  GodotInstallation,
  GodotInstallationSource,
} from "./godot/installations.js";
export {
  classifyGodotEdition,
  classifyGodotSupport,
  isEditorSelectionCandidate,
  type GodotEdition,
  type GodotEditionClassification,
  type GodotEditionConfidence,
  type GodotEditionEvidence,
  type GodotEngineProfile,
  type GodotSupportClassificationInput,
  type SiralosGodotSupport,
} from "./godot/engine-profile.js";
export type {
  GodotApiDumpProbe,
  GodotApiDumpSummary,
  GodotHelpProbe,
  GodotProbeRunner,
  GodotVersionProbe,
} from "./godot/probes.js";
export {
  createEmptyGodotExecutableContentInventory,
  createEmptyGodotProjectProfile,
  type GodotAutoloadSummary,
  type GodotExecutableContentInventory,
  type GodotGDExtensionSummary,
  type GodotLanguageProfile,
  type GodotPluginSummary,
  type GodotProjectProfile,
  type GodotScanTruncationReason,
} from "./godot/project.js";
export {
  assessGodotCompatibility,
  type GodotCompatibilityAssessment,
  type GodotCompatibilityStatus,
} from "./godot/compatibility.js";
export {
  GODOT_SELECTION_RANKS,
  rankCandidate,
  rankGodotCandidates,
  type GodotRankedCandidate,
  type GodotSelectionOutcome,
  type GodotSelectionPreference,
} from "./godot/selection.js";
export type {
  GodotDiscoveryResult,
  GodotDoctorReport,
  GodotInstallationOverview,
  GodotInspector,
  GodotSelectedInstallation,
  GodotStatusSnapshot,
} from "./godot/inspector.js";
export type { GodotApplicationEvent } from "./godot/events.js";
export {
  computeGodotPreparedProbeDigest,
  computeGodotRiskManifestDigest,
  createPreparedGodotProbe,
  type GodotAutoloadRiskEntry,
  type GodotDiagnostic,
  type GodotFileRiskEntry,
  type GodotGDExtensionRiskEntry,
  type GodotImportState,
  type GodotLibraryRiskEntry,
  type GodotPluginRiskEntry,
  type GodotPreparedProbeDigestParts,
  type GodotProbeExecutionContext,
  type GodotProbePreparationResult,
  type GodotProbePreview,
  type GodotProbeStatus,
  type GodotProjectProbe,
  type GodotProjectProbeStatus,
  type GodotProjectRiskManifest,
  type GodotProjectTrustState,
  type GodotRecoveryProbeResult,
  type GodotRecoveryProbeSupport,
  type PreparedGodotProbe,
} from "./godot/probe.js";
export type {
  PreparedProjectMirror,
  ProjectMirror,
  ProjectMirrorFileEntry,
  ProjectMirrorPreparationResult,
  ProjectMirrorRequest,
  ProjectMirrorVerification,
} from "./godot/mirror.js";
export type {
  GodotApiIndex,
  GodotApiLookupResult,
  GodotApiParameter,
  GodotApiSearchOutcome,
  GodotApiSearchQuery,
  GodotApiSearchRank,
  GodotApiSearchResult,
  GodotApiSearchKind,
  GodotApiSymbol,
  GodotApiSymbolDetails,
  GodotApiSymbolKind,
  GodotApiType,
} from "./godot/api.js";
export { godotSymbolId } from "./godot/api.js";
export {
  KNOWLEDGE_SCHEMA_VERSION,
  classifyGodotManualChannel,
  computeGodotKnowledgeProfileDigest,
  validateGodotKnowledgeCache,
  type GodotKnowledge,
  type GodotKnowledgeBase,
  type GodotKnowledgeCacheValidation,
  type GodotKnowledgeLookupResult,
  type GodotKnowledgeProfileV1,
  type GodotKnowledgeQueryResult,
  type GodotKnowledgeRefreshResult,
  type GodotKnowledgeState,
  type GodotKnowledgeStatus,
  type GodotKnowledgeSupport,
} from "./godot/knowledge.js";
export {
  aggregateGDScriptDiagnostics,
  computeGodotCheckOnlyCommandDigest,
  computeGodotPreparedCheckDigest,
  createPreparedGDScriptCheck,
  type GDScriptCheckResult,
  type GodotCheckOnlyCommandDigestParts,
  type GodotCheckPreparationResult,
  type GodotDiagnostics,
  type GodotDiagnosticsExecutionContext,
  type GodotDiagnosticsRequest,
  type GodotDiagnosticsState,
  type GodotDiagnosticsStatus,
  type GodotDiagnosticsSupport,
  type GodotDiagnosticPreview,
  type GodotGDScriptDiagnostic,
  type GodotPreparedCheckDigestParts,
  type GodotProjectCheckResult,
  type GodotScriptCheckTarget,
  type PreparedGDScriptCheck,
} from "./godot/gdscript.js";
export {
  computeGDScriptPreparedSessionDigest,
  createPreparedGDScriptSession,
  EMPTY_GDScript_LSP_CAPABILITIES,
  type GDScriptCompletionItem,
  type GDScriptCompletionResult,
  type GDScriptDefinitionLocation,
  type GDScriptDefinitionResult,
  type GDScriptDiagnosticResult,
  type GDScriptDocumentRequest,
  type GDScriptHoverResult,
  type GDScriptHoverSection,
  type GDScriptLanguageService,
  type GDScriptLanguageSession,
  type GDScriptLanguageSupport,
  type GDScriptLSPCapabilities,
  type GDScriptLSPSessionPreview,
  type GDScriptPosition,
  type GDScriptPositionRequest,
  type GDScriptPreparedSessionDigestParts,
  type GDScriptQueryOutcome,
  type GDScriptSessionPreparationResult,
  type GDScriptSessionStartContext,
  type GDScriptSessionStartResult,
  type GDScriptSessionState,
  type GDScriptSessionStatus,
  type GDScriptSourceRange,
  type GodotSelectedEngine,
  type LanguageSessionEvent,
  type PreparedGDScriptSession,
} from "./godot/lsp.js";
export {
  DEVELOPMENT_LIMITS,
  computeGDScriptDevelopmentDigest,
  type DevelopmentChangeRecord,
  type DevelopmentEvent,
  type DevelopmentEvidence,
  type DevelopmentPhase,
  type DevelopmentState,
  type DevelopmentStatus,
  type DevelopmentValidationStatus,
  type GDScriptDevelopmentDigestParts,
  type GDScriptDevelopmentPreview,
  type GDScriptDevelopmentResult,
  type GDScriptDevelopmentSession,
  type GDScriptDevelopmentStatus,
} from "./godot/development/development-model.js";
export {
  computeChangeSetDigest,
  countChangeSetResultBytes,
  validateChangeSetRequest,
  type ChangeSetOperation,
  type ChangeSetReplacement,
  type ChangeSetRequest,
  type PreparedChangeSetDigestParts,
  type PreparedChangeSetFile,
} from "./godot/development/development-change-set.js";
export {
  classifyDevelopmentSurface,
  classifyDevelopmentSurfacePath,
  type DevelopmentSurfaceDecision,
  type DevelopmentSurfaceInput,
  type DevelopmentSurfaceKind,
  type DevelopmentSurfaceTouchpoint,
} from "./godot/development/development-surface.js";
export {
  approveUnifiedTarget,
  computeTextTargetDigest,
  computeUnifiedChangeSetDigest,
  createUnifiedChangeSet,
  unifiedChangeSetReadyToApply,
  unifiedPreStateMap,
  type CreateUnifiedChangeSetInput,
  type UnifiedChangeSet,
  type UnifiedChangeSetTargetEntry,
  type UnifiedTarget,
  type UnifiedTargetApprovalState,
} from "./godot/development/unified-change-set.js";
export {
  deriveUnifiedApplyOrder,
  deriveUnifiedOrderEdges,
  type UnifiedApplyOrder,
  type UnifiedOrderEdge,
  type UnifiedOrderTarget,
} from "./godot/development/unified-order.js";
export {
  verifyCrossSurfaceConsistency,
  type ConsistencyCheck,
  type ConsistencyCheckStatus,
  type CrossSurfaceConsistencyInput,
  type CrossSurfaceConsistencyResult,
} from "./godot/development/cross-surface-consistency.js";
export {
  blockedReasonText,
  createBlockedDisposition,
  type BlockedDisposition,
  type BlockedReasonKind,
} from "./godot/development/blocked-disposition.js";
export {
  type ChangeSetApplyFileRequest,
  type ChangeSetApplyOutcome,
  type ChangeSetApplyRequest,
  type ChangeSetFilePrimitives,
  type DevelopmentChangeSetApplier,
} from "./godot/development/development-change-set-apply.js";
export {
  type DevelopmentCancelResult,
  type DevelopmentChangeSetApplicationResult,
  type DevelopmentChangeSetExecutionContext,
  type DevelopmentChangeSetPreparationResult,
  type DevelopmentStartPreparationResult,
  type DevelopmentStartResult,
  type DevelopmentSupport,
  type GDScriptDevelopmentService,
} from "./godot/development/development-service.js";
export {
  QUALITY_LIMITS,
  computeQualityReportStatus,
  createQualityGateResult,
  gateClassification,
  type ChangeDiffMetrics,
  type DevelopmentQualityReport,
  type IndependentReviewResult,
  type QualityEvent,
  type QualityEvidence,
  type QualityGateClassification,
  type QualityGateId,
  type QualityGateResult,
  type QualityGateStatus,
  type QualityStatus,
} from "./godot/quality/quality-model.js";
export {
  computeWarningDelta,
  diagnosticIdentityKey,
  normalizeDiagnosticMessage,
  type WarningClassification,
  type WarningDeltaEntry,
  type WarningDeltaSummary,
} from "./godot/quality/quality-warnings.js";
export {
  analyzeConventions,
  extractAddedLines,
  type ConventionAnalysisOptions,
  type ConventionBasis,
  type ConventionChangeInput,
  type ConventionFinding,
  type ConventionRule,
} from "./godot/quality/quality-conventions.js";
export {
  classifyValidationGate,
  discoverValidationPlan,
  type DevelopmentValidationPlan,
  type QualityValidationExecutor,
  type ValidationPlanDiscovery,
  type ValidationRunOutcome,
  type ValidationRunStatus,
  type ValidationStep,
  type ValidationStepKind,
} from "./godot/quality/quality-validation.js";
export {
  aggregateReviewResults,
  chunkChangeReviewRequests,
  classifyReviewFindingBlocking,
  countBlockingFindings,
  countReviewFindingsBySeverity,
  deduplicateReviewFindings,
  deterministicFindingId,
  normalizeReviewFindings,
  type ChangeReviewConfidence,
  type ChangeReviewFile,
  type ChangeReviewFinding,
  type ChangeReviewFindingCategory,
  type ChangeReviewFindingSeverity,
  type ChangeReviewRequest,
  type ChangeReviewResult,
  type ChangeReviewResultStatus,
  type ChangeReviewer,
} from "./godot/quality/quality-review.js";
export {
  WORKSPACE_REVISION_HANDLE_PATTERN,
  type StaleRevisionError,
} from "./godot/development/development-change-set.js";
export {
  createAdHocTaskContract,
  createTaskContract,
  validateTaskContract,
  reviseTaskContract,
  computeTaskContractDigest,
  TASK_CONTRACT_LIMITS,
  type AcceptanceCriterion,
  type AcceptanceCriterionId,
  type CreateTaskContractInput,
  type PausePolicy,
  type ReviseTaskContractInput,
  type TaskConstraint,
  type TaskConstraintId,
  type TaskConstraintKind,
  type TaskContract,
  type TaskContractId,
  type VerificationKind,
} from "./tasks/task-contract.js";
export {
  isTerminalPhase,
  type AcceptanceState,
  type AcceptanceStatus,
  type EvidenceKind,
  type EvidenceRecord,
  type EvidenceRef,
  type EvidenceSource,
  type EvidenceVerification,
  type EvidenceVerificationOutcome,
  type FindingRef,
  type FindingSeverity,
  type ProgressState,
  type ProgressStateValue,
  type MilestoneEvidenceTarget,
  type TaskId,
  type TaskPhase,
  type TaskReviewStatus,
  type TaskState,
  type TaskStepId,
  type TaskStepKind,
  type TaskStepSpec,
  type TaskStepState,
  type TaskStepStatus,
  type TaskValidationStatus,
  type WorkflowDisposition,
} from "./tasks/task-model.js";
export {
  TASK_ACTIVITY_EVENT_KEYS,
  TASK_ACTIVITY_EVENT_TYPES,
  type TaskActivityEvent,
} from "./tasks/task-events.js";
export {
  TASK_RUNTIME_VERSION,
  capabilityPolicyFingerprint,
  createTaskRuntimeSnapshot,
  type TaskRuntimeSnapshot,
  type TaskRuntimeSnapshotProviderIdentity,
  type TaskRuntimeSnapshotSources,
  type TaskRuntimeSnapshotWorkflowIdentity,
} from "./tasks/task-snapshot.js";
export {
  MAX_EVIDENCE_SOURCE_BYTES,
  MAX_TASK_EVIDENCE_RECORDS,
  MAX_TASK_EVIDENCE_ID_BYTES,
  MAX_TASK_FINDINGS,
  MAX_TASK_FINDING_FIELD_BYTES,
  MAX_TASK_STEPS,
  MAX_TASK_STEP_DESCRIPTION_BYTES,
  PROGRESS_DEGRADED_REPETITIONS,
  PROGRESS_STALLED_REPETITIONS,
  PROGRESS_WINDOW_SIZE,
  createTaskRuntime,
  type CompletionEvaluation,
  type CompletionResult,
  type CreateTaskInput,
  type CriterionResult,
  type DispositionResult,
  type EvidenceAttachResult,
  type HostObservation,
  type StepOpResult,
  type TaskHandle,
  type TaskRuntime,
  type TaskRuntimeOptions,
} from "./tasks/task-runtime.js";
export {
  DEVELOPMENT_WORKFLOW_ID,
  DEVELOPMENT_WORKFLOW_VERSION,
  createDevelopmentAcceptanceCriteria,
  createDevelopmentTaskContract,
  createDevelopmentTaskFlow,
  createDevelopmentTaskSteps,
  type DevelopmentTaskFlow,
  type DevelopmentTaskFlowOptions,
} from "./tasks/task-development.js";
export {
  NO_TASK_PLAN,
  PLANNING_LIMITS,
  PLAN_CONSTRAINT_ID_PATTERN,
  PLAN_ID_PATTERN,
  PLAN_REVISION_HANDLE_PATTERN,
  PLAN_RISK_ID_PATTERN,
  PLAN_STEP_ID_PATTERN,
  PLAN_TOUCHPOINT_ID_PATTERN,
  computePlanRevisionDigest,
  createTaskPlan,
  hasMeaningfulAcceptanceCriteria,
  reviseTaskPlan,
  summarizePlan,
  type CreateTaskPlanInput,
  type PlanApproval,
  type PlanConstraint,
  type PlanRisk,
  type PlanRiskSeverity,
  type PlanRollbackStrategy,
  type PlanScope,
  type PlanStep,
  type PlanTouchpoint,
  type PlanValidationStrategy,
  type PlanningDepth,
  type ReviseTaskPlanInput,
  type TaskPlan,
  type TaskPlanContent,
  type TaskPlanId,
  type TaskPlanState,
  type TouchpointConfidence,
} from "./planning/planning-model.js";
export {
  containsGodotSceneOrResourceReference,
  containsProtectedConfigReference,
  createPlanningPolicy,
  type PlanningDecision,
  type PlanningDecisionInput,
  type PlanningDecisionReason,
  type PlanningPolicy,
} from "./planning/planning-policy.js";
export {
  extractPlanCandidateJson,
  isSafePlanPath,
  planTouchpointStaleness,
  rejectPlanPolicyClaims,
  validatePlanCandidate,
  type PlanCandidateContext,
  type PlanCandidateResult,
} from "./planning/planning-validation.js";
export {
  createPlanningFlow,
  type PlanFlowResult,
  type PlannerOutcome,
  type PlannerPort,
  type PlannerRequest,
  type PlanningFlow,
  type PlanningFlowOptions,
} from "./planning/planning-flow.js";
export {
  DEFAULT_EXECUTION_CONTRACT,
  EXECUTION_CONTRACT_LIMITS,
  computeExecutionContractDigest,
  createExecutionContract,
  executionContractRef,
  reviseExecutionContract,
  validateExecutionContract,
  type CreateExecutionContractInput,
  type ExecutionContract,
  type ExecutionContractId,
  type ExecutionContractRef,
  type ExecutionRule,
  type ExecutionRuleId,
  type ExecutionRuleKind,
  type ReportingRequirement,
  type ReviseExecutionContractInput,
} from "./executor/execution-contract.js";
export {
  MILESTONE_MANIFEST_LIMITS,
  computeMilestoneManifestDigest,
  createMilestoneManifest,
  milestoneManifestRef,
  reviseMilestoneManifest,
  validateMilestoneManifest,
  type AcceptanceRequirement,
  type AcceptanceRequirementInput,
  type ArchitectureConcern,
  type CreateMilestoneManifestInput,
  type MilestoneDeliverable,
  type MilestoneId,
  type MilestoneInvariant,
  type MilestoneManifest,
  type MilestoneManifestRef,
  type MilestoneRef,
  type MilestoneRequirement,
  type ReviseMilestoneManifestInput,
  type TestRequirement,
} from "./executor/milestone-manifest.js";
export { S3M8_MILESTONE_MANIFEST } from "./executor/s3m8-manifest.js";
export { S3M9_MILESTONE_MANIFEST } from "./executor/s3m9-manifest.js";
export { S3M10_MILESTONE_MANIFEST } from "./executor/s3m10-manifest.js";
export { S3M11_MILESTONE_MANIFEST } from "./executor/s3m11-manifest.js";
export { S3R2_MILESTONE_MANIFEST } from "./executor/s3r2-manifest.js";
export {
  abbreviateDigest,
  abbreviateHexDigest,
  ARTIFACT_DIGEST_ALGORITHM,
  canonicalArtifactPayload,
  computeArtifactDigest,
  computeArtifactDigestHex,
  digestReference,
  sameArtifactDigest,
  validateArtifactDigest,
  type ArtifactDigest,
} from "./identity/artifact-digest.js";
export {
  canonicalValuesEqual,
  computeItemListDelta,
  computeSectionDelta,
  digestItemList,
  type SemanticDelta,
} from "./identity/semantic-delta.js";
export {
  computeTaskContractArtifactDigest,
  computeTaskContractContentDigest,
  computeTaskContractDelta,
  computeTaskPlanArtifactDigest,
  computeTaskPlanContentDigest,
  computeTaskPlanDelta,
  taskContractContentPayload,
  taskPlanContentPayload,
  type TaskContractChangeSection,
  type TaskContractDelta,
  type TaskPlanChangeSection,
  type TaskPlanDelta,
} from "./identity/contract-plan-identity.js";
export {
  computeAcceptanceCriteriaDigest,
  computeCapabilitySnapshotDigest,
  computeExecutionInputDelta,
  computeGuidanceDelta,
  computeToolSurfaceDelta,
  computeValidationDelta,
  computeValidationEvidenceDigest,
  canonicalChangesetIdentity,
  createAcceptanceEvidenceManifest,
  createExecutionInputManifest,
  createGuidanceManifest,
  createReviewInputManifest,
  createToolSurfaceManifest,
  createValidationResultIdentity,
  type AcceptanceEvidenceManifest,
  type ExecutionInputDelta,
  type ExecutionInputManifest,
  type ExecutionInputReference,
  type GuidanceDelta,
  type GuidanceManifest,
  type GuidanceManifestEntry,
  type ReviewInputManifest,
  type ToolSurfaceDelta,
  type ToolSurfaceEntry,
  type ToolSurfaceManifest,
  type ToolSurfacePhase,
  type ToolSurfaceRole,
  type ValidationDelta,
  type ValidationResultIdentity,
} from "./identity/manifests.js";
export {
  deriveIdentityStaleness,
  type IdentityStaleness,
  type IdentityStalenessInput,
} from "./identity/staleness.js";
export {
  createFixedClock,
  createOrderingPolicy,
  createSeededRandomSource,
  createSystemClock,
  createSystemRandomSource,
  normalizeKeyedResults,
  type Clock,
  type OrderingPolicy,
  type RandomSource,
} from "./determinism/context.js";
export {
  computeEnvironmentDelta,
  createEnvironmentManifest,
  type EnvironmentDelta,
  type EnvironmentManifest,
  type EnvironmentManifestInput,
} from "./determinism/environment.js";
export {
  computeProviderInputIdentityDigest,
  computeReproducibilityDelta,
  createReproducibilityManifest,
  type ProviderInputIdentity,
  type ReproducibilityDelta,
  type ReproducibilityManifest,
  type ReproducibilityManifestInput,
  type ReproducibilitySection,
} from "./determinism/reproducibility.js";
export {
  classifyRetry,
  deriveValidationPlan,
  evaluateAcceptance,
  normalizeConcurrentResults,
  DEFAULT_RETRY_POLICY,
  type AcceptanceInput,
  type AcceptanceOutcome,
  type AcceptanceResult,
  type RetryCategory,
  type RetryDecision,
  type RetryPolicy,
  type ValidationItem,
  type ValidationPlan,
  type ValidationPlanInput,
  type ValidationRequirementClass,
} from "./determinism/decisions.js";
export {
  discoverRepository,
  listOwnership,
  OWNERSHIP_INDEX,
  resolveOwner,
  type DiscoveryCandidate,
  type DiscoveryInput,
  type DiscoveryRelevance,
  type DiscoveryResult,
  type OwnershipEntry,
} from "./determinism/discovery.js";
export {
  CONTEXT_CLASSES,
  CONTEXT_CLASS_ARTIFACT_KINDS,
  PHASE_CONTRACTS,
  classArtifactKinds,
  contextClassesForPhase,
  createPhaseContract,
  validateAuthorityProfile,
  type ContextClass,
  type PhaseAuthorityProfile,
  type PhaseContract,
  type PhaseContractId,
  type PhaseInputRequirement,
  type PhaseOperation,
  type PhaseOutputRequirement,
  type PhaseVerificationRequirement,
} from "./context/phase-contract.js";
export {
  buildDependencyManifest,
  computeArtifactLineage,
  createArtifactDependencyManifest,
  createWorkflowArtifactIdentity,
  HIGH_VALUE_DEPENDENCIES,
  renderArtifactIdentity,
  renderLineage,
  type ArtifactDependency,
  type ArtifactDependencyManifest,
  type LineageLink,
  type WorkflowArtifactIdentity,
} from "./context/artifacts.js";
export {
  computeStalenessDigest,
  deriveArtifactStaleness,
  isPreparedMutationStale,
  type ArtifactStalenessInput,
  type ArtifactStalenessResult,
} from "./context/staleness.js";
export {
  computeProvenanceDigest,
  createContextProvenanceRef,
  renderWhyAcceptanceFailed,
  renderWhyBlocked,
  renderWhyStale,
  renderWhyValidationRequired,
  whyValidationRequired,
  type ContextProvenanceRef,
  type ProvenanceSourceKind,
  type WhyAcceptanceFailed,
  type WhyBlocked,
  type WhyStale,
  type WhyValidationRequired,
} from "./context/provenance.js";
export {
  accumulateCorrectionPattern,
  computeSourceProblemCandidateDigest,
  createSourceProblemCandidate,
  recordCorrectionPattern,
  renderSourceProblemCandidate,
  type CorrectionPattern,
  type CorrectionPatternKind,
  type SourceProblemCandidate,
  type SourceProblemClass,
} from "./context/source-integrity.js";
export {
  phaseRequiresRepositoryWideContext,
  projectPhaseContext,
  toolSurfaceForPhase,
  type PhaseContextSources,
} from "./context/projection.js";
export {
  createOperationId,
  createRunId,
  createRunTraceRef,
  formatRunTraceRef,
  type OperationId,
  type PhaseId,
  type RunId,
  type RunTraceRef,
} from "./runtime/identity.js";
export {
  RUNTIME_MODES,
  createRunManifest,
  evaluateRuntimeModeCapability,
  renderRunManifest,
  runIdForManifest,
  type RunManifest,
  type RunManifestInput,
  type RuntimeCapabilityState,
  type RuntimeMode,
  type RuntimeModeCapabilityInput,
} from "./runtime/modes.js";
export {
  authorizesSourceMutation,
  cleanupScopeForRun,
  createRunFilesystemBoundary,
  createRuntimeSideEffectPolicy,
  isPathWithinRunRoot,
  resolveRunOwnedPath,
  type RunFilesystemBoundary,
  type RunOwnedPathKind,
  type RuntimeNetworkPolicy,
  type RuntimeSideEffectPolicy,
} from "./runtime/side-effects.js";
export {
  DEFAULT_RUNTIME_ARTIFACT_BUDGET,
  createRuntimeArtifactRef,
  createRuntimeArtifactStore,
  enforceArtifactBudget,
  planRunCleanup,
  projectRuntimeArtifactsForContext,
  renderCleanupOutcome,
  type ArtifactAdmission,
  type ArtifactBudget,
  type ArtifactBudgetState,
  type CleanupOutcome,
  type CleanupStatus,
  type RetentionClass,
  type RuntimeArtifactKind,
  type RuntimeArtifactRef,
  type RuntimeArtifactStore,
} from "./runtime/artifacts.js";
export {
  INITIAL_SUPERVISOR_STATE,
  RUNTIME_FAILURE_KINDS,
  createRunActivityLog,
  createRunOutcome,
  digestRunActivity,
  isSupervisorTerminal,
  renderRunOutcome,
  transitionSupervisor,
  type RunActivityEvent,
  type RunOutcome,
  type RunTerminalStatus,
  type RuntimeFailureKind,
  type RunTiming,
  type ResourceSummary,
  type SupervisorObservation,
  type SupervisorState,
  type SupervisorStateView,
} from "./runtime/supervision.js";
export {
  DEFAULT_RUNTIME_BUDGET,
  classifyIncompleteRun,
  createRuntimeBudget,
  finalizeCancellation,
  renderCancellationState,
  renderIncompleteRunClassification,
  renderRuntimeBudget,
  requestCancellation,
  type CancellationPhase,
  type CancellationState,
  type IncompleteRunClassification,
  type IncompleteRunRecord,
  type RuntimeBudget,
  type RuntimeBudgetCapabilities,
} from "./runtime/budget.js";
export {
  executionAllowed,
  evaluateRuntimeReadiness,
  renderRuntimeReadiness,
  type ReadinessItem,
  type ReadinessItemId,
  type RuntimeReadinessInput,
  type RuntimeReadinessManifest,
} from "./runtime/readiness.js";
export {
  FAULT_SCRIPTS,
  createFakeProcessDriver,
  expectedFailureKind,
  listFaultScripts,
  type FaultScript,
  type FakeProcessDriver,
} from "./runtime/faults.js";
export {
  buildRuntimeReadinessDiagnostic,
  type RuntimeReadinessDiagnosticResult,
} from "./runtime/doctor.js";
export {
  STANDARD_ACCEPTANCE_DEFINITIONS,
  STANDARD_ACCEPTANCE_IDS,
  resolveAcceptanceEvidenceKinds,
  type StandardAcceptanceDefinition,
  type StandardAcceptanceId,
} from "./executor/standard-acceptance.js";
export {
  createAcceptanceEvaluator,
  type AcceptanceEvaluationInput,
  type AcceptanceEvaluator,
  type AcceptanceRequirementStatus,
  type MilestoneAcceptanceCounts,
  type MilestoneAcceptanceReport,
  type MilestoneRequirementResult,
} from "./executor/acceptance.js";
export {
  STANDARD_REPO_VALIDATION,
  VALIDATION_PROFILE_LIMITS,
  createValidationProfile,
  summarizeValidationProfile,
  validateValidationProfile,
  type ValidationCheckRef,
  type ValidationProfile,
  type ValidationProfileRef,
} from "./executor/validation-profile.js";
export {
  ARCHITECTURE_INDEX,
  selectArchitectureContext,
  type ArchitectureContextEntry,
  type ArchitectureContextRef,
  type SelectArchitectureContextInput,
} from "./executor/architecture-context.js";
export {
  ADR_DOCUMENTATION_ENTRIES,
  ADR_METADATA_LIMITS,
  ARCHIVE_DOCUMENTATION_PREFIX,
  DOCUMENTATION_BUDGET,
  DOCUMENTATION_INDEX,
  isArchivedDocumentationPath,
  parseAdrFrontmatter,
  selectDocumentationContext,
  validateAdrMetadata,
  type AdrMetadata,
  type DocumentationEntry,
  type DocumentationKind,
  type DocumentationSelection,
  type DocumentationStatus,
  type SelectDocumentationContextInput,
} from "./executor/documentation-context.js";
export {
  ACTIVE_WORKING_SET_LIMITS,
  DEFAULT_SOURCE_EXCLUSIONS,
  DEFAULT_WORKSPACE_CONTEXT_BUDGET,
  WORKSPACE_SCOPE_LIMITS,
  addCandidateFile,
  addVerifiedFile,
  createActiveWorkingSet,
  createWorkspaceScope,
  evictLowValueContext,
  isExcludedSourcePath,
  promoteCandidateFile,
  setFileView,
  type ActiveFile,
  type ActiveWorkingSet,
  type EvictionRecord,
  type FileInclusionReason,
  type ScopePromotionRecord,
  type SourceFileConfidence,
  type SourceFileRef,
  type SourceView,
  type WorkspaceContextBudget,
  type WorkspaceScope,
} from "./executor/workspace-scope.js";
export {
  NEW_FILE_DISCIPLINE_LIMITS,
  PROLIFERATION_HEURISTICS,
  createNewFileRationale,
  detectProliferationSignals,
  evaluateScopeDiff,
  pathMatchesPattern,
  type NewFileRationale,
  type ProliferationSignal,
  type ScopeDiffClassification,
  type ScopeDiffEntry,
  type ScopeDiffReport,
} from "./executor/new-file-discipline.js";
export {
  EXECUTOR_CONTEXT_PACK_LIMITS,
  buildExecutorContextPack,
  type AcceptanceRequirementRef,
  type ActiveWorkingSetRef,
  type BuildExecutorContextPackInput,
  type CapabilityRef,
  type ExecutorContextPack,
  type InstructionRef,
  type NewFileRef,
  type ScopeSignalRef,
  type TaskContractRef,
  type TaskPlanRef,
  type TouchpointRef,
  type WorkspaceScopeRef,
} from "./executor/context-pack.js";
export {
  EXECUTOR_BRIEF_LIMITS,
  EXECUTOR_BRIEF_SCHEMA_VERSION,
  compileExecutorBrief,
  computeExecutorBriefFingerprint,
  renderExecutorBrief,
  summarizeExecutorBrief,
  type CompileExecutorBriefInput,
  type ExecutorBrief,
} from "./executor/brief-compiler.js";
export {
  createExecutorBriefing,
  type ExecutorBriefing,
  type ExecutorBriefingOptions,
} from "./executor/briefing-service.js";
export {
  DEFAULT_CONTEXT_MAX_OUTPUT_TOKENS,
  DEFAULT_CONTEXT_WORKING_MAXIMUM,
  createRouteContextCapacity,
  type ContextCapacity,
} from "./projection/context-capacity.js";
export {
  DEFAULT_CONTEXT_PRESSURE_LIMITS,
  classifyPressure,
  type ContextPressure,
  type ContextPressureLimits,
  type ContextPressureState,
} from "./projection/context-pressure.js";
export {
  estimateConversationItemTokens,
  estimateJsonTokens,
  estimateTokens,
  type TokenEstimate,
} from "./projection/context-estimator.js";
export {
  SIRALOS_SYSTEM_INSTRUCTIONS,
  createContextProjector,
  serializeContextPrefix,
  serializeSegments,
  type ContextProjection,
  type ContextProjectionInput,
  type ContextProjector,
  type ContextSegment,
  type ContextSegmentInput,
  type ContextStability,
} from "./projection/context-projector.js";
export {
  createToolProjector,
  type ProjectedTool,
  type ProjectionMode,
  type ToolProjection,
  type ToolProjectionInput,
  type ToolProjector,
  type ToolProjectorOptions,
  type ToolVisibility,
} from "./projection/tool-projector.js";
export {
  DEFAULT_EVIDENCE_MAX_LINE_BYTES,
  DEFAULT_EVIDENCE_MAX_TOTAL_BYTES,
  boundLineLength,
  collapseRepeatedLines,
  createEvidenceProjector,
  redactSecrets,
  stripAnsiAndControl,
  truncateText,
  type EvidenceProjectionOptions,
  type EvidenceProjector,
  type ModelEvidenceView,
} from "./projection/evidence-projector.js";
export {
  createWatermarkCache,
  type WatermarkCache,
  type WatermarkCacheEntry,
  type WatermarkCacheOptions,
} from "./projection/watermark-cache.js";
export {
  createRevisionGuard,
  awaitCurrent,
  type RevisionBound,
  type RevisionGuard,
} from "./projection/stale-result.js";
export {
  estimateConversationTokens,
  trimConversationPreservingPairs,
  type ConversationTrimResult,
} from "./projection/conversation-trim.js";
export {
  createProjectionService,
  type ProjectedRequest,
  type ProjectionService,
  type ProjectionServiceOptions,
} from "./projection/projection-service.js";

export {
  DEFAULT_REVISION_REGISTRY_LIMIT,
  WORKSPACE_REVISION_HANDLE_PREFIX,
  computeWorkspaceRevisionHandle,
  createWorkspaceRevisionRegistry,
  type ObservedWorkspaceRead,
  type WorkspaceRevisionHandle,
  type WorkspaceRevisionIdentity,
  type WorkspaceRevisionRegistry,
  type WorkspaceRevisionRegistryOptions,
} from "./workspace/workspace-revision.js";
export {
  INSTRUCTION_ID_PREFIX,
  INSTRUCTION_PRECEDENCE,
  MAX_INSTRUCTION_CONTENT_BYTES,
  computeInstructionId,
  computeResolvedInstructionSetRevision,
  createProjectInstruction,
  describeInstructionScope,
  instructionAppliesTo,
  instructionPriority,
  normalizeInstructionContent,
  renderResolvedInstructions,
  type InstructionConflict,
  type InstructionScope,
  type InstructionSource,
  type InstructionSourceKind,
  type ProjectInstruction,
  type ResolvedInstructionSet,
} from "./instructions/instruction-model.js";
export {
  buildInstruction,
  compareInstructions,
  computeInstructionInventoryRevision,
  detectConflicts,
  resolveInstructionSet,
  resolveInstructionsForPath,
  type ResolveInstructionsInput,
} from "./instructions/instruction-resolver.js";
export {
  type InstructionDiscoveryOutcome,
  type ProjectInstructionService,
} from "./instructions/instruction-service.js";
export {
  BEHAVIORAL_CONFIG_DIRECTORY,
  BEHAVIORAL_INSTRUCTION_FILE,
  classifyBehavioralConfigPaths,
  isProtectedBehavioralConfigPath,
} from "./security/behavioral-config.js";

export {
  KNOWLEDGE_LIMITS,
  KNOWLEDGE_RETRIEVAL_SCORING,
  KNOWLEDGE_STATE_VERSION,
  SUBJECT_KEY_PATTERN,
  computeKnowledgeFactId,
  computeKnowledgeStateRevision,
  freshnessScore,
  isValidSubjectKey,
  normalizeFactContent,
  rejectPolicyShapedContent,
  tokenizeFactText,
  type KnowledgeActivation,
  type KnowledgeCandidate,
  type KnowledgeConfidence,
  type KnowledgeFactId,
  type KnowledgeFactType,
  type KnowledgeProvenanceRef,
  type KnowledgeRetrievalQuery,
  type KnowledgeRetrievalResult,
  type KnowledgeRetrievalSelection,
  type KnowledgeRetrievalTrace,
  type KnowledgeScope,
  type KnowledgeVolatility,
  type ProjectKnowledgeFact,
} from "./knowledge/knowledge-model.js";
export {
  createKnowledgeCoordinator,
  type KnowledgeCoordinator,
  type KnowledgeCoordinatorOptions,
  type KnowledgeProposalResult,
} from "./knowledge/knowledge-coordinator.js";
export {
  buildGodotProjectKnowledgeCandidates,
  type GodotProjectKnowledgeSeed,
} from "./knowledge/knowledge-seeding.js";
export {
  KNOWLEDGE_FRAMING_LINE,
  renderPinnedKnowledge,
  renderRetrievedKnowledge,
} from "./knowledge/knowledge-projection.js";

export {
  WORKSPACE_READ_MODES,
  isWorkspaceReadMode,
  type WorkspaceReadMode,
} from "./workspace/workspace-read-mode.js";
export {
  GDSCRIPT_STRUCTURE_LIMITS,
  extractGDScriptStructure,
  type GDScriptAnnotationInfo,
  type GDScriptConstantInfo,
  type GDScriptEnumInfo,
  type GDScriptFunctionInfo,
  type GDScriptParameter,
  type GDScriptParserError,
  type GDScriptPropertyInfo,
  type GDScriptSignalInfo,
  type GDScriptStructure,
} from "./workspace/gdscript-structure.js";
export {
  DEFAULT_SUMMARY_MAX_BYTES,
  DEFAULT_SUMMARY_NOTABLE_METHODS,
  buildWorkspaceSummary,
  type WorkspaceSummaryOptions,
  type WorkspaceSummaryResult,
} from "./workspace/workspace-summary.js";

// --- Stage 3 milestone 5: references (external read-only sources) ---

export {
  REFERENCE_ID_PREFIX,
  REFERENCE_LIMITS,
  createReferenceId,
  formatReferenceAlias,
  referenceDisplayName,
  referenceIdOf,
  validateReferenceAlias,
  type MaterializationStatus,
  type Reference,
  type ReferenceAlias,
  type ReferenceId,
  type ReferenceKind,
  type ReferenceLimits,
  type ReferenceRevision,
  type ReferenceSource,
  type ReferenceStatus,
  type ReferenceTaskBinding,
  type ReferenceTrustClass,
  type RepositoryRef,
  type ResolvedReferenceIdentity,
} from "./reference/reference-model.js";
export {
  isAbsolutePath,
  normalizeRepositoryOrigin,
  parseReferenceDeclaration,
  parseReferenceDeclarationsSection,
  type ReferenceDeclaration,
} from "./reference/reference-declaration.js";
export type {
  MaterializationOutcome,
  ReferenceAccessPort,
  ReferenceListRequest,
  ReferenceListResult,
  ReferenceMaterializerPort,
  ReferenceReadRequest,
  ReferenceReadResult,
  ReferenceResolutionOutcome,
  ReferenceResolverPort,
  ReferenceSearchRequest,
  ReferenceSearchResult,
} from "./reference/reference-ports.js";
export {
  createReferenceRegistry,
  isPathWithin,
  type ReferenceRefreshResult,
  type ReferenceRegistry,
  type ReferenceRegistryOptions,
} from "./reference/reference-registry.js";
export {
  DEFAULT_REFERENCE_VIEW_MAX_BYTES,
  formatReferenceEvidenceLine,
  formatReferenceEvidenceView,
  referenceIdentityAnchor,
  type ReferenceEvidenceView,
  type ReferenceObservation,
} from "./reference/reference-evidence.js";

// --- Stage 3 milestone 5: research (bounded external fetches) ---

export {
  RESEARCH_LIMITS,
  computeResearchDocumentId,
  computeResearchDocumentContentDigest,
  defaultResearchBounds,
  isResearchSourceKind,
  isValidResearchSourceRef,
  validateResearchRequest,
  type ResearchBounds,
  type ResearchContentType,
  type ResearchDocument,
  type ResearchDocumentId,
  type ResearchLink,
  type ResearchOutcome,
  type ResearchProvenance,
  type ResearchRequest,
  type ResearchSection,
  type ResearchSourceKind,
  type ResearchSourceRef,
} from "./research/research-model.js";
export type {
  ResearchSourcePort,
  ResearchTransportPort,
  TransportOutcome,
} from "./research/research-ports.js";
export {
  DEFAULT_RESEARCH_VIEW_MAX_BYTES,
  createResearchService,
  formatResearchEvidenceView,
  type ResearchEvidence,
  type ResearchFetchResult,
  type ResearchTaskBinding,
  type ResearchService,
  type ResearchServiceOptions,
} from "./research/research-service.js";

// --- Stage 3 milestone 5: projection integration ---

export {
  MAX_RESEARCH_EVIDENCE_VIEWS,
  MAX_REFERENCE_EVIDENCE_VIEWS,
  REFERENCE_RESEARCH_VOLATILE_BUDGET_BYTES,
} from "./projection/projection-service.js";

// --- Stage 3 milestone 6: self-reference (installed runtime surface) ---

export {
  COMMAND_CATALOG,
  COMMAND_CATALOG_IDS,
  COMMAND_CATALOG_REVISION,
  catalogEntry,
  type CommandCatalog,
  type CommandCatalogEntry,
  type CommandCatalogGroup,
  type CommandId,
} from "./commands/command-catalog.js";
export {
  CONFIG_SCHEMA_REVISION,
  CONFIG_SCHEMA_SUMMARY,
  type ConfigSchemaKey,
  type ConfigSchemaSection,
} from "./self/config-schema-summary.js";
export {
  SELF_REFERENCE_NAME,
  computeSelfReferenceRevision,
  createSelfReference,
  toolAbiRevision,
  type SelfReference,
  type SelfReferenceInput,
  type SelfReferenceLine,
  type SelfReferencePort,
  type SelfReferenceSearchMatch,
  type SelfReferenceSection,
  type SelfReferenceSectionId,
  type SiralosRuntimeIdentity,
} from "./self/self-reference.js";

// --- Stage 3 milestone 6: capability doctor (read-only diagnostics) ---

export {
  DOCTOR_AREAS,
  DOCTOR_EXIT_FAILURES,
  DOCTOR_EXIT_INVOCATION,
  DOCTOR_EXIT_OK,
  DOCTOR_SCHEMA_VERSION,
  DoctorInvocationError,
  countDoctorReport,
  doctorExitCodeFor,
  normalizeDoctorRequest,
  type CapabilitySnapshot,
  type CapabilityState,
  type DoctorArea,
  type DoctorCheckResult,
  type DoctorDetail,
  type DoctorRemediation,
  type DoctorReport,
  type DoctorReportCounts,
  type DoctorRequest,
  type DoctorStatus,
  type GodotCapabilityStatus,
  type ProviderCapabilityStatus,
  type ReferenceCapabilityStatus,
  type ResearchCapabilityStatus,
  type SandboxCapabilityStatus,
  type ToolCapabilityStatus,
  type WorkspaceCapabilityStatus,
} from "./doctor/doctor-model.js";
export type {
  CapabilityDiagnosticResult,
  ConfigurationDiagnosticResult,
  CredentialRefStatus,
  DoctorSources,
  GodotDiagnosticResult,
  GodotVersionMatchStatus,
  ProviderDiagnosticResult,
  ProviderEndpointStatus,
  ProviderModelStatus,
  ProjectedToolStatus,
  ReferenceDiagnosticResult,
  ReferenceEntryStatus,
  ResearchDiagnosticResult,
  RuntimeDiagnosticResult,
  SandboxDiagnosticResult,
  TaskSnapshotDiagnosticResult,
  TaskSnapshotDifference,
  WorkspaceDiagnosticResult,
} from "./doctor/doctor-ports.js";
export {
  buildCapabilitySnapshot,
  type CapabilitySnapshotInput,
} from "./doctor/capability-state.js";
export {
  DEFAULT_DOCTOR_CHECK_TIMEOUT_MS,
  DoctorCancelledError,
  DoctorTimeoutError,
  createCapabilityDoctor,
  type CapabilityDoctor,
  type CapabilityDoctorOptions,
} from "./doctor/capability-doctor.js";
export {
  sanitizeSafeDoctorText,
  toSafeReport,
  type SafeDoctorCheck,
  type SafeDoctorErrorCategory,
  type SafeDoctorReport,
} from "./doctor/safe-report.js";
// Stage 3 milestone 8: read-only Godot scene and resource intelligence.
export { GODOT_SCENE_LIMITS } from "./godot/scene/limits.js";
export { parseGodotScene, type ParseSceneOptions } from "./godot/scene/scene-parser.js";
export {
  REVIEW_CONTEXT_LIMITS,
  validateReviewContextManifest,
  type ImpactConfidence,
  type ImpactDiagnostic,
  type ImpactRegressionArea,
  type ImpactRelation,
  type ImpactRelationKind,
  type ImpactSurface,
  type ImpactSurfaceKind,
  type ImpactValidationRecommendation,
  type ImpactCompleteness,
  type ReviewContextManifest,
  type ValidationKind,
  type ValidationPriority,
} from "./godot/impact/review-context.js";
export {
  analyzeImpact,
  type AnalyzeImpactInput,
  type ImpactEdge,
  type ImpactRelationshipSource,
  type ImpactSignalConnection,
} from "./godot/impact/impact-analyzer.js";
export {
  MUTATION_LIMITS,
  expectedSemanticEffect,
  validateMutationOperation,
  validateMutationOperations,
  validateNodePath,
  type MutationOperation,
  type MutationValue,
  type ResourceMutationOperation,
  type SceneMutationOperation,
  type SemanticExpectation,
} from "./godot/scene-mutation/operations.js";
export {
  applyResourceOperations,
  applySceneOperations,
  buildNodePathIndex,
  findNodeByPath,
  isDescendantOf,
  type NodePathIndex,
} from "./godot/scene-mutation/model-apply.js";
export {
  serializeResource,
  serializeScene,
  serializeVariantValue,
} from "./godot/scene-mutation/serializer.js";
export {
  verifyResourceSemanticEffect,
  verifySceneSemanticEffect,
  type SemanticVerification,
  type VerificationCheck,
  type VerificationStatus,
} from "./godot/scene-mutation/verify.js";
export {
  computeMutationFingerprint,
  createPreparedGodotMutation,
  type CreatePreparedGodotMutationInput,
  type GodotMutationPreview,
  type PreparedGodotMutation,
} from "./godot/scene-mutation/prepared.js";
export { parseGodotResource, type ParseResourceOptions } from "./godot/scene/resource-parser.js";
export {
  parseGodotVariant,
  parseQuotedString,
  splitTopLevelArguments,
  type VariantParseResult,
} from "./godot/scene/variant.js";
export {
  buildSceneNodeTree,
  nodesInGroup,
  renderSceneTreeText,
  type GodotSceneNodeTree,
  type GodotSceneTreeNode,
} from "./godot/scene/scene-tree.js";
export {
  isBalancedText,
  parseHeaderAttributes,
  scanBalanced,
  splitKeyValue,
  type BalancedScan,
  type HeaderAttribute,
} from "./godot/scene/text.js";
export { nodePaths } from "./godot/scene/scene-parser.js";
export {
  createGodotRelationshipIndex,
  type GodotRelationshipEntry,
  type GodotRelationshipIndex,
  type GodotRelationshipKind,
} from "./godot/scene/relationship-index.js";
export { isGodotUid, resolveResPath, type ResPathResolution } from "./godot/scene/resolution.js";
export type {
  ExternalResourceRef,
  GodotDiagnosticCode,
  GodotDiagnosticSeverity,
  GodotParseStatus,
  GodotProperty,
  GodotRawValue,
  GodotResourceModel,
  GodotSceneModel,
  GodotSceneNode,
  GodotSignalConnection,
  GodotTextDiagnostic,
  GodotTextDocument,
  GodotVariantValue,
  ResourceReference,
  SceneReference,
  SourceRange,
  SubResourceRef,
} from "./godot/scene/models.js";
export type {
  GodotAutoload,
  GodotDependencyEdge,
  GodotDependencyResult,
  GodotImpactRequest,
  GodotImpactResult,
  GodotInspectionOutcome,
  GodotInputAction,
  GodotIntelligenceStatus,
  GodotMainSceneReference,
  GodotProjectRelationshipResult,
  GodotResourceInspectionResult,
  GodotSceneEvidenceView,
  GodotSceneInspectionResult,
  GodotSceneIntelligence,
  GodotSceneIntelligenceSupport,
} from "./godot/scene/intelligence.js";
export {
  normalizeDefinitionLocations,
  type DefinitionLimits,
  type DefinitionLocation,
} from "./language/definition.js";
export {
  normalizeDiagnosticPayload,
  normalizeDiagnosticSet,
  mapLspSeverity,
  type DiagnosticPayloadLimits,
  type DiagnosticSeverity,
  type LanguageDiagnostic,
} from "./language/diagnostic.js";
export {
  isOneBasedPosition,
  isOrderedRange,
  toOneBasedPosition,
  toOneBasedRange,
  type LanguagePosition,
  type LanguageRange,
} from "./language/position.js";
export { sanitizeControlCharacters } from "./language/sanitize.js";
export { truncateUtf8Bytes, utf8ByteLength } from "./language/truncate.js";
