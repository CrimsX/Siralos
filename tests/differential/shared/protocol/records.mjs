/**
 * Outcome-record dispatch over every subject validator and the public record-document API.
 */
import { canonicalizeJson } from "../canonical.mjs";
import { ALLOWED_SUBJECTS, CONTRACT_LIMITS } from "../contract.mjs";
import { validateToolLoopResult } from "../tool-loop-protocol.mjs";
import {
  validateContextBenchmarkResult,
  validateContextGraphResult,
  validateContextProjectionResult,
  validateContextRepresentationResult,
  validateContextSchedulerResult,
  validateContextSessionResult,
  validateContextToolResult,
  validateSessionReplayResult,
  validateTuiRenderResult,
} from "./context-session.mjs";
import {
  HARNESS_DIAGNOSTIC_PREFIX,
  LOWER_SHA256,
  RUNNER_PROTOCOL_SCHEMA_VERSION,
  SCENARIO_OUTCOME,
  VERSION,
  assertExactKeys,
  isObject,
  validateCategory,
  validateTaskContractResult,
} from "./core.mjs";
import {
  validateDomainCapabilityResult,
  validateDomainLifecycleResult,
  validateGodotDevelopPlanResult,
  validateGodotDiagnosticsResult,
  validateGodotDiscoveryResult,
  validateGodotKnowledgeResult,
  validateGodotLspResult,
  validateGodotMutationPrepareResult,
  validateGodotReviewContextResult,
  validateGodotSceneResolveResult,
  validateLanguageDefinitionResult,
  validateLanguageDiagnosticsResult,
  validateLanguageStructureResult,
} from "./language-domain.mjs";
import {
  validateProviderGenericResult,
  validateProviderReplayResult,
  validateProviderTurnResult,
  validateReplayStoreResult,
  validateUserConfigResult,
} from "./provider-config.mjs";
import {
  validateCliSessionCase,
  validateExecutorBriefCase,
  validateInstructionsResolutionCase,
  validateKnowledgeRevisionsCase,
  validatePlanningRuntimeCase,
  validateR13AuthorityResult,
  validateReferenceIdentityCase,
  validateResearchPolicyCase,
} from "./r13-authority.mjs";
import {
  validateCompositionContextControlV46Result,
  validateCompositionEffectiveV40Result,
  validateCompositionLockV42Result,
  validateCompositionLockVerifyV47Result,
  validateCompositionPluginActivationV45Result,
  validateCompositionPluginSelectionV43Result,
  validateCompositionProfileV39Result,
  validateCompositionSkillConsumptionV48Result,
  validateCompositionSkillsV44Result,
  validateContextControlsV41Result,
  validateGodotRuntimeV34Result,
  validateIcmDependencyManifestsResult,
  validateIcmPhaseContractResult,
  validateQaWorkflowV37Result,
  validateRecoveryTaxonomyResult,
  validateRunInteractionV36Result,
  validateRunProfileV38Result,
  validateRuntimeReadinessResult,
  validateRuntimeV32Result,
  validateVisualEvidenceV35Result,
} from "./runtime-records.mjs";
import {
  validateApplyResult,
  validateCheckpointResult,
  validateDefaultToolsResult,
  validateGitResult,
  validateListResult,
  validatePrepareResult,
  validateReadResult,
  validateRevisionResult,
  validateSearchResult,
  validateUiComponentsResult,
} from "./workspace-ops.mjs";

