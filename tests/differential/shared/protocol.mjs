/**
 * Versioned outcome protocol shared by the R2 runners and comparator.
 *
 * Scenario outcomes describe product behavior. Runner outcomes describe the
 * lifecycle of the isolated reference/candidate process. Keeping those layers
 * distinct prevents a product failure from being mistaken for a harness crash.
 */
import { canonicalizeJson } from "./canonical.mjs";
import { ALLOWED_SUBJECTS, CONTRACT_LIMITS } from "./contract.mjs";
import { validateToolLoopResult } from "./tool-loop-protocol.mjs";

export const RUNNER_PROTOCOL_SCHEMA_VERSION = 1;

export const SCENARIO_OUTCOME = Object.freeze({
  COMPLETED: "COMPLETED",
  PRODUCT_FAILURE: "PRODUCT_FAILURE",
  UNIMPLEMENTED: "UNIMPLEMENTED",
  UNSUPPORTED: "UNSUPPORTED",
});

export const RUNNER_OUTCOME = Object.freeze({
  COMPLETED: "COMPLETED",
  TIMED_OUT: "TIMED_OUT",
  PROCESS_CRASHED: "PROCESS_CRASHED",
  PROTOCOL_ERROR: "PROTOCOL_ERROR",
  HARNESS_ERROR: "HARNESS_ERROR",
});

export const HARNESS_DIAGNOSTIC_PREFIX = "SIRALOS_HARNESS_ERROR ";

const LOWER_SHA256 = /^[0-9a-f]{64}$/u;
const CATEGORY = /^[A-Z][A-Z0-9_]*$/u;
const VERSION =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/u;

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function assertExactKeys(value, expected, label) {
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    throw new Error(`${label} has unknown or missing fields`);
  }
}

function validateCategory(error, label) {
  if (!isObject(error)) {
    throw new Error(`${label}.error must be an object`);
  }
  assertExactKeys(error, ["category"], `${label}.error`);
  if (
    typeof error.category !== "string" ||
    Buffer.byteLength(error.category, "utf8") > CONTRACT_LIMITS.identifierBytes ||
    !CATEGORY.test(error.category)
  ) {
    throw new Error(`${label}.error.category is invalid`);
  }
}

const TASK_PHASE_VALUES = new Set([
  "prepared",
  "working",
  "validating",
  "reviewing",
  "blocked",
  "completed",
  "cancelled",
  "failed",
]);
const TASK_STEP_STATUS_VALUES = new Set(["pending", "active", "completed", "failed", "blocked"]);
const TASK_ACCEPTANCE_STATUS_VALUES = new Set(["pending", "satisfied", "failed"]);
const TASK_EVIDENCE_KIND_VALUES = new Set([
  "workspace_read",
  "parser_result",
  "validation_result",
  "review_result",
  "user_approval",
]);
const TASK_VALIDATION_STATUS_VALUES = new Set([
  "not_run",
  "clean",
  "warnings",
  "failed",
  "incomplete",
]);
const TASK_REVIEW_STATUS_VALUES = new Set(["not_run", "clean", "findings", "incomplete"]);
const TASK_FINDING_SEVERITY_VALUES = new Set(["critical", "high", "medium", "low"]);
const TASK_PROGRESS_STATE_VALUES = new Set(["healthy", "degraded", "stalled"]);
const TASK_ACTIVITY_TYPES = new Set([
  "task_started",
  "task_phase_changed",
  "step_started",
  "step_completed",
  "step_failed",
  "evidence_attached",
  "criterion_verified",
  "task_blocked",
  "task_completed",
  "task_cancelled",
  "task_failed",
  "task_contract_revised",
  "disposition_submitted",
]);

function validateBoundedStringArray(value, maximum, label) {
  if (!Array.isArray(value) || value.length > maximum) {
    throw new Error(`${label} must be a bounded array`);
  }
  for (const entry of value) {
    if (typeof entry !== "string" || entry.length === 0) {
      throw new Error(`${label} entries must be non-empty strings`);
    }
  }
}

function validateTaskContractResult(result, label) {
  if (result.rejected === true) {
    assertExactKeys(result, ["rejected", "code"], `${label}.result`);
    if (typeof result.code !== "string" || result.code.length === 0) {
      throw new Error(`${label}.result.code is invalid`);
    }
    return;
  }
  assertExactKeys(
    result,
    [
      "rejected",
      "finalPhase",
      "contractRevision",
      "contractDigest",
      "stepStates",
      "acceptance",
      "evidenceIds",
      "validationStatus",
      "reviewStatus",
      "iteration",
      "currentFindings",
      "terminalReason",
      "startedAtMs",
      "completedAtMs",
      "ops",
      "activity",
      "completion",
      "progress",
    ],
    `${label}.result`,
  );
  if (result.rejected !== false) {
    throw new Error(`${label}.result.rejected must be a boolean`);
  }
  if (!TASK_PHASE_VALUES.has(result.finalPhase)) {
    throw new Error(`${label}.result.finalPhase is invalid`);
  }
  if (
    !Number.isSafeInteger(result.contractRevision) ||
    result.contractRevision < 1 ||
    typeof result.contractDigest !== "string" ||
    !LOWER_SHA256.test(result.contractDigest)
  ) {
    throw new Error(`${label}.result contract identity is invalid`);
  }
  if (
    !Array.isArray(result.stepStates) ||
    !Array.isArray(result.acceptance) ||
    !Array.isArray(result.evidenceIds) ||
    !Array.isArray(result.currentFindings) ||
    !Array.isArray(result.ops) ||
    !Array.isArray(result.activity)
  ) {
    throw new Error(`${label}.result arrays are invalid`);
  }
  validateBoundedStringArray(result.evidenceIds, 256, `${label}.result.evidenceIds`);
  if (!TASK_VALIDATION_STATUS_VALUES.has(result.validationStatus)) {
    throw new Error(`${label}.result.validationStatus is invalid`);
  }
  if (!TASK_REVIEW_STATUS_VALUES.has(result.reviewStatus)) {
    throw new Error(`${label}.result.reviewStatus is invalid`);
  }
  if (!Number.isSafeInteger(result.iteration) || result.iteration < 0) {
    throw new Error(`${label}.result.iteration is invalid`);
  }
  for (const step of result.stepStates) {
    if (!isObject(step)) {
      throw new Error(`${label}.result.stepStates entries must be objects`);
    }
    assertExactKeys(step, ["id", "status", "evidenceRefs"], `${label}.result.stepStates`);
    if (
      typeof step.id !== "string" ||
      step.id.length === 0 ||
      !TASK_STEP_STATUS_VALUES.has(step.status) ||
      !Array.isArray(step.evidenceRefs)
    ) {
      throw new Error(`${label}.result.stepStates entry is invalid`);
    }
    for (const reference of step.evidenceRefs) {
      if (
        !isObject(reference) ||
        typeof reference.evidenceId !== "string" ||
        typeof reference.kind !== "string" ||
        !TASK_EVIDENCE_KIND_VALUES.has(reference.kind)
      ) {
        throw new Error(`${label}.result evidenceRef is invalid`);
      }
    }
  }
  for (const criterion of result.acceptance) {
    if (
      !isObject(criterion) ||
      typeof criterion.criterionId !== "string" ||
      !TASK_ACCEPTANCE_STATUS_VALUES.has(criterion.status) ||
      (criterion.verifiedBy !== null && typeof criterion.verifiedBy !== "string")
    ) {
      throw new Error(`${label}.result acceptance entry is invalid`);
    }
  }
  for (const finding of result.currentFindings) {
    if (
      !isObject(finding) ||
      typeof finding.findingId !== "string" ||
      !TASK_FINDING_SEVERITY_VALUES.has(finding.severity) ||
      typeof finding.source !== "string"
    ) {
      throw new Error(`${label}.result finding entry is invalid`);
    }
  }
  if (result.terminalReason !== null && typeof result.terminalReason !== "string") {
    throw new Error(`${label}.result.terminalReason is invalid`);
  }
  if (
    !Number.isSafeInteger(result.startedAtMs) ||
    (result.completedAtMs !== null && !Number.isSafeInteger(result.completedAtMs))
  ) {
    throw new Error(`${label}.result timestamps are invalid`);
  }
  for (const op of result.ops) {
    if (!isObject(op) || typeof op.op !== "string" || op.op.length === 0) {
      throw new Error(`${label}.result.ops entries are invalid`);
    }
  }
  for (const event of result.activity) {
    if (
      !isObject(event) ||
      typeof event.type !== "string" ||
      !TASK_ACTIVITY_TYPES.has(event.type) ||
      !Number.isSafeInteger(event.sequence) ||
      event.sequence < 1
    ) {
      throw new Error(`${label}.result.activity entries are invalid`);
    }
  }
  if (!isObject(result.completion)) {
    throw new Error(`${label}.result.completion is invalid`);
  }
  assertExactKeys(result.completion, ["allowed", "missing"], `${label}.result.completion`);
  if (
    typeof result.completion.allowed !== "boolean" ||
    !Array.isArray(result.completion.missing) ||
    result.completion.missing.some((entry) => typeof entry !== "string")
  ) {
    throw new Error(`${label}.result.completion is invalid`);
  }
  if (!isObject(result.progress)) {
    throw new Error(`${label}.result.progress is invalid`);
  }
  assertExactKeys(
    result.progress,
    ["state", "usefulObservations", "repeatedActions"],
    `${label}.result.progress`,
  );
  if (
    !TASK_PROGRESS_STATE_VALUES.has(result.progress.state) ||
    !Number.isSafeInteger(result.progress.usefulObservations) ||
    result.progress.usefulObservations < 0 ||
    !Number.isSafeInteger(result.progress.repeatedActions) ||
    result.progress.repeatedActions < 0
  ) {
    throw new Error(`${label}.result.progress is invalid`);
  }
}

