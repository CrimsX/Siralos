/**
 * R13 authority case validation: security permissions, command catalog, capability doctor, instructions, knowledge, references, research policy, planning runtime, executor brief, and CLI session.
 */
import { LOWER_SHA256, assertExactKeys, isObject } from "./core.mjs";

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

export {
  validateCliSessionCase,
  validateExecutorBriefCase,
  validateInstructionsResolutionCase,
  validateKnowledgeRevisionsCase,
  validatePlanningRuntimeCase,
  validateR13AuthorityResult,
  validateReferenceIdentityCase,
  validateResearchPolicyCase,
};
