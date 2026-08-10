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
  createSolarisApplication,
  DEFAULT_MAX_TOOL_ROUNDS,
  PROVIDER_TURN_LIMITS,
  type ApplicationEvent,
  type SessionStatus,
  type SolarisApplication,
  type SolarisApplicationDependencies,
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
  createSolarisSecurity,
  type SolarisSecurity,
  type SolarisSecurityDependencies,
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
  type SolarisGodotSupport,
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
  reviseTaskContract,
  computeTaskContractDigest,
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
  type FindingRef,
  type FindingSeverity,
  type ProgressState,
  type ProgressStateValue,
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
  SOLARIS_SYSTEM_INSTRUCTIONS,
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
  type ResearchRevisionBound,
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
  type SolarisRuntimeIdentity,
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