// ---------------------------------------------------------------------------
// Stage 3R R4 subjects: generic workspace / project foundation.
// ---------------------------------------------------------------------------

const READ_CODES = new Set([
  "invalid_input",
  "outside_workspace",
  "excluded",
  "empty",
  "null_byte",
  "absolute",
  "unresolvable",
  "inspect_failed",
  "not_file",
  "too_large",
  "unreadable",
  "binary",
  "not_utf8",
  "start_beyond",
]);
const LIST_CODES = new Set([
  "invalid_input",
  "outside_workspace",
  "excluded",
  "empty",
  "null_byte",
  "absolute",
  "unresolvable",
  "not_directory",
  "inspect_failed",
  "list_failed",
  "entry_inspect_failed",
]);
const SEARCH_CODES = new Set([
  "invalid_input",
  "query_required",
  "query_not_string",
  "max_results_invalid",
  "path_not_string",
  "not_an_object",
  "outside_workspace",
  "excluded",
  "empty",
  "null_byte",
  "absolute",
  "unresolvable",
]);
const TRUNCATION_REASONS = new Set([
  "directory_budget",
  "entry_budget",
  "file_budget",
  "scan_budget",
  "input_budget",
  "output_budget",
  "time_budget",
  "match_limit",
  "depth_budget",
]);
const PREPARE_CODES = new Set(["mutation_unavailable"]);
const CHECKPOINT_STATES_SET = new Set([
  "prepared",
  "applied",
  "undone",
  "abandoned",
  "conflicted",
  "uncertain",
]);
const CHECKPOINT_OPERATIONS_SET = new Set(["create", "update", "delete"]);
const UNDO_DECISIONS = new Set(["ready_create", "ready_restore", "ready_delete", "conflict"]);

function validateReadResult(record, label) {
  assertExactKeys(record.result, ["reads"], `${label}.result`);
  if (!Array.isArray(record.result.reads) || record.result.reads.length > 64) {
    throw new Error(`${label}.result.reads must be a bounded array`);
  }
  for (const entry of record.result.reads) {
    if (!isObject(entry) || typeof entry.path !== "string") {
      throw new Error(`${label}.result.reads entries must carry a path`);
    }
    if (entry.status === "cancelled") {
      assertExactKeys(entry, ["path", "status"], `${label}.read`);
      continue;
    }
    if (entry.status === "success") {
      const keys =
        entry.mode === undefined
          ? [
              "path",
              "status",
              "sha256",
              "revision",
              "content",
              "startLine",
              "endLine",
              "totalLines",
              "truncated",
            ]
          : ["path", "status", "mode", "revision", "supported", "reason"];
      assertExactKeys(entry, keys, `${label}.read`);
      if (entry.mode === undefined) {
        if (!LOWER_SHA256.test(entry.sha256) || !/^rev_[0-9a-f]{32}$/u.test(entry.revision)) {
          throw new Error(`${label}.read identity is invalid`);
        }
        if (typeof entry.content !== "string" || typeof entry.truncated !== "boolean") {
          throw new Error(`${label}.read content is invalid`);
        }
        if (![entry.startLine, entry.endLine, entry.totalLines].every(Number.isSafeInteger)) {
          throw new Error(`${label}.read line numbers are invalid`);
        }
      } else {
        if (typeof entry.supported !== "boolean" || typeof entry.reason !== "string") {
          throw new Error(`${label}.read unsupported disposition is invalid`);
        }
      }
      continue;
    }
    if (["denied", "failed", "invalid_input"].includes(entry.status)) {
      assertExactKeys(entry, ["path", "status", "code"], `${label}.read`);
      if (entry.status !== "invalid_input" && !READ_CODES.has(entry.code)) {
        throw new Error(`${label}.read has an invalid code`);
      }
      continue;
    }
    throw new Error(`${label}.read has an unsupported status`);
  }
}

function validateListResult(record, label) {
  assertExactKeys(record.result, ["lists"], `${label}.result`);
  if (!Array.isArray(record.result.lists) || record.result.lists.length > 32) {
    throw new Error(`${label}.result.lists must be a bounded array`);
  }
  for (const entry of record.result.lists) {
    if (!isObject(entry) || typeof entry.path !== "string") {
      throw new Error(`${label}.result.lists entries must carry a path`);
    }
    if (entry.status === "success") {
      assertExactKeys(
        entry,
        ["path", "status", "resolvedPath", "entries", "truncated"],
        `${label}.list`,
      );
      if (typeof entry.truncated !== "boolean" || !Array.isArray(entry.entries)) {
        throw new Error(`${label}.list success is invalid`);
      }
      for (const item of entry.entries) {
        if (!isObject(item) || typeof item.name !== "string" || typeof item.path !== "string") {
          throw new Error(`${label}.list entry is invalid`);
        }
        if (!["file", "directory", "symlink", "other"].includes(item.type)) {
          throw new Error(`${label}.list entry type is invalid`);
        }
      }
      continue;
    }
    if (["denied", "failed", "invalid_input"].includes(entry.status)) {
      assertExactKeys(entry, ["path", "status", "code"], `${label}.list`);
      if (!LIST_CODES.has(entry.code)) {
        throw new Error(`${label}.list has an invalid code`);
      }
      continue;
    }
    throw new Error(`${label}.list has an unsupported status`);
  }
}

function validateSearchResult(record, label) {
  assertExactKeys(record.result, ["searches"], `${label}.result`);
  if (!Array.isArray(record.result.searches) || record.result.searches.length > 32) {
    throw new Error(`${label}.result.searches must be a bounded array`);
  }
  for (const entry of record.result.searches) {
    if (!isObject(entry) || typeof entry.query !== "string") {
      throw new Error(`${label}.result.searches entries must carry a query`);
    }
    if (entry.status === "success") {
      assertExactKeys(
        entry,
        [
          "query",
          "status",
          "path",
          "matches",
          "scannedFiles",
          "skippedFiles",
          "truncated",
          "truncationReason",
        ],
        `${label}.search`,
      );
      if (!Array.isArray(entry.matches) || entry.matches.length > 100) {
        throw new Error(`${label}.search matches must be bounded`);
      }
      for (const match of entry.matches) {
        if (!isObject(match) || typeof match.path !== "string") {
          throw new Error(`${label}.search match is invalid`);
        }
        if (![match.line, match.column].every(Number.isSafeInteger)) {
          throw new Error(`${label}.search match coordinates are invalid`);
        }
        if (typeof match.text !== "string") {
          throw new Error(`${label}.search match text is invalid`);
        }
      }
      if (typeof entry.truncated !== "boolean") {
        throw new Error(`${label}.search truncated flag is invalid`);
      }
      if (entry.truncationReason !== null && !TRUNCATION_REASONS.has(entry.truncationReason)) {
        throw new Error(`${label}.search truncation reason is invalid`);
      }
      if (![entry.scannedFiles, entry.skippedFiles].every(Number.isSafeInteger)) {
        throw new Error(`${label}.search counters are invalid`);
      }
      continue;
    }
    if (["denied", "failed", "invalid_input"].includes(entry.status)) {
      assertExactKeys(entry, ["query", "status", "code"], `${label}.search`);
      if (!SEARCH_CODES.has(entry.code)) {
        throw new Error(`${label}.search has an invalid code`);
      }
      continue;
    }
    throw new Error(`${label}.search has an unsupported status`);
  }
}

