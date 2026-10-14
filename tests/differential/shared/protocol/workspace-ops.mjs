/**
 * Generic workspace/project foundation records: read, list, search, revision, prepare, apply, checkpoint, and git.
 */
import { LOWER_SHA256, assertExactKeys, isObject } from "./core.mjs";

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

const DEFAULT_TOOLS_STATUSES = new Set([
  "success",
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

function validateDefaultToolsResult(record, label) {
  assertExactKeys(record.result, ["calls", "files", "checkpointCount"], `${label}.result`);
  if (!Array.isArray(record.result.calls) || record.result.calls.length > 16) {
    throw new Error(`${label}.result.calls must be a bounded array`);
  }
  for (const [index, entry] of record.result.calls.entries()) {
    const entryLabel = `${label}.result.calls[${index}]`;
    if (!isObject(entry)) {
      throw new Error(`${entryLabel} must be an object`);
    }
    if (!["write", "edit", "bash"].includes(entry.tool)) {
      throw new Error(`${entryLabel}.tool is invalid`);
    }
    if (!DEFAULT_TOOLS_STATUSES.has(entry.status)) {
      throw new Error(`${entryLabel}.status is invalid`);
    }
    if (entry.status === "success") {
      assertExactKeys(entry, ["tool", "status", "output"], entryLabel);
      if (!isObject(entry.output)) {
        throw new Error(`${entryLabel}.output must be an object`);
      }
      if (entry.output.sha256 !== undefined && !LOWER_SHA256.test(entry.output.sha256)) {
        throw new Error(`${entryLabel}.output.sha256 is invalid`);
      }
      if (
        entry.output.bytes !== undefined &&
        (!Number.isSafeInteger(entry.output.bytes) || entry.output.bytes < 0)
      ) {
        throw new Error(`${entryLabel}.output.bytes is invalid`);
      }
      if (entry.output.path !== undefined && typeof entry.output.path !== "string") {
        throw new Error(`${entryLabel}.output.path is invalid`);
      }
    } else {
      assertExactKeys(entry, ["tool", "status", "message"], entryLabel);
      if (
        typeof entry.message !== "string" ||
        entry.message.length === 0 ||
        entry.message.length > 1024
      ) {
        throw new Error(`${entryLabel}.message is invalid`);
      }
    }
  }
  if (!Array.isArray(record.result.files)) {
    throw new Error(`${label}.result.files must be an array`);
  }
  for (const [index, entry] of record.result.files.entries()) {
    const entryLabel = `${label}.result.files[${index}]`;
    assertExactKeys(entry, ["path", "sha256"], entryLabel);
    if (typeof entry.path !== "string") {
      throw new Error(`${entryLabel}.path is invalid`);
    }
    if (
      entry.sha256 !== null &&
      (typeof entry.sha256 !== "string" || !LOWER_SHA256.test(entry.sha256))
    ) {
      throw new Error(`${entryLabel}.sha256 is invalid`);
    }
  }
  if (!Number.isSafeInteger(record.result.checkpointCount) || record.result.checkpointCount < 0) {
    throw new Error(`${label}.result.checkpointCount is invalid`);
  }
}

export {
  validateApplyResult,
  validateCheckpointResult,
  validateDefaultToolsResult,
  validateGitResult,
  validateListResult,
  validatePrepareResult,
  validateReadResult,
  validateRevisionResult,
  validateSearchResult,
};
