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
export type { ApprovalDecision, ApprovalRequest, ApprovalReviewer } from "./security/approval.js";
export { DEFAULT_MAX_PENDING_APPROVAL_MS } from "./security/approval.js";
export type {
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