function validateCompletedResult(record, label) {
  if (!isObject(record.result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (record.subject === "state-dir") {
    assertExactKeys(record.result, ["stateDirSha256"], `${label}.result`);
    if (
      typeof record.result.stateDirSha256 !== "string" ||
      !LOWER_SHA256.test(record.result.stateDirSha256)
    ) {
      throw new Error(`${label}.result.stateDirSha256 is invalid`);
    }
    return;
  }
  if (record.subject === "task-contract") {
    validateTaskContractResult(record.result, label);
    return;
  }
  if (record.subject === "workspace-read") {
    validateReadResult(record, label);
    return;
  }
  if (record.subject === "workspace-list") {
    validateListResult(record, label);
    return;
  }
  if (record.subject === "workspace-search") {
    validateSearchResult(record, label);
    return;
  }
  if (record.subject === "workspace-revision") {
    validateRevisionResult(record, label);
    return;
  }
  if (record.subject === "workspace-prepare") {
    validatePrepareResult(record, label);
    return;
  }
  if (record.subject === "workspace-apply") {
    validateApplyResult(record, label);
    return;
  }
  if (record.subject === "default-tools") {
    validateDefaultToolsResult(record, label);
    return;
  }
  if (record.subject === "ui-components") {
    validateUiComponentsResult(record, label);
    return;
  }
  if (record.subject === "checkpoint") {
    validateCheckpointResult(record, label);
    return;
  }
  if (record.subject === "git-inspection") {
    validateGitResult(record, label);
    return;
  }
  if (record.subject === "language-diagnostics") {
    validateLanguageDiagnosticsResult(record, label);
    return;
  }
  if (record.subject === "language-structure") {
    validateLanguageStructureResult(record, label);
    return;
  }
  if (record.subject === "language-definition") {
    validateLanguageDefinitionResult(record, label);
    return;
  }
  if (record.subject === "domain-lifecycle") {
    validateDomainLifecycleResult(record, label);
    return;
  }
  if (record.subject === "domain-capability") {
    validateDomainCapabilityResult(record, label);
    return;
  }
  if (record.subject === "provider-turn") {
    validateProviderTurnResult(record, label);
    return;
  }
  if (record.subject === "provider-generic") {
    validateProviderGenericResult(record, label);
    return;
  }
  if (record.subject === "provider-replay") {
    validateProviderReplayResult(record, label);
    return;
  }
  if (record.subject === "session-replay") {
    validateSessionReplayResult(record, label);
    return;
  }
  if (record.subject === "replay-store") {
    validateReplayStoreResult(record, label);
    return;
  }
  if (record.subject === "context-graph") {
    validateContextGraphResult(record, label);
    return;
  }
  if (record.subject === "context-representation") {
    validateContextRepresentationResult(record, label);
    return;
  }
  if (record.subject === "context-scheduler") {
    validateContextSchedulerResult(record, label);
    return;
  }
  if (record.subject === "context-tool") {
    validateContextToolResult(record, label);
    return;
  }
  if (record.subject === "context-benchmark") {
    validateContextBenchmarkResult(record, label);
    return;
  }
  if (record.subject === "context-session") {
    validateContextSessionResult(record, label);
    return;
  }
  if (record.subject === "tui-render") {
    validateTuiRenderResult(record, label);
    return;
  }
  if (record.subject === "tool-loop") {
    validateToolLoopResult(record, label);
    return;
  }
  if (record.subject === "context-projection") {
    validateContextProjectionResult(record, label);
    return;
  }
  if (record.subject === "user-config") {
    validateUserConfigResult(record, label);
    return;
  }
  if (
    record.subject === "security-permissions" ||
    record.subject === "command-catalog" ||
    record.subject === "capability-doctor"
  ) {
    validateR13AuthorityResult(record, label);
    return;
  }
  if (record.subject === "instructions-resolution") {
    assertExactKeys(record.result, ["cases"], `${label}.result`);
    for (const [index, entry] of record.result.cases.entries()) {
      validateInstructionsResolutionCase(entry, `${label}.result.cases[${index}]`);
    }
    return;
  }
  if (record.subject === "knowledge-revisions") {
    assertExactKeys(record.result, ["cases"], `${label}.result`);
    for (const [index, entry] of record.result.cases.entries()) {
      validateKnowledgeRevisionsCase(entry, `${label}.result.cases[${index}]`);
    }
    return;
  }
  if (record.subject === "reference-identity") {
    assertExactKeys(record.result, ["cases"], `${label}.result`);
    if (
      !Array.isArray(record.result.cases) ||
      record.result.cases.length === 0 ||
      record.result.cases.length > 16
    ) {
      throw new Error(`${label}.result.cases must be a bounded array`);
    }
    for (const [index, entry] of record.result.cases.entries()) {
      validateReferenceIdentityCase(entry, `${label}.result.cases[${index}]`);
    }
    return;
  }
  if (record.subject === "research-policy") {
    assertExactKeys(record.result, ["cases"], `${label}.result`);
    if (
      !Array.isArray(record.result.cases) ||
      record.result.cases.length === 0 ||
      record.result.cases.length > 16
    ) {
      throw new Error(`${label}.result.cases must be a bounded array`);
    }
    for (const [index, entry] of record.result.cases.entries()) {
      validateResearchPolicyCase(entry, `${label}.result.cases[${index}]`);
    }
    return;
  }
  if (record.subject === "planning-runtime") {
    assertExactKeys(record.result, ["cases"], `${label}.result`);
    if (
      !Array.isArray(record.result.cases) ||
      record.result.cases.length === 0 ||
      record.result.cases.length > 16
    ) {
      throw new Error(`${label}.result.cases must be a bounded array`);
    }
    for (const [index, entry] of record.result.cases.entries()) {
      validatePlanningRuntimeCase(entry, `${label}.result.cases[${index}]`);
    }
    return;
  }
  if (record.subject === "executor-brief") {
    assertExactKeys(record.result, ["cases"], `${label}.result`);
    if (
      !Array.isArray(record.result.cases) ||
      record.result.cases.length === 0 ||
      record.result.cases.length > 16
    ) {
      throw new Error(`${label}.result.cases must be a bounded array`);
    }
    for (const [index, entry] of record.result.cases.entries()) {
      validateExecutorBriefCase(entry, `${label}.result.cases[${index}]`);
    }
    return;
  }
  if (record.subject === "cli-session") {
    assertExactKeys(record.result, ["cases"], `${label}.result`);
    if (
      !Array.isArray(record.result.cases) ||
      record.result.cases.length === 0 ||
      record.result.cases.length > 16
    ) {
      throw new Error(`${label}.result.cases must be a bounded array`);
    }
    for (const [index, entry] of record.result.cases.entries()) {
      validateCliSessionCase(entry, `${label}.result.cases[${index}]`);
    }
    return;
  }
  if (record.subject === "godot-scene-resolve") {
    validateGodotSceneResolveResult(record.result, label);
    return;
  }
  if (record.subject === "godot-discovery") {
    validateGodotDiscoveryResult(record.result, label);
    return;
  }
  if (record.subject === "godot-knowledge") {
    validateGodotKnowledgeResult(record.result, label);
    return;
  }
  if (record.subject === "godot-diagnostics") {
    validateGodotDiagnosticsResult(record.result, label);
    return;
  }
  if (record.subject === "godot-lsp") {
    validateGodotLspResult(record.result, label);
    return;
  }
  if (record.subject === "godot-review-context") {
    validateGodotReviewContextResult(record.result, label);
    return;
  }
  if (record.subject === "godot-mutation-prepare") {
    validateGodotMutationPrepareResult(record.result, label);
    return;
  }
  if (record.subject === "godot-develop-plan") {
    validateGodotDevelopPlanResult(record.result, label);
    return;
  }
  const R10A_DIGEST_SUBJECTS = new Set([
    "content-identity-artifact-digest",
    "content-identity-contract-digest",
    "determinism-replay",
  ]);
  if (R10A_DIGEST_SUBJECTS.has(record.subject)) {
    assertExactKeys(record.result, ["digest"], `${label}.result`);
    return;
  }
  if (record.subject === "content-identity-manifests") {
    assertExactKeys(record.result, ["aggregateDigest", "entryCount"], `${label}.result`);
    return;
  }
  if (record.subject === "content-identity-delta") {
    assertExactKeys(record.result, ["changed", "unchanged"], `${label}.result`);
    if (
      !Array.isArray(record.result.changed) ||
      !Array.isArray(record.result.unchanged) ||
      record.result.changed
        .concat(record.result.unchanged)
        .some((entry) => typeof entry !== "string")
    ) {
      throw new Error(`${label}.result delta arrays are invalid`);
    }
    return;
  }
  if (record.subject === "icm.phase-contract") {
    validateIcmPhaseContractResult(record.result, label);
    return;
  }
  if (record.subject === "icm.dependency-manifests") {
    validateIcmDependencyManifestsResult(record.result, label);
    return;
  }
  if (
    record.subject === "runtime-readiness.identity" ||
    record.subject === "runtime-readiness.budgets" ||
    record.subject === "runtime-readiness.lifecycle" ||
    record.subject === "runtime-readiness.doctor"
  ) {
    validateRuntimeReadinessResult(record.subject, record.result, label);
    return;
  }
  if (record.subject === "recovery-taxonomy") {
    validateRecoveryTaxonomyResult(record.result, label);
    return;
  }
  if (record.subject === "runtime-execution" || record.subject === "runtime-evidence") {
    validateRuntimeV32Result(record.subject, record.result, label);
    return;
  }
  if (record.subject === "godot-runtime-launch" || record.subject === "godot-runtime-evidence") {
    validateGodotRuntimeV34Result(record.subject, record.result, label);
    return;
  }
  if (record.subject === "visual-evidence") {
    validateVisualEvidenceV35Result(record.result, label);
    return;
  }
  if (record.subject === "run-interaction") {
    validateRunInteractionV36Result(record.result, label);
    return;
  }
  if (record.subject === "qa-workflow") {
    validateQaWorkflowV37Result(record.result, label);
    return;
  }
  if (record.subject === "context-controls") {
    validateContextControlsV41Result(record.result, label);
    return;
  }
  if (record.subject === "composition-lock") {
    validateCompositionLockV42Result(record.result, label);
    return;
  }
  if (record.subject === "composition-plugin-selection") {
    validateCompositionPluginSelectionV43Result(record.result, label);
    return;
  }
  if (record.subject === "composition-skills") {
    validateCompositionSkillsV44Result(record.result, label);
    return;
  }
  if (record.subject === "composition-plugin-activation") {
    validateCompositionPluginActivationV45Result(record.result, label);
    return;
  }
  if (record.subject === "composition-context-control") {
    validateCompositionContextControlV46Result(record.result, label);
    return;
  }
  if (record.subject === "composition-lock-verify") {
    validateCompositionLockVerifyV47Result(record.result, label);
    return;
  }
  if (record.subject === "composition-skill-consumption") {
    validateCompositionSkillConsumptionV48Result(record.result, label);
    return;
  }
  if (record.subject === "evolve-corpus") {
    validateEvolveCorpusV49Result(record.result, label);
    return;
  }
  if (record.subject === "evolve-workflow") {
    validateEvolveWorkflowV50Result(record.result, label);
    return;
  }
  if (record.subject === "evolve-proposal") {
    validateEvolveProposalV51Result(record.result, label);
    return;
  }
  if (record.subject === "evolve-packaging") {
    validateEvolvePackagingV52Result(record.result, label);
    return;
  }
  if (record.subject === "composition-effective") {
    validateCompositionEffectiveV40Result(record.result, label);
    return;
  }
  if (record.subject === "composition-profile") {
    validateCompositionProfileV39Result(record.result, label);
    return;
  }
  if (record.subject === "run-profile") {
    validateRunProfileV38Result(record.result, label);
    return;
  }
  function validateEvolveCorpusV49Result(result, label) {
    assertExactKeys(
      result,
      [
        "corpusDigest",
        "corpusId",
        "disposition",
        "matches",
        "reason",
        "rendered",
        "score",
        "scoreValue",
        "total",
      ],
      `${label}.result`,
    );
    if (result.disposition !== "valid" && result.disposition !== "invalid") {
      throw new Error(`${label}.result.disposition is invalid`);
    }
    if (typeof result.rendered !== "string" || result.rendered.length === 0) {
      throw new Error(`${label}.result.rendered is invalid`);
    }
    if (result.disposition === "valid") {
      if (typeof result.corpusDigest !== "string" || !/^[0-9a-f]{64}$/u.test(result.corpusDigest)) {
        throw new Error(`${label}.result.corpusDigest is invalid`);
      }
      if (typeof result.corpusId !== "string" || result.corpusId.length === 0) {
        throw new Error(`${label}.result.corpusId is invalid`);
      }
      if (!Number.isSafeInteger(result.matches) || result.matches < 0) {
        throw new Error(`${label}.result.matches is invalid`);
      }
      if (!Number.isSafeInteger(result.total) || result.total < 0) {
        throw new Error(`${label}.result.total is invalid`);
      }
      if (typeof result.score !== "string" || typeof result.scoreValue !== "string") {
        throw new Error(`${label}.result score fields are invalid`);
      }
      if (result.reason !== null) {
        throw new Error(`${label}.result.reason must be null for valid disposition`);
      }
    } else {
      if (
        result.corpusDigest !== null ||
        result.corpusId !== null ||
        result.matches !== null ||
        result.total !== null ||
        result.score !== null ||
        result.scoreValue !== null
      ) {
        throw new Error(`${label}.result fields must be null for invalid disposition`);
      }
      if (typeof result.reason !== "string" || result.reason.length === 0) {
        throw new Error(`${label}.result.reason is invalid`);
      }
    }
  }

  function validateEvolveWorkflowV50Result(result, label) {
    assertExactKeys(
      result,
      [
        "baselineDigest",
        "baselineScore",
        "candidateDigest",
        "candidateScore",
        "decision",
        "disposition",
        "escalation",
        "improvement",
        "reason",
        "rendered",
        "workflowDigest",
      ],
      `${label}.result`,
    );
    if (result.disposition !== "valid" && result.disposition !== "invalid") {
      throw new Error(`${label}.result.disposition is invalid`);
    }
    if (typeof result.rendered !== "string" || result.rendered.length === 0) {
      throw new Error(`${label}.result.rendered is invalid`);
    }
    if (result.disposition === "valid") {
      if (
        typeof result.baselineDigest !== "string" ||
        !/^[0-9a-f]{64}$/u.test(result.baselineDigest) ||
        typeof result.candidateDigest !== "string" ||
        !/^[0-9a-f]{64}$/u.test(result.candidateDigest)
      ) {
        throw new Error(`${label}.result digests are invalid`);
      }
      if (typeof result.baselineScore !== "string" || typeof result.candidateScore !== "string") {
        throw new Error(`${label}.result score fields are invalid`);
      }
      if (result.decision !== "reject" && result.decision !== "propose") {
        throw new Error(`${label}.result.decision is invalid`);
      }
      if (
        typeof result.escalation !== "string" ||
        !["profile", "context", "skill", "module", "host"].includes(result.escalation)
      ) {
        throw new Error(`${label}.result.escalation is invalid`);
      }
      if (typeof result.improvement !== "string") {
        throw new Error(`${label}.result.improvement is invalid`);
      }
      if (
        typeof result.workflowDigest !== "string" ||
        !/^[0-9a-f]{64}$/u.test(result.workflowDigest)
      ) {
        throw new Error(`${label}.result.workflowDigest is invalid`);
      }
      if (result.reason !== null) {
        throw new Error(`${label}.result.reason must be null for valid disposition`);
      }
    } else {
      if (
        result.baselineDigest !== null ||
        result.candidateDigest !== null ||
        result.baselineScore !== null ||
        result.candidateScore !== null ||
        result.decision !== null ||
        result.escalation !== null ||
        result.improvement !== null ||
        result.workflowDigest !== null
      ) {
        throw new Error(`${label}.result fields must be null for invalid disposition`);
      }
      if (typeof result.reason !== "string" || result.reason.length === 0) {
        throw new Error(`${label}.result.reason is invalid`);
      }
    }
  }

  function validateEvolveProposalV51Result(result, label) {
    assertExactKeys(
      result,
      ["disposition", "proposalDigest", "proposalId", "reason", "rendered", "requiresHostApproval"],
      `${label}.result`,
    );
    if (result.disposition !== "valid" && result.disposition !== "invalid") {
      throw new Error(`${label}.result.disposition is invalid`);
    }
    if (typeof result.rendered !== "string" || result.rendered.length === 0) {
      throw new Error(`${label}.result.rendered is invalid`);
    }
    if (result.disposition === "valid") {
      if (
        typeof result.proposalDigest !== "string" ||
        !/^[0-9a-f]{64}$/u.test(result.proposalDigest)
      ) {
        throw new Error(`${label}.result.proposalDigest is invalid`);
      }
      if (typeof result.proposalId !== "string" || result.proposalId.length === 0) {
        throw new Error(`${label}.result.proposalId is invalid`);
      }
      if (typeof result.requiresHostApproval !== "boolean") {
        throw new Error(`${label}.result.requiresHostApproval is invalid`);
      }
      if (result.reason !== null) {
        throw new Error(`${label}.result.reason must be null for valid disposition`);
      }
    } else {
      if (
        result.proposalDigest !== null ||
        result.proposalId !== null ||
        result.requiresHostApproval !== null
      ) {
        throw new Error(`${label}.result fields must be null for invalid disposition`);
      }
      if (typeof result.reason !== "string" || result.reason.length === 0) {
        throw new Error(`${label}.result.reason is invalid`);
      }
    }
  }

  function validateEvolvePackagingV52Result(result, label) {
    assertExactKeys(
      result,
      [
        "compatibility",
        "disposition",
        "reason",
        "releaseDigest",
        "releaseId",
        "rendered",
        "version",
      ],
      `${label}.result`,
    );
    if (result.disposition !== "valid" && result.disposition !== "invalid") {
      throw new Error(`${label}.result.disposition is invalid`);
    }
    if (typeof result.rendered !== "string" || result.rendered.length === 0) {
      throw new Error(`${label}.result.rendered is invalid`);
    }
    if (result.disposition === "valid") {
      if (
        typeof result.releaseDigest !== "string" ||
        !/^[0-9a-f]{64}$/u.test(result.releaseDigest)
      ) {
        throw new Error(`${label}.result.releaseDigest is invalid`);
      }
      if (typeof result.releaseId !== "string" || result.releaseId.length === 0) {
        throw new Error(`${label}.result.releaseId is invalid`);
      }
      if (typeof result.version !== "string" || result.version.length === 0) {
        throw new Error(`${label}.result.version is invalid`);
      }
      if (!["patch", "compatible", "breaking"].includes(result.compatibility)) {
        throw new Error(`${label}.result.compatibility is invalid`);
      }
      if (result.reason !== null) {
        throw new Error(`${label}.result.reason must be null for valid disposition`);
      }
    } else {
      if (
        result.releaseDigest !== null ||
        result.releaseId !== null ||
        result.version !== null ||
        result.compatibility !== null
      ) {
        throw new Error(`${label}.result fields must be null for invalid disposition`);
      }
      if (typeof result.reason !== "string" || result.reason.length === 0) {
        throw new Error(`${label}.result.reason is invalid`);
      }
    }
  }

  if (record.subject === "context-scan") {
    assertExactKeys(
      record.result,
      ["filesNotScanned", "nodes", "oversizedSkipped", "protectedSkipped", "truncated"],
      `${label}.result`,
    );
    if (!Array.isArray(record.result.nodes) || record.result.nodes.length > 512) {
      throw new Error(`${label}.result.nodes must be a bounded array`);
    }
    for (const [index, node] of record.result.nodes.entries()) {
      const nodeLabel = `${label}.result.nodes[${index}]`;
      assertExactKeys(node, ["byteLen", "contentDigest", "relativePath"], nodeLabel);
      if (typeof node.relativePath !== "string" || node.relativePath.length === 0) {
        throw new Error(`${nodeLabel}.relativePath is invalid`);
      }
      if (typeof node.contentDigest !== "string" || !LOWER_SHA256.test(node.contentDigest)) {
        throw new Error(`${nodeLabel}.contentDigest is invalid`);
      }
      if (!Number.isSafeInteger(node.byteLen) || node.byteLen < 0) {
        throw new Error(`${nodeLabel}.byteLen is invalid`);
      }
    }
    if (
      !Number.isSafeInteger(record.result.protectedSkipped) ||
      record.result.protectedSkipped < 0
    ) {
      throw new Error(`${label}.result.protectedSkipped is invalid`);
    }
    if (
      !Number.isSafeInteger(record.result.oversizedSkipped) ||
      record.result.oversizedSkipped < 0
    ) {
      throw new Error(`${label}.result.oversizedSkipped is invalid`);
    }
    if (!Number.isSafeInteger(record.result.filesNotScanned) || record.result.filesNotScanned < 0) {
      throw new Error(`${label}.result.filesNotScanned is invalid`);
    }
    if (typeof record.result.truncated !== "boolean") {
      throw new Error(`${label}.result.truncated is invalid`);
    }
    return;
  }

  assertExactKeys(record.result, ["version"], `${label}.result`);
  if (
    typeof record.result.version !== "string" ||
    Buffer.byteLength(record.result.version, "utf8") > CONTRACT_LIMITS.identifierBytes ||
    !VERSION.test(record.result.version)
  ) {
    throw new Error(`${label}.result.version is invalid`);
  }
}

/** Validate one semantic outcome record and return it unchanged. */
export function validateOutcomeRecord(record, source, expectedScenario = undefined) {
  const label = `${source} record`;
  if (!isObject(record)) {
    throw new Error(`malformed ${label}: expected an object`);
  }
  // Retired subjects stay parseable here so the frozen oracle bundle —
  // which keeps every retired record byte-identical — still loads. They
  // can never execute: the corpus contract (ALLOWED_SUBJECTS) no longer
  // admits them, and the retirement list binds each absence by digest.
  // Keep both sets in step: removing a subject from ALLOWED_SUBJECTS
  // without retiring it fails the orphan-frozen gate instead. The
  // retired-subject binding check in check-supersessions.mjs fails the
  // gate when this set diverges from the digest-bound list: do not
  // extend this set by hand.
  const RETIRED_SUBJECTS = new Set([
    "godot-scene-resolve",
    "godot-discovery",
    "godot-knowledge",
    "godot-diagnostics",
    "godot-lsp",
    "godot-review-context",
    "godot-mutation-prepare",
    "godot-develop-plan",
  ]);
  if (
    typeof record.subject !== "string" ||
    (!ALLOWED_SUBJECTS.has(record.subject) && !RETIRED_SUBJECTS.has(record.subject))
  ) {
    throw new Error(`malformed ${label}: unsupported subject`);
  }
  if (
    typeof record.scenarioId !== "string" ||
    record.scenarioId.length === 0 ||
    Buffer.byteLength(record.scenarioId, "utf8") > CONTRACT_LIMITS.identifierBytes
  ) {
    throw new Error(`malformed ${label}: invalid scenarioId`);
  }
  if (!Object.values(SCENARIO_OUTCOME).includes(record.outcome)) {
    throw new Error(`malformed ${label}: unsupported outcome`);
  }
  if (record.outcome === SCENARIO_OUTCOME.COMPLETED) {
    assertExactKeys(record, ["scenarioId", "subject", "outcome", "result"], label);
    validateCompletedResult(record, label);
  } else {
    assertExactKeys(record, ["scenarioId", "subject", "outcome", "error"], label);
    validateCategory(record.error, label);
  }
  if (expectedScenario !== undefined) {
    if (record.scenarioId !== expectedScenario.id) {
      throw new Error(
        `${source} record set is out of order, incomplete, or contains an extra scenario`,
      );
    }
    if (record.subject !== expectedScenario.subject) {
      throw new Error(`${source} record ${record.scenarioId} has a subject mismatch`);
    }
  }
  return record;
}

/** Exact canonical runner-protocol document. */
export function canonicalRecordDocument(records) {
  return `${canonicalizeJson({ records, schemaVersion: RUNNER_PROTOCOL_SCHEMA_VERSION })}\n`;
}

/** Parse a bounded runner-protocol document and validate every record. */
export function parseCanonicalRecordDocument(text, source) {
  if (Buffer.byteLength(text, "utf8") > CONTRACT_LIMITS.recordsBytes) {
    throw new Error(`${source} record file exceeds the harness byte bound`);
  }
  let document;
  try {
    document = JSON.parse(text);
  } catch (error) {
    throw new Error(
      `${source} record file is not JSON: ${error instanceof Error ? error.message : error}`,
      { cause: error },
    );
  }
  if (!isObject(document)) {
    throw new Error(`${source} record file must be a protocol object`);
  }
  assertExactKeys(document, ["records", "schemaVersion"], `${source} record document`);
  if (document.schemaVersion !== RUNNER_PROTOCOL_SCHEMA_VERSION) {
    throw new Error(
      `${source} record file has unsupported schemaVersion ${JSON.stringify(document.schemaVersion)}`,
    );
  }
  if (!Array.isArray(document.records) || document.records.length > CONTRACT_LIMITS.scenarios) {
    throw new Error(`${source} record file must contain a bounded records array`);
  }
  if (canonicalRecordDocument(document.records) !== text) {
    throw new Error(`${source} record file is not exact canonical JSON with one trailing newline`);
  }
  for (const record of document.records) {
    validateOutcomeRecord(record, source);
  }
  return document.records;
}

/** Machine-readable stderr diagnostic for an expected harness failure. */
export function harnessDiagnostic(category, code, message) {
  return `${HARNESS_DIAGNOSTIC_PREFIX}${canonicalizeJson({ category, code, message })}`;
}
