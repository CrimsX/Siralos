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

function validateApplyResult(record, label) {
  assertExactKeys(
    record.result,
    ["applies", "classified", "workspaceSha256", "checkpointCount"],
    `${label}.result`,
  );
  if (!Array.isArray(record.result.applies) || record.result.applies.length > 16) {
    throw new Error(`${label}.result.applies must be a bounded array`);
  }
  for (const entry of record.result.applies) {
    assertExactKeys(entry, ["tool", "status", "code"], `${label}.result.applies`);
    if (typeof entry.tool !== "string") {
      throw new Error(`${label}.result.applies entries must carry a tool`);
    }
    if (entry.status !== "unavailable") {
      throw new Error(`${label}.result.applies status is invalid`);
    }
    if (entry.code !== "mutation_unavailable") {
      throw new Error(`${label}.result.applies code is invalid`);
    }
  }
  if (
    !Array.isArray(record.result.classified) ||
    record.result.classified.some((entry) => typeof entry !== "string")
  ) {
    throw new Error(`${label}.result.classified is invalid`);
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

const R13_CAPABILITY_IDS = new Set([
  "workspace.read",
  "workspace.write",
  "git.inspect",
  "godot.inspect",
  "godot.probe_project",
  "godot.api",
  "godot.diagnose",
  "godot.lsp",
  "godot.development",
  "process.execute",
  "network.outbound",
  "reference.inspect",
  "research.fetch",
  "self.inspect",
]);
const R13_PROFILE_IDS = new Set([
  "inspect",
  "develop-offline",
  "validation-offline",
  "godot-probe-offline",
  "godot-recovery-probe-offline",
  "godot-diagnostics-offline",
  "godot-lsp-local",
]);
const R13_PERMISSION_RULES = new Set(["allow", "ask", "deny"]);
const R13_DOCTOR_AREAS = new Set([
  "runtime",
  "configuration",
  "providers",
  "sandbox",
  "workspace",
  "godot",
  "project",
  "references",
  "research",
  "capabilities",
  "determinism",
  "readiness",
]);

function r13IsSha256(value) {
  return typeof value === "string" && LOWER_SHA256.test(value);
}

function validateSecurityPermissionsResultCase(entry, label) {
  if (Object.hasOwn(entry, "profiles")) {
    assertExactKeys(entry, ["profiles"], label);
    if (!Array.isArray(entry.profiles) || entry.profiles.length !== R13_PROFILE_IDS.size) {
      throw new Error(`${label}.profiles must cover every built-in profile`);
    }
    for (const [index, profile] of entry.profiles.entries()) {
      const profileLabel = `${label}.profiles[${index}]`;
      assertExactKeys(profile, ["id", "rules"], profileLabel);
      if (!R13_PROFILE_IDS.has(profile.id)) {
        throw new Error(`${profileLabel}.id is invalid`);
      }
      if (!Array.isArray(profile.rules) || profile.rules.length !== R13_CAPABILITY_IDS.size) {
        throw new Error(`${profileLabel}.rules must cover every capability`);
      }
      for (const pair of profile.rules) {
        if (
          !Array.isArray(pair) ||
          pair.length !== 2 ||
          !R13_CAPABILITY_IDS.has(pair[0]) ||
          !R13_PERMISSION_RULES.has(pair[1])
        ) {
          throw new Error(`${profileLabel}.rules entries are invalid`);
        }
      }
    }
    return;
  }
  if (Object.hasOwn(entry, "baseSha256")) {
    assertExactKeys(entry, ["baseSha256", "sameBinding", "changedBinding"], label);
    if (!r13IsSha256(entry.baseSha256)) {
      throw new Error(`${label}.baseSha256 is invalid`);
    }
    if (typeof entry.sameBinding !== "boolean" || typeof entry.changedBinding !== "boolean") {
      throw new Error(`${label} binding flags are invalid`);
    }
    return;
  }
  if (Object.hasOwn(entry, "protected")) {
    assertExactKeys(entry, ["protected"], label);
    if (
      !Array.isArray(entry.protected) ||
      entry.protected.length > 32 ||
      entry.protected.some((flag) => typeof flag !== "boolean")
    ) {
      throw new Error(`${label}.protected must be a bounded boolean array`);
    }
    return;
  }
  const decisionKeys = entry.decision === "allow" ? ["decision"] : ["decision", "reason"];
  assertExactKeys(entry, decisionKeys, label);
  if (!R13_PERMISSION_RULES.has(entry.decision)) {
    throw new Error(`${label}.decision is invalid`);
  }
  if (
    entry.decision !== "allow" &&
    (typeof entry.reason !== "string" || entry.reason.length === 0)
  ) {
    throw new Error(`${label}.reason is invalid`);
  }
}

function validateCommandCatalogResultCase(entry, label) {
  if (Object.hasOwn(entry, "entries")) {
    assertExactKeys(entry, ["entries", "revision"], label);
    if (!Array.isArray(entry.entries) || entry.entries.length > 64) {
      throw new Error(`${label}.entries must be a bounded array`);
    }
    for (const item of entry.entries) {
      assertExactKeys(item, ["id", "description", "group"], `${label}.entries[]`);
      if (
        typeof item.id !== "string" ||
        typeof item.description !== "string" ||
        typeof item.group !== "string"
      ) {
        throw new Error(`${label}.entries[] fields are invalid`);
      }
    }
    if (!r13IsSha256(entry.revision)) {
      throw new Error(`${label}.revision is invalid`);
    }
    return;
  }
  if (Object.hasOwn(entry, "found")) {
    if (entry.found === false) {
      assertExactKeys(entry, ["found"], label);
      return;
    }
    assertExactKeys(entry, ["found", "entry"], label);
    if (!isObject(entry.entry)) {
      throw new Error(`${label}.entry must be an object`);
    }
    assertExactKeys(entry.entry, ["id", "description", "group"], `${label}.entry`);
    return;
  }
  if (Object.hasOwn(entry, "stable")) {
    assertExactKeys(entry, ["stable", "ids"], label);
    if (
      typeof entry.stable !== "boolean" ||
      !Array.isArray(entry.ids) ||
      entry.ids.some((id) => typeof id !== "string")
    ) {
      throw new Error(`${label} recomputation record is invalid`);
    }
    return;
  }
  assertExactKeys(entry, ["nodeScript", "npmScript"], label);
  for (const runner of [entry.nodeScript, entry.npmScript]) {
    assertExactKeys(runner, ["definitionId", "available"], `${label} runner`);
    if (typeof runner.definitionId !== "string" || runner.available !== false) {
      throw new Error(`${label} runner availability must be truthfully unavailable`);
    }
  }
}

function validateCapabilityDoctorResultCase(entry, label) {
  if (Object.hasOwn(entry, "failing")) {
    assertExactKeys(entry, ["clean", "failing"], label);
    for (const side of [entry.failing, entry.clean]) {
      assertExactKeys(side, ["counts", "exit"], `${label} side`);
      assertExactKeys(side.counts, ["pass", "warn", "fail", "skip", "total"], `${label} counts`);
      for (const value of Object.values(side.counts)) {
        if (!Number.isSafeInteger(value) || value < 0) {
          throw new Error(`${label} counts are invalid`);
        }
      }
      if (side.exit !== 0 && side.exit !== 1) {
        throw new Error(`${label}.exit is invalid`);
      }
    }
    return;
  }
  if (Object.hasOwn(entry, "all")) {
    assertExactKeys(entry, ["all", "reordered", "emptyMeansAll", "unknownArea"], label);
    for (const list of [entry.all, entry.reordered]) {
      if (!Array.isArray(list) || list.some((area) => !R13_DOCTOR_AREAS.has(area))) {
        throw new Error(`${label} area lists are invalid`);
      }
    }
    if (typeof entry.emptyMeansAll !== "boolean" || entry.unknownArea !== "doctor_invocation") {
      throw new Error(`${label} normalization record is invalid`);
    }
    return;
  }
  if (Object.hasOwn(entry, "checks")) {
    assertExactKeys(
      entry,
      ["checks", "detailsDropped", "errorCategories", "secretsOnlyRelativeKept"],
      label,
    );
    if (!Array.isArray(entry.checks) || entry.checks.length > 8) {
      throw new Error(`${label}.checks must be a bounded array`);
    }
    for (const check of entry.checks) {
      assertExactKeys(check, ["id", "summary"], `${label}.checks[]`);
    }
    if (entry.detailsDropped !== true || typeof entry.secretsOnlyRelativeKept !== "boolean") {
      throw new Error(`${label} redaction record is invalid`);
    }
    return;
  }
  if (Object.hasOwn(entry, "revision")) {
    assertExactKeys(
      entry,
      ["name", "revision", "sensitiveToVersion", "stableRepeat", "toolAbi"],
      label,
    );
    if (
      entry.name !== "@siralos" ||
      !r13IsSha256(entry.revision) ||
      !r13IsSha256(entry.toolAbi) ||
      typeof entry.sensitiveToVersion !== "boolean" ||
      typeof entry.stableRepeat !== "boolean"
    ) {
      throw new Error(`${label} self-reference record is invalid`);
    }
    return;
  }
  assertExactKeys(entry, ["sectionNames", "stable"], label);
  if (
    !Array.isArray(entry.sectionNames) ||
    entry.sectionNames.join(",") !== "sandbox,godot,quality,references" ||
    typeof entry.stable !== "boolean"
  ) {
    throw new Error(`${label} config-schema record is invalid`);
  }
}

function validateInstructionsResolutionCase(entry, label) {
  if (Object.hasOwn(entry, "order")) {
    assertExactKeys(entry, ["order", "revisionPrefix"], label);
    if (!Array.isArray(entry.order) || typeof entry.revisionPrefix !== "string") {
      throw new Error(`${label} ordering record is invalid`);
    }
    for (const item of entry.order) {
      assertExactKeys(item, ["id", "kind", "scope", "priority"], `${label}.order[]`);
      if (!/^instr_[0-9a-f]{24}$/.test(item.id) || !Number.isInteger(item.priority)) {
        throw new Error(`${label}.order[] identity is invalid`);
      }
    }
    return;
  }
  if (Object.hasOwn(entry, "insideApplies")) {
    assertExactKeys(
      entry,
      ["insideApplies", "outsideEmpty", "universalAppliesToBoth", "trailingNormalized"],
      label,
    );
    for (const key of [
      "insideApplies",
      "outsideEmpty",
      "universalAppliesToBoth",
      "trailingNormalized",
    ]) {
      if (typeof entry[key] !== "boolean") throw new Error(`${label}.${key} must be boolean`);
    }
    return;
  }
  if (Object.hasOwn(entry, "conflictCount")) {
    assertExactKeys(
      entry,
      ["agreeingConflictCount", "conflictCount", "rawBytesDiffer", "reason"],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "sameId")) {
    assertExactKeys(entry, ["differentId", "idFormat", "normalizedProbe", "sameId"], label);
    return;
  }
  if (Object.hasOwn(entry, "stable")) {
    assertExactKeys(entry, ["revisionChangesOnSourceRevision", "stable"], label);
    for (const key of ["revisionChangesOnSourceRevision", "stable"]) {
      if (typeof entry[key] !== "boolean") throw new Error(`${label}.${key} must be boolean`);
    }
    return;
  }
  if (Object.hasOwn(entry, "leadsWithAuthorityFraming")) {
    assertExactKeys(
      entry,
      [
        "conflictReasonIncluded",
        "conflictSurfaced",
        "leadsWithAuthorityFraming",
        "neverGrantsMentioned",
      ],
      label,
    );
    for (const key of [
      "conflictReasonIncluded",
      "conflictSurfaced",
      "leadsWithAuthorityFraming",
      "neverGrantsMentioned",
    ]) {
      if (typeof entry[key] !== "boolean") throw new Error(`${label}.${key} must be boolean`);
    }
    return;
  }
  assertExactKeys(entry, ["orderInsensitive"], label);
  if (typeof entry.orderInsensitive !== "boolean") {
    throw new Error(`${label}.orderInsensitive must be boolean`);
  }
}

function validateKnowledgeRevisionsCase(entry, label) {
  if (Object.hasOwn(entry, "status") && Object.hasOwn(entry, "fact")) {
    assertExactKeys(entry, ["digestMatchesModel", "fact", "size", "status"], label);
    const fact = entry.fact;
    if (fact !== null) {
      assertExactKeys(
        fact,
        [
          "activation",
          "confidence",
          "contentDigest",
          "id",
          "revision",
          "subjectKey",
          "type",
          "volatility",
        ],
        `${label}.fact`,
      );
      if (!/^kf_[0-9a-f]{24}$/.test(fact.id)) {
        throw new Error(`${label}.fact.id is invalid`);
      }
    }
    return;
  }
  if (Object.hasOwn(entry, "unchangedStatus")) {
    assertExactKeys(
      entry,
      [
        "evolvedRevision",
        "firstRevision",
        "historyLength",
        "stateRevisionStable",
        "unchangedStatus",
      ],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "alwaysAllowReason")) {
    assertExactKeys(
      entry,
      ["alwaysAllowReason", "factualAccepted", "noApprovalRejected", "sameReasonText"],
      label,
    );
    return;
  }
  if (
    Object.hasOwn(entry, "rejected") &&
    Object.hasOwn(entry, "reason") &&
    Object.keys(entry).length === 2
  ) {
    assertExactKeys(entry, ["reason", "rejected"], label);
    return;
  }
  if (Object.hasOwn(entry, "goodFileAccepted")) {
    assertExactKeys(
      entry,
      [
        "badShaReason",
        "badShaRejected",
        "goodFileAccepted",
        "researchWithPortAccepted",
        "researchWithoutPortReason",
      ],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "selected")) {
    assertExactKeys(
      entry,
      ["budget", "consideredCount", "facts", "omittedCount", "selected"],
      label,
    );
    for (const selection of entry.selected) {
      assertExactKeys(selection, ["factId", "matchReasons", "score"], `${label}.selected[]`);
      if (!Array.isArray(selection.matchReasons)) {
        throw new Error(`${label}.selected[].matchReasons must be an array`);
      }
    }
    if (
      !Array.isArray(entry.facts) ||
      typeof entry.consideredCount !== "number" ||
      typeof entry.omittedCount !== "number" ||
      !isObject(entry.budget)
    ) {
      throw new Error(`${label} retrieval record is invalid`);
    }
    return;
  }
  if (Object.hasOwn(entry, "candidateCount") && Object.hasOwn(entry, "subjectKeys")) {
    assertExactKeys(
      entry,
      ["candidateCount", "hasHasDotnet", "hasName", "hasVersion", "subjectKeys"],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "acceptedCount") && Object.hasOwn(entry, "activeFacts")) {
    assertExactKeys(
      entry,
      ["acceptedCount", "activeFacts", "candidateCount", "hasDotnetFact"],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "emptyVersionCount")) {
    assertExactKeys(
      entry,
      ["emptyVersionCount", "emptyVersionHasDotnet", "nullNameCount", "nullNameHasVersion"],
      label,
    );
    return;
  }
  assertExactKeys(
    entry,
    [
      "afterRetire",
      "beforeRetire",
      "pinAOk",
      "pinBOk",
      "pinCExhausted",
      "pinCReason",
      "revisionChanged",
    ],
    label,
  );
  assertExactKeys(
    entry.afterRetire,
    ["activeHasSubject", "historyKept", "retiredListed"],
    `${label}.afterRetire`,
  );
}

function validateReferenceIdentityCase(entry, label) {
  if (typeof entry.name !== "string" || entry.name.length === 0) {
    throw new Error(`${label}.name must be a non-empty string`);
  }
  if (Object.hasOwn(entry, "attempts")) {
    assertExactKeys(
      entry,
      [
        "aliasInvalidLength",
        "aliasValid",
        "attempts",
        "countReason",
        "idDeterministic",
        "idSample",
        "mismatchReason",
        "name",
        "validSectionOk",
      ],
      label,
    );
    if (!/^ref_[0-9a-f]{24}$/.test(entry.idSample)) {
      throw new Error(`${label}.idSample is invalid`);
    }
    return;
  }
  if (Object.hasOwn(entry, "results")) {
    assertExactKeys(entry, ["name", "results"], label);
    for (const item of entry.results) {
      if (typeof item.tag !== "string" || typeof item.ok !== "boolean") {
        throw new Error(`${label}.results[] is invalid`);
      }
      if (item.ok && !Object.hasOwn(item, "reason")) {
        continue;
      }
    }
    return;
  }
  if (Object.hasOwn(entry, "declinedStatus")) {
    assertExactKeys(
      entry,
      [
        "declinedReason",
        "declinedStatus",
        "name",
        "pinnedCommit",
        "pinnedStatus",
        "preResolverSpyCalls",
        "requestedRef",
        "resolvedAtMatchesClock",
        "resolvedCommit",
        "resolvedStatus",
      ],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "pureChecks")) {
    assertExactKeys(
      entry,
      ["demotion", "name", "pureChecks", "realEnumeration", "references"],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "statuses")) {
    assertExactKeys(
      entry,
      ["duplicateReason", "firstAddressable", "name", "sharedId", "size", "statuses"],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "matrix")) {
    assertExactKeys(
      entry,
      ["idFormat", "idsStableAcrossRegistries", "matrix", "name", "order"],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "unchangedStatus")) {
    assertExactKeys(
      entry,
      [
        "bindingRetainsHistorical",
        "declinedRefreshReason",
        "declinedRefreshStatus",
        "failedStatus",
        "name",
        "refreshedStatus",
        "refreshedTimestamp",
        "revisionNullAfterFailure",
        "unknownRefreshReason",
        "unknownRefreshStatus",
        "unchangedKeptTimestamp",
        "unchangedStatus",
      ],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "evictedReadsNull")) {
    assertExactKeys(
      entry,
      ["b2Snapshot", "b3Snapshot", "currentFingerprint", "evictedReadsNull", "name"],
      label,
    );
    return;
  }
  if (
    Object.hasOwn(entry, "count") &&
    Object.hasOwn(entry, "firstPath") &&
    Object.keys(entry).length === 4
  ) {
    assertExactKeys(entry, ["count", "firstPath", "name", "secondPath"], label);
    return;
  }
  if (
    Object.hasOwn(entry, "localStatus") &&
    Object.hasOwn(entry, "repositoryStatus") &&
    Object.keys(entry).length === 3
  ) {
    assertExactKeys(entry, ["localStatus", "name", "repositoryStatus"], label);
    return;
  }
  if (Object.hasOwn(entry, "matchCount") && Object.keys(entry).length === 3) {
    assertExactKeys(entry, ["firstMatch", "matchCount", "name"], label);
    return;
  }
  if (Object.hasOwn(entry, "visibleWhenReady") && Object.keys(entry).length === 3) {
    assertExactKeys(entry, ["hiddenWhenNone", "name", "visibleWhenReady"], label);
    return;
  }
  assertExactKeys(
    entry,
    [
      "localMaterializationStatus",
      "localRootMatchesCanonical",
      "localStatus",
      "name",
      "repositoryMaterializationStatus",
      "repositoryReason",
      "repositoryStatus",
      "unknownStatus",
    ],
    label,
  );
}

function validateResearchPolicyCase(entry, label) {
  if (typeof entry.name !== "string" || entry.name.length === 0) {
    throw new Error(`${label}.name must be a non-empty string`);
  }
  if (Object.hasOwn(entry, "profiles")) {
    assertExactKeys(
      entry,
      [
        "askBranch",
        "denyBranch",
        "evaluatorDecisionForInspect",
        "gateSpyCalls",
        "name",
        "profiles",
      ],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "normalizedMaxBytes")) {
    assertExactKeys(entry, ["name", "normalizedMaxBytes", "results", "validationSpyCalls"], label);
    return;
  }
  if (Object.hasOwn(entry, "byIdStatus")) {
    assertExactKeys(
      entry,
      [
        "byIdDocumentSourceId",
        "byIdStatus",
        "byLabelDocumentSourceId",
        "byLabelStatus",
        "name",
        "unconfiguredReason",
        "unconfiguredStatus",
      ],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "noneTaskStatus")) {
    assertExactKeys(
      entry,
      [
        "blankTaskIdStatus",
        "evidenceRevision",
        "evidenceTaskId",
        "name",
        "noneTaskReason",
        "noneTaskStatus",
        "zeroRevisionStatus",
      ],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "retainedEvidenceCount")) {
    assertExactKeys(entry, ["name", "reason", "retainedEvidenceCount", "status"], label);
    return;
  }
  if (Object.hasOwn(entry, "abortedPreFetchStatus")) {
    assertExactKeys(
      entry,
      [
        "abortedPreFetchReason",
        "abortedPreFetchStatus",
        "abortedSpyCalls",
        "activeRequestsAfter",
        "cancelledReason",
        "cancelledStatus",
        "name",
        "timeoutReason",
        "timeoutStatus",
      ],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "sectionLimit")) {
    assertExactKeys(
      entry,
      [
        "classification",
        "digestCheck",
        "headingBound",
        "idDeterministic",
        "idFormatOk",
        "idSample",
        "jsonCases",
        "name",
        "plainOverflow",
        "sectionLimit",
      ],
      label,
    );
    if (!/^rd_[0-9a-f]{24}$/.test(entry.idSample)) {
      throw new Error(`${label}.idSample is invalid`);
    }
    return;
  }
  if (Object.hasOwn(entry, "directFetchedAtMatchesClock")) {
    assertExactKeys(
      entry,
      [
        "branchPin",
        "commitPin",
        "direct",
        "directFetchedAtMatchesClock",
        "fallbackCase",
        "name",
        "unknownTopicReason",
        "unknownTopicStatus",
      ],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "idsSeen")) {
    assertExactKeys(
      entry,
      [
        "excerptByteLengths",
        "idsSeen",
        "name",
        "retainedIds",
        "sequenceOrdering",
        "snapshotDetached",
        "truncatedFlags",
      ],
      label,
    );
    return;
  }
  if (Object.hasOwn(entry, "count") && Object.hasOwn(entry, "firstEvidenceId")) {
    assertExactKeys(entry, ["count", "firstEvidenceId", "name"], label);
    return;
  }
  if (Object.hasOwn(entry, "found") && Object.hasOwn(entry, "notFoundStatus")) {
    assertExactKeys(entry, ["found", "name", "notFoundStatus"], label);
    return;
  }
  if (Object.hasOwn(entry, "visibleWhenAllow")) {
    assertExactKeys(entry, ["hiddenWhenDeny", "name", "visibleWhenAllow"], label);
    return;
  }
  if (Object.hasOwn(entry, "hasProvenance")) {
    assertExactKeys(entry, ["hasProvenance", "hasSource", "name"], label);
    return;
  }
  assertExactKeys(
    entry,
    ["boundedTruncated", "boundedView", "defaultMaxBytes", "name", "view"],
    label,
  );
}

function validateR13AuthorityResult(record, label) {
  assertExactKeys(record.result, ["cases"], `${label}.result`);
  if (
    !Array.isArray(record.result.cases) ||
    record.result.cases.length === 0 ||
    record.result.cases.length > 32
  ) {
    throw new Error(`${label}.result.cases must be a bounded array`);
  }
  for (const [index, entry] of record.result.cases.entries()) {
    const caseLabel = `${label}.result.cases[${index}]`;
    if (!isObject(entry)) {
      throw new Error(`${caseLabel} must be an object`);
    }
    if (record.subject === "security-permissions") {
      validateSecurityPermissionsResultCase(entry, caseLabel);
    } else if (record.subject === "command-catalog") {
      validateCommandCatalogResultCase(entry, caseLabel);
    } else {
      validateCapabilityDoctorResultCase(entry, caseLabel);
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

function validateIcmPhaseContractResult(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (result.ok === true) {
    if (Array.isArray(result.registry)) {
      assertExactKeys(result, ["ok", "registry"], `${label}.result`);
      for (const entry of result.registry) {
        assertExactKeys(entry, ["id", "digest"], `${label}.result.registry`);
        if (typeof entry.id !== "string" || !LOWER_SHA256.test(entry.digest ?? "")) {
          throw new Error(`${label}.result.registry entry is invalid`);
        }
      }
      return;
    }
    assertExactKeys(result, ["ok", "id", "version", "digest"], `${label}.result`);
    if (
      typeof result.id !== "string" ||
      !Number.isSafeInteger(result.version) ||
      result.version < 1 ||
      !LOWER_SHA256.test(result.digest)
    ) {
      throw new Error(`${label}.result contract identity is invalid`);
    }
    return;
  }
  assertExactKeys(result, ["ok", "error"], `${label}.result`);
  if (typeof result.error !== "string" || result.error.length === 0) {
    throw new Error(`${label}.result.error is invalid`);
  }
}

function validateIcmDependencyManifestsResult(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  // staleness
  if (isObject(result.stale) && Object.hasOwn(result, "unrelatedChanges")) {
    assertExactKeys(result, ["stale", "current", "unrelatedChanges", "digest"], `${label}.result`);
    for (const [id, reason] of Object.entries(result.stale)) {
      if (typeof id !== "string" || typeof reason !== "string") {
        throw new Error(`${label}.result.stale entry is invalid`);
      }
    }
    for (const key of ["current", "unrelatedChanges"]) {
      if (!Array.isArray(result[key]) || result[key].some((entry) => typeof entry !== "string")) {
        throw new Error(`${label}.result.${key} is invalid`);
      }
    }
    if (!LOWER_SHA256.test(result.digest ?? "")) {
      throw new Error(`${label}.result.digest is invalid`);
    }
    return;
  }
  // prepared-mutation-stale
  if (typeof result.stale === "boolean" && Array.isArray(result.stalePaths)) {
    assertExactKeys(result, ["stale", "stalePaths"], `${label}.result`);
    if (result.stalePaths.some((entry) => typeof entry !== "string")) {
      throw new Error(`${label}.result.stalePaths is invalid`);
    }
    return;
  }
  // manifest create/build or provenance ref creation failure
  if (Object.hasOwn(result, "ok")) {
    if (result.ok === false) {
      assertExactKeys(result, ["ok", "error"], `${label}.result`);
      if (typeof result.error !== "string" || result.error.length === 0) {
        throw new Error(`${label}.result.error is invalid`);
      }
      return;
    }
    assertExactKeys(result, ["ok", "manifest"], `${label}.result`);
    if (result.manifest === null) {
      return;
    }
    const manifest = result.manifest;
    assertExactKeys(
      manifest,
      ["artifactType", "artifactId", "dependsOn", "digest"],
      `${label}.result.manifest`,
    );
    if (!Array.isArray(manifest.dependsOn)) {
      throw new Error(`${label}.result.manifest.dependsOn is invalid`);
    }
    for (const dependency of manifest.dependsOn) {
      assertExactKeys(dependency, ["artifactType", "digest"], `${label}.result.manifest.dependsOn`);
      if (typeof dependency.artifactType !== "string") {
        throw new Error(`${label}.result.manifest.dependsOn entry is invalid`);
      }
    }
    return;
  }
  // provenance
  if (isObject(result.created)) {
    assertExactKeys(result, ["created", "digest"], `${label}.result`);
    assertExactKeys(result.created, ["item", "source"], `${label}.result.created`);
    assertExactKeys(
      result.created.source,
      ["kind", "id", "digest"],
      `${label}.result.created.source`,
    );
    if (!LOWER_SHA256.test(result.digest ?? "")) {
      throw new Error(`${label}.result.digest is invalid`);
    }
    return;
  }
  // why-validation-required
  if (Object.hasOwn(result, "found")) {
    assertExactKeys(result, ["found", "itemId", "rendered"], `${label}.result`);
    if (
      typeof result.found !== "boolean" ||
      typeof result.itemId !== "string" ||
      typeof result.rendered !== "string"
    ) {
      throw new Error(`${label}.result why-diagnostic is invalid`);
    }
    return;
  }
  throw new Error(`${label}.result shape is unsupported`);
}

const RR_FAILURE_KINDS = new Set([
  "readiness_failed",
  "spawn_failed",
  "sandbox_denied",
  "startup_timeout",
  "idle_timeout",
  "hard_timeout",
  "cancelled",
  "process_crashed",
  "kill_failed",
  "output_limit",
  "artifact_limit",
  "environment_unavailable",
  "cleanup_failed",
]);
const RR_TERMINAL_STATUSES = new Set([
  "success",
  "failure",
  "cancelled",
  "resource_limit",
  "uncertain",
]);

function validateRrObservation(observation, label) {
  if (!isObject(observation) || typeof observation.type !== "string") {
    throw new Error(`${label} observation is invalid`);
  }
}

function validateRrStateView(state, label) {
  assertExactKeys(
    state,
    ["state", "startedAtMs", "terminatedAtMs", "terminalDisposition", "failureKind"],
    label,
  );
  if (
    typeof state.state !== "string" ||
    (state.terminalDisposition !== null && !RR_TERMINAL_STATUSES.has(state.terminalDisposition)) ||
    (state.failureKind !== null && !RR_FAILURE_KINDS.has(state.failureKind))
  ) {
    throw new Error(`${label} state view is invalid`);
  }
}

function validateRuntimeReadinessResult(subject, result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (subject === "runtime-readiness.identity") {
    if (result.ok === true) {
      assertExactKeys(result, ["ok", "runId"], `${label}.result`);
      return;
    }
    if (Object.hasOwn(result, "operationId")) {
      assertExactKeys(result, ["operationId"], `${label}.result`);
      return;
    }
    if (isObject(result.ref)) {
      assertExactKeys(result, ["ref", "formatted"], `${label}.result`);
      assertExactKeys(
        result.ref,
        ["taskId", "phaseId", "runId", "operationId", "producer"],
        `${label}.result.ref`,
      );
      return;
    }
    assertExactKeys(result, ["ok", "error"], `${label}.result`);
    return;
  }
  if (subject === "runtime-readiness.budgets") {
    if (Object.hasOwn(result, "digest")) {
      assertExactKeys(result, ["digest", "rendered"], `${label}.result`);
      if (!LOWER_SHA256.test(result.digest ?? "")) {
        throw new Error(`${label}.result.digest is invalid`);
      }
      return;
    }
    assertExactKeys(result, ["cases"], `${label}.result`);
    for (const entry of result.cases) {
      if (entry.status === "admit") {
        assertExactKeys(entry, ["status", "truncated"], `${label}.result.cases`);
      } else {
        assertExactKeys(entry, ["status", "reason"], `${label}.result.cases`);
        if (typeof entry.reason !== "string") {
          throw new Error(`${label}.result.cases reason is invalid`);
        }
      }
    }
    return;
  }
  if (subject === "runtime-readiness.lifecycle") {
    assertExactKeys(result, ["steps", "expectedFailureKind"], `${label}.result`);
    for (const step of result.steps) {
      assertExactKeys(step, ["atMs", "observations", "state"], `${label}.result.steps`);
      for (const observation of step.observations) {
        validateRrObservation(observation, `${label}.result.steps.observations`);
      }
      validateRrStateView(step.state, `${label}.result.steps`);
    }
    if (result.expectedFailureKind !== null && !RR_FAILURE_KINDS.has(result.expectedFailureKind)) {
      throw new Error(`${label}.result.expectedFailureKind is invalid`);
    }
    return;
  }
  // runtime-readiness.doctor
  if (Object.hasOwn(result, "headless")) {
    assertExactKeys(result, ["headless", "visual"], `${label}.result`);
    for (const key of ["headless", "visual"]) {
      assertExactKeys(result[key], ["ready", "digest"], `${label}.result.${key}`);
      if (!LOWER_SHA256.test(result[key].digest ?? "")) {
        throw new Error(`${label}.result.${key}.digest is invalid`);
      }
    }
    return;
  }
  assertExactKeys(
    result,
    ["ready", "executionAllowed", "blockedReasons", "items", "digest", "rendered"],
    `${label}.result`,
  );
  if (!LOWER_SHA256.test(result.digest ?? "")) {
    throw new Error(`${label}.result.digest is invalid`);
  }
  if (
    typeof result.ready !== "boolean" ||
    typeof result.executionAllowed !== "boolean" ||
    !Array.isArray(result.blockedReasons) ||
    !Array.isArray(result.items)
  ) {
    throw new Error(`${label}.result readiness summary is invalid`);
  }
  for (const item of result.items) {
    assertExactKeys(item, ["id", "state", "detail"], `${label}.result.items`);
  }
}

function validateRuntimeV32Result(subject, result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (subject === "runtime-execution") {
    if (Object.hasOwn(result, "error")) {
      assertExactKeys(result, ["error", "available"], `${label}.result`);
      if (typeof result.error !== "string") {
        throw new Error(`${label}.result.error is invalid`);
      }
      return;
    }
    assertExactKeys(result, ["outcome", "available", "reason"], `${label}.result`);
    if (typeof result.available !== "boolean") {
      throw new Error(`${label}.result.available is invalid`);
    }
    if (typeof result.reason !== "string") {
      throw new Error(`${label}.result.reason is invalid`);
    }
    const outcome = result.outcome;
    if (!isObject(outcome)) {
      throw new Error(`${label}.result.outcome must be an object`);
    }
    if (outcome.disposition === "success") {
      assertExactKeys(outcome, ["disposition", "runId", "operationId"], `${label}.result.outcome`);
      return;
    }
    assertExactKeys(outcome, ["disposition", "reason"], `${label}.result.outcome`);
    if (typeof outcome.reason !== "string") {
      throw new Error(`${label}.result.outcome.reason is invalid`);
    }
    return;
  }
  // runtime-evidence
  if (Object.hasOwn(result, "error")) {
    assertExactKeys(result, ["error"], `${label}.result`);
    if (typeof result.error !== "string") {
      throw new Error(`${label}.result.error is invalid`);
    }
    return;
  }
  assertExactKeys(result, ["evidence", "rendered"], `${label}.result`);
  if (typeof result.rendered !== "string") {
    throw new Error(`${label}.result.rendered is invalid`);
  }
  const evidence = result.evidence;
  if (!isObject(evidence)) {
    throw new Error(`${label}.result.evidence must be an object`);
  }
  assertExactKeys(
    evidence,
    [
      "runId",
      "operationId",
      "exitCode",
      "durationMs",
      "stdoutLength",
      "stderrLength",
      "truncated",
      "artifactDigest",
      "digest",
    ],
    `${label}.result.evidence`,
  );
  if (typeof evidence.truncated !== "boolean") {
    throw new Error(`${label}.result.evidence.truncated is invalid`);
  }
  if (!Number.isSafeInteger(evidence.stdoutLength) || evidence.stdoutLength < 0) {
    throw new Error(`${label}.result.evidence.stdoutLength is invalid`);
  }
  if (!Number.isSafeInteger(evidence.stderrLength) || evidence.stderrLength < 0) {
    throw new Error(`${label}.result.evidence.stderrLength is invalid`);
  }
  if (!LOWER_SHA256.test(evidence.artifactDigest ?? "")) {
    throw new Error(`${label}.result.evidence.artifactDigest is invalid`);
  }
  if (!LOWER_SHA256.test(evidence.digest ?? "")) {
    throw new Error(`${label}.result.evidence.digest is invalid`);
  }
}

function validateGodotRuntimeV34Result(subject, result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (subject === "godot-runtime-launch") {
    if (Object.hasOwn(result, "error")) {
      assertExactKeys(result, ["error", "available"], `${label}.result`);
      if (typeof result.error !== "string" || typeof result.available !== "boolean") {
        throw new Error(`${label}.result error shape is invalid`);
      }
      return;
    }
    assertExactKeys(result, ["outcome", "available", "reason", "engine"], `${label}.result`);
    if (typeof result.available !== "boolean" || typeof result.reason !== "string") {
      throw new Error(`${label}.result availability shape is invalid`);
    }
    const outcome = result.outcome;
    if (!isObject(outcome)) {
      throw new Error(`${label}.result.outcome must be an object`);
    }
    assertExactKeys(outcome, ["disposition", "reason", "isUnavailable"], `${label}.result.outcome`);
    if (typeof outcome.disposition !== "string" || typeof outcome.isUnavailable !== "boolean") {
      throw new Error(`${label}.result.outcome shape is invalid`);
    }
    if (outcome.reason !== null && typeof outcome.reason !== "string") {
      throw new Error(`${label}.result.outcome.reason is invalid`);
    }
    const engine = result.engine;
    if (!isObject(engine)) {
      throw new Error(`${label}.result.engine must be an object`);
    }
    assertExactKeys(
      engine,
      ["engineId", "engineVersion", "projectPath", "mode"],
      `${label}.result.engine`,
    );
    for (const key of ["engineId", "engineVersion", "projectPath", "mode"]) {
      if (typeof engine[key] !== "string") {
        throw new Error(`${label}.result.engine.${key} is invalid`);
      }
    }
    return;
  }
  if (Object.hasOwn(result, "error")) {
    assertExactKeys(result, ["error"], `${label}.result`);
    if (typeof result.error !== "string") {
      throw new Error(`${label}.result.error is invalid`);
    }
    return;
  }
  assertExactKeys(result, ["evidence", "detail", "godotDigest", "rendered"], `${label}.result`);
  const evidence = result.evidence;
  if (!isObject(evidence)) {
    throw new Error(`${label}.result.evidence must be an object`);
  }
  assertExactKeys(
    evidence,
    [
      "runId",
      "operationId",
      "exitCode",
      "durationMs",
      "stdoutLength",
      "stderrLength",
      "truncated",
      "artifactDigest",
      "digest",
    ],
    `${label}.result.evidence`,
  );
  if (
    typeof evidence.runId !== "string" ||
    typeof evidence.operationId !== "string" ||
    typeof evidence.durationMs !== "number" ||
    typeof evidence.stdoutLength !== "number" ||
    typeof evidence.stderrLength !== "number" ||
    typeof evidence.truncated !== "boolean"
  ) {
    throw new Error(`${label}.result.evidence shape is invalid`);
  }
  if (evidence.exitCode !== null && typeof evidence.exitCode !== "number") {
    throw new Error(`${label}.result.evidence.exitCode is invalid`);
  }
  for (const key of ["artifactDigest", "digest"]) {
    if (typeof evidence[key] !== "string" || !LOWER_SHA256.test(evidence[key])) {
      throw new Error(`${label}.result.evidence.${key} is invalid`);
    }
  }
  const detail = result.detail;
  if (!isObject(detail)) {
    throw new Error(`${label}.result.detail must be an object`);
  }
  assertExactKeys(
    detail,
    ["engineId", "engineVersion", "projectPath", "mode"],
    `${label}.result.detail`,
  );
  for (const key of ["engineId", "engineVersion", "projectPath", "mode"]) {
    if (typeof detail[key] !== "string") {
      throw new Error(`${label}.result.detail.${key} is invalid`);
    }
  }
  if (typeof result.godotDigest !== "string" || !LOWER_SHA256.test(result.godotDigest)) {
    throw new Error(`${label}.result.godotDigest is invalid`);
  }
  if (typeof result.rendered !== "string") {
    throw new Error(`${label}.result.rendered is invalid`);
  }
}

function validateVisualEvidenceV35Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (Object.hasOwn(result, "error")) {
    assertExactKeys(result, ["error"], `${label}.result`);
    if (typeof result.error !== "string") {
      throw new Error(`${label}.result.error is invalid`);
    }
    return;
  }
  assertExactKeys(
    result,
    ["outcome", "available", "reason", "capability", "detail", "captureDigest", "rendered"],
    `${label}.result`,
  );
  if (
    typeof result.available !== "boolean" ||
    typeof result.reason !== "string" ||
    typeof result.capability !== "string" ||
    typeof result.rendered !== "string"
  ) {
    throw new Error(`${label}.result availability shape is invalid`);
  }
  const outcome = result.outcome;
  if (!isObject(outcome)) {
    throw new Error(`${label}.result.outcome must be an object`);
  }
  assertExactKeys(outcome, ["disposition", "reason", "isUnavailable"], `${label}.result.outcome`);
  if (typeof outcome.disposition !== "string" || typeof outcome.isUnavailable !== "boolean") {
    throw new Error(`${label}.result.outcome shape is invalid`);
  }
  if (outcome.reason !== null && typeof outcome.reason !== "string") {
    throw new Error(`${label}.result.outcome.reason is invalid`);
  }
  const detail = result.detail;
  if (!isObject(detail)) {
    throw new Error(`${label}.result.detail must be an object`);
  }
  assertExactKeys(
    detail,
    ["mode", "frameCount", "frameDigests", "totalBytes"],
    `${label}.result.detail`,
  );
  if (typeof detail.mode !== "string" || detail.mode !== "visual") {
    throw new Error(`${label}.result.detail.mode is invalid`);
  }
  if (!Number.isInteger(detail.frameCount) || detail.frameCount < 1) {
    throw new Error(`${label}.result.detail.frameCount is invalid`);
  }
  if (!Number.isInteger(detail.totalBytes) || detail.totalBytes < 1) {
    throw new Error(`${label}.result.detail.totalBytes is invalid`);
  }
  if (!Array.isArray(detail.frameDigests) || detail.frameDigests.length !== detail.frameCount) {
    throw new Error(`${label}.result.detail.frameDigests is invalid`);
  }
  for (const digest of detail.frameDigests) {
    if (typeof digest !== "string" || !LOWER_SHA256.test(digest)) {
      throw new Error(`${label}.result.detail.frameDigests entries are invalid`);
    }
  }
  if (typeof result.captureDigest !== "string" || !LOWER_SHA256.test(result.captureDigest)) {
    throw new Error(`${label}.result.captureDigest is invalid`);
  }
}

function validateCompositionProfileV39Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(
    result,
    ["disposition", "narrowedOverlay", "profile", "profileDigest", "reason", "rendered"],
    `${label}.result`,
  );
  const disposition = result.disposition;
  if (
    typeof disposition !== "string" ||
    !["resolved", "default", "refused", "invalid"].includes(disposition)
  ) {
    throw new Error(`${label}.result.disposition is invalid`);
  }
  if (result.reason !== null && typeof result.reason !== "string") {
    throw new Error(`${label}.result.reason is invalid`);
  }
  if (typeof result.rendered !== "string") {
    throw new Error(`${label}.result.rendered is invalid`);
  }
  if (disposition === "invalid") {
    if (
      result.profile !== null ||
      result.narrowedOverlay !== null ||
      result.profileDigest !== null ||
      typeof result.reason !== "string"
    ) {
      throw new Error(`${label}.result invalid-disposition shape mismatch`);
    }
    return;
  }
  if (typeof result.profileDigest !== "string" || !LOWER_SHA256.test(result.profileDigest)) {
    throw new Error(`${label}.result.profileDigest is invalid`);
  }
  if (disposition === "resolved") {
    const profile = result.profile;
    if (!isObject(profile)) {
      throw new Error(`${label}.result.profile must be an object`);
    }
    assertExactKeys(profile, ["name", "overlayEntries"], `${label}.result.profile`);
    if (typeof profile.name !== "string" || !Number.isInteger(profile.overlayEntries)) {
      throw new Error(`${label}.result.profile shape is invalid`);
    }
    const narrowed = result.narrowedOverlay;
    if (!isObject(narrowed)) {
      throw new Error(`${label}.result.narrowedOverlay must be an object`);
    }
    for (const [capability, rule] of Object.entries(narrowed)) {
      if (!["allow", "ask", "deny"].includes(rule)) {
        throw new Error(`${label}.result.narrowedOverlay entries are invalid`);
      }
      if (capability === "") {
        throw new Error(`${label}.result.narrowedOverlay keys are invalid`);
      }
    }
    return;
  }
  if (disposition === "default") {
    if (result.profile !== null || result.narrowedOverlay !== null || result.reason !== null) {
      throw new Error(`${label}.result default-disposition shape mismatch`);
    }
    return;
  }
  if (
    result.profile !== null ||
    result.narrowedOverlay !== null ||
    typeof result.reason !== "string" ||
    !result.reason.includes("PROFILE_REFUSED")
  ) {
    throw new Error(`${label}.result refused-disposition shape mismatch`);
  }
}

function validateCompositionEffectiveV40Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(
    result,
    ["applied", "diagnostic", "effective", "effectiveDigest", "rendered"],
    `${label}.result`,
  );
  if (typeof result.applied !== "boolean") {
    throw new Error(`${label}.result.applied is invalid`);
  }
  if (result.diagnostic !== null && typeof result.diagnostic !== "string") {
    throw new Error(`${label}.result.diagnostic is invalid`);
  }
  if (result.applied && result.diagnostic !== null) {
    throw new Error(`${label}.result applied profile cannot carry a diagnostic`);
  }
  if (typeof result.rendered !== "string") {
    throw new Error(`${label}.result.rendered is invalid`);
  }
  if (typeof result.effectiveDigest !== "string" || !LOWER_SHA256.test(result.effectiveDigest)) {
    throw new Error(`${label}.result.effectiveDigest is invalid`);
  }
  if (!isObject(result.effective)) {
    throw new Error(`${label}.result.effective must be an object`);
  }
  if (Object.keys(result.effective).length === 0) {
    throw new Error(`${label}.result.effective must not be empty`);
  }
  for (const [capability, rule] of Object.entries(result.effective)) {
    if (capability === "") {
      throw new Error(`${label}.result.effective keys are invalid`);
    }
    if (!["allow", "ask", "deny"].includes(rule)) {
      throw new Error(`${label}.result.effective entries are invalid`);
    }
  }
}

function validateContextControlsV41Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(
    result,
    ["actualDigest", "boundDigest", "controlDigest", "disposition", "expectedDigest", "rendered"],
    `${label}.result`,
  );
  if (!result.disposition || !["fresh", "stale", "blocked"].includes(result.disposition)) {
    throw new Error(`${label}.result.disposition is invalid`);
  }
  for (const field of ["actualDigest", "boundDigest", "controlDigest", "expectedDigest"]) {
    const value = result[field];
    if (value === null) {
      continue;
    }
    if (typeof value !== "string" || !LOWER_SHA256.test(value)) {
      throw new Error(`${label}.result.${field} is invalid`);
    }
  }
  if (typeof result.controlDigest !== "string" || !LOWER_SHA256.test(result.controlDigest)) {
    throw new Error(`${label}.result.controlDigest is required`);
  }
  if (result.disposition === "fresh") {
    if (result.expectedDigest !== null) {
      throw new Error(`${label}.result fresh outcome cannot name an expected digest`);
    }
  } else {
    if (typeof result.expectedDigest !== "string" || typeof result.actualDigest !== "string") {
      throw new Error(
        `${label}.result ${result.disposition} outcome requires expected and actual digests`,
      );
    }
    if (result.expectedDigest === result.actualDigest) {
      throw new Error(
        `${label}.result ${result.disposition} outcome cannot match its bound digest`,
      );
    }
  }
  if (typeof result.rendered !== "string") {
    throw new Error(`${label}.result.rendered is invalid`);
  }
}

function validateCompositionLockV42Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(
    result,
    ["disposition", "identities", "lockDigest", "rendered"],
    `${label}.result`,
  );
  if (!result.disposition || !["resolved", "current", "stale"].includes(result.disposition)) {
    throw new Error(`${label}.result.disposition is invalid`);
  }
  if (typeof result.lockDigest !== "string" || !LOWER_SHA256.test(result.lockDigest)) {
    throw new Error(`${label}.result.lockDigest is invalid`);
  }
  if (!Array.isArray(result.identities)) {
    throw new Error(`${label}.result.identities must be an array`);
  }
  let previousId = null;
  for (const identity of result.identities) {
    if (!isObject(identity)) {
      throw new Error(`${label}.result.identities entries must be objects`);
    }
    assertExactKeys(identity, ["digest", "id", "path"], `${label}.result.identities entry`);
    if (typeof identity.id !== "string" || identity.id.length === 0) {
      throw new Error(`${label}.result.identities entry id is invalid`);
    }
    if (typeof identity.path !== "string" || identity.path.length === 0) {
      throw new Error(`${label}.result.identities entry path is invalid`);
    }
    if (typeof identity.digest !== "string" || !LOWER_SHA256.test(identity.digest)) {
      throw new Error(`${label}.result.identities entry digest is invalid`);
    }
    if (previousId !== null && identity.id <= previousId) {
      throw new Error(`${label}.result.identities must be sorted by id`);
    }
    previousId = identity.id;
  }
  if (result.disposition === "resolved") {
    if (!result.rendered.startsWith("resolved plugins=")) {
      throw new Error(`${label}.result resolved rendering is invalid`);
    }
  } else if (result.disposition === "current") {
    if (result.rendered !== "verified current") {
      throw new Error(`${label}.result current rendering is invalid`);
    }
  } else if (!result.rendered.startsWith("verified stale expected=")) {
    throw new Error(`${label}.result stale rendering is invalid`);
  }
}