function validateRevisionResult(record, label) {
  assertExactKeys(record.result, ["ops"], `${label}.result`);
  if (!Array.isArray(record.result.ops) || record.result.ops.length > 256) {
    throw new Error(`${label}.result.ops must be a bounded array`);
  }
  for (const entry of record.result.ops) {
    if (!isObject(entry) || typeof entry.op !== "string" || !Object.hasOwn(entry, "result")) {
      throw new Error(`${label}.result.ops entries are invalid`);
    }
    if (entry.result !== null) {
      if (typeof entry.result === "string" && !/^rev_[0-9a-f]{32}$/u.test(entry.result)) {
        throw new Error(`${label}.result.ops handle is invalid`);
      }
    }
  }
}

function validatePrepareResult(record, label) {
  assertExactKeys(
    record.result,
    ["prepares", "workspaceSha256", "checkpointCount"],
    `${label}.result`,
  );
  if (!Array.isArray(record.result.prepares) || record.result.prepares.length > 16) {
    throw new Error(`${label}.result.prepares must be a bounded array`);
  }
  for (const entry of record.result.prepares) {
    if (!isObject(entry) || typeof entry.tool !== "string") {
      throw new Error(`${label}.result.prepares entries must carry a tool`);
    }
    if (!["unavailable", "cancelled"].includes(entry.status)) {
      throw new Error(`${label}.result.prepares status is invalid`);
    }
    if (entry.status === "unavailable" && !PREPARE_CODES.has(entry.code)) {
      throw new Error(`${label}.result.prepares code is invalid`);
    }
  }
  if (!LOWER_SHA256.test(record.result.workspaceSha256)) {
    throw new Error(`${label}.result.workspaceSha256 is invalid`);
  }
  if (!Number.isSafeInteger(record.result.checkpointCount) || record.result.checkpointCount < 0) {
    throw new Error(`${label}.result.checkpointCount is invalid`);
  }
}

function validateCheckpointRecord(checkpoint, label) {
  if (!isObject(checkpoint) || typeof checkpoint.id !== "string") {
    throw new Error(`${label} checkpoints must carry an id`);
  }
  if (!CHECKPOINT_OPERATIONS_SET.has(checkpoint.operation)) {
    throw new Error(`${label} checkpoint operation is invalid`);
  }
  if (!CHECKPOINT_STATES_SET.has(checkpoint.state)) {
    throw new Error(`${label} checkpoint state is invalid`);
  }
  if (typeof checkpoint.fingerprintValid !== "boolean") {
    throw new Error(`${label} checkpoint fingerprint validity is invalid`);
  }
}

function validateCheckpointResult(record, label) {
  assertExactKeys(record.result, ["ops"], `${label}.result`);
  if (!Array.isArray(record.result.ops) || record.result.ops.length > 64) {
    throw new Error(`${label}.result.ops must be a bounded array`);
  }
  for (const entry of record.result.ops) {
    if (!isObject(entry) || typeof entry.op !== "string") {
      throw new Error(`${label}.result.ops entries are invalid`);
    }
    if (entry.op === "list" || entry.op === "list-after") {
      if (!Array.isArray(entry.checkpoints)) {
        throw new Error(`${label}.result.ops list must carry checkpoints`);
      }
      for (const checkpoint of entry.checkpoints) {
        validateCheckpointRecord(checkpoint, `${label}.result.ops`);
      }
      continue;
    }
    if (entry.op === "get") {
      if (entry.checkpoint !== null) {
        validateCheckpointRecord(entry.checkpoint, `${label}.result.ops`);
      }
      continue;
    }
    if (entry.op === "reconcile") {
      assertExactKeys(
        entry,
        ["op", "checked", "abandoned", "applied", "uncertain", "undoneAfterRestore"],
        `${label}.result.ops.reconcile`,
      );
      continue;
    }
    if (entry.op === "undo-plan") {
      if (!isObject(entry) || !UNDO_DECISIONS.has(entry.decision)) {
        throw new Error(`${label}.result.ops undo-plan decision is invalid`);
      }
      continue;
    }
    throw new Error(`${label}.result.ops has an unsupported op`);
  }
}

function validateGitResult(record, label) {
  assertExactKeys(record.result, ["disposition", "code"], `${label}.result`);
  if (record.result.disposition !== "unavailable") {
    throw new Error(`${label}.result.disposition is invalid`);
  }
  if (record.result.code !== "git_unavailable") {
    throw new Error(`${label}.result.code is invalid`);
  }
}

const LANGUAGE_SEVERITY_VALUES = new Set(["error", "warning", "info", "unknown"]);

function validateDiagnosticEntry(entry, label) {
  if (!isObject(entry)) {
    throw new Error(`${label} diagnostics entries must be objects`);
  }
  assertExactKeys(
    entry,
    ["source", "severity", "path", "line", "column", "code", "message", "rawCategory"],
    label,
  );
  if (typeof entry.source !== "string" || !LANGUAGE_SEVERITY_VALUES.has(entry.severity)) {
    throw new Error(`${label} diagnostic severity is invalid`);
  }
  if (entry.path !== null && typeof entry.path !== "string") {
    throw new Error(`${label} diagnostic path is invalid`);
  }
  if (entry.line !== null && !Number.isSafeInteger(entry.line)) {
    throw new Error(`${label} diagnostic line is invalid`);
  }
  if (entry.column !== null && !Number.isSafeInteger(entry.column)) {
    throw new Error(`${label} diagnostic column is invalid`);
  }
  if (entry.code !== null && typeof entry.code !== "string") {
    throw new Error(`${label} diagnostic code is invalid`);
  }
  if (typeof entry.message !== "string") {
    throw new Error(`${label} diagnostic message is invalid`);
  }
  if (entry.rawCategory !== null && typeof entry.rawCategory !== "string") {
    throw new Error(`${label} diagnostic rawCategory is invalid`);
  }
}

function validateRangeValue(range, label) {
  if (!isObject(range) || !isObject(range.start) || !isObject(range.end)) {
    throw new Error(`${label} range is invalid`);
  }
  assertExactKeys(range.start, ["line", "column"], label);
  assertExactKeys(range.end, ["line", "column"], label);
  if (
    ![range.start.line, range.start.column, range.end.line, range.end.column].every(
      Number.isSafeInteger,
    ) ||
    [range.start.line, range.start.column, range.end.line, range.end.column].some(
      (value) => value < 1,
    )
  ) {
    throw new Error(`${label} range coordinates must be positive integers`);
  }
}

function validateLanguageDiagnosticsResult(record, label) {
  assertExactKeys(record.result, ["documents", "aggregate"], `${label}.result`);
  if (!Array.isArray(record.result.documents) || record.result.documents.length > 64) {
    throw new Error(`${label}.result.documents must be a bounded array`);
  }
  for (const entry of record.result.documents) {
    if (!isObject(entry) || typeof entry.uri !== "string") {
      throw new Error(`${label}.result.documents entries must carry a uri`);
    }
    if (entry.status === "rejected") {
      assertExactKeys(entry, ["uri", "status"], `${label}.document`);
      continue;
    }
    if (entry.status === "normalized") {
      assertExactKeys(
        entry,
        ["uri", "status", "path", "revision", "diagnostics", "truncated"],
        `${label}.document`,
      );
      if (typeof entry.path !== "string") {
        throw new Error(`${label}.document path is invalid`);
      }
      if (entry.revision !== null && !/^rev_[0-9a-f]{32}$/u.test(entry.revision)) {
        throw new Error(`${label}.document revision is invalid`);
      }
      if (typeof entry.truncated !== "boolean" || !Array.isArray(entry.diagnostics)) {
        throw new Error(`${label}.document payload is invalid`);
      }
      for (const diagnostic of entry.diagnostics) {
        validateDiagnosticEntry(diagnostic, `${label}.document`);
      }
      continue;
    }
    throw new Error(`${label}.result.documents has an unsupported status`);
  }
  if (!isObject(record.result.aggregate)) {
    throw new Error(`${label}.result.aggregate must be an object`);
  }
  assertExactKeys(
    record.result.aggregate,
    ["diagnostics", "truncated"],
    `${label}.result.aggregate`,
  );
  if (
    typeof record.result.aggregate.truncated !== "boolean" ||
    !Array.isArray(record.result.aggregate.diagnostics)
  ) {
    throw new Error(`${label}.result.aggregate is invalid`);
  }
  for (const diagnostic of record.result.aggregate.diagnostics) {
    validateDiagnosticEntry(diagnostic, `${label}.result.aggregate`);
  }
}

