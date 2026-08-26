/**
 * Oracle-only re-export shim for the `@siralos/adapters` package
 * specifier (mirrors core-shim.mjs). Each entry aliases a real source
 * module; nothing is reimplemented.
 */
export { createDeterministicFakeProvider } from "../../../packages/adapters/src/providers/deterministic-fake-provider.js";
export { DEFAULT_CHECKPOINT_ROOT } from "../../../packages/adapters/src/checkpoints/filesystem/checkpoint-store.js";
export {
  runSandboxConformance,
  removeConformanceArtifacts,
} from "../../../packages/adapters/src/sandbox/conformance/run-conformance.js";
export { createFakeSandboxBackend } from "../../../packages/adapters/src/sandbox/fake-sandbox-backend.js";
export { createReferenceServices } from "../../../packages/adapters/src/reference/reference-services.js";
export { GodotSelectionError } from "../../../packages/adapters/src/godot/errors.js";
export {
  readConfigurationDiagnostics,
  readConfigurationFileState,
} from "../../../packages/adapters/src/config/config-diagnostics.js";
export { createAnthropicSandboxRuntimeBackend } from "../../../packages/adapters/src/sandbox/anthropic-runtime/anthropic-sandbox-runtime-backend.js";
export {
  getDefaultUserConfigPath,
  loadUserConfig,
} from "../../../packages/adapters/src/config/user-config.js";
export { resolveWorkspaceRoot } from "../../../packages/adapters/src/tools/workspace/workspace-path.js";
export { SELF_REFERENCE_TOOL_METADATA } from "../../../packages/adapters/src/tools/self/self-reference-tools.js";
export { createEngineProfileCache } from "../../../packages/adapters/src/godot/cache/engine-profile-cache.js";
export { createFailClosedChangeSetFilePrimitives } from "../../../packages/adapters/src/godot/development/change-set-executor.js";
export { createFilesystemCheckpointStore } from "../../../packages/adapters/src/checkpoints/filesystem/checkpoint-store.js";
export { createGDScriptDevelopmentService } from "../../../packages/adapters/src/godot/development/gdscript-development-service.js";
export { createGDScriptLanguageService } from "../../../packages/adapters/src/godot/lsp/godot-lsp-service.js";
export { createGitCliAdapter } from "../../../packages/adapters/src/git/cli/git-cli-adapter.js";
export { createGitDiffTool } from "../../../packages/adapters/src/git/tools/git-diff-tool.js";
export { createGitHubResearchSource } from "../../../packages/adapters/src/research/github-source.js";
export { createGitStatusTool } from "../../../packages/adapters/src/git/tools/git-status-tool.js";
export { createGodotApiLookupTool } from "../../../packages/adapters/src/godot/tools/godot-api-lookup-tool.js";
export { createGodotApiSearchTool } from "../../../packages/adapters/src/godot/tools/godot-api-search-tool.js";
export { createGodotCheckProjectScriptsTool } from "../../../packages/adapters/src/godot/tools/godot-check-project-scripts-tool.js";
export { createGodotCheckScriptTool } from "../../../packages/adapters/src/godot/tools/godot-check-script-tool.js";
export { createGodotCompleteTool } from "../../../packages/adapters/src/godot/tools/godot-lsp-query-tools.js";
export { createGodotDefinitionTool } from "../../../packages/adapters/src/godot/tools/godot-lsp-query-tools.js";
export { createGodotDependenciesTool } from "../../../packages/adapters/src/godot/tools/godot-dependencies-tool.js";
export { createGodotDevelopmentStatusTool } from "../../../packages/adapters/src/godot/tools/godot-development-status-tool.js";
export { createGodotDiagnosticsService } from "../../../packages/adapters/src/godot/diagnostics/godot-diagnostics-service.js";
export { createGodotDocsResearchSource } from "../../../packages/adapters/src/research/godot-docs-source.js";
export { createGodotHoverTool } from "../../../packages/adapters/src/godot/tools/godot-lsp-query-tools.js";
export { createGodotInspectEngineTool } from "../../../packages/adapters/src/godot/tools/godot-inspect-engine-tool.js";
export { createGodotInspectProjectTool } from "../../../packages/adapters/src/godot/tools/godot-inspect-project-tool.js";
export { createGodotInspectResourceTool } from "../../../packages/adapters/src/godot/tools/godot-inspect-resource-tool.js";
export { createGodotInspectSceneTool } from "../../../packages/adapters/src/godot/tools/godot-inspect-scene-tool.js";
export { createGodotInspector } from "../../../packages/adapters/src/godot/godot-inspector.js";
export { createGodotKnowledgeCache } from "../../../packages/adapters/src/godot/knowledge/knowledge-cache.js";
export { createGodotKnowledgeService } from "../../../packages/adapters/src/godot/knowledge/godot-knowledge-service.js";
export { createGodotLSPDiagnosticsTool } from "../../../packages/adapters/src/godot/tools/godot-lsp-query-tools.js";
export { createGodotLSPSessionTool } from "../../../packages/adapters/src/godot/tools/godot-lsp-session-tool.js";
export { createGodotProbeProjectTool } from "../../../packages/adapters/src/godot/tools/godot-probe-project-tool.js";
export { createGodotProbeRunner } from "../../../packages/adapters/src/godot/process/godot-probe-runner.js";
export { createGodotProjectProbeService } from "../../../packages/adapters/src/godot/probe/godot-project-probe-service.js";
export { createGodotReviewContextTool } from "../../../packages/adapters/src/godot/tools/godot-review-context-tool.js";
export { createGodotSceneIntelligence } from "../../../packages/adapters/src/godot/intelligence/scene-intelligence-service.js";
export { createMutationLock } from "../../../packages/adapters/src/tools/workspace/mutations/mutation-lock.js";
export { createNodeHttpsTransport } from "../../../packages/adapters/src/research/http-transport.js";
export { createNodeScriptRunner } from "../../../packages/adapters/src/process/runners/node-script-runner.js";
export { createNpmScriptRunner } from "../../../packages/adapters/src/process/runners/npm-script-runner.js";
export { createPlannerExecutor } from "../../../packages/adapters/src/planning/planner-executor.js";
export { createPlannerToolRegistry } from "../../../packages/adapters/src/planning/planner-tools.js";
export { createProcessRunTool } from "../../../packages/adapters/src/process/process-run-tool.js";
export { createProjectInstructionService } from "../../../packages/adapters/src/instructions/instruction-discovery.js";
export { createProviderChangeReviewer } from "../../../packages/adapters/src/godot/quality/provider-change-reviewer.js";
export { createQualityValidationExecutor } from "../../../packages/adapters/src/godot/quality/quality-validation-executor.js";
export { createReviewerToolRegistry } from "../../../packages/adapters/src/godot/quality/reviewer-tools.js";
export { createRunDirectoryProvider } from "../../../packages/adapters/src/process/run-directories.js";
export { createSelfReferenceTools } from "../../../packages/adapters/src/tools/self/self-reference-tools.js";
export { createSha256CommandDigestService } from "../../../packages/adapters/src/process/command-digest.js";
export { createUndoService } from "../../../packages/adapters/src/checkpoints/filesystem/undo-service.js";
export { createValidationPlanDiscovery } from "../../../packages/adapters/src/godot/quality/validation-plan-discovery.js";
export { createWorkspaceApplyTextChangesetTool } from "../../../packages/adapters/src/tools/workspace/mutations/workspace-apply-text-changeset-tool.js";
export { createWorkspaceCreateFileTool } from "../../../packages/adapters/src/tools/workspace/mutations/workspace-create-file-tool.js";
export { createWorkspaceDeleteFileTool } from "../../../packages/adapters/src/tools/workspace/mutations/workspace-delete-file-tool.js";
export { createWorkspaceEditFileTool } from "../../../packages/adapters/src/tools/workspace/mutations/workspace-edit-file-tool.js";
export { createWorkspaceListTool } from "../../../packages/adapters/src/tools/workspace/workspace-list-tool.js";
export { createWorkspaceReadTool } from "../../../packages/adapters/src/tools/workspace/workspace-read-tool.js";
export { createWorkspaceSearchTool } from "../../../packages/adapters/src/tools/workspace/workspace-search-tool.js";
export { readGodotEnvironmentOverrides } from "../../../packages/adapters/src/environment/godot-overrides.js";
export { readParentEnvironment } from "../../../packages/adapters/src/environment/child-environment.js";
export { reconcileWorkspaceCheckpoints } from "../../../packages/adapters/src/index.js";
export { resolveGodotSelection } from "../../../packages/adapters/src/godot/config/selection-request.js";
