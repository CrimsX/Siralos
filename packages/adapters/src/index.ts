export {
  createDeterministicFakeProvider,
  DETERMINISTIC_FAKE_PROVIDER_ID,
} from "./providers/deterministic-fake-provider.js";
export {
  loadUserConfig,
  parseUserConfig,
  getDefaultUserConfigPath,
  DEFAULT_USER_CONFIG,
  type UserConfig,
  type UserSandboxBackendId,
  type UserSandboxProfileId,
} from "./config/user-config.js";
export {
  INSTRUCTION_DISCOVERY_LIMITS,
  createProjectInstructionService,
  discoverProjectInstructions,
  type ProjectInstructionDiscoveryOptions,
} from "./instructions/instruction-discovery.js";
export {
  createAnthropicSandboxRuntimeBackend,
  ANTHROPIC_SANDBOX_RUNTIME_BACKEND_ID,
  ANTHROPIC_SANDBOX_RUNTIME_VERSION,
  type AnthropicSandboxRuntimeBackendOptions,
} from "./sandbox/anthropic-runtime/anthropic-sandbox-runtime-backend.js";
export { getSandboxDirectories } from "./sandbox/sandbox-directories.js";
export {
  runSandboxConformance,
  removeConformanceArtifacts,
  type ConformanceOptions,
  type ConformanceProbeResult,
  type ConformanceReport,
} from "./sandbox/conformance/run-conformance.js";
export {
  createFakeSandboxBackend,
  completedResult,
  type FakeSandboxBackendOptions,
} from "./sandbox/fake-sandbox-backend.js";
export {
  buildChildEnvironment,
  isDeniedVariable,
  isProtectedEnvironmentKey,
  PROTECTED_ENVIRONMENT_KEYS,
  readParentEnvironment,
  type SandboxEnvironmentPaths,
} from "./environment/child-environment.js";
export {
  buildCommandEnvironment,
  type CommandEnvironmentOptions,
  type CommandEnvironmentPaths,
} from "./environment/command-environment.js";
export {
  createProcessRunTool,
  type ProcessRunToolDependencies,
} from "./process/process-run-tool.js";
export {
  createRunDirectoryProvider,
  type RunCleanupOutcome,
  type RunDirectoryProvider,
  type RunDirectoryProviderOptions,
} from "./process/run-directories.js";
export {
  createNpmScriptRunner,
  type NpmScriptRunnerOptions,
} from "./process/runners/npm-script-runner.js";
export {
  createNodeScriptRunner,
  type NodeScriptRunnerOptions,
} from "./process/runners/node-script-runner.js";
export { createSha256CommandDigestService } from "./process/command-digest.js";
export { resolveNpmCli, type NpmCliResolution } from "./process/trusted-executables.js";
export { createWorkspaceListTool } from "./tools/workspace/workspace-list-tool.js";
export { createWorkspaceReadTool } from "./tools/workspace/workspace-read-tool.js";
export { createWorkspaceSearchTool } from "./tools/workspace/workspace-search-tool.js";
export { createWorkspaceCreateFileTool } from "./tools/workspace/mutations/workspace-create-file-tool.js";
export { createWorkspaceEditFileTool } from "./tools/workspace/mutations/workspace-edit-file-tool.js";
export { createWorkspaceDeleteFileTool } from "./tools/workspace/mutations/workspace-delete-file-tool.js";
export { createWorkspaceApplyTextChangesetTool } from "./tools/workspace/mutations/workspace-apply-text-changeset-tool.js";
export { createMutationLock } from "./tools/workspace/mutations/mutation-lock.js";
export {
  CheckpointStorageLimitError,
  createFilesystemCheckpointStore,
  DEFAULT_CHECKPOINT_ROOT,
  DEFAULT_MAX_CHECKPOINTS,
  DEFAULT_MAX_CHECKPOINT_STORAGE_BYTES,
  DEFAULT_MAX_PREIMAGE_BYTES,
  MAX_SUPPORTED_PREIMAGE_BYTES,
  type FilesystemCheckpointStoreOptions,
} from "./checkpoints/filesystem/checkpoint-store.js";
export {
  reconcileWorkspaceCheckpoints,
  type ReconciliationOptions,
  type ReconciliationReport,
} from "./checkpoints/filesystem/reconciliation.js";
export {
  createUndoService,
  type UndoServiceDependencies,
} from "./checkpoints/filesystem/undo-service.js";
export { resolveWorkspaceRoot } from "./tools/workspace/workspace-path.js";
export { createGitCliAdapter, type GitCliAdapterOptions } from "./git/cli/git-cli-adapter.js";
export { createGitStatusTool } from "./git/tools/git-status-tool.js";
export { createGitDiffTool } from "./git/tools/git-diff-tool.js";
export { createGodotInspector, type GodotInspectorDependencies } from "./godot/godot-inspector.js";
export { createGodotProbeRunner } from "./godot/process/godot-probe-runner.js";
export {
  createGodotProjectProbeService,
  GODOT_MIRROR_COPY_POLICY_VERSION,
  GODOT_RECOVERY_EXECUTION_UNAVAILABLE_MESSAGE,
  type GodotProjectProbeServiceDependencies,
} from "./godot/probe/godot-project-probe-service.js";
export {
  scanAuthoredFiles,
  computeAuthoredFileDigest,
  type AuthoredFileEntry,
  type AuthoredFileManifest,
  type ScanAuthoredFilesOptions,
} from "./godot/probe/authored-files.js";
export {
  classifyRecoveryDiagnostics,
  classifyDiagnosticLine,
  sanitizeDiagnosticText,
  emptyRecoveryDiagnosticSummary,
  type RecoveryDiagnosticLimits,
  type RecoveryDiagnosticSummary,
} from "./godot/probe/recovery-diagnostics.js";
export {
  canonicalizeGitStatus,
  compareWorkspaceIntegrity,
  snapshotWorkspaceIntegrity,
  type WorkspaceIntegrityComparison,
  type WorkspaceIntegritySnapshot,
} from "./godot/probe/workspace-integrity.js";
export {
  createPreparedProbeStore,
  type PreparedProbePlan,
  type PreparedProbeStoreConfig,
} from "./godot/probe/prepared-probe-store.js";
export { createGodotProbeProjectTool } from "./godot/tools/godot-probe-project-tool.js";
export {
  validateExecutable,
  revalidateExecutableIdentity,
  hashFile,
  type ExecutableIdentity,
  type ExecutableValidationResult,
  type ValidateExecutableOptions,
} from "./godot/discovery/executable-validation.js";
export {
  installationFromIdentity,
  invalidInstallation,
  discoverOnPath,
  type PathDiscoveryOptions,
} from "./godot/discovery/path-discovery.js";
export { createEngineProfileCache } from "./godot/cache/engine-profile-cache.js";
export { createGodotInspectEngineTool } from "./godot/tools/godot-inspect-engine-tool.js";
export { createGodotInspectProjectTool } from "./godot/tools/godot-inspect-project-tool.js";
export { createGodotApiSearchTool } from "./godot/tools/godot-api-search-tool.js";
export { createGodotApiLookupTool } from "./godot/tools/godot-api-lookup-tool.js";
export { createGodotCheckScriptTool } from "./godot/tools/godot-check-script-tool.js";
export { createGodotCheckProjectScriptsTool } from "./godot/tools/godot-check-project-scripts-tool.js";
export { createGodotKnowledgeCache } from "./godot/knowledge/knowledge-cache.js";
export { createGodotKnowledgeService } from "./godot/knowledge/godot-knowledge-service.js";
export { createGodotDiagnosticsService } from "./godot/diagnostics/godot-diagnostics-service.js";
export {
  createGodotCheckOnlyRunner,
  GODOT_CHECK_ONLY_BASE_ARGUMENTS,
  GODOT_CHECK_ONLY_UNAVAILABLE_MESSAGE,
  godotCheckOnlyArguments,
  godotCheckOnlyArgumentTemplate,
  type GodotCheckOnlyRunRequest,
  type GodotCheckOnlyRunOutcome,
} from "./godot/process/godot-check-only-runner.js";
export {
  createGodotKnowledgeRunner,
  GODOT_KNOWLEDGE_BASE_ARGUMENTS,
  GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE,
  godotKnowledgeArguments,
  type GodotKnowledgeRunRequest,
  type GodotKnowledgeRunOutcome,
} from "./godot/process/godot-knowledge-runner.js";
export {
  normalizeGodotCheckOutput,
  type GodotCheckOutputInput,
  type GodotCheckOutputNormalization,
} from "./godot/diagnostics/diagnostic-normalizer.js";
export { createGDScriptLanguageService } from "./godot/lsp/godot-lsp-service.js";
export {
  createLSPPortAllocator,
  type AllocatedLSPPort,
  type LSPPortAllocator,
} from "./godot/lsp/port-allocator.js";
export {
  LSPFrameParser,
  frameMessage,
  type LSPFrameParserOptions,
} from "./godot/lsp/frame-parser.js";
export { JSONRPCClient, ServerRequestRejectedError, JSONRPC_CODES } from "./godot/lsp/json-rpc.js";
export {
  normalizeCompletion,
  normalizeDefinition,
  normalizeHover,
  normalizePublishDiagnostics,
  type LSPNormalizationContext,
} from "./godot/lsp/normalizers.js";
export {
  createGodotLSPServerRunner,
  GODOT_LSP_BASE_ARGUMENTS,
  GODOT_LSP_UNAVAILABLE_MESSAGE,
  godotLSPArguments,
  godotLSPArgumentTemplate,
  type GodotLSPServerStartRequest,
  type GodotLSPServerStartOutcome,
} from "./godot/process/godot-lsp-runner.js";
export { createGodotLSPSessionTool } from "./godot/tools/godot-lsp-session-tool.js";
export { createGodotDevelopmentStatusTool } from "./godot/tools/godot-development-status-tool.js";
export {
  createGDScriptDevelopmentService,
  type GDScriptDevelopmentServiceDependencies,
} from "./godot/development/gdscript-development-service.js";
export {
  runQualityStage,
  type QualityStageChangeFile,
  type QualityStageInput,
  type QualityStageOutput,
  type QualityWarningBaseline,
} from "./godot/quality/quality-stage-runner.js";
export { createValidationPlanDiscovery } from "./godot/quality/validation-plan-discovery.js";
export { createQualityValidationExecutor } from "./godot/quality/quality-validation-executor.js";
export {
  createFakeChangeReviewer,
  type FakeChangeReviewerOptions,
  type FakeReviewerControl,
  type FakeReviewerScenario,
} from "./godot/quality/fake-change-reviewer.js";
export {
  createProviderChangeReviewer,
  type ProviderChangeReviewerOptions,
} from "./godot/quality/provider-change-reviewer.js";
export {
  createReviewerToolRegistry,
  type ReviewerToolDependencies,
} from "./godot/quality/reviewer-tools.js";
export {
  createDevelopmentChangeSetApplier,
  createFailClosedChangeSetFilePrimitives,
  CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE,
  type ChangeSetExecutorDependencies,
} from "./godot/development/change-set-executor.js";
export {
  prepareChangeSet,
  type ChangeSetPreparationResult,
  type ChangeSetPreparationDependencies,
} from "./godot/development/change-set-preparation.js";
export {
  createGodotCompleteTool,
  createGodotDefinitionTool,
  createGodotHoverTool,
  createGodotLSPDiagnosticsTool,
} from "./godot/tools/godot-lsp-query-tools.js";
export { readGodotEnvironmentOverrides } from "./environment/godot-overrides.js";
export {
  resolveGodotSelection,
  type GodotSelectionInput,
  type GodotSelectionResolution,
} from "./godot/config/selection-request.js";
export { GodotSelectionError } from "./godot/errors.js";
export {
  createFakeDiagnosticsService,
  createFakeGitInspector,
  createFakeLanguageService,
  createWorkspaceFilePrimitives,
  sha256Of,
  sha256OfBytes,
  type FakeLanguageControl,
  type FakeLanguageOptions,
  type FakeParserControl,
  type FakeSessionControl,
} from "./godot/development/gdscript-development-testing.js";

