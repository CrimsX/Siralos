/**
 * Context projection/graph/representation/scheduler/tool/benchmark, context session, TUI render, and session-replay record validation.
 */
import { canonicalizeJson } from "../canonical.mjs";
import { CONTRACT_LIMITS } from "../contract.mjs";
import { LOWER_SHA256, assertExactKeys, isObject } from "./core.mjs";

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

function validateContextGraphResult(record, label) {
  assertExactKeys(
    record.result,
    [
      "danglingEdgeError",
      "edgeCount",
      "graphDigest",
      "nodeCount",
      "staleAfter",
      "staleBefore",
      "staleBindings",
    ],
    `${label}.result`,
  );
  if (!Number.isInteger(record.result.nodeCount) || record.result.nodeCount < 0) {
    throw new Error(`${label}.result.nodeCount must be a non-negative integer`);
  }
  if (!Number.isInteger(record.result.edgeCount) || record.result.edgeCount < 0) {
    throw new Error(`${label}.result.edgeCount must be a non-negative integer`);
  }
  if (
    typeof record.result.graphDigest !== "string" ||
    !LOWER_SHA256.test(record.result.graphDigest)
  ) {
    throw new Error(`${label}.result.graphDigest must be a lowercase SHA-256 digest`);
  }
  if (!Array.isArray(record.result.staleBefore)) {
    throw new Error(`${label}.result.staleBefore must be an array`);
  }
  if (!Array.isArray(record.result.staleAfter)) {
    throw new Error(`${label}.result.staleAfter must be an array`);
  }
  if (!Array.isArray(record.result.staleBindings)) {
    throw new Error(`${label}.result.staleBindings must be an array`);
  }
  if (
    typeof record.result.danglingEdgeError !== "string" ||
    record.result.danglingEdgeError.length === 0
  ) {
    throw new Error(`${label}.result.danglingEdgeError must be a non-empty string`);
  }
}

function validateContextRepresentationResult(record, label) {
  assertExactKeys(
    record.result,
    [
      "modelDerivedL2Error",
      "repCount",
      "resolvedL2Digest",
      "setCount",
      "storeDigest",
      "unprovenancedDerivedError",
    ],
    `${label}.result`,
  );
  if (!Number.isInteger(record.result.setCount) || record.result.setCount < 0) {
    throw new Error(`${label}.result.setCount must be a non-negative integer`);
  }
  if (!Number.isInteger(record.result.repCount) || record.result.repCount < 0) {
    throw new Error(`${label}.result.repCount must be a non-negative integer`);
  }
  if (
    typeof record.result.storeDigest !== "string" ||
    !LOWER_SHA256.test(record.result.storeDigest)
  ) {
    throw new Error(`${label}.result.storeDigest must be a lowercase SHA-256 digest`);
  }
  if (
    typeof record.result.resolvedL2Digest !== "string" ||
    !LOWER_SHA256.test(record.result.resolvedL2Digest)
  ) {
    throw new Error(`${label}.result.resolvedL2Digest must be a lowercase SHA-256 digest`);
  }
  if (
    typeof record.result.modelDerivedL2Error !== "string" ||
    record.result.modelDerivedL2Error.length === 0
  ) {
    throw new Error(`${label}.result.modelDerivedL2Error must be a non-empty string`);
  }
  if (
    typeof record.result.unprovenancedDerivedError !== "string" ||
    record.result.unprovenancedDerivedError.length === 0
  ) {
    throw new Error(`${label}.result.unprovenancedDerivedError must be a non-empty string`);
  }
}

