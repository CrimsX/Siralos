/**
 * Versioned outcome protocol shared by the R2 runners and comparator.
 *
 * Scenario outcomes describe product behavior. Runner outcomes describe the
 * lifecycle of the isolated reference/candidate process. Keeping those layers
 * distinct prevents a product failure from being mistaken for a harness crash.
 */
import { canonicalizeJson } from "./canonical.mjs";
import { ALLOWED_SUBJECTS, CONTRACT_LIMITS } from "./contract.mjs";

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
