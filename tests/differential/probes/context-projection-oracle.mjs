/**
 * context-projection oracle probe (Stage 3R R7.3).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Runs each context-projection case against the REAL TypeScript reference:
 * capacity, estimator, pressure, trim, segments, evidence, tool visibility,
 * and the full ProjectedRequest composition via createProjectionService.
 *
 * Deterministic: no clock, randomness, or env enters records.
 */
import { readFileSync } from "node:fs";
import {
  estimateTokens,
  estimateConversationItemTokens,
} from "../../../packages/core/src/projection/context-estimator.js";
import { classifyPressure } from "../../../packages/core/src/projection/context-pressure.js";
import { trimConversationPreservingPairs } from "../../../packages/core/src/projection/conversation-trim.js";
import {
  createContextProjector,
  serializeContextPrefix,
} from "../../../packages/core/src/projection/context-projector.js";
import { createEvidenceProjector } from "../../../packages/core/src/projection/evidence-projector.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES)
    throw new Error("probe input must be bounded non-empty JSON");
  return JSON.parse(bytes.toString("utf8"));
}

function materialize(value) {
  if (Array.isArray(value)) return value.map(materialize);
  if (value !== null && typeof value === "object") {
    if (Object.hasOwn(value, "$repeat")) {
      const r = value.$repeat;
      if (
        typeof r.character !== "string" ||
        [...r.character].length !== 1 ||
        !Number.isSafeInteger(r.count) ||
        r.count < 0 ||
        r.count > 1048576
      )
        throw new Error("invalid $repeat");
      return r.character.repeat(r.count);
    }
    const out = {};
    for (const [k, v] of Object.entries(value)) out[k] = materialize(v);
    return out;
  }
  return value;
}

function runEstimateCase(c) {
  const texts = c.texts.map((t) => (typeof t === "string" ? t : ""));
  const textEstimates = texts.map((t) => ({
    text: t,
    bytes: Buffer.byteLength(t, "utf8"),
    tokens: estimateTokens(t),
  }));
  const conversationItems = c.conversationItems ?? [];
  const itemEstimates = conversationItems.map((item) => {
    const est = estimateConversationItemTokens(item);
    return { item, ...est };
  });
  return { textEstimates, itemEstimates };
}

function runPressureCase(c) {
  const limits = c.limits ?? { warnRatio: 0.7, autoRatio: 0.85, hardRatio: 1.0 };
  const results = c.cases.map((entry) => {
    const p = classifyPressure(entry.estimatedTokens, entry.workingMaximum, limits);
    return {
      estimatedTokens: p.estimatedTokens,
      workingMaximum: p.workingMaximum,
      ratio: p.ratio,
      state: p.state,
    };
  });
  return { results };
}

function runTrimCase(c) {
  const budget = c.maxTokens;
  const items = c.messages;
  const trimmed = trimConversationPreservingPairs(items, budget);
  const originalTokens = (() => {
    let s = 0;
    for (const it of items) s += estimateConversationItemTokens(it).tokens;
    return s;
  })();
  // Also check trimmed validity: classify
  return {
    originalTokens,
    maxTokens: budget,
    kept: trimmed.items,
    droppedItems: trimmed.droppedItems,
    estimatedTokens: trimmed.estimatedTokens,
  };
}

function runSegmentsCase(c) {
  const projector = createContextProjector();
  const projection = projector.project({ segments: c.segments });
  const systemPrefix = serializeContextPrefix(projection);
  return {
    stableSegments: projection.stableSegments,
    contextualSegments: projection.contextualSegments,
    volatileSegments: projection.volatileSegments,
    stableFingerprint: projection.stableFingerprint,
    stableBytes: projection.stableBytes,
    stablePrefixBytes: projection.stablePrefixBytes,
    totalBytes: projection.totalBytes,
    estimatedTokens: projection.estimatedTokens,
    systemPrefix,
    systemPrefixBytes: Buffer.byteLength(systemPrefix, "utf8"),
  };
}