function validateContextSchedulerResult(record, label) {
  assertExactKeys(
    record.result,
    [
      "assembly",
      "budgetDemotions",
      "hotTokensAfter",
      "tiersAfterStale",
      "tiersBefore",
      "tickReport",
      "unknownNodeError",
    ],
    `${label}.result`,
  );
  if (!isObject(record.result.tiersBefore)) {
    throw new Error(`${label}.result.tiersBefore must be an object`);
  }
  if (!isObject(record.result.tiersAfterStale)) {
    throw new Error(`${label}.result.tiersAfterStale must be an object`);
  }
  if (!isObject(record.result.tickReport)) {
    throw new Error(`${label}.result.tickReport must be an object`);
  }
  assertExactKeys(
    record.result.tickReport,
    ["demoted", "promoted", "tick"],
    `${label}.result.tickReport`,
  );
  if (!Number.isInteger(record.result.tickReport.tick) || record.result.tickReport.tick < 0) {
    throw new Error(`${label}.result.tickReport.tick must be a non-negative integer`);
  }
  if (!Array.isArray(record.result.tickReport.promoted)) {
    throw new Error(`${label}.result.tickReport.promoted must be an array`);
  }
  if (!Array.isArray(record.result.tickReport.demoted)) {
    throw new Error(`${label}.result.tickReport.demoted must be an array`);
  }
  if (!Array.isArray(record.result.budgetDemotions)) {
    throw new Error(`${label}.result.budgetDemotions must be an array`);
  }
  if (!Number.isInteger(record.result.hotTokensAfter) || record.result.hotTokensAfter < 0) {
    throw new Error(`${label}.result.hotTokensAfter must be a non-negative integer`);
  }
  if (
    typeof record.result.unknownNodeError !== "string" ||
    record.result.unknownNodeError.length === 0
  ) {
    throw new Error(`${label}.result.unknownNodeError must be a non-empty string`);
  }
  if (!isObject(record.result.assembly)) {
    throw new Error(`${label}.result.assembly must be an object`);
  }
  assertExactKeys(
    record.result.assembly,
    ["demoted", "entries", "totalTokensAfter", "totalTokensBefore"],
    `${label}.result.assembly`,
  );
  if (!Array.isArray(record.result.assembly.entries)) {
    throw new Error(`${label}.result.assembly.entries must be an array`);
  }
  for (const [index, entry] of record.result.assembly.entries.entries()) {
    assertExactKeys(
      entry,
      ["contentDigest", "neighbors", "nodeId", "summary", "tokenEstimate"],
      `${label}.result.assembly.entries[${index}]`,
    );
    if (typeof entry.nodeId !== "string" || entry.nodeId.length === 0) {
      throw new Error(
        `${label}.result.assembly.entries[${index}].nodeId must be a non-empty string`,
      );
    }
    if (typeof entry.contentDigest !== "string" || !LOWER_SHA256.test(entry.contentDigest)) {
      throw new Error(
        `${label}.result.assembly.entries[${index}].contentDigest must be a lowercase SHA-256 digest`,
      );
    }
    if (typeof entry.summary !== "string") {
      throw new Error(`${label}.result.assembly.entries[${index}].summary must be a string`);
    }
    if (!Number.isInteger(entry.tokenEstimate) || entry.tokenEstimate < 0) {
      throw new Error(
        `${label}.result.assembly.entries[${index}].tokenEstimate must be a non-negative integer`,
      );
    }
    if (!Array.isArray(entry.neighbors)) {
      throw new Error(`${label}.result.assembly.entries[${index}].neighbors must be an array`);
    }
    if (entry.neighbors.length > 8) {
      throw new Error(
        `${label}.result.assembly.entries[${index}].neighbors must have at most 8 entries`,
      );
    }
    for (const [nIdx, neigh] of entry.neighbors.entries()) {
      assertExactKeys(
        neigh,
        ["contentDigest", "nodeId"],
        `${label}.result.assembly.entries[${index}].neighbors[${nIdx}]`,
      );
      if (typeof neigh.nodeId !== "string" || neigh.nodeId.length === 0) {
        throw new Error(
          `${label}.result.assembly.entries[${index}].neighbors[${nIdx}].nodeId must be a non-empty string`,
        );
      }
      if (typeof neigh.contentDigest !== "string" || !LOWER_SHA256.test(neigh.contentDigest)) {
        throw new Error(
          `${label}.result.assembly.entries[${index}].neighbors[${nIdx}].contentDigest must be a lowercase SHA-256 digest`,
        );
      }
    }
    // No content leak: neighbors must not contain summary/content
    if ("content" in entry || "summaryDigest" in entry) {
      throw new Error(`${label}.result.assembly.entries[${index}] must not leak content`);
    }
  }
  if (
    !Number.isInteger(record.result.assembly.totalTokensBefore) ||
    record.result.assembly.totalTokensBefore < 0
  ) {
    throw new Error(`${label}.result.assembly.totalTokensBefore must be a non-negative integer`);
  }
  if (
    !Number.isInteger(record.result.assembly.totalTokensAfter) ||
    record.result.assembly.totalTokensAfter < 0
  ) {
    throw new Error(`${label}.result.assembly.totalTokensAfter must be a non-negative integer`);
  }
  if (!Array.isArray(record.result.assembly.demoted)) {
    throw new Error(`${label}.result.assembly.demoted must be an array`);
  }
}