function validateCompositionContextControlV46Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(
    result,
    ["controlDigest", "disposition", "reason", "rendered"],
    `${label}.result`,
  );
  if (!result.disposition || !["fresh", "stale", "blocked"].includes(result.disposition)) {
    throw new Error(`${label}.result.disposition is invalid`);
  }
  if (typeof result.controlDigest !== "string" || !LOWER_SHA256.test(result.controlDigest)) {
    throw new Error(`${label}.result.controlDigest is invalid`);
  }
  if (result.disposition === "fresh") {
    if (
      result.reason !== null ||
      !["context claim unbound", "context claim fresh"].includes(result.rendered)
    ) {
      throw new Error(`${label}.result fresh outcome is invalid`);
    }
  } else {
    if (typeof result.reason !== "string" || !result.reason.startsWith("the ")) {
      throw new Error(`${label}.result ${result.disposition} reason is invalid`);
    }
    if (result.rendered !== `context claim ${result.disposition} (${result.reason})`) {
      throw new Error(`${label}.result ${result.disposition} rendering is invalid`);
    }
  }
}
function validateCompositionSkillConsumptionV48Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(
    result,
    ["bound", "consumptionDigest", "disposition", "reason", "rendered"],
    `${label}.result`,
  );
  if (!result.disposition || !["none", "bound", "unknown"].includes(result.disposition)) {
    throw new Error(`${label}.result.disposition is invalid`);
  }
  if (
    typeof result.consumptionDigest !== "string" ||
    !LOWER_SHA256.test(result.consumptionDigest)
  ) {
    throw new Error(`${label}.result.consumptionDigest is invalid`);
  }
  if (!Array.isArray(result.bound)) {
    throw new Error(`${label}.result.bound must be an array`);
  }
  if (!Array.isArray(result.reason)) {
    throw new Error(`${label}.result.reason must be the sorted unknown list`);
  }
  for (const name of result.reason) {
    if (typeof name !== "string") {
      throw new Error(`${label}.result.reason entries must be strings`);
    }
  }
  if (result.disposition === "none") {
    if (result.bound.length !== 0 || result.reason.length !== 0) {
      throw new Error(`${label}.result none outcome is invalid`);
    }
    if (result.rendered !== "skills none (guidance only)") {
      throw new Error(`${label}.result none rendering is invalid`);
    }
  } else {
    for (const reference of result.bound) {
      if (
        !isObject(reference) ||
        reference.name === undefined ||
        typeof reference.digest !== "string" ||
        !LOWER_SHA256.test(reference.digest)
      ) {
        throw new Error(`${label}.result bound reference is invalid`);
      }
    }
    if (result.disposition === "unknown" && result.reason.length === 0) {
      throw new Error(`${label}.result unknown outcome is invalid`);
    }
    if (result.disposition === "bound" && result.reason.length !== 0) {
      throw new Error(`${label}.result bound outcome is invalid`);
    }
    const expected =
      result.disposition === "bound"
        ? `skills bound bound skills=${result.bound.length} (guidance only)`
        : `skills unknown bound skills=${result.bound.length} (guidance only) unknown=${result.reason.length}`;
    if (result.rendered !== expected) {
      throw new Error(`${label}.result ${result.disposition} rendering is invalid`);
    }
  }
}
function validateCompositionLockVerifyV47Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(result, ["decision", "lockDigest", "reason", "rendered"], `${label}.result`);
  if (!result.decision || !["missing", "current", "stale", "invalid"].includes(result.decision)) {
    throw new Error(`${label}.result.decision is invalid`);
  }
  if (typeof result.lockDigest !== "string" || !LOWER_SHA256.test(result.lockDigest)) {
    throw new Error(`${label}.result.lockDigest is invalid`);
  }
  if (result.decision === "missing" || result.decision === "current") {
    if (
      result.reason !== null ||
      (result.decision === "missing" &&
        result.rendered !== "lock verification missing (transparent)") ||
      (result.decision === "current" && result.rendered !== "lock verified current")
    ) {
      throw new Error(`${label}.result ${result.decision} outcome is invalid`);
    }
  } else {
    if (typeof result.reason !== "string" || !result.reason.startsWith("the ")) {
      throw new Error(`${label}.result ${result.decision} reason is invalid`);
    }
    if (result.rendered !== `lock ${result.decision} (${result.reason})`) {
      throw new Error(`${label}.result ${result.decision} rendering is invalid`);
    }
  }
}
function validateCompositionPluginActivationV45Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(
    result,
    ["activationDigest", "decision", "reason", "rendered"],
    `${label}.result`,
  );
  if (
    !result.decision ||
    !["activated", "refused-filtered", "refused-not-enabled"].includes(result.decision)
  ) {
    throw new Error(`${label}.result.decision is invalid`);
  }
  if (typeof result.activationDigest !== "string" || !LOWER_SHA256.test(result.activationDigest)) {
    throw new Error(`${label}.result.activationDigest is invalid`);
  }
  if (result.decision === "activated") {
    if (result.reason !== null || !result.rendered.startsWith("activated ")) {
      throw new Error(`${label}.result activated outcome is invalid`);
    }
  } else {
    if (typeof result.reason !== "string" || !result.reason.startsWith("the ")) {
      throw new Error(`${label}.result refusal reason is invalid`);
    }
    if (
      !result.rendered.startsWith(`${result.decision} `) ||
      !result.rendered.endsWith(`(${result.reason})`)
    ) {
      throw new Error(`${label}.result refusal rendering is invalid`);
    }
  }
}
function validateCompositionSkillsV44Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(
    result,
    ["bound", "disposition", "reason", "rendered", "resolutionDigest"],
    `${label}.result`,
  );
  if (!result.disposition || !["none", "bound"].includes(result.disposition)) {
    throw new Error(`${label}.result.disposition is invalid`);
  }
  if (typeof result.resolutionDigest !== "string" || !LOWER_SHA256.test(result.resolutionDigest)) {
    throw new Error(`${label}.result.resolutionDigest is invalid`);
  }
  if (!Array.isArray(result.bound)) {
    throw new Error(`${label}.result.bound must be an array`);
  }
  let previousName = null;
  for (const reference of result.bound) {
    if (!isObject(reference)) {
      throw new Error(`${label}.result.bound entries must be objects`);
    }
    assertExactKeys(reference, ["digest", "name"], `${label}.result.bound entry`);
    if (
      typeof reference.name !== "string" ||
      reference.name.length === 0 ||
      reference.name.length > 64
    ) {
      throw new Error(`${label}.result.bound entry name is invalid`);
    }
    if (typeof reference.digest !== "string" || !LOWER_SHA256.test(reference.digest)) {
      throw new Error(`${label}.result.bound entry digest is invalid`);
    }
    if (previousName !== null && reference.name <= previousName) {
      throw new Error(`${label}.result.bound must be sorted by name`);
    }
    previousName = reference.name;
  }
  if (result.disposition === "none") {
    if (
      result.bound.length !== 0 ||
      result.reason !== null ||
      result.rendered !== "none skills=0"
    ) {
      throw new Error(`${label}.result none outcome is invalid`);
    }
  } else {
    if (
      result.bound.length === 0 ||
      !result.rendered.startsWith(`bound skills=${result.bound.length} (guidance only)`)
    ) {
      throw new Error(`${label}.result bound outcome is invalid`);
    }
    if (result.reason === null) {
      if (result.rendered !== `bound skills=${result.bound.length} (guidance only)`) {
        throw new Error(`${label}.result bound rendering is invalid`);
      }
    } else if (
      !result.reason.startsWith("selection names ") ||
      !result.rendered.includes("unknown=")
    ) {
      throw new Error(`${label}.result bound reason is invalid`);
    }
  }
}
function validateCompositionPluginSelectionV43Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  assertExactKeys(
    result,
    ["activated", "disposition", "reason", "rendered", "selectionDigest"],
    `${label}.result`,
  );
  if (!result.disposition || !["unfiltered", "narrowed"].includes(result.disposition)) {
    throw new Error(`${label}.result.disposition is invalid`);
  }
  if (typeof result.selectionDigest !== "string" || !LOWER_SHA256.test(result.selectionDigest)) {
    throw new Error(`${label}.result.selectionDigest is invalid`);
  }
  if (!Array.isArray(result.activated)) {
    throw new Error(`${label}.result.activated must be an array`);
  }
  let previousId = null;
  for (const id of result.activated) {
    if (typeof id !== "string" || id.length === 0 || id.length > 64) {
      throw new Error(`${label}.result.activated id is invalid`);
    }
    if (previousId !== null && id <= previousId) {
      throw new Error(`${label}.result.activated must be sorted`);
    }
    previousId = id;
  }
  if (result.reason !== null && typeof result.reason !== "string") {
    throw new Error(`${label}.result.reason is invalid`);
  }
  if (result.disposition === "unfiltered") {
    if (
      result.reason !== null ||
      result.rendered !== `unfiltered plugins=${result.activated.length}`
    ) {
      throw new Error(`${label}.result unfiltered outcome is invalid`);
    }
  } else {
    if (!result.rendered.startsWith(`narrowed plugins=${result.activated.length}`)) {
      throw new Error(`${label}.result narrowed rendering is invalid`);
    }
    if (result.reason === null) {
      if (result.rendered !== `narrowed plugins=${result.activated.length}`) {
        throw new Error(`${label}.result narrowed rendering is invalid`);
      }
    } else {
      if (!result.reason.startsWith("selection names ") || !result.rendered.includes("unknown=")) {
        throw new Error(`${label}.result narrowed reason is invalid`);
      }
    }
  }
}
function validateRunProfileV38Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (Object.hasOwn(result, "error")) {
    assertExactKeys(result, ["error"], `${label}.result`);
    if (typeof result.error !== "string") {
      throw new Error(`${label}.result.error is invalid`);
    }
    return;
  }
  assertExactKeys(
    result,
    ["outcome", "available", "reason", "capability", "detail", "profileDigest", "rendered"],
    `${label}.result`,
  );
  if (
    typeof result.available !== "boolean" ||
    result.available !== false ||
    typeof result.reason !== "string" ||
    result.reason !== "identity-bound profiling primitive not available" ||
    typeof result.capability !== "string" ||
    result.capability !== "run.profile" ||
    typeof result.rendered !== "string"
  ) {
    throw new Error(`${label}.result availability shape is invalid`);
  }
  const outcome = result.outcome;
  if (!isObject(outcome)) {
    throw new Error(`${label}.result.outcome must be an object`);
  }
  assertExactKeys(outcome, ["disposition", "reason", "isUnavailable"], `${label}.result.outcome`);
  if (typeof outcome.disposition !== "string" || typeof outcome.isUnavailable !== "boolean") {
    throw new Error(`${label}.result.outcome shape is invalid`);
  }
  if (outcome.reason !== null && typeof outcome.reason !== "string") {
    throw new Error(`${label}.result.outcome.reason is invalid`);
  }
  const detail = result.detail;
  if (!isObject(detail)) {
    throw new Error(`${label}.result.detail must be an object`);
  }
  assertExactKeys(detail, ["sampleCount", "sampleDigests", "totalBytes"], `${label}.result.detail`);
  if (!Number.isInteger(detail.sampleCount) || detail.sampleCount < 1) {
    throw new Error(`${label}.result.detail.sampleCount is invalid`);
  }
  if (!Number.isInteger(detail.totalBytes) || detail.totalBytes < 1) {
    throw new Error(`${label}.result.detail.totalBytes is invalid`);
  }
  if (!Array.isArray(detail.sampleDigests) || detail.sampleDigests.length !== detail.sampleCount) {
    throw new Error(`${label}.result.detail.sampleDigests is invalid`);
  }
  for (const digest of detail.sampleDigests) {
    if (typeof digest !== "string" || !LOWER_SHA256.test(digest)) {
      throw new Error(`${label}.result.detail.sampleDigests entries are invalid`);
    }
  }
  if (typeof result.profileDigest !== "string" || !LOWER_SHA256.test(result.profileDigest)) {
    throw new Error(`${label}.result.profileDigest is invalid`);
  }
}