function validateLanguageStructureResult(record, label) {
  assertExactKeys(record.result, ["summaries"], `${label}.result`);
  if (!Array.isArray(record.result.summaries) || record.result.summaries.length > 64) {
    throw new Error(`${label}.result.summaries must be a bounded array`);
  }
  for (const entry of record.result.summaries) {
    assertExactKeys(
      entry,
      ["path", "revision", "mode", "advisory", "truncated", "bytes", "text"],
      `${label}.summary`,
    );
    if (typeof entry.path !== "string" || typeof entry.text !== "string") {
      throw new Error(`${label}.summary text is invalid`);
    }
    if (entry.revision !== null && !/^rev_[0-9a-f]{32}$/u.test(entry.revision)) {
      throw new Error(`${label}.summary revision is invalid`);
    }
    if (entry.mode !== "summary" || entry.advisory !== true) {
      throw new Error(`${label}.summary advisory semantics are invalid`);
    }
    if (
      typeof entry.truncated !== "boolean" ||
      !Number.isSafeInteger(entry.bytes) ||
      entry.bytes < 0
    ) {
      throw new Error(`${label}.summary bounds are invalid`);
    }
  }
}

function validateLanguageDefinitionResult(record, label) {
  assertExactKeys(record.result, ["queries"], `${label}.result`);
  if (!Array.isArray(record.result.queries) || record.result.queries.length > 64) {
    throw new Error(`${label}.result.queries must be a bounded array`);
  }
  for (const entry of record.result.queries) {
    assertExactKeys(entry, ["uri", "path", "locations", "truncated"], `${label}.query`);
    if (
      typeof entry.uri !== "string" ||
      typeof entry.path !== "string" ||
      typeof entry.truncated !== "boolean"
    ) {
      throw new Error(`${label}.query is invalid`);
    }
    if (!Array.isArray(entry.locations) || entry.locations.length > 128) {
      throw new Error(`${label}.query locations must be a bounded array`);
    }
    for (const location of entry.locations) {
      if (
        !isObject(location) ||
        typeof location.path !== "string" ||
        typeof location.external !== "boolean"
      ) {
        throw new Error(`${label}.query location is invalid`);
      }
      validateRangeValue(location.range, `${label}.query location`);
    }
  }
}
const DOMAIN_STATE_VALUES = new Set(["absent", "installed", "enabled", "active"]);
const DOMAIN_OP_VALUES = new Set([
  "inspect",
  "install",
  "uninstall",
  "enable",
  "disable",
  "deactivate",
  "eligibility",
  "activate",
  "workspaceScan",
  "decide",
  "inspectAuthority",
  "invalid",
]);
const DOMAIN_FAILURE_CODE = /^[A-Z][A-Z0-9_]*$/u;

function validateDomainStringArray(value, maximum, label) {
  if (!Array.isArray(value) || value.length > maximum) {
    throw new Error(`${label} must be a bounded array`);
  }
  for (const entry of value) {
    if (typeof entry !== "string" || entry.length === 0) {
      throw new Error(`${label} entries must be non-empty strings`);
    }
  }
}

function validateDomainPackageValue(value, label) {
  if (!isObject(value)) {
    throw new Error(`${label} must be an object`);
  }
  assertExactKeys(value, ["id", "digest", "abi", "requestedCapabilities"], label);
  if (
    typeof value.id !== "string" ||
    typeof value.abi !== "string" ||
    !LOWER_SHA256.test(value.digest)
  ) {
    throw new Error(`${label} identity fields are invalid`);
  }
  validateDomainStringArray(value.requestedCapabilities, 32, `${label}.requestedCapabilities`);
}

function validateDomainBinding(value, label) {
  if (!isObject(value)) {
    throw new Error(`${label} must be an object`);
  }
  assertExactKeys(value, ["packageId", "digest", "abi"], label);
  if (
    typeof value.packageId !== "string" ||
    typeof value.abi !== "string" ||
    !LOWER_SHA256.test(value.digest)
  ) {
    throw new Error(`${label} identity fields are invalid`);
  }
}

function validateDomainActivation(value, label) {
  if (!isObject(value)) {
    throw new Error(`${label} must be an object`);
  }
  assertExactKeys(value, ["sessionId", "binding", "grant"], label);
  if (!Number.isSafeInteger(value.sessionId) || value.sessionId < 1) {
    throw new Error(`${label}.sessionId is invalid`);
  }
  validateDomainBinding(value.binding, `${label}.binding`);
  validateDomainStringArray(value.grant, 32, `${label}.grant`);
}

function validateDomainFailureOp(op, label) {
  assertExactKeys(op, ["op", "ok", "code"], label);
  if (op.ok !== false || typeof op.code !== "string" || !DOMAIN_FAILURE_CODE.test(op.code)) {
    throw new Error(`${label} failure op is invalid`);
  }
}

function validateDomainLifecycleResult(record, label) {
  assertExactKeys(record.result, ["ops"], `${label}.result`);
  if (!Array.isArray(record.result.ops) || record.result.ops.length > 128) {
    throw new Error(`${label}.result.ops must be a bounded array`);
  }
  for (const op of record.result.ops) {
    if (!isObject(op) || typeof op.op !== "string" || !DOMAIN_OP_VALUES.has(op.op)) {
      throw new Error(`${label}.result.ops entries must carry a known op`);
    }
    if (op.op === "inspect") {
      assertExactKeys(
        op,
        ["op", "state", "available", "enabled", "active", "package", "activation"],
        `${label}.inspect`,
      );
      if (!DOMAIN_STATE_VALUES.has(op.state)) {
        throw new Error(`${label}.inspect state is invalid`);
      }
      if (
        typeof op.available !== "boolean" ||
        typeof op.enabled !== "boolean" ||
        typeof op.active !== "boolean"
      ) {
        throw new Error(`${label}.inspect flags are invalid`);
      }
      if (op.package !== null) {
        validateDomainPackageValue(op.package, `${label}.inspect.package`);
      }
      if (op.activation !== null) {
        validateDomainActivation(op.activation, `${label}.inspect.activation`);
      }
      continue;
    }
    if (op.op === "install") {
      if (op.ok === true) {
        assertExactKeys(op, ["op", "ok", "state"], `${label}.install`);
        if (!DOMAIN_STATE_VALUES.has(op.state)) {
          throw new Error(`${label}.install state is invalid`);
        }
      } else {
        validateDomainFailureOp(op, `${label}.install`);
      }
      continue;
    }
    if (
      op.op === "uninstall" ||
      op.op === "enable" ||
      op.op === "disable" ||
      op.op === "deactivate"
    ) {
      if (op.ok === true) {
        assertExactKeys(op, ["op", "ok"], `${label}.${op.op}`);
      } else {
        validateDomainFailureOp(op, `${label}.${op.op}`);
      }
      continue;
    }
    if (op.op === "eligibility") {
      if (op.ready === true || op.ready === false) {
        assertExactKeys(op, ["op", "ready", "reasons"], `${label}.eligibility`);
        validateDomainStringArray(op.reasons, 8, `${label}.eligibility.reasons`);
      } else {
        validateDomainFailureOp(op, `${label}.eligibility`);
      }
      continue;
    }
    if (op.op === "activate") {
      if (op.ok === true) {
        assertExactKeys(op, ["op", "ok", "sessionId", "binding", "grant"], `${label}.activate`);
        if (!Number.isSafeInteger(op.sessionId) || op.sessionId < 1) {
          throw new Error(`${label}.activate sessionId is invalid`);
        }
        validateDomainBinding(op.binding, `${label}.activate.binding`);
        validateDomainStringArray(op.grant, 32, `${label}.activate.grant`);
      } else {
        const keys =
          op.code === "CAPABILITY_DENIED" || op.code === "UNDECLARED_CAPABILITY"
            ? ["op", "ok", "code", "missing"]
            : ["op", "ok", "code"];
        assertExactKeys(op, keys, `${label}.activate`);
        if (op.ok !== false || typeof op.code !== "string" || !DOMAIN_FAILURE_CODE.test(op.code)) {
          throw new Error(`${label}.activate failure is invalid`);
        }
        if (op.code === "CAPABILITY_DENIED" || op.code === "UNDECLARED_CAPABILITY") {
          validateDomainStringArray(op.missing, 32, `${label}.activate.missing`);
        }
      }
      continue;
    }
    if (op.op === "workspaceScan") {
      assertExactKeys(
        op,
        [
          "op",
          "files",
          "candidates",
          "installs",
          "enables",
          "activations",
          "downloads",
          "recommendations",
        ],
        `${label}.workspaceScan`,
      );
      if (!Array.isArray(op.files) || op.files.length > 256) {
        throw new Error(`${label}.workspaceScan files must be a bounded array`);
      }
      for (const file of op.files) {
        if (!isObject(file)) {
          throw new Error(`${label}.workspaceScan files entries must be objects`);
        }
        assertExactKeys(file, ["name", "kind"], `${label}.workspaceScan file`);
        if (typeof file.name !== "string" || file.kind !== "opaque") {
          throw new Error(`${label}.workspaceScan file classification is invalid`);
        }
      }
      for (const key of [
        "candidates",
        "installs",
        "enables",
        "activations",
        "downloads",
        "recommendations",
      ]) {
        if (!Number.isSafeInteger(op[key]) || op[key] < 0) {
          throw new Error(`${label}.workspaceScan ${key} is invalid`);
        }
      }
      continue;
    }
    throw new Error(`${label}.result.ops has an unsupported lifecycle op`);
  }
}

