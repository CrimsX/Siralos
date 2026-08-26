/**
 * Oracle-only re-export shim for the `@siralos/core` package specifier.
 *
 * The R4 probes exercise adapter modules that import selected VALUES
 * from `@siralos/core`. The full source index uses TypeScript syntax
 * (constructor parameter properties) that strip-only mode cannot load,
 * so this harness shim re-exports exactly those values from their real
 * source modules. Type-only imports are erased by the type stripper and
 * need no shim. The shim aliases modules; it never reimplements behavior.
 */
export { buildWorkspaceSummary } from "../../../packages/core/src/workspace/workspace-summary.js";
export { extractGDScriptStructure } from "../../../packages/core/src/workspace/gdscript-structure.js";
export { isWorkspaceReadMode } from "../../../packages/core/src/workspace/workspace-read-mode.js";
export { GODOT_LIMITS } from "../../../packages/core/src/godot/limits.js";
export { normalizeDefinitionLocations } from "../../../packages/core/src/language/definition.js";
export { normalizeDiagnosticPayload } from "../../../packages/core/src/language/diagnostic.js";
export { sanitizeControlCharacters } from "../../../packages/core/src/language/sanitize.js";
export { toOneBasedRange } from "../../../packages/core/src/language/position.js";
export { truncateUtf8Bytes } from "../../../packages/core/src/language/truncate.js";
export { isProtectedBehavioralConfigPath } from "../../../packages/core/src/security/behavioral-config.js";
export { GitError } from "../../../packages/core/src/git/git-errors.js";
// R13.3 reference/research parity probes: the adapter modules import these
// values from `@siralos/core`; each aliases its real source module.
export { canonicalizeJson, sha256Hex } from "../../../packages/core/src/godot/digest.js";
export { normalizeRepositoryOrigin } from "../../../packages/core/src/reference/reference-declaration.js";
export {
  computeResearchDocumentContentDigest,
  computeResearchDocumentId,
} from "../../../packages/core/src/research/research-model.js";
export { VALIDATION_OFFLINE_PROFILE } from "../../../packages/core/src/security/profile.js";
export {
  REFERENCE_LIMITS,
  validateReferenceAlias,
} from "../../../packages/core/src/reference/reference-model.js";
// R8 Godot parity probes: the discovery profiler, knowledge,
// diagnostics, and LSP services import these values from
// `@siralos/core`; each aliases its real source module.
export {
  classifyGodotEdition,
  classifyGodotSupport,
} from "../../../packages/core/src/godot/engine-profile.js";
export {
  classifyGodotReleaseChannel,
  parseDeclaredVersion,
} from "../../../packages/core/src/godot/version.js";
export {
  GODOT_SELECTION_RANKS,
  rankGodotCandidates,
} from "../../../packages/core/src/godot/selection.js";
export { assessGodotCompatibility } from "../../../packages/core/src/godot/compatibility.js";
export {
  computeGodotCheckOnlyCommandDigest,
  computeGodotPreparedCheckDigest,
  createPreparedGDScriptCheck,
} from "../../../packages/core/src/godot/gdscript.js";
export { GODOT_DIAGNOSTICS_OFFLINE_PROFILE } from "../../../packages/core/src/security/profile.js";
export {
  KNOWLEDGE_SCHEMA_VERSION,
  classifyGodotManualChannel,
} from "../../../packages/core/src/godot/knowledge.js";
export { computeGDScriptPreparedSessionDigest } from "../../../packages/core/src/godot/lsp.js";
export { createPreparedGDScriptSession } from "../../../packages/core/src/godot/lsp.js";
export { GODOT_LSP_LOCAL_PROFILE } from "../../../packages/core/src/security/profile.js";
export { computeGodotRiskManifestDigest } from "../../../packages/core/src/godot/probe.js";
export { godotSymbolId } from "../../../packages/core/src/godot/api.js";
export { isBalancedText } from "../../../packages/core/src/godot/scene/text.js";
export { parseGodotVariant } from "../../../packages/core/src/godot/scene/variant.js";
// R13.5 cli-session parity probe: the CLI composition imports these
export { createSelfReference } from "../../../packages/core/src/self/self-reference.js";
export { doctorExitCodeFor } from "../../../packages/core/src/doctor/doctor-model.js";
export { toSafeReport } from "../../../packages/core/src/doctor/safe-report.js";
export { parseReferenceDeclarationsSection } from "../../../packages/core/src/reference/reference-declaration.js";
export { renderExecutorBrief } from "../../../packages/core/src/executor/brief-compiler.js";
export { createAcceptanceEvaluator } from "../../../packages/core/src/executor/acceptance.js";
export { COMMAND_LIMITS } from "../../../packages/core/src/commands/command-limits.js";
export { COMMAND_CATALOG_IDS } from "../../../packages/core/src/commands/command-catalog.js";
export { SandboxError } from "../../../packages/core/src/security/sandbox-error.js";
export { PHASE_CONTRACTS } from "../../../packages/core/src/context/phase-contract.js";
export { isPathWithin } from "../../../packages/core/src/reference/reference-registry.js";
export { describeInstructionScope } from "../../../packages/core/src/instructions/instruction-model.js";
export { referenceIdentityAnchor } from "../../../packages/core/src/reference/reference-evidence.js";
export { formatReferenceAlias } from "../../../packages/core/src/reference/reference-model.js";
export { createReferenceRegistry } from "../../../packages/core/src/index.js";
export { DEFAULT_EXECUTION_CONTRACT } from "../../../packages/core/src/executor/execution-contract.js";
export { classifyDevelopmentSurface } from "../../../packages/core/src/godot/development/development-surface.js";
export { S3M11_MILESTONE_MANIFEST } from "../../../packages/core/src/executor/s3m11-manifest.js";
export { computeExecutorBriefFingerprint } from "../../../packages/core/src/executor/brief-compiler.js";
export { computePlanRevisionDigest } from "../../../packages/core/src/planning/planning-model.js";
export { TASK_RUNTIME_VERSION } from "../../../packages/core/src/tasks/task-snapshot.js";
export { containsGodotSceneOrResourceReference } from "../../../packages/core/src/planning/planning-policy.js";
export { buildGodotProjectKnowledgeCandidates } from "../../../packages/core/src/knowledge/knowledge-seeding.js";
export { containsProtectedConfigReference } from "../../../packages/core/src/planning/planning-policy.js";
export { capabilityPolicyFingerprint } from "../../../packages/core/src/tasks/task-snapshot.js";
export { createAdHocTaskContract } from "../../../packages/core/src/tasks/task-contract.js";
export { createCommandRunnerRegistry } from "../../../packages/core/src/commands/command-registry.js";
export { createDevelopmentTaskFlow } from "../../../packages/core/src/tasks/task-development.js";
export { createDefaultPolicy } from "../../../packages/core/src/security/default-policy.js";
export { createEnvironmentManifest } from "../../../packages/core/src/determinism/environment.js";
export { createExecutionInputManifest } from "../../../packages/core/src/identity/manifests.js";
export { createExecutorBriefing } from "../../../packages/core/src/executor/briefing-service.js";
export { createGuidanceManifest } from "../../../packages/core/src/identity/manifests.js";
export { createPlanningFlow } from "../../../packages/core/src/planning/planning-flow.js";
export { createReproducibilityManifest } from "../../../packages/core/src/determinism/reproducibility.js";
export { createTaskRuntimeSnapshot } from "../../../packages/core/src/tasks/task-snapshot.js";
export { DEVELOPMENT_LIMITS } from "../../../packages/core/src/godot/development/development-model.js";
export { planTouchpointStaleness } from "../../../packages/core/src/planning/planning-validation.js";
export { computeChangeSetDigest } from "../../../packages/core/src/godot/development/development-change-set.js";
export { countChangeSetResultBytes } from "../../../packages/core/src/godot/development/development-change-set.js";
export { validateChangeSetRequest } from "../../../packages/core/src/godot/development/development-change-set.js";
export { QUALITY_LIMITS } from "../../../packages/core/src/godot/quality/quality-model.js";
export { aggregateReviewResults } from "../../../packages/core/src/godot/quality/quality-review.js";
export { analyzeConventions } from "../../../packages/core/src/godot/quality/quality-conventions.js";
export { chunkChangeReviewRequests } from "../../../packages/core/src/godot/quality/quality-review.js";
export { classifyReviewFindingBlocking } from "../../../packages/core/src/godot/quality/quality-review.js";
export { classifyValidationGate } from "../../../packages/core/src/godot/quality/quality-validation.js";
export { computeQualityReportStatus } from "../../../packages/core/src/godot/quality/quality-model.js";
export { computeWarningDelta } from "../../../packages/core/src/godot/quality/quality-warnings.js";
export { countReviewFindingsBySeverity } from "../../../packages/core/src/godot/quality/quality-review.js";
export { createQualityGateResult } from "../../../packages/core/src/godot/quality/quality-model.js";
export { discoverValidationPlan } from "../../../packages/core/src/godot/quality/quality-validation.js";
export { computeGDScriptDevelopmentDigest } from "../../../packages/core/src/godot/development/development-model.js";
export { DEFAULT_DOCTOR_CHECK_TIMEOUT_MS } from "../../../packages/core/src/doctor/capability-doctor.js";
export { buildRuntimeReadinessDiagnostic } from "../../../packages/core/src/runtime/doctor.js";
export { createCapabilityDoctor } from "../../../packages/core/src/doctor/capability-doctor.js";
export { createToolProjector } from "../../../packages/core/src/projection/tool-projector.js";
export { createPreparedGodotProbe } from "../../../packages/core/src/godot/probe.js";
export { GODOT_RECOVERY_PROBE_OFFLINE_PROFILE } from "../../../packages/core/src/security/profile.js";
export { computeGodotPreparedProbeDigest } from "../../../packages/core/src/godot/probe.js";
export { GODOT_SCENE_LIMITS } from "../../../packages/core/src/godot/scene/limits.js";
export { REVIEW_CONTEXT_LIMITS } from "../../../packages/core/src/godot/impact/review-context.js";
export { analyzeImpact } from "../../../packages/core/src/index.js";
export { buildSceneNodeTree } from "../../../packages/core/src/godot/scene/scene-tree.js";
export { createGodotRelationshipIndex } from "../../../packages/core/src/godot/scene/relationship-index.js";
export { parseGodotResource } from "../../../packages/core/src/godot/scene/resource-parser.js";
export { parseGodotScene } from "../../../packages/core/src/godot/scene/scene-parser.js";
export { resolveResPath } from "../../../packages/core/src/godot/scene/resolution.js";
export { createKnowledgeCoordinator } from "../../../packages/core/src/knowledge/knowledge-coordinator.js";
export { extractPlanCandidateJson } from "../../../packages/core/src/planning/planning-validation.js";
export { isPreparedCommandTool } from "../../../packages/core/src/tools/prepared-mutation-tool.js";
export { isPreparedDiagnosticTool } from "../../../packages/core/src/tools/prepared-diagnostic-tool.js";
export { isPreparedLSPSessionTool } from "../../../packages/core/src/tools/prepared-lsp-session-tool.js";
export { isPreparedMutationTool } from "../../../packages/core/src/tools/prepared-mutation-tool.js";
export { isPreparedProbeTool } from "../../../packages/core/src/tools/prepared-probe-tool.js";
export { validatePlanCandidate } from "../../../packages/core/src/planning/planning-validation.js";
export { createToolRegistry } from "../../../packages/core/src/tools/tool-registry.js";
export { createPreparedCommand } from "../../../packages/core/src/commands/command-runners.js";
export { evaluatePermission } from "../../../packages/core/src/security/permission-evaluator.js";
export { MAX_INSTRUCTION_CONTENT_BYTES } from "../../../packages/core/src/instructions/instruction-model.js";
export { buildInstruction } from "../../../packages/core/src/instructions/instruction-resolver.js";
export { computeInstructionInventoryRevision } from "../../../packages/core/src/instructions/instruction-resolver.js";
export { resolveInstructionSet } from "../../../packages/core/src/instructions/instruction-resolver.js";
export { resolveInstructionsForPath } from "../../../packages/core/src/instructions/instruction-resolver.js";
export { createProjectionService } from "../../../packages/core/src/projection/projection-service.js";
export { DEFAULT_MAX_TOOL_ROUNDS } from "../../../packages/core/src/application/application.js";
export { normalizeReviewFindings } from "../../../packages/core/src/godot/quality/quality-review.js";
export { PROCESS_RUN_TOOL_NAME } from "../../../packages/core/src/commands/command-tool.js";
export { createResearchService } from "../../../packages/core/src/research/research-service.js";
export { createRouteContextCapacity } from "../../../packages/core/src/projection/context-capacity.js";
export { canonicalizeCommandDigest } from "../../../packages/core/src/commands/command-digest.js";
export { createSiralosApplication } from "../../../packages/core/src/application/application.js";
export { createSiralosSecurity } from "../../../packages/core/src/security/sandbox-service.js";
export { createTaskRuntime } from "../../../packages/core/src/tasks/task-runtime.js";
export { createPreparedMutation } from "../../../packages/core/src/tools/prepared-mutation-tool.js";
export { createWorkspaceRevisionRegistry } from "../../../packages/core/src/workspace/workspace-revision.js";
export { getBuiltInProfile } from "../../../packages/core/src/security/profile.js";
export { isTerminalPhase } from "../../../packages/core/src/tasks/task-model.js";
export { applyResourceOperations } from "../../../packages/core/src/godot/scene-mutation/model-apply.js";
export { applySceneOperations } from "../../../packages/core/src/godot/scene-mutation/model-apply.js";
export { createPreparedGodotMutation } from "../../../packages/core/src/godot/scene-mutation/prepared.js";
export { expectedSemanticEffect } from "../../../packages/core/src/godot/scene-mutation/operations.js";
export { serializeResource } from "../../../packages/core/src/godot/scene-mutation/serializer.js";
export { serializeScene } from "../../../packages/core/src/godot/scene-mutation/serializer.js";
export { validateMutationOperations } from "../../../packages/core/src/godot/scene-mutation/operations.js";
export { verifyResourceSemanticEffect } from "../../../packages/core/src/godot/scene-mutation/verify.js";
export { verifySceneSemanticEffect } from "../../../packages/core/src/godot/scene-mutation/verify.js";
export { approveUnifiedTarget } from "../../../packages/core/src/godot/development/unified-change-set.js";
export { createBlockedDisposition } from "../../../packages/core/src/godot/development/blocked-disposition.js";
export { createUnifiedChangeSet } from "../../../packages/core/src/godot/development/unified-change-set.js";
export { deriveUnifiedApplyOrder } from "../../../packages/core/src/godot/development/unified-order.js";
export { deriveUnifiedOrderEdges } from "../../../packages/core/src/godot/development/unified-order.js";
export { unifiedChangeSetReadyToApply } from "../../../packages/core/src/godot/development/unified-change-set.js";
export { verifyCrossSurfaceConsistency } from "../../../packages/core/src/godot/development/cross-surface-consistency.js";
export { deterministicFindingId } from "../../../packages/core/src/godot/quality/quality-review.js";