function validateQaWorkflowV37Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (Object.hasOwn(result, "error")) {
    assertExactKeys(result, ["error"], `${label}.result`);
    if (typeof result.error !== "string") {
      throw new Error(`${label}.result.error is invalid`);
    }
    return;
  }
  assertExactKeys(
    result,
    ["outcome", "available", "reason", "capability", "detail", "workflowDigest", "rendered"],
    `${label}.result`,
  );
  if (
    typeof result.available !== "boolean" ||
    result.available !== false ||
    typeof result.reason !== "string" ||
    result.reason !== "identity-bound QA workflow execution primitive not available" ||
    typeof result.capability !== "string" ||
    result.capability !== "qa.workflow" ||
    typeof result.rendered !== "string"
  ) {
    throw new Error(`${label}.result availability shape is invalid`);
  }
  const outcome = result.outcome;
  if (!isObject(outcome)) {
    throw new Error(`${label}.result.outcome must be an object`);
  }
  assertExactKeys(outcome, ["disposition", "reason", "isUnavailable"], `${label}.result.outcome`);
  if (typeof outcome.disposition !== "string" || typeof outcome.isUnavailable !== "boolean") {
    throw new Error(`${label}.result.outcome shape is invalid`);
  }
  if (outcome.reason !== null && typeof outcome.reason !== "string") {
    throw new Error(`${label}.result.outcome.reason is invalid`);
  }
  const detail = result.detail;
  if (!isObject(detail)) {
    throw new Error(`${label}.result.detail must be an object`);
  }
  assertExactKeys(detail, ["stepCount", "stepDigests", "totalBytes"], `${label}.result.detail`);
  if (!Number.isInteger(detail.stepCount) || detail.stepCount < 1) {
    throw new Error(`${label}.result.detail.stepCount is invalid`);
  }
  if (!Number.isInteger(detail.totalBytes) || detail.totalBytes < 1) {
    throw new Error(`${label}.result.detail.totalBytes is invalid`);
  }
  if (!Array.isArray(detail.stepDigests) || detail.stepDigests.length !== detail.stepCount) {
    throw new Error(`${label}.result.detail.stepDigests is invalid`);
  }
  for (const digest of detail.stepDigests) {
    if (typeof digest !== "string" || !LOWER_SHA256.test(digest)) {
      throw new Error(`${label}.result.detail.stepDigests entries are invalid`);
    }
  }
  if (typeof result.workflowDigest !== "string" || !LOWER_SHA256.test(result.workflowDigest)) {
    throw new Error(`${label}.result.workflowDigest is invalid`);
  }
}

