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