async function runToolVisibility(c) {
  const { createToolProjector } =
    await import("../../../packages/core/src/projection/tool-projector.js");
  const { createDefaultPolicy } =
    await import("../../../packages/core/src/security/default-policy.js");
  const { DEVELOP_OFFLINE_PROFILE } =
    await import("../../../packages/core/src/security/profile.js");
  const registeredTools = c.registeredTools.map((t) => ({
    definition: { name: t.name, description: t.description, inputSchema: t.inputSchema },
    capability: t.capability,
  }));
  // Build policy from explicit rules or use default
  let policy;
  if (c.policyRules) {
    const base = createDefaultPolicy("develop-offline");
    const rules = { ...base.rules };
    for (const r of c.policyRules) rules[r.capability] = r.decision;
    policy = { rules };
  } else {
    policy = createDefaultPolicy("develop-offline");
  }
  const projector = createToolProjector({ policy, profile: DEVELOP_OFFLINE_PROFILE });
  const proj = projector.project({ mode: c.mode, registeredTools, surface: c.surface });
  return {
    tools: proj.tools.map((t) => ({ name: t.name, visibility: t.visibility })),
    counts: proj.counts,
    fingerprint: proj.fingerprint,
    requestTools: proj.requestTools.map((t) => ({ name: t.name, description: t.description })),
    approvedNames: [...proj.requestTools.map((t) => t.name)].sort(),
  };
}

function runEvidenceCase(c) {
  const projector = createEvidenceProjector({
    secrets: c.secrets ?? [],
    maxTotalBytes: c.maxTotalBytes,
    maxLineBytes: c.maxLineBytes,
  });
  const view = projector.projectForModel({ rawText: c.rawText });
  // Also project ToolResult if provided
  let toolResultView = null;
  if (c.toolResult) {
    const tr = c.toolResult;
    // Simulate what projection-service does: project summary or message
    if (tr.status === "success") {
      const v = projector.projectForModel({ rawText: tr.summary });
      toolResultView = {
        status: "success",
        summary: v.text,
        truncated: v.truncated,
        transformations: v.transformations,
        shownBytes: v.shownBytes,
        originalBytes: v.originalBytes,
      };
    } else {
      const v = projector.projectForModel({ rawText: tr.message });
      toolResultView = {
        status: tr.status,
        message: v.text,
        truncated: v.truncated,
        transformations: v.transformations,
      };
    }
  }
  return {
    text: view.text,
    truncated: view.truncated,
    shownBytes: view.shownBytes,
    originalBytes: view.originalBytes,
    transformations: view.transformations,
    toolResultView: toolResultView ?? null,
  };
}

function runFingerprintsCase(c) {
  const projector = createContextProjector();
  const base = projector.project({ segments: c.baseSegments });
  const variant = projector.project({ segments: c.variantSegments });
  // Tool fingerprint via service
  return {
    stableFingerprint: base.stableFingerprint,
    variantStableFingerprint: variant.stableFingerprint,
    stableFingerprintUnchanged: base.stableFingerprint === variant.stableFingerprint,
    stableBytesUnchanged: base.stableBytes === variant.stableBytes,
    stablePrefixBytesUnchanged: base.stablePrefixBytes === variant.stablePrefixBytes,
  };
}