function validateDomainCapabilityResult(record, label) {
  assertExactKeys(record.result, ["ops"], `${label}.result`);
  if (!Array.isArray(record.result.ops) || record.result.ops.length > 128) {
    throw new Error(`${label}.result.ops must be a bounded array`);
  }
  for (const op of record.result.ops) {
    if (!isObject(op) || typeof op.op !== "string" || !DOMAIN_OP_VALUES.has(op.op)) {
      throw new Error(`${label}.result.ops entries must carry a known op`);
    }
    if (op.op === "decide") {
      if (op.granted === true) {
        assertExactKeys(op, ["op", "granted", "grant"], `${label}.decide`);
        validateDomainStringArray(op.grant, 32, `${label}.decide.grant`);
      } else if (op.granted === false) {
        assertExactKeys(op, ["op", "granted", "missing"], `${label}.decide`);
        validateDomainStringArray(op.missing, 32, `${label}.decide.missing`);
      } else {
        validateDomainFailureOp(op, `${label}.decide`);
      }
      continue;
    }
    if (op.op === "inspectAuthority") {
      assertExactKeys(op, ["op", "authority"], `${label}.inspectAuthority`);
      validateDomainStringArray(op.authority, 32, `${label}.inspectAuthority.authority`);
      continue;
    }
    if (op.op === "invalid") {
      assertExactKeys(op, ["op", "ok", "code"], `${label}.invalid`);
      continue;
    }
    throw new Error(`${label}.result.ops has an unsupported capability op`);
  }
}

const PROVIDER_TURN_FAILURE_CODES = new Set([
  "LIMIT_ASSISTANT_TEXT_BYTES",
  "LIMIT_TEXT_EVENT_COUNT",
  "LIMIT_TOOL_CALL_COUNT",
  "LIMIT_CALL_ID_BYTES",
  "LIMIT_TOOL_NAME_BYTES",
  "LIMIT_TOOL_ARGUMENT_BYTES",
  "LIMIT_AGGREGATE_TURN_BYTES",
  "INVALID_TOOL_ARGUMENT_JSON",
  "EVENT_AFTER_COMPLETION",
  "EOF_WITHOUT_COMPLETION",
  "UNKNOWN_EVENT_TYPE",
  "MALFORMED_EVENT",
  "MALFORMED_TEXT_EVENT",
  "MALFORMED_TOOL_CALL",
  "INVALID_TRANSCRIPT",
  "PROVIDER_FAILED",
]);
const TOOL_RESULT_FAILURE_STATUSES = new Set([
  "invalid_input",
  "denied",
  "conflict",
  "failed",
  "cancelled",
  "timed_out",
  "output_limit",
  "sandbox_denied",
  "sandbox_unavailable",
  "workspace_violation",
  "unavailable",
]);

function validateToolCallEntry(entry, label) {
  if (!isObject(entry)) {
    throw new Error(`${label} tool calls must be objects`);
  }
  if (entry.kind === "execute") {
    assertExactKeys(entry, ["kind", "callId", "toolName", "input"], label);
  } else if (entry.kind === "invalid") {
    assertExactKeys(entry, ["kind", "callId", "toolName", "message"], label);
    if (typeof entry.message !== "string" || entry.message.length === 0) {
      throw new Error(`${label} invalid tool call message is invalid`);
    }
  } else {
    throw new Error(`${label} tool call kind is invalid`);
  }
  if (
    typeof entry.callId !== "string" ||
    entry.callId.length === 0 ||
    Buffer.byteLength(entry.callId, "utf8") > 256 ||
    typeof entry.toolName !== "string" ||
    Buffer.byteLength(entry.toolName, "utf8") > 256
  ) {
    throw new Error(`${label} tool call identity fields are invalid`);
  }
}

function validateDetachRecord(detach, label) {
  if (!isObject(detach)) {
    throw new Error(`${label} detach must be an object`);
  }
  if (detach.ok === true) {
    assertExactKeys(detach, ["ok", "result", "byteLength"], label);
    if (!Number.isSafeInteger(detach.byteLength) || detach.byteLength < 0) {
      throw new Error(`${label} detach byteLength is invalid`);
    }
    if (!isObject(detach.result)) {
      throw new Error(`${label} detach result must be an object`);
    }
    if (detach.result.status === "success") {
      assertExactKeys(detach.result, ["status", "output", "summary"], label);
      if (typeof detach.result.summary !== "string") {
        throw new Error(`${label} detach success summary is invalid`);
      }
    } else {
      assertExactKeys(detach.result, ["status", "message"], label);
      if (!TOOL_RESULT_FAILURE_STATUSES.has(detach.result.status)) {
        throw new Error(`${label} detach failure status is invalid`);
      }
      if (typeof detach.result.message !== "string") {
        throw new Error(`${label} detach failure message is invalid`);
      }
    }
  } else if (detach.ok === false) {
    assertExactKeys(detach, ["ok", "message"], label);
    if (typeof detach.message !== "string" || detach.message.length === 0) {
      throw new Error(`${label} detach rejection message is invalid`);
    }
  } else {
    throw new Error(`${label} detach ok flag is invalid`);
  }
}

function validateProviderTurnTurnRecord(turn, label) {
  if (turn.kind === "turn") {
    assertExactKeys(turn, ["kind", "assistantText", "textDeltas", "toolCalls"], label);
    if (
      typeof turn.assistantText !== "string" ||
      Buffer.byteLength(turn.assistantText, "utf8") > 65_536
    ) {
      throw new Error(`${label} assistantText is invalid`);
    }
    if (!Array.isArray(turn.textDeltas) || turn.textDeltas.length > 4096) {
      throw new Error(`${label} textDeltas must be bounded`);
    }
    for (const delta of turn.textDeltas) {
      if (typeof delta !== "string" || Buffer.byteLength(delta, "utf8") > 65_536) {
        throw new Error(`${label} textDeltas entries are invalid`);
      }
    }
    if (!Array.isArray(turn.toolCalls) || turn.toolCalls.length > 32) {
      throw new Error(`${label} toolCalls must be bounded`);
    }
    for (const entry of turn.toolCalls) {
      validateToolCallEntry(entry, `${label}.toolCalls`);
    }
    return;
  }
  if (turn.kind === "cancelled") {
    assertExactKeys(turn, ["kind"], label);
    return;
  }
  if (turn.kind === "failed") {
    assertExactKeys(turn, ["kind", "failure", "message"], label);
    if (typeof turn.failure !== "string" || !PROVIDER_TURN_FAILURE_CODES.has(turn.failure)) {
      throw new Error(`${label} failure category is invalid`);
    }
    if (typeof turn.message !== "string" || turn.message.length === 0) {
      throw new Error(`${label} failure message is invalid`);
    }
    return;
  }
  throw new Error(`${label} turn kind is invalid`);
}

