/**
 * Provider turn/tool-call/detach, provider replay/replay-store, and user-config record validation.
 */
import { LOWER_SHA256, assertExactKeys, isObject } from "./core.mjs";

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

function validateProviderGenericResult(record, label) {
  assertExactKeys(record.result, ["providerId", "events"], `${label}.result`);
  if (typeof record.result.providerId !== "string" || record.result.providerId.length === 0) {
    throw new Error(`${label}.result.providerId must be a non-empty string`);
  }
  if (!Array.isArray(record.result.events)) {
    throw new Error(`${label}.result.events must be an array`);
  }
}

function validateProviderReplayResult(record, label) {
  assertExactKeys(
    record.result,
    [
      "model",
      "providerId",
      "recorderSnapshotCount",
      "remainingCounts",
      "turn1Events",
      "turn2Events",
      "turn3Events",
    ],
    `${label}.result`,
  );
  if (typeof record.result.providerId !== "string" || record.result.providerId.length === 0) {
    throw new Error(`${label}.result.providerId must be a non-empty string`);
  }
  if (typeof record.result.model !== "string" || record.result.model.length === 0) {
    throw new Error(`${label}.result.model must be a non-empty string`);
  }
  if (!Array.isArray(record.result.remainingCounts) || record.result.remainingCounts.length !== 3) {
    throw new Error(`${label}.result.remainingCounts must be an array of length 3`);
  }
  for (const [index, value] of record.result.remainingCounts.entries()) {
    if (!Number.isInteger(value) || value < 0) {
      throw new Error(`${label}.result.remainingCounts[${index}] must be a non-negative integer`);
    }
  }
  for (const key of ["turn1Events", "turn2Events", "turn3Events"]) {
    if (!Array.isArray(record.result[key])) {
      throw new Error(`${label}.result.${key} must be an array`);
    }
  }
  if (
    !Number.isInteger(record.result.recorderSnapshotCount) ||
    record.result.recorderSnapshotCount < 0
  ) {
    throw new Error(`${label}.result.recorderSnapshotCount must be a non-negative integer`);
  }
}

function validateReplayStoreResult(record, label) {
  assertExactKeys(
    record.result,
    [
      "loadedCount",
      "persistedCount",
      "storeDigest",
      "tamperedLoad",
      "turn1Events",
      "turn2Events",
      "turn3Events",
    ],
    `${label}.result`,
  );
  if (
    typeof record.result.storeDigest !== "string" ||
    !LOWER_SHA256.test(record.result.storeDigest)
  ) {
    throw new Error(`${label}.result.storeDigest must be a lowercase SHA-256 digest`);
  }
  if (record.result.tamperedLoad !== "untrusted-digest") {
    throw new Error(`${label}.result.tamperedLoad must be "untrusted-digest"`);
  }
  if (!Number.isInteger(record.result.loadedCount) || record.result.loadedCount < 0) {
    throw new Error(`${label}.result.loadedCount must be a non-negative integer`);
  }
  if (!Number.isInteger(record.result.persistedCount) || record.result.persistedCount < 0) {
    throw new Error(`${label}.result.persistedCount must be a non-negative integer`);
  }
  for (const key of ["turn1Events", "turn2Events", "turn3Events"]) {
    if (!Array.isArray(record.result[key])) {
      throw new Error(`${label}.result.${key} must be an array`);
    }
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

export {
  validateProviderGenericResult,
  validateProviderReplayResult,
  validateProviderTurnResult,
  validateReplayStoreResult,
  validateUserConfigResult,
};