async function runPipeline(c) {
  const { createProjectionService } =
    await import("../../../packages/core/src/projection/projection-service.js");
  const { createDefaultPolicy } =
    await import("../../../packages/core/src/security/default-policy.js");
  const { DEVELOP_OFFLINE_PROFILE } =
    await import("../../../packages/core/src/security/profile.js");
  const capacity =
    c.capacity ??
    (c.workingMaximum !== undefined
      ? {
          workingMaximum: c.workingMaximum,
          advertisedMaximum: null,
          verifiedMaximum: null,
          maxOutputTokens: 4096,
        }
      : {
          workingMaximum: 32768,
          advertisedMaximum: null,
          verifiedMaximum: null,
          maxOutputTokens: 4096,
        });
  const service = createProjectionService({
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    capacity,
    stableInstructions: c.stableInstructions ?? "You are Siralos.",
    getTaskSnapshot: () => null,
    getCurrentPlan: () => null,
    evidence: {
      secrets: c.secrets ?? [],
      maxTotalBytes: c.maxTotalBytes,
      maxLineBytes: c.maxLineBytes,
    },
  });
  const messages = c.messages ?? [];
  const tools = (c.registeredTools ?? []).map((t) => ({
    definition: { name: t.name, description: t.description, inputSchema: t.inputSchema },
    capability: t.capability,
  }));
  const req = service.projectRequest({
    mode: c.mode ?? "generic",
    messages,
    tools,
    providerToolCalling: c.providerToolCalling ?? true,
  });
  return {
    estimatedTokens: req.estimatedTokens,
    workingMaximum: capacity.workingMaximum,
    pressure: req.pressure,
    blocked: req.blocked,
    providerCalled: req.blocked === null,
    toolProjection: {
      counts: req.toolProjection.counts,
      fingerprint: req.toolProjection.fingerprint,
    },
    contextFingerprint: req.contextProjection.stableFingerprint,
    systemPrefixBytes: Buffer.byteLength(req.system ?? "", "utf8"),
  };
}

// Main
const input = readStdinBounded();
const cases = [];
for (const entry of input.cases ?? []) {
  const kind = entry.kind;
  const c = materialize(entry.input);
  let result;
  if (kind === "estimate") result = { kind, result: runEstimateCase(c) };
  else if (kind === "pressure") result = { kind, result: runPressureCase(c) };
  else if (kind === "trim") result = { kind, result: runTrimCase(c) };
  else if (kind === "segments") result = { kind, result: runSegmentsCase(c) };
  else if (kind === "tool-visibility") result = { kind, result: await runToolVisibility(c) };
  else if (kind === "evidence") result = { kind, result: runEvidenceCase(c) };
  else if (kind === "fingerprints") result = { kind, result: runFingerprintsCase(c) };
  else if (kind === "pipeline") result = { kind, result: await runPipeline(c) };
  else if (kind === "unsupported-tool-calling") {
    const { createProjectionService } =
      await import("../../../packages/core/src/projection/projection-service.js");
    const { createDefaultPolicy } =
      await import("../../../packages/core/src/security/default-policy.js");
    const { DEVELOP_OFFLINE_PROFILE } =
      await import("../../../packages/core/src/security/profile.js");
    const service = createProjectionService({
      policy: (() => {
        if (c.policyRules) {
          const base = createDefaultPolicy("develop-offline");
          const rules = { ...base.rules };
          for (const r of c.policyRules) rules[r.capability] = r.decision;
          return { rules };
        }
        return createDefaultPolicy("develop-offline");
      })(),
      profile: DEVELOP_OFFLINE_PROFILE,
      capacity: {
        workingMaximum: 32768,
        advertisedMaximum: null,
        verifiedMaximum: null,
        maxOutputTokens: 4096,
      },
      stableInstructions: c.segments
        ? c.segments.map((s) => `[${s.title}]\n${s.content}`).join("\n\n")
        : "",
      getTaskSnapshot: () => null,
    });
    const req = service.projectRequest({
      mode: c.mode,
      messages: c.messages ?? [],
      tools: (c.registeredTools ?? []).map((t) => ({
        definition: { name: t.name, description: t.description, inputSchema: t.inputSchema },
        capability: t.capability,
      })),
      providerToolCalling: false,
    });
    result = {
      kind,
      result: {
        mode: req.mode,
        blocked: req.blocked,
        toolCounts: req.toolProjection.counts,
        fingerprint: req.toolProjection.fingerprint,
        requestTools: req.toolProjection.requestTools.map((t) => t.name),
        estimatedTokens: req.estimatedTokens,
        providerCalled: false,
      },
    };
  } else throw new Error(`unknown context-projection kind ${kind}`);
  cases.push(result);
}
process.stdout.write(JSON.stringify({ cases }));