function validateContextProjectionResult(record, label) {
  assertExactKeys(record.result, ["cases"], `${label}.result`);
  if (!Array.isArray(record.result.cases) || record.result.cases.length > 32) {
    throw new Error(`${label}.result.cases must be a bounded array`);
  }
  for (const [index, entry] of record.result.cases.entries()) {
    const caseLabel = `${label}.result.cases[${index}]`;
    if (!isObject(entry) || typeof entry.kind !== "string") {
      throw new Error(`${caseLabel} must be an object with string kind`);
    }
    // Each context-projection case record contains exactly {kind, result}
    assertExactKeys(entry, ["kind", "result"], caseLabel);
    if (!isObject(entry.result)) {
      throw new Error(`${caseLabel}.result must be an object`);
    }
    // Permit arbitrary context-projection result fields; deep validation is
    // semantic parity (oracle vs candidate byte-identical), not structural rejection.
    // Bound check: result must be serializable within the probe output bound.
    if (Buffer.byteLength(canonicalizeJson(entry.result), "utf8") > CONTRACT_LIMITS.recordsBytes) {
      throw new Error(`${caseLabel}.result exceeds the harness bound`);
    }
  }
}

const USER_CONFIG_ERROR_CATEGORIES = new Set([
  "CANNOT_READ",
  "NOT_REGULAR",
  "TOO_LARGE",
  "INVALID_UTF8",
  "INVALID_JSON",
  "INVALID_VALUE",
  "UNKNOWN_REVIEW_PROVIDER",
]);

function validateUserConfigDiagnostics(diagnostics, label) {
  assertExactKeys(
    diagnostics,
    [
      "loaded",
      "sections",
      "unknownFields",
      "validationErrors",
      "credentialRefs",
      "overrideInUse",
      "fileState",
    ],
    label,
  );
  if (
    typeof diagnostics.loaded !== "boolean" ||
    !Array.isArray(diagnostics.sections) ||
    diagnostics.sections.length !== 4 ||
    !Array.isArray(diagnostics.unknownFields) ||
    !Array.isArray(diagnostics.validationErrors) ||
    !Array.isArray(diagnostics.credentialRefs) ||
    typeof diagnostics.overrideInUse !== "boolean" ||
    !["readable", "missing", "unreadable"].includes(diagnostics.fileState)
  ) {
    throw new Error(`${label} has invalid fields`);
  }
  const names = ["sandbox", "godot", "quality", "references"];
  for (const [index, section] of diagnostics.sections.entries()) {
    assertExactKeys(section, ["name", "present"], `${label}.sections[${index}]`);
    if (section.name !== names[index] || typeof section.present !== "boolean") {
      throw new Error(`${label}.sections order or value is invalid`);
    }
  }
  for (const error of diagnostics.validationErrors) {
    if (!USER_CONFIG_ERROR_CATEGORIES.has(error)) {
      throw new Error(`${label}.validationErrors contains an unsupported category`);
    }
  }
  for (const value of diagnostics.unknownFields.concat(diagnostics.credentialRefs)) {
    if (typeof value !== "string") {
      throw new Error(`${label} string arrays are invalid`);
    }
  }
}

function validateUserConfigValue(config, label) {
  assertExactKeys(config, ["sandbox", "godot", "quality", "references"], label);
  assertExactKeys(config.sandbox, ["profile", "backend"], `${label}.sandbox`);
  if (
    !["inspect", "develop-offline"].includes(config.sandbox.profile) ||
    !["auto", "anthropic-runtime"].includes(config.sandbox.backend)
  ) {
    throw new Error(`${label}.sandbox is invalid`);
  }
  assertExactKeys(
    config.godot,
    ["activeInstallation", "installations", "discoverOnPath"],
    `${label}.godot`,
  );
  if (
    config.godot.activeInstallation !== null &&
    typeof config.godot.activeInstallation !== "string"
  ) {
    throw new Error(`${label}.godot.activeInstallation is invalid`);
  }
  if (!isObject(config.godot.installations) || typeof config.godot.discoverOnPath !== "boolean") {
    throw new Error(`${label}.godot is invalid`);
  }
  for (const [id, installation] of Object.entries(config.godot.installations)) {
    assertExactKeys(installation, ["path", "editionHint"], `${label}.installation.${id}`);
    if (
      typeof installation.path !== "string" ||
      !["standard", "dotnet", "unknown"].includes(installation.editionHint)
    ) {
      throw new Error(`${label}.installation.${id} is invalid`);
    }
  }
  assertExactKeys(config.quality, ["reviewProvider"], `${label}.quality`);
  if (config.quality.reviewProvider !== null && typeof config.quality.reviewProvider !== "string") {
    throw new Error(`${label}.quality is invalid`);
  }
  if (!isObject(config.references)) {
    throw new Error(`${label}.references is invalid`);
  }
  for (const [alias, reference] of Object.entries(config.references)) {
    if (!isObject(reference) || typeof reference.kind !== "string") {
      throw new Error(`${label}.references.${alias} is invalid`);
    }
    if (reference.kind === "local-directory") {
      const keys = Object.hasOwn(reference, "description")
        ? ["kind", "path", "description"]
        : ["kind", "path"];
      assertExactKeys(reference, keys, `${label}.references.${alias}`);
      if (typeof reference.path !== "string") {
        throw new Error(`${label}.references.${alias}.path is invalid`);
      }
    } else if (reference.kind === "repository") {
      const keys = ["kind", "repository"];
      if (Object.hasOwn(reference, "ref")) keys.push("ref");
      if (Object.hasOwn(reference, "description")) keys.push("description");
      assertExactKeys(reference, keys, `${label}.references.${alias}`);
      if (typeof reference.repository !== "string") {
        throw new Error(`${label}.references.${alias}.repository is invalid`);
      }
      if (Object.hasOwn(reference, "ref")) {
        assertExactKeys(
          reference.ref,
          ["kind", reference.ref.kind],
          `${label}.references.${alias}.ref`,
        );
        if (
          !["commit", "tag", "branch"].includes(reference.ref.kind) ||
          typeof reference.ref[reference.ref.kind] !== "string"
        ) {
          throw new Error(`${label}.references.${alias}.ref is invalid`);
        }
      }
    } else {
      throw new Error(`${label}.references.${alias}.kind is invalid`);
    }
  }
}

function validateUserConfigResult(record, label) {
  assertExactKeys(record.result, ["cases"], `${label}.result`);
  if (!Array.isArray(record.result.cases) || record.result.cases.length > 64) {
    throw new Error(`${label}.result.cases must be a bounded array`);
  }
  for (const [index, entry] of record.result.cases.entries()) {
    const caseLabel = `${label}.result.cases[${index}]`;
    if (!isObject(entry) || typeof entry.status !== "string") {
      throw new Error(`${caseLabel} is invalid`);
    }
    if (entry.status === "ok") {
      assertExactKeys(
        entry,
        ["status", "config", "reviewProviderId", "referenceConfigError", "diagnostics"],
        caseLabel,
      );
      validateUserConfigValue(entry.config, `${caseLabel}.config`);
      if (entry.reviewProviderId !== "deterministic-fake") {
        throw new Error(`${caseLabel}.reviewProviderId is invalid`);
      }
      if (entry.referenceConfigError !== null && typeof entry.referenceConfigError !== "string") {
        throw new Error(`${caseLabel}.referenceConfigError is invalid`);
      }
    } else if (entry.status === "error") {
      assertExactKeys(entry, ["status", "category", "diagnostics"], caseLabel);
      if (!USER_CONFIG_ERROR_CATEGORIES.has(entry.category)) {
        throw new Error(`${caseLabel}.category is invalid`);
      }
    } else {
      throw new Error(`${caseLabel}.status is invalid`);
    }
    validateUserConfigDiagnostics(entry.diagnostics, `${caseLabel}.diagnostics`);
  }
}