// --- Stage 3 milestone 5: reference adapters (read-only external inspection) ---

export {
  assertReferenceRoot,
  isReferenceRootWithin,
  resolveReferencePath,
  type ResolveReferencePathResult,
} from "./reference/reference-path.js";
export {
  createFakeRepositoryBackend,
  createLocalDirectoryResolver,
  createReferenceResolver,
  createRepositoryResolver,
  createUnavailableRepositoryBackend,
  REPOSITORY_RESOLUTION_UNAVAILABLE_MESSAGE,
  type FakeRepositoryFixture,
  type RepositoryRevisionBackend,
} from "./reference/reference-resolver.js";
export {
  createInMemoryCacheStore,
  createReferenceCacheStore,
  REFERENCE_CACHE_UNAVAILABLE_MESSAGE,
  type CacheStatus,
  type CacheStoreOutcome,
  type ReferenceCacheMetadata,
  type ReferenceCacheStore,
} from "./reference/reference-cache.js";
export {
  createReferenceMaterializer,
  createReferenceRootProvider,
  REPOSITORY_MATERIALIZATION_UNAVAILABLE_MESSAGE,
  type ReferenceRoot,
  type RootProvider,
} from "./reference/reference-materializer.js";
export {
  createFakeRepositoryMaterializer,
  type CreateFakeRepositoryMaterializerOptions,
} from "./reference/reference-test-support.js";
export {
  createReferenceAccess,
  REFERENCE_ACCESS_LIMITS,
  type ReferenceAccessLimits,
  type ReferenceAccessOptions,
  type ReferenceInfo,
  type ReferenceInfoProvider,
} from "./reference/reference-access.js";
export {
  createReferenceListTool,
  createReferenceTools,
  mapReferenceFailure,
  referenceRevisionAnchor,
  resolveReferenceSelector,
  type ReferenceTool,
  type ReferenceToolDependencies,
} from "./tools/reference/reference-list-tool.js";
export { createReferenceReadTool } from "./tools/reference/reference-read-tool.js";
export { createReferenceSearchTool } from "./tools/reference/reference-search-tool.js";
export {
  createReferenceServices,
  type ReferenceServices,
  type ReferenceServicesOptions,
} from "./reference/reference-services.js";
