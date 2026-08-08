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
export {
  resolveNpmCli,
  resolveTrustedNode,
  type NpmCliResolution,
  type TrustedNodeIdentity,
} from "./process/trusted-executables.js";
export { createWorkspaceListTool } from "./tools/workspace/workspace-list-tool.js";
export { createWorkspaceReadTool } from "./tools/workspace/workspace-read-tool.js";
export { createWorkspaceSearchTool } from "./tools/workspace/workspace-search-tool.js";
export { createWorkspaceCreateFileTool } from "./tools/workspace/mutations/workspace-create-file-tool.js";
export { createWorkspaceEditFileTool } from "./tools/workspace/mutations/workspace-edit-file-tool.js";
export { createWorkspaceDeleteFileTool } from "./tools/workspace/mutations/workspace-delete-file-tool.js";
export { createMutationLock } from "./tools/workspace/mutations/mutation-lock.js";
export {
  createFilesystemCheckpointStore,
  DEFAULT_CHECKPOINT_ROOT,
  DEFAULT_MAX_CHECKPOINTS,
  DEFAULT_MAX_CHECKPOINT_STORAGE_BYTES,
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
export {
  runGitProcess,
  GIT_ALLOWED_SUBCOMMANDS,
  GIT_SAFETY_ENVIRONMENT,
  type GitProcessOptions,
  type GitProcessResult,
} from "./git/cli/git-process.js";
export { createGodotInspector, type GodotInspectorDependencies } from "./godot/godot-inspector.js";
export {
  createGodotEngineProfiler,
  type GodotEngineProfiler,
  type GodotEngineProfilerDependencies,
  type GodotProfiledCandidate,
} from "./godot/profile/engine-profiler.js";
export { createGodotProbeRunner } from "./godot/process/godot-probe-runner.js";
export { createGodotInspectEngineTool } from "./godot/tools/godot-inspect-engine-tool.js";
export { createGodotInspectProjectTool } from "./godot/tools/godot-inspect-project-tool.js";
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
export {
  createGodotProjectProbeService,
  GODOT_MIRROR_COPY_POLICY_VERSION,
  type GodotProjectProbeServiceDependencies,
} from "./godot/probe/godot-project-probe-service.js";
export {
  classifyRecoveryDiagnostics,
  classifyDiagnosticLine,
  sanitizeDiagnosticText,
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
  computeAuthoredFileDigest,
  scanAuthoredFiles,
  type AuthoredFileEntry,
  type AuthoredFileManifest,
  type ScanAuthoredFilesOptions,
} from "./godot/probe/authored-files.js";
export { readGodotEnvironmentOverrides } from "./environment/godot-overrides.js";
export {
  resolveGodotSelection,
  type GodotSelectionInput,
  type GodotSelectionResolution,
} from "./godot/config/selection-request.js";
export { GodotSelectionError } from "./godot/errors.js";