function validateProviderTurnResult(record, label) {
  assertExactKeys(record.result, ["cases"], `${label}.result`);
  if (!Array.isArray(record.result.cases) || record.result.cases.length > 32) {
    throw new Error(`${label}.result.cases must be a bounded array`);
  }
  for (const [index, entry] of record.result.cases.entries()) {
    const caseLabel = `${label}.result.cases[${index}]`;
    if (!isObject(entry)) {
      throw new Error(`${caseLabel} must be an object`);
    }
    const hasTurn = Object.hasOwn(entry, "turn");
    const hasDetach = Object.hasOwn(entry, "detach");
    if (hasTurn === hasDetach) {
      throw new Error(`${caseLabel} must have exactly one of turn/detach`);
    }
    if (hasTurn) {
      validateProviderTurnTurnRecord(entry.turn, `${caseLabel}.turn`);
    } else {
      validateDetachRecord(entry.detach, `${caseLabel}.detach`);
    }
  }
}

function validateGodotSceneResolveResult(result, label) {
  assertExactKeys(result, ["status", "diagnostics", "truncated"], label);
  if (!["complete", "partial", "invalid"].includes(result.status)) {
    throw new Error(`${label}.result.status is invalid`);
  }
  if (!Number.isSafeInteger(result.diagnostics) || result.diagnostics < 0) {
    throw new Error(`${label}.result.diagnostics is invalid`);
  }
  if (typeof result.truncated !== "boolean") {
    throw new Error(`${label}.result.truncated is invalid`);
  }
}

const GODOT_INSTALLATION_SOURCES = new Set([
  "user-config",
  "path",
  "cli-path",
  "cli-installation",
  "environment-path",
  "environment-installation",
  "active-config",
]);

function validateGodotOverview(overview, label) {
  if (!isObject(overview)) {
    throw new Error(`${label} overview must be an object`);
  }
  assertExactKeys(
    overview,
    ["id", "sourceLabel", "source", "invalid", "isDuplicate", "selected"],
    label,
  );
  if (
    typeof overview.id !== "string" ||
    typeof overview.sourceLabel !== "string" ||
    !GODOT_INSTALLATION_SOURCES.has(overview.source)
  ) {
    throw new Error(`${label} overview identity is invalid`);
  }
  if (
    (overview.invalid !== null && typeof overview.invalid !== "string") ||
    typeof overview.isDuplicate !== "boolean" ||
    typeof overview.selected !== "boolean"
  ) {
    throw new Error(`${label} overview fields are invalid`);
  }
}

const GODOT_SEVERITY_VALUES = new Set(["info", "warning", "error"]);

function validateGodotDiscoveryResult(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  const keys = Object.keys(result).sort();
  if (keys.length === 2 && keys[0] === "error" && keys[1] === "ok") {
    if (result.ok !== false || typeof result.error !== "string" || result.error.length === 0) {
      throw new Error(`${label}.result failure shape is invalid`);
    }
    return;
  }
  if (keys.length === 2 && keys[0] === "ok" && keys[1] === "selected") {
    if (typeof result.ok !== "boolean" || typeof result.selected !== "boolean") {
      throw new Error(`${label}.result select shape is invalid`);
    }
    return;
  }
  assertExactKeys(
    result,
    ["ok", "selected", "candidates", "configuration", "rationale", "diagnostics"],
    `${label}.result`,
  );
  if (
    !Array.isArray(result.candidates) ||
    result.candidates.length > 64 ||
    !Array.isArray(result.rationale) ||
    result.rationale.length > 32 ||
    !Array.isArray(result.diagnostics) ||
    result.diagnostics.length > 64
  ) {
    throw new Error(`${label}.result arrays must be bounded`);
  }
  for (const candidate of result.candidates) {
    validateGodotOverview(candidate, `${label}.result.candidates`);
  }
  validateGodotOverviewOrNothing(result.selected, `${label}.result.selected`);
  for (const diagnostic of result.diagnostics) {
    if (
      !isObject(diagnostic) ||
      !GODOT_SEVERITY_VALUES.has(diagnostic.severity) ||
      typeof diagnostic.message !== "string"
    ) {
      throw new Error(`${label}.result.diagnostics entry is invalid`);
    }
  }
  if (!isObject(result.configuration)) {
    throw new Error(`${label}.result.configuration must be an object`);
  }
  assertExactKeys(
    result.configuration,
    ["activeInstallation", "configuredCount", "discoverOnPath", "overrides"],
    `${label}.result.configuration`,
  );
  validateBoundedStringArray(result.configuration.overrides, 8, `${label}.configuration.overrides`);
  if (
    (result.configuration.activeInstallation !== null &&
      typeof result.configuration.activeInstallation !== "string") ||
    !Number.isSafeInteger(result.configuration.configuredCount) ||
    result.configuration.configuredCount < 0 ||
    typeof result.configuration.discoverOnPath !== "boolean"
  ) {
    throw new Error(`${label}.result.configuration is invalid`);
  }
}

function validateGodotOverviewOrNothing(value, label) {
  if (value !== null) {
    validateGodotOverview(value, label);
  }
}

const GODOT_KNOWLEDGE_STATUS_STATES = new Set(["ready", "unavailable", "unsupported"]);
const GODOT_KNOWLEDGE_NONREADY_STATUSES = new Set([
  "unavailable",
  "unsupported",
  "failed",
  "cancelled",
  "invalid_input",
  "not_found",
]);

function validateGodotKnowledgeResult(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (Object.hasOwn(result, "state")) {
    assertExactKeys(
      result,
      ["state", "reason", "platform", "cacheEnabled", "schemaVersion", "profile", "manualChannel"],
      `${label}.result`,
    );
    if (
      !GODOT_KNOWLEDGE_STATUS_STATES.has(result.state) ||
      (result.reason !== null && typeof result.reason !== "string") ||
      typeof result.platform !== "string" ||
      typeof result.cacheEnabled !== "boolean" ||
      !Number.isSafeInteger(result.schemaVersion) ||
      result.profile !== null ||
      (result.manualChannel !== null && typeof result.manualChannel !== "string")
    ) {
      throw new Error(`${label}.result status shape is invalid`);
    }
    return;
  }
  assertExactKeys(result, ["status", "message"], `${label}.result`);
  if (!GODOT_KNOWLEDGE_NONREADY_STATUSES.has(result.status) || typeof result.message !== "string") {
    throw new Error(`${label}.result outcome shape is invalid`);
  }
}

const GODOT_PREPARATION_STATUSES = new Set([
  "unavailable",
  "unsupported",
  "invalid_input",
  "failed",
  "cancelled",
]);
const GODOT_RUN_STATUSES = new Set([
  ...GODOT_PREPARATION_STATUSES,
  "conflict",
  "denied",
  "timed-out",
  "sandbox-failed",
]);

function validateGodotDiagnosticsResult(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (Object.hasOwn(result, "status")) {
    assertExactKeys(result, ["status", "message"], `${label}.result`);
    if (!GODOT_RUN_STATUSES.has(result.status) || typeof result.message !== "string") {
      throw new Error(`${label}.result outcome shape is invalid`);
    }
    return;
  }
  if (Object.hasOwn(result, "reason")) {
    assertExactKeys(result, ["state", "reason", "platform"], `${label}.result`);
    if (
      result.state !== "unavailable" ||
      (result.reason !== null && typeof result.reason !== "string") ||
      typeof result.platform !== "string"
    ) {
      throw new Error(`${label}.result support shape is invalid`);
    }
    return;
  }
  assertExactKeys(result, ["state"], `${label}.result`);
  if (!["untrusted", "check-invalidated"].includes(result.state)) {
    throw new Error(`${label}.result state is invalid`);
  }
}

function validateGodotLspResult(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (Object.hasOwn(result, "status")) {
    assertExactKeys(result, ["status", "message"], `${label}.result`);
    if (!GODOT_PREPARATION_STATUSES.has(result.status) || typeof result.message !== "string") {
      throw new Error(`${label}.result outcome shape is invalid`);
    }
    return;
  }
  if (Object.hasOwn(result, "reason")) {
    assertExactKeys(result, ["state", "reason", "platform"], `${label}.result`);
    if (
      result.state !== "unavailable" ||
      (result.reason !== null && typeof result.reason !== "string") ||
      typeof result.platform !== "string"
    ) {
      throw new Error(`${label}.result support shape is invalid`);
    }
    return;
  }
  assertExactKeys(
    result,
    ["state", "openDocumentCount", "diagnosticCount", "networkIsolation"],
    `${label}.result`,
  );
  if (
    !["starting", "ready", "stale", "closed", "unavailable"].includes(result.state) ||
    ![result.openDocumentCount, result.diagnosticCount].every(
      (value) => Number.isSafeInteger(value) && value >= 0,
    ) ||
    !["loopback-only", "unverified", "unavailable"].includes(result.networkIsolation)
  ) {
    throw new Error(`${label}.result status shape is invalid`);
  }
}