function validateRunInteractionV36Result(result, label) {
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (Object.hasOwn(result, "error")) {
    assertExactKeys(result, ["error"], `${label}.result`);
    if (typeof result.error !== "string") {
      throw new Error(`${label}.result.error is invalid`);
    }
    return;
  }
  assertExactKeys(
    result,
    ["outcome", "available", "reason", "capability", "detail", "interactionDigest", "rendered"],
    `${label}.result`,
  );
  if (
    typeof result.available !== "boolean" ||
    result.available !== false ||
    typeof result.reason !== "string" ||
    result.reason !== "identity-bound interactive-run primitive not available" ||
    typeof result.capability !== "string" ||
    result.capability !== "run.interact" ||
    typeof result.rendered !== "string"
  ) {
    throw new Error(`${label}.result availability shape is invalid`);
  }
  const outcome = result.outcome;
  if (!isObject(outcome)) {
    throw new Error(`${label}.result.outcome must be an object`);
  }
  assertExactKeys(outcome, ["disposition", "reason", "isUnavailable"], `${label}.result.outcome`);
  if (typeof outcome.disposition !== "string" || typeof outcome.isUnavailable !== "boolean") {
    throw new Error(`${label}.result.outcome shape is invalid`);
  }
  if (outcome.reason !== null && typeof outcome.reason !== "string") {
    throw new Error(`${label}.result.outcome.reason is invalid`);
  }
  const detail = result.detail;
  if (!isObject(detail)) {
    throw new Error(`${label}.result.detail must be an object`);
  }
  assertExactKeys(detail, ["roundCount", "roundDigests", "totalBytes"], `${label}.result.detail`);
  if (!Number.isInteger(detail.roundCount) || detail.roundCount < 1) {
    throw new Error(`${label}.result.detail.roundCount is invalid`);
  }
  if (!Number.isInteger(detail.totalBytes) || detail.totalBytes < 1) {
    throw new Error(`${label}.result.detail.totalBytes is invalid`);
  }
  if (!Array.isArray(detail.roundDigests) || detail.roundDigests.length !== detail.roundCount) {
    throw new Error(`${label}.result.detail.roundDigests is invalid`);
  }
  for (const digest of detail.roundDigests) {
    if (typeof digest !== "string" || !LOWER_SHA256.test(digest)) {
      throw new Error(`${label}.result.detail.roundDigests entries are invalid`);
    }
  }
  if (
    typeof result.interactionDigest !== "string" ||
    !LOWER_SHA256.test(result.interactionDigest)
  ) {
    throw new Error(`${label}.result.interactionDigest is invalid`);
  }
}

