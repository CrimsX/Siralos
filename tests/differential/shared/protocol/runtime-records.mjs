/**
 * ICM, runtime-readiness, recovery-taxonomy, and versioned milestone record validation (runtime v32 through composition v48).
 */
import { LOWER_SHA256, assertExactKeys, isObject } from "./core.mjs";

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
    if (!result.rendered.startsWith("resolved modules=")) {
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
      result.rendered !== `unfiltered modules=${result.activated.length}`
    ) {
      throw new Error(`${label}.result unfiltered outcome is invalid`);
    }
  } else {
    if (!result.rendered.startsWith(`narrowed modules=${result.activated.length}`)) {
      throw new Error(`${label}.result narrowed rendering is invalid`);
    }
    if (result.reason === null) {
      if (result.rendered !== `narrowed modules=${result.activated.length}`) {
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

export {
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
};