const GODOT_REVIEW_CONTEXT_KINDS = new Set([
  "script",
  "scene",
  "resource",
  "autoload",
  "signal-endpoint",
  "test",
  "project-config",
]);
const GODOT_RELATION_KINDS = new Set([
  "script_attachment",
  "scene_inheritance",
  "scene_instancing",
  "resource_dependency",
  "script_dependency",
  "signal_connection",
  "autoload_global",
  "test_covers",
]);

function validateGodotReviewContextResult(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(
    result,
    [
      "taskId",
      "taskContractRevision",
      "primaryChanges",
      "relatedSurfaces",
      "regressionAreas",
      "validation",
      "evidence",
      "completeness",
      "diagnostics",
    ],
    `${label}.result`,
  );
  if (
    typeof result.taskId !== "string" ||
    !Number.isSafeInteger(result.taskContractRevision) ||
    result.taskContractRevision < 1 ||
    !["complete", "bounded", "partial"].includes(result.completeness)
  ) {
    throw new Error(`${label}.result identity is invalid`);
  }
  const checkSurface = (surface, surfaceLabel) => {
    const keys = Object.keys(surface).sort();
    const required = ["path", "kind", "revision", "confidence", "evidence"].sort();
    for (const key of keys) {
      if (!["path", "kind", "revision", "confidence", "evidence", "note"].includes(key)) {
        throw new Error(`${surfaceLabel} has unknown fields`);
      }
    }
    for (const key of required) {
      if (!keys.includes(key)) {
        throw new Error(`${surfaceLabel} has missing fields`);
      }
    }
    if (
      typeof surface.path !== "string" ||
      !GODOT_REVIEW_CONTEXT_KINDS.has(surface.kind) ||
      (surface.revision !== null && typeof surface.revision !== "string") ||
      !["verified", "candidate"].includes(surface.confidence) ||
      typeof surface.evidence !== "string" ||
      (surface.note !== undefined && surface.note !== null && typeof surface.note !== "string")
    ) {
      throw new Error(`${surfaceLabel} is invalid`);
    }
  };
  if (!Array.isArray(result.primaryChanges) || result.primaryChanges.length > 16) {
    throw new Error(`${label}.result.primaryChanges must be bounded`);
  }
  for (const surface of result.primaryChanges) {
    checkSurface(surface, `${label}.result.primaryChanges`);
  }
  if (!Array.isArray(result.relatedSurfaces) || result.relatedSurfaces.length > 64) {
    throw new Error(`${label}.result.relatedSurfaces must be bounded`);
  }
  for (const relation of result.relatedSurfaces) {
    assertExactKeys(
      relation,
      [
        "kind",
        "sourcePath",
        "targetPath",
        "sourceRevision",
        "targetRevision",
        "confidence",
        "evidence",
      ],
      `${label}.result.relatedSurfaces`,
    );
    if (
      !GODOT_RELATION_KINDS.has(relation.kind) ||
      typeof relation.sourcePath !== "string" ||
      typeof relation.targetPath !== "string" ||
      (relation.sourceRevision !== null && typeof relation.sourceRevision !== "string") ||
      (relation.targetRevision !== null && typeof relation.targetRevision !== "string") ||
      !["verified", "candidate"].includes(relation.confidence) ||
      typeof relation.evidence !== "string"
    ) {
      throw new Error(`${label}.result.relatedSurfaces entry is invalid`);
    }
  }
  for (const key of ["regressionAreas", "validation", "evidence", "diagnostics"]) {
    if (!Array.isArray(result[key])) {
      throw new Error(`${label}.result.${key} must be an array`);
    }
  }
  for (const area of result.regressionAreas) {
    assertExactKeys(area, ["id", "title", "reason", "surfaces"], `${label}.result.regressionAreas`);
    if (
      typeof area.id !== "string" ||
      typeof area.title !== "string" ||
      typeof area.reason !== "string" ||
      !Array.isArray(area.surfaces)
    ) {
      throw new Error(`${label}.result.regressionAreas entry is invalid`);
    }
  }
  for (const recommendation of result.validation) {
    assertExactKeys(
      recommendation,
      ["kind", "priority", "rationale", "surfaces"],
      `${label}.result.validation`,
    );
    if (typeof recommendation.rationale !== "string" || !Array.isArray(recommendation.surfaces)) {
      throw new Error(`${label}.result.validation entry is invalid`);
    }
  }
  for (const evidence of result.evidence) {
    if (typeof evidence !== "string") {
      throw new Error(`${label}.result.evidence entries are invalid`);
    }
  }
  for (const diagnostic of result.diagnostics) {
    assertExactKeys(diagnostic, ["code", "message"], `${label}.result.diagnostics`);
    if (typeof diagnostic.code !== "string" || typeof diagnostic.message !== "string") {
      throw new Error(`${label}.result.diagnostics entry is invalid`);
    }
  }
}

function validateGodotMutationPrepareResult(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  const keys = Object.keys(result).sort();
  if (keys.length === 2 && keys[0] === "error" && keys[1] === "ok") {
    if (result.ok !== false || typeof result.error !== "string") {
      throw new Error(`${label}.result failure shape is invalid`);
    }
    return;
  }
  assertExactKeys(
    result,
    ["ok", "fingerprint", "operations", "expectedSemanticEffect", "structuralSummary", "diff"],
    `${label}.result`,
  );
  if (
    result.ok !== true ||
    !/^[0-9a-f]{64}$/u.test(result.fingerprint ?? "") ||
    typeof result.structuralSummary !== "string" ||
    typeof result.diff !== "string" ||
    !Array.isArray(result.operations) ||
    result.operations.some((operation) => !isObject(operation) || typeof operation.op !== "string")
  ) {
    throw new Error(`${label}.result prepared shape is invalid`);
  }
  for (const expectation of result.expectedSemanticEffect) {
    if (!isObject(expectation) || typeof expectation.kind !== "string") {
      throw new Error(`${label}.result.expectedSemanticEffect entry is invalid`);
    }
  }
}

const GODOT_SURFACE_KINDS = new Set(["script_only", "native_only", "mixed", "none"]);

function validateGodotDevelopPlanResult(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(
    result,
    [
      "surface",
      "edges",
      "unresolvedReferences",
      ...(Object.hasOwn(result, "applyOrderError") ? ["applyOrderError"] : ["applyOrder"]),
    ],
    `${label}.result`,
  );
  const surface = result.surface;
  if (
    !isObject(surface) ||
    !GODOT_SURFACE_KINDS.has(surface.kind) ||
    typeof surface.rationale !== "string" ||
    !Array.isArray(surface.evidence)
  ) {
    throw new Error(`${label}.result.surface is invalid`);
  }
  if (!Array.isArray(result.edges)) {
    throw new Error(`${label}.result.edges must be an array`);
  }
  for (const edge of result.edges) {
    assertExactKeys(edge, ["before", "after"], `${label}.result.edges`);
    if (typeof edge.before !== "string" || typeof edge.after !== "string") {
      throw new Error(`${label}.result.edges entry is invalid`);
    }
  }
  if (!Array.isArray(result.unresolvedReferences)) {
    throw new Error(`${label}.result.unresolvedReferences must be an array`);
  }
  for (const reference of result.unresolvedReferences) {
    assertExactKeys(reference, ["targetId", "path"], `${label}.result.unresolvedReferences`);
  }
  if (Object.hasOwn(result, "applyOrderError")) {
    if (typeof result.applyOrderError !== "string") {
      throw new Error(`${label}.result.applyOrderError is invalid`);
    }
    return;
  }
  const applyOrder = result.applyOrder;
  if (
    !isObject(applyOrder) ||
    !Array.isArray(applyOrder.order) ||
    applyOrder.order.some((id) => typeof id !== "string") ||
    typeof applyOrder.rationale !== "string"
  ) {
    throw new Error(`${label}.result.applyOrder is invalid`);
  }
}

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
  if (typeof record.subject !== "string" || !ALLOWED_SUBJECTS.has(record.subject)) {
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