function validateContextToolResult(record, label) {
  assertExactKeys(
    record.result,
    [
      "expandDigest",
      "expandNotFound",
      "expandOrigin",
      "expandUnavailable",
      "inspectCtxA",
      "inspectKnowledgeStale",
      "mutationGuard",
      "searchAuthHits",
    ],
    `${label}.result`,
  );
  if (!isObject(record.result.inspectCtxA)) {
    throw new Error(`${label}.result.inspectCtxA must be an object`);
  }
  assertExactKeys(
    record.result.inspectCtxA,
    [
      "availableLevels",
      "content_digest",
      "id",
      "kind",
      "pinned",
      "stale",
      "summary",
      "tier",
      "tokenEstimate",
    ],
    `${label}.result.inspectCtxA`,
  );
  if (
    typeof record.result.inspectCtxA.id !== "string" ||
    record.result.inspectCtxA.id.length === 0
  ) {
    throw new Error(`${label}.result.inspectCtxA.id must be a non-empty string`);
  }
  if (
    typeof record.result.inspectCtxA.kind !== "string" ||
    record.result.inspectCtxA.kind.length === 0
  ) {
    throw new Error(`${label}.result.inspectCtxA.kind must be a non-empty string`);
  }
  if (
    typeof record.result.inspectCtxA.content_digest !== "string" ||
    !LOWER_SHA256.test(record.result.inspectCtxA.content_digest)
  ) {
    throw new Error(
      `${label}.result.inspectCtxA.content_digest must be a lowercase SHA-256 digest`,
    );
  }
  if (!Array.isArray(record.result.inspectCtxA.availableLevels)) {
    throw new Error(`${label}.result.inspectCtxA.availableLevels must be an array`);
  }
  if (!Array.isArray(record.result.searchAuthHits)) {
    throw new Error(`${label}.result.searchAuthHits must be an array`);
  }
  for (const [index, hit] of record.result.searchAuthHits.entries()) {
    const hitLabel = `${label}.result.searchAuthHits[${index}]`;
    if (!isObject(hit)) {
      throw new Error(`${hitLabel} must be an object`);
    }
    assertExactKeys(hit, ["kind", "matched_in", "node_id", "score", "tier"], hitLabel);
    if (typeof hit.node_id !== "string" || hit.node_id.length === 0) {
      throw new Error(`${hitLabel}.node_id must be a non-empty string`);
    }
    if (typeof hit.kind !== "string" || hit.kind.length === 0) {
      throw new Error(`${hitLabel}.kind must be a non-empty string`);
    }
    if (typeof hit.tier !== "string" || hit.tier.length === 0) {
      throw new Error(`${hitLabel}.tier must be a non-empty string`);
    }
    if (typeof hit.matched_in !== "string" || !["id", "summary"].includes(hit.matched_in)) {
      throw new Error(`${hitLabel}.matched_in must be "id" or "summary"`);
    }
    if (!Number.isSafeInteger(hit.score)) {
      throw new Error(`${hitLabel}.score must be an integer`);
    }
  }
  if (
    typeof record.result.expandDigest !== "string" ||
    !LOWER_SHA256.test(record.result.expandDigest)
  ) {
    throw new Error(`${label}.result.expandDigest must be a lowercase SHA-256 digest`);
  }
  if (typeof record.result.expandOrigin !== "string" || record.result.expandOrigin.length === 0) {
    throw new Error(`${label}.result.expandOrigin must be a non-empty string`);
  }
  if (
    typeof record.result.expandUnavailable !== "string" ||
    !record.result.expandUnavailable.includes("not resident at level") ||
    !record.result.expandUnavailable.includes("available:")
  ) {
    throw new Error(
      `${label}.result.expandUnavailable must be a not-resident message with available list`,
    );
  }
  if (
    typeof record.result.expandNotFound !== "string" ||
    !record.result.expandNotFound.includes("unknown node")
  ) {
    throw new Error(`${label}.result.expandNotFound must be a not-found message`);
  }
  if (typeof record.result.inspectKnowledgeStale !== "boolean") {
    throw new Error(`${label}.result.inspectKnowledgeStale must be a boolean`);
  }
  if (record.result.inspectKnowledgeStale !== true) {
    throw new Error(`${label}.result.inspectKnowledgeStale must be true`);
  }
  if (
    typeof record.result.mutationGuard !== "string" ||
    record.result.mutationGuard !== "unchanged"
  ) {
    throw new Error(`${label}.result.mutationGuard must be "unchanged"`);
  }
}

