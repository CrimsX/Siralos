/**
 * Language diagnostics and domain records: domain-neutral lifecycle/capability plus the Godot domain surface.
 */
import { LOWER_SHA256, assertExactKeys, isObject, validateBoundedStringArray } from "./core.mjs";

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

export {
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
};