function validateRecoveryTaxonomyResult(record, label) {
  const result = record;
  if (!isObject(result)) {
    throw new Error(`${label}.result must be an object`);
  }
  if (Object.hasOwn(result, "code")) {
    assertExactKeys(
      result,
      [
        "code",
        ...(result.code === "CAPABILITY_DENIED"
          ? ["missing"]
          : result.code === "RESOURCE_EXCEEDED"
            ? ["resourceKind"]
            : result.code === "UNAVAILABLE"
              ? ["reason"]
              : []),
      ],
      `${label}.result`,
    );
    if (result.code === "CAPABILITY_DENIED" && !Array.isArray(result.missing)) {
      throw new Error(`${label}.result.missing is invalid`);
    }
    if (Array.isArray(result.missing)) {
      for (const id of result.missing) {
        if (typeof id !== "string" || id.length === 0) {
          throw new Error(`${label}.result.missing is invalid`);
        }
      }
    }
    return;
  }
  if (
    Object.hasOwn(result, "cases") &&
    Array.isArray(result.cases) &&
    result.cases.every((entry) => Object.hasOwn(entry, "decision"))
  ) {
    assertExactKeys(result, ["cases"], `${label}.result`);
    for (const entry of result.cases) {
      assertExactKeys(
        entry,
        ["category", "attemptsUsed", "decision", "reason", "nextBackoffMs"],
        `${label}.result.cases`,
      );
      if (
        !["retry", "repair", "no_retry"].includes(entry.decision) ||
        typeof entry.reason !== "string" ||
        (entry.nextBackoffMs !== null && !Number.isSafeInteger(entry.nextBackoffMs))
      ) {
        throw new Error(`${label}.result.cases classification is invalid`);
      }
    }
    return;
  }
  assertExactKeys(result, ["cases"], `${label}.result`);
  for (const entry of result.cases) {
    assertExactKeys(
      entry,
      ["lastKnownState", "runStateMayExist", "classification", "reason"],
      `${label}.result.cases`,
    );
    if (
      !["interrupted", "unknown", "cleanup_required"].includes(entry.classification) ||
      typeof entry.reason !== "string" ||
      typeof entry.runStateMayExist !== "boolean"
    ) {
      throw new Error(`${label}.result.cases reconciliation is invalid`);
    }
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
  if (record.subject === "workspace-apply") {
    validateApplyResult(record, label);
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
        !["profile", "context", "skill", "plugin", "host"].includes(result.escalation)
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

const PLANNING_RUNTIME_CASES = new Set([
  "plan-model-identity",
  "plan-validation-strict",
  "planning-policy-depth",
  "planning-flow-phases",
  "plan-set-lifecycle",
  "plan-staleness-contract-advance",
  "plan-approval-binding",
  "plan-revision-cap",
  "plan-immutability-detach",
  "plan-invalidate-reasons",
]);

const EXECUTOR_BRIEF_CASES = new Set([
  "execution-contract-identity",
  "milestone-manifest-acceptance-ids",
  "acceptance-evaluator-evidence-only",
  "brief-compile-determinism",
  "brief-active-working-set",
  "workspace-scope-classification",
  "documentation-selection",
  "new-file-discipline-signals",
  "brief-render-bounded",
  "context-pack-refs",
  "briefing-service-memoization",
  "s3m8-real-manifest",
  "s3m9-real-manifest",
  "s3m10-real-manifest",
  "s3m11-real-manifest",
  "milestone-selection-by-request",
  "dynamic-context-digest-invalidation",
  "fingerprint-canonical-stability",
]);

const R13_4_MAX_VALUE_NODES = 8192;
const R13_4_MAX_DEPTH = 12;
const R13_4_MAX_STRING_BYTES = 16 * 1024;

function r13_4BoundedValue(value, label, depth = 0) {
  if (depth > R13_4_MAX_DEPTH) {
    throw new Error(`${label} exceeds the maximum nesting depth`);
  }
  if (value === null || typeof value === "boolean" || typeof value === "number") {
    return;
  }
  if (typeof value === "string") {
    if (Buffer.byteLength(value, "utf8") > R13_4_MAX_STRING_BYTES) {
      throw new Error(`${label} exceeds the string byte bound`);
    }
    return;
  }
  if (Array.isArray(value)) {
    if (value.length > R13_4_MAX_VALUE_NODES) {
      throw new Error(`${label} exceeds the array bound`);
    }
    for (const entry of value) r13_4BoundedValue(entry, label, depth + 1);
    return;
  }
  if (typeof value === "object") {
    const keys = Object.keys(value);
    if (keys.length > R13_4_MAX_VALUE_NODES) {
      throw new Error(`${label} exceeds the object bound`);
    }
    for (const key of keys) r13_4BoundedValue(value[key], `${label}.${key}`, depth + 1);
    return;
  }
  throw new Error(`${label} contains a non-JSON value`);
}

function validatePlanningRuntimeCase(entry, label) {
  if (entry === null || typeof entry !== "object" || Array.isArray(entry)) {
    throw new Error(`${label} must be an object`);
  }
  if (
    !PLANNING_RUNTIME_CASES.has(entry.name) ||
    Object.keys(entry).some((key) => key !== "name" && !r13IsPlainObjectValue(entry[key]))
  ) {
    throw new Error(`${label}.name is unknown or fields are malformed`);
  }
  r13_4BoundedValue(entry, label);
}

function validateExecutorBriefCase(entry, label) {
  if (entry === null || typeof entry !== "object" || Array.isArray(entry)) {
    throw new Error(`${label} must be an object`);
  }
  if (!EXECUTOR_BRIEF_CASES.has(entry.name)) {
    throw new Error(`${label}.name is unknown`);
  }
  r13_4BoundedValue(entry, label);
}

const CLI_SESSION_CASES = new Set([
  "input-parsing",
  "session-lifecycle",
  "help-and-commands",
  "status-view",
  "unknown-command",
  "prompt-turn",
  "godot-commands-unavailable",
  "gdscript-commands-unavailable",
  "develop-commands-unavailable",
  "system-commands-unavailable",
  "input-queue-ownership",
  "sanitizer-boundary",
  "session-ordering-determinism",
]);

function validateCliSessionCase(entry, label) {
  if (entry === null || typeof entry !== "object" || Array.isArray(entry)) {
    throw new Error(`${label} must be an object`);
  }
  if (!CLI_SESSION_CASES.has(entry.name)) {
    throw new Error(`${label}.name is unknown`);
  }
  r13_4BoundedValue(entry, label);
}

function r13IsPlainObjectValue(value) {
  return (
    value === null ||
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean" ||
    Array.isArray(value) ||
    (value !== null && typeof value === "object")
  );
}