function validateContextBenchmarkResult(record, label) {
  // New canonical shape (v62): { baseline, strategyV1, strategyV2, go, reason } plus backward aggregates/scenarioMetrics for compatibility
  const result = record.result;
  if (typeof result.go !== "boolean") {
    throw new Error(`${label}.result.go must be a boolean`);
  }
  if (typeof result.reason !== "string" || result.reason.length === 0) {
    throw new Error(`${label}.result.reason must be a non-empty string`);
  }
  if (!result.reason.includes("total_recall_paged") || !result.reason.includes("total_paged")) {
    throw new Error(`${label}.result.reason must cite the compared numbers`);
  }
  const hasNew = result.baseline && result.strategyV1 && result.strategyV2;
  const hasOld = result.aggregates && result.scenarioMetrics;
  if (!hasNew) {
    throw new Error(`${label}.result must contain baseline, strategyV1 and strategyV2`);
  }
  if (hasNew) {
    assertExactKeys(
      result.baseline,
      ["totalBaseline", "totalKey", "totalRecallBaseline"],
      `${label}.result.baseline`,
    );
    for (const key of ["totalBaseline", "totalKey", "totalRecallBaseline"]) {
      if (!Number.isSafeInteger(result.baseline[key]) || result.baseline[key] < 0) {
        throw new Error(`${label}.result.baseline.${key} must be a non-negative integer`);
      }
    }
    for (const strat of ["strategyV1", "strategyV2"]) {
      const s = result[strat];
      const sLabel = `${label}.result.${strat}`;
      assertExactKeys(
        s,
        ["scenarioMetrics", "totalPaged", "totalRecallPaged", "totalToolCalls"],
        sLabel,
      );
      if (!Array.isArray(s.scenarioMetrics) || s.scenarioMetrics.length === 0) {
        throw new Error(`${sLabel}.scenarioMetrics must be a non-empty array`);
      }
      for (const [index, entry] of s.scenarioMetrics.entries()) {
        const entryLabel = `${sLabel}.scenarioMetrics[${index}]`;
        assertExactKeys(
          entry,
          ["name", "recallBaseline", "recallPaged", "tokensBaseline", "tokensPaged", "toolCalls"],
          entryLabel,
        );
        if (typeof entry.name !== "string" || entry.name.length === 0) {
          throw new Error(`${entryLabel}.name must be a non-empty string`);
        }
        for (const key of [
          "recallBaseline",
          "recallPaged",
          "tokensBaseline",
          "tokensPaged",
          "toolCalls",
        ]) {
          if (!Number.isSafeInteger(entry[key]) || entry[key] < 0) {
            throw new Error(`${entryLabel}.${key} must be a non-negative integer`);
          }
        }
      }
      for (const key of ["totalPaged", "totalRecallPaged", "totalToolCalls"]) {
        if (!Number.isSafeInteger(s[key]) || s[key] < 0) {
          throw new Error(`${sLabel}.${key} must be a non-negative integer`);
        }
      }
    }
  }
  // Backward compatibility: if top-level aggregates/scenarioMetrics present, validate them as before
  if (hasOld || Object.hasOwn(result, "aggregates")) {
    assertExactKeys(
      result.aggregates,
      ["totalBaseline", "totalPaged", "totalRecallBaseline", "totalRecallPaged", "totalToolCalls"],
      `${label}.result.aggregates`,
    );
    for (const key of [
      "totalBaseline",
      "totalPaged",
      "totalRecallBaseline",
      "totalRecallPaged",
      "totalToolCalls",
    ]) {
      if (!Number.isSafeInteger(result.aggregates[key]) || result.aggregates[key] < 0) {
        throw new Error(`${label}.result.aggregates.${key} must be a non-negative integer`);
      }
    }
  }
  if (hasOld || Object.hasOwn(result, "scenarioMetrics")) {
    if (!hasOld && !Object.hasOwn(result, "scenarioMetrics")) {
      // No-op if neither, but validated above
    } else {
      if (!Array.isArray(result.scenarioMetrics) || result.scenarioMetrics.length === 0) {
        throw new Error(`${label}.result.scenarioMetrics must be a non-empty array`);
      }
      for (const [index, entry] of result.scenarioMetrics.entries()) {
        const entryLabel = `${label}.result.scenarioMetrics[${index}]`;
        assertExactKeys(
          entry,
          ["name", "recallBaseline", "recallPaged", "tokensBaseline", "tokensPaged", "toolCalls"],
          entryLabel,
        );
        if (typeof entry.name !== "string" || entry.name.length === 0) {
          throw new Error(`${entryLabel}.name must be a non-empty string`);
        }
        for (const key of [
          "recallBaseline",
          "recallPaged",
          "tokensBaseline",
          "tokensPaged",
          "toolCalls",
        ]) {
          if (!Number.isSafeInteger(entry[key]) || entry[key] < 0) {
            throw new Error(`${entryLabel}.${key} must be a non-negative integer`);
          }
        }
      }
    }
  }
  // v63 additions: decomposition, sensitivity, informational
  if (!result.decomposition || typeof result.decomposition !== "object") {
    throw new Error(`${label}.result.decomposition is required`);
  }
  assertExactKeys(
    result.decomposition,
    ["dedupSaved", "rerankSaved", "levelSaved", "dedupSharePct", "dedupGuardOk"],
    `${label}.result.decomposition`,
  );
  for (const key of ["dedupSaved", "rerankSaved", "levelSaved", "dedupSharePct"]) {
    if (!Number.isSafeInteger(result.decomposition[key]) || result.decomposition[key] < 0) {
      throw new Error(`${label}.result.decomposition.${key} must be a non-negative integer`);
    }
  }
  if (typeof result.decomposition.dedupGuardOk !== "boolean") {
    throw new Error(`${label}.result.decomposition.dedupGuardOk must be a boolean`);
  }
  if (!result.sensitivity || typeof result.sensitivity !== "object") {
    throw new Error(`${label}.result.sensitivity is required`);
  }
  assertExactKeys(result.sensitivity, ["cells", "allOk"], `${label}.result.sensitivity`);
  if (!Array.isArray(result.sensitivity.cells) || result.sensitivity.cells.length !== 9) {
    throw new Error(`${label}.result.sensitivity.cells must be a 9-element array`);
  }
  for (const [index, cell] of result.sensitivity.cells.entries()) {
    const cellLabel = `${label}.result.sensitivity.cells[${index}]`;
    assertExactKeys(
      cell,
      ["bytesPerToken", "overhead", "recallOk", "marginOk", "dedupOk"],
      cellLabel,
    );
    if (!Number.isSafeInteger(cell.bytesPerToken) || ![3, 4, 5].includes(cell.bytesPerToken)) {
      throw new Error(`${cellLabel}.bytesPerToken must be 3, 4, or 5`);
    }
    if (!Number.isSafeInteger(cell.overhead) || ![0, 8, 16].includes(cell.overhead)) {
      throw new Error(`${cellLabel}.overhead must be 0, 8, or 16`);
    }
    for (const k of ["recallOk", "marginOk", "dedupOk"]) {
      if (typeof cell[k] !== "boolean") {
        throw new Error(`${cellLabel}.${k} must be a boolean`);
      }
    }
  }
  if (typeof result.sensitivity.allOk !== "boolean") {
    throw new Error(`${label}.result.sensitivity.allOk must be a boolean`);
  }
  if (!result.informational || typeof result.informational !== "object") {
    throw new Error(`${label}.result.informational is required`);
  }
  assertExactKeys(result.informational, ["paraphraseGap"], `${label}.result.informational`);
  const pg = result.informational.paraphraseGap;
  assertExactKeys(
    pg,
    ["queryTokens", "keyOverlapCount", "inHits", "neighborReachable", "neighborCost"],
    `${label}.result.informational.paraphraseGap`,
  );
  if (!Array.isArray(pg.queryTokens) || pg.queryTokens.length === 0) {
    throw new Error(`${label}.result.informational.paraphraseGap.queryTokens must be non-empty`);
  }
  for (const tok of pg.queryTokens) {
    if (typeof tok !== "string" || tok.length === 0) {
      throw new Error(
        `${label}.result.informational.paraphraseGap.queryTokens entries must be non-empty strings`,
      );
    }
  }
  if (!Number.isSafeInteger(pg.keyOverlapCount) || pg.keyOverlapCount < 0) {
    throw new Error(
      `${label}.result.informational.paraphraseGap.keyOverlapCount must be non-negative integer`,
    );
  }
  if (typeof pg.inHits !== "boolean") {
    throw new Error(`${label}.result.informational.paraphraseGap.inHits must be a boolean`);
  }
  if (typeof pg.neighborReachable !== "boolean") {
    throw new Error(
      `${label}.result.informational.paraphraseGap.neighborReachable must be a boolean`,
    );
  }
  if (!Number.isSafeInteger(pg.neighborCost) || pg.neighborCost < 0) {
    throw new Error(
      `${label}.result.informational.paraphraseGap.neighborCost must be a non-negative integer`,
    );
  }
  // Decision 115: neighborCandidates per gated scenario
  if (!Array.isArray(result.neighborCandidates) || result.neighborCandidates.length !== 6) {
    throw new Error(`${label}.result.neighborCandidates must be a 6-element array`);
  }
  for (const [index, entry] of result.neighborCandidates.entries()) {
    const eLabel = `${label}.result.neighborCandidates[${index}]`;
    assertExactKeys(entry, ["name", "generated", "admitted", "tokens"], eLabel);
    if (typeof entry.name !== "string" || entry.name.length === 0) {
      throw new Error(`${eLabel}.name must be a non-empty string`);
    }
    for (const k of ["generated", "admitted", "tokens"]) {
      if (!Number.isSafeInteger(entry[k]) || entry[k] < 0) {
        throw new Error(`${eLabel}.${k} must be a non-negative integer`);
      }
    }
    if (entry.admitted > 8) {
      throw new Error(`${eLabel}.admitted must be <=8`);
    }
    if (entry.generated < entry.admitted) {
      throw new Error(`${eLabel}.generated must be >= admitted`);
    }
  }
  // v64 corrected-baseline re-gate: require DeepAll as sole gated reference plus informational SummariesAll / IdentityDiag and audit tables
  for (const key of [
    "deepAll",
    "summariesAll",
    "identityDiag",
    "pagedV1",
    "pagedV2",
    "depthPremiumBps",
  ]) {
    if (!Number.isSafeInteger(result[key]) || result[key] < 0) {
      throw new Error(`${label}.result.${key} must be a non-negative integer`);
    }
  }
  if (!Array.isArray(result.perScenario) || result.perScenario.length !== 6) {
    throw new Error(
      `${label}.result.perScenario must be a 6-element array (one per gated scenario)`,
    );
  }
  for (const [index, entry] of result.perScenario.entries()) {
    const eLabel = `${label}.result.perScenario[${index}]`;
    assertExactKeys(
      entry,
      [
        "name",
        "deepAll",
        "summariesAll",
        "identityDiag",
        "pagedV1",
        "pagedV2",
        "recallV1",
        "recallV2",
        "toolCallsV1",
        "toolCallsV2",
      ],
      eLabel,
    );
    if (typeof entry.name !== "string" || entry.name.length === 0) {
      throw new Error(`${eLabel}.name must be a non-empty string`);
    }
    for (const k of [
      "deepAll",
      "summariesAll",
      "identityDiag",
      "pagedV1",
      "pagedV2",
      "recallV1",
      "recallV2",
      "toolCallsV1",
      "toolCallsV2",
    ]) {
      if (!Number.isSafeInteger(entry[k]) || entry[k] < 0) {
        throw new Error(`${eLabel}.${k} must be a non-negative integer`);
      }
    }
  }
  if (!Array.isArray(result.levelCensus) || result.levelCensus.length !== 6) {
    throw new Error(`${label}.result.levelCensus must be a 6-element array`);
  }
  for (const [index, entry] of result.levelCensus.entries()) {
    const cLabel = `${label}.result.levelCensus[${index}]`;
    assertExactKeys(
      entry,
      ["name", "source", "detailed", "structured", "summary", "identity"],
      cLabel,
    );
    if (typeof entry.name !== "string" || entry.name.length === 0) {
      throw new Error(`${cLabel}.name must be a non-empty string`);
    }
    for (const k of ["source", "detailed", "structured", "summary", "identity"]) {
      if (!Number.isSafeInteger(entry[k]) || entry[k] < 0) {
        throw new Error(`${cLabel}.${k} must be a non-negative integer`);
      }
    }
  }
  // D88: depthAwareRecall + answerLevelCensus + perLevelRecallSplit + escalationCurve + policyAdopted
  if (!result.depthAwareRecall || typeof result.depthAwareRecall !== "object") {
    throw new Error(`${label}.result.depthAwareRecall is required`);
  }
  assertExactKeys(result.depthAwareRecall, ["v1", "v2", "v3"], `${label}.result.depthAwareRecall`);
  for (const k of ["v1", "v2", "v3"]) {
    if (!Number.isSafeInteger(result.depthAwareRecall[k]) || result.depthAwareRecall[k] < 0) {
      throw new Error(`${label}.result.depthAwareRecall.${k} must be a non-negative integer`);
    }
  }
  if (!result.answerLevelCensus || typeof result.answerLevelCensus !== "object") {
    throw new Error(`${label}.result.answerLevelCensus is required`);
  }
  assertExactKeys(
    result.answerLevelCensus,
    ["asRun", "floorOk", "preCommitted"],
    `${label}.result.answerLevelCensus`,
  );
  for (const part of ["preCommitted", "asRun"]) {
    const obj = result.answerLevelCensus[part];
    if (!obj || typeof obj !== "object") {
      throw new Error(`${label}.result.answerLevelCensus.${part} must be an object`);
    }
    assertExactKeys(obj, ["structured", "summary"], `${label}.result.answerLevelCensus.${part}`);
    for (const k of ["structured", "summary"]) {
      if (!Number.isSafeInteger(obj[k]) || obj[k] < 0) {
        throw new Error(
          `${label}.result.answerLevelCensus.${part}.${k} must be a non-negative integer`,
        );
      }
    }
  }
  if (typeof result.answerLevelCensus.floorOk !== "boolean") {
    throw new Error(`${label}.result.answerLevelCensus.floorOk must be a boolean`);
  }
  if (!result.perLevelRecallSplit || typeof result.perLevelRecallSplit !== "object") {
    throw new Error(`${label}.result.perLevelRecallSplit is required`);
  }
  assertExactKeys(
    result.perLevelRecallSplit,
    ["structuredKeys", "summaryKeys"],
    `${label}.result.perLevelRecallSplit`,
  );
  for (const part of ["summaryKeys", "structuredKeys"]) {
    const obj = result.perLevelRecallSplit[part];
    if (!obj || typeof obj !== "object") {
      throw new Error(`${label}.result.perLevelRecallSplit.${part} must be an object`);
    }
    assertExactKeys(
      obj,
      ["recallV2", "recallV3", "total"],
      `${label}.result.perLevelRecallSplit.${part}`,
    );
    for (const k of ["total", "recallV2", "recallV3"]) {
      if (!Number.isSafeInteger(obj[k]) || obj[k] < 0) {
        throw new Error(
          `${label}.result.perLevelRecallSplit.${part}.${k} must be a non-negative integer`,
        );
      }
    }
  }
  if (!Array.isArray(result.escalationCurve) || result.escalationCurve.length !== 3) {
    throw new Error(`${label}.result.escalationCurve must be a 3-element array`);
  }
  for (const [index, entry] of result.escalationCurve.entries()) {
    const eLabel = `${label}.result.escalationCurve[${index}]`;
    assertExactKeys(entry, ["cost", "depthAwareRecall", "k", "toolCalls"], eLabel);
    if (![1, 2, 3].includes(entry.k)) {
      throw new Error(`${eLabel}.k must be 1,2,or 3`);
    }
    for (const k of ["cost", "depthAwareRecall", "toolCalls"]) {
      if (!Number.isSafeInteger(entry[k]) || entry[k] < 0) {
        throw new Error(`${eLabel}.${k} must be a non-negative integer`);
      }
    }
  }
  if (typeof result.policyAdopted !== "boolean") {
    throw new Error(`${label}.result.policyAdopted must be a boolean`);
  }
  if (typeof result.policyReason !== "string" || result.policyReason.length === 0) {
    throw new Error(`${label}.result.policyReason must be a non-empty string`);
  }
}

function validateContextSessionResult(record, label) {
  const result = record.result;
  // B4 (decision 100): audit is an optional additive field for session-audit-* scenarios.
  // Existing 4 scenarios remain byte-identical without it; new audit scenarios include it.
  const allowed = [
    "audit",
    "diagnostic",
    "enabled",
    "nodes",
    "off",
    "rounds",
    "tools",
    "workingSetNodes",
  ];
  const keys = Object.keys(result).sort();
  const required = ["diagnostic", "enabled", "nodes", "off", "rounds", "tools", "workingSetNodes"];
  for (const key of required) {
    if (!keys.includes(key)) {
      throw new Error(`${label}.result has unknown or missing fields`);
    }
  }
  for (const key of keys) {
    if (!allowed.includes(key)) {
      throw new Error(`${label}.result has unknown or missing fields`);
    }
  }
  if (result.audit !== undefined) {
    if (result.audit === null || typeof result.audit !== "object" || Array.isArray(result.audit)) {
      throw new Error(`${label}.result.audit must be an object`);
    }
    assertExactKeys(result.audit, ["counters", "ring"], `${label}.result.audit`);
    if (result.audit.counters !== null) {
      if (typeof result.audit.counters !== "object" || Array.isArray(result.audit.counters)) {
        throw new Error(`${label}.result.audit.counters must be an object or null`);
      }
      const counterKeys = [
        "assembled_summary_tokens_total",
        "budget_demotions_total",
        "coalesced_noop_ticks_total",
        "demand_updates_total",
        "demotions_total",
        "events_dropped_total",
        "events_total",
        "neighbor_stub_tokens_total",
        "pin_quota_demotions_total",
        "promotions_total",
        "stale_demotions_total",
        "ticks_total",
      ];
      assertExactKeys(result.audit.counters, counterKeys, `${label}.result.audit.counters`);
      for (const k of counterKeys) {
        if (!Number.isSafeInteger(result.audit.counters[k]) || result.audit.counters[k] < 0) {
          throw new Error(`${label}.result.audit.counters.${k} must be a non-negative integer`);
        }
      }
    }
    if (!Array.isArray(result.audit.ring) || result.audit.ring.length > 8) {
      throw new Error(`${label}.result.audit.ring must be a bounded array (<=8)`);
    }
    for (const [idx, rec] of result.audit.ring.entries()) {
      const rLabel = `${label}.result.audit.ring[${idx}]`;
      assertExactKeys(
        rec,
        [
          "assembled_summary_total",
          "assembled_unique_total",
          "canonical_event_count",
          "demotion_counts",
          "events_dropped",
          "now",
          "promotion_count",
          "stub_total",
          "tier_counts",
        ],
        rLabel,
      );
      if (!Number.isSafeInteger(rec.now) || rec.now < 0) {
        throw new Error(`${rLabel}.now must be a non-negative integer`);
      }
      if (!Number.isSafeInteger(rec.canonical_event_count) || rec.canonical_event_count < 0) {
        throw new Error(`${rLabel}.canonical_event_count must be a non-negative integer`);
      }
      if (!Number.isSafeInteger(rec.events_dropped) || rec.events_dropped < 0) {
        throw new Error(`${rLabel}.events_dropped must be a non-negative integer`);
      }
      assertExactKeys(rec.tier_counts, ["archive", "cold", "hot", "warm"], `${rLabel}.tier_counts`);
      for (const tk of ["archive", "cold", "hot", "warm"]) {
        if (!Number.isSafeInteger(rec.tier_counts[tk]) || rec.tier_counts[tk] < 0) {
          throw new Error(`${rLabel}.tier_counts.${tk} must be a non-negative integer`);
        }
      }
      if (!Number.isSafeInteger(rec.assembled_unique_total) || rec.assembled_unique_total < 0) {
        throw new Error(`${rLabel}.assembled_unique_total must be a non-negative integer`);
      }
      if (!Number.isSafeInteger(rec.assembled_summary_total) || rec.assembled_summary_total < 0) {
        throw new Error(`${rLabel}.assembled_summary_total must be a non-negative integer`);
      }
      if (!Number.isSafeInteger(rec.stub_total) || rec.stub_total < 0) {
        throw new Error(`${rLabel}.stub_total must be a non-negative integer`);
      }
      assertExactKeys(
        rec.demotion_counts,
        ["budget", "pin_quota", "stale"],
        `${rLabel}.demotion_counts`,
      );
      for (const dk of ["budget", "pin_quota", "stale"]) {
        if (!Number.isSafeInteger(rec.demotion_counts[dk]) || rec.demotion_counts[dk] < 0) {
          throw new Error(`${rLabel}.demotion_counts.${dk} must be a non-negative integer`);
        }
      }
      if (!Number.isSafeInteger(rec.promotion_count) || rec.promotion_count < 0) {
        throw new Error(`${rLabel}.promotion_count must be a non-negative integer`);
      }
    }
  }
  if (typeof result.enabled !== "boolean") {
    throw new Error(`${label}.result.enabled must be a boolean`);
  }
  if (typeof result.off !== "boolean") {
    throw new Error(`${label}.result.off must be a boolean`);
  }
  if (typeof result.diagnostic !== "string" && result.diagnostic !== null) {
    throw new Error(`${label}.result.diagnostic must be a string or null`);
  }
  if (!Number.isSafeInteger(result.nodes) || result.nodes < 0 || result.nodes > 512) {
    throw new Error(`${label}.result.nodes must be a small non-negative integer`);
  }
  if (!Number.isSafeInteger(result.workingSetNodes) || result.workingSetNodes < 0) {
    throw new Error(`${label}.result.workingSetNodes must be a non-negative integer`);
  }
  if (!Array.isArray(result.tools) || result.tools.length > 3) {
    throw new Error(`${label}.result.tools must be a bounded array`);
  }
  const toolNames = new Set(["context.search", "context.inspect", "context.expand"]);
  for (const [index, name] of result.tools.entries()) {
    if (typeof name !== "string" || !toolNames.has(name)) {
      throw new Error(`${label}.result.tools[${index}] must be a context tool name`);
    }
  }
  if (!Array.isArray(result.rounds) || result.rounds.length > 32) {
    throw new Error(`${label}.result.rounds must be a bounded array`);
  }
  for (const [index, round] of result.rounds.entries()) {
    const rLabel = `${label}.result.rounds[${index}]`;
    assertExactKeys(round, ["demoted", "promoted", "tick"], rLabel);
    if (!Number.isSafeInteger(round.tick) || round.tick < 0) {
      throw new Error(`${rLabel}.tick must be a non-negative integer`);
    }
    for (const key of ["promoted", "demoted"]) {
      if (!Array.isArray(round[key])) {
        throw new Error(`${rLabel}.${key} must be an array`);
      }
      for (const [j, id] of round[key].entries()) {
        if (typeof id !== "string" || id.length === 0) {
          throw new Error(`${rLabel}.${key}[${j}] must be a non-empty string`);
        }
      }
    }
  }
}

function validateTuiRenderResult(record, label) {
  assertExactKeys(record.result, ["frame", "scenario", "viewport"], `${label}.result`);
  const scenario = record.result.scenario;
  assertExactKeys(
    scenario,
    [
      "id",
      "input",
      "metrics",
      "model",
      "pane_present",
      "pending_approval",
      "provider",
      "scroll_offset",
      "status",
      "transcript_lines",
    ],
    `${label}.result.scenario`,
  );
  if (typeof scenario.id !== "string" || scenario.id.length === 0) {
    throw new Error(`${label}.result.scenario.id must be a non-empty string`);
  }
  if (!Array.isArray(scenario.transcript_lines) || scenario.transcript_lines.length > 200) {
    throw new Error(`${label}.result.scenario.transcript_lines must be a bounded array`);
  }
  for (const line of scenario.transcript_lines) {
    if (typeof line !== "string") {
      throw new Error(`${label}.result.scenario.transcript_lines entries must be strings`);
    }
  }
  if (typeof scenario.input !== "string" || typeof scenario.status !== "string") {
    throw new Error(`${label}.result.scenario.input/status must be strings`);
  }
  if (!Number.isSafeInteger(scenario.scroll_offset) || scenario.scroll_offset < 0) {
    throw new Error(`${label}.result.scenario.scroll_offset must be a non-negative integer`);
  }
  if (scenario.pending_approval !== null && !Array.isArray(scenario.pending_approval)) {
    throw new Error(`${label}.result.scenario.pending_approval must be null or an array`);
  }
  if (typeof scenario.pane_present !== "boolean") {
    throw new Error(`${label}.result.scenario.pane_present must be a boolean`);
  }
  for (const key of ["provider", "model"]) {
    if (scenario[key] !== null && typeof scenario[key] !== "string") {
      throw new Error(`${label}.result.scenario.${key} must be null or a string`);
    }
  }
  const metrics = scenario.metrics;
  assertExactKeys(
    metrics,
    ["cached_tokens", "context_assembled", "input_tokens", "output_tokens", "provider_millis"],
    `${label}.result.scenario.metrics`,
  );
  for (const key of ["input_tokens", "output_tokens", "provider_millis"]) {
    if (!Number.isSafeInteger(metrics[key]) || metrics[key] < 0) {
      throw new Error(`${label}.result.scenario.metrics.${key} must be a non-negative integer`);
    }
  }
  for (const key of ["cached_tokens", "context_assembled"]) {
    if (metrics[key] !== null && (!Number.isSafeInteger(metrics[key]) || metrics[key] < 0)) {
      throw new Error(
        `${label}.result.scenario.metrics.${key} must be null or a non-negative integer`,
      );
    }
  }
  assertExactKeys(record.result.viewport, ["height", "width"], `${label}.result.viewport`);
  if (record.result.viewport.width !== 80 || record.result.viewport.height !== 24) {
    throw new Error(`${label}.result.viewport must be the fixed 80x24 viewport`);
  }
  if (!Array.isArray(record.result.frame) || record.result.frame.length !== 24) {
    throw new Error(`${label}.result.frame must be exactly 24 row strings`);
  }
  for (const row of record.result.frame) {
    if (typeof row !== "string" || row.length > 80) {
      throw new Error(`${label}.result.frame rows must be strings of at most 80 characters`);
    }
  }
}

function validateSessionReplayResult(record, label) {
  assertExactKeys(
    record.result,
    [
      "evidenceDigest",
      "model",
      "providerId",
      "recordedCount",
      "remainingCounts",
      "snapshotCount",
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
  if (!Number.isInteger(record.result.recordedCount) || record.result.recordedCount < 0) {
    throw new Error(`${label}.result.recordedCount must be a non-negative integer`);
  }
  if (!Number.isInteger(record.result.snapshotCount) || record.result.snapshotCount < 0) {
    throw new Error(`${label}.result.snapshotCount must be a non-negative integer`);
  }
  if (
    typeof record.result.evidenceDigest !== "string" ||
    !LOWER_SHA256.test(record.result.evidenceDigest)
  ) {
    throw new Error(`${label}.result.evidenceDigest must be a lowercase SHA-256 digest`);
  }
}

export {
  validateContextBenchmarkResult,
  validateContextGraphResult,
  validateContextProjectionResult,
  validateContextRepresentationResult,
  validateContextSchedulerResult,
  validateContextSessionResult,
  validateContextToolResult,
  validateSessionReplayResult,
  validateTuiRenderResult,
};
