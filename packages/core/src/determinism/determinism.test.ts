import { describe, expect, it } from "vitest";
import {
  createFixedClock,
  createSeededRandomSource,
  createSystemClock,
  normalizeKeyedResults,
} from "./context.js";
import { computeEnvironmentDelta, createEnvironmentManifest } from "./environment.js";
import {
  computeProviderInputIdentityDigest,
  computeReproducibilityDelta,
  createReproducibilityManifest,
} from "./reproducibility.js";
import {
  classifyRetry,
  deriveValidationPlan,
  evaluateAcceptance,
  normalizeConcurrentResults,
  type ValidationPlanInput,
} from "./decisions.js";
import { discoverRepository, listOwnership, resolveOwner } from "./discovery.js";

const SHA = (letter: string): string => letter.repeat(64);

describe("clock", () => {
  it("fixed clock gives repeatable decisions and advances only on demand", () => {
    const clock = createFixedClock(1_000);
    expect(clock.now()).toBe(1_000);
    expect(clock.now()).toBe(1_000);
    clock.advance(500);
    expect(clock.now()).toBe(1_500);
    clock.set(2_000);
    expect(clock.now()).toBe(2_000);
  });

  it("system clock is the explicit adapter boundary", () => {
    const system = createSystemClock();
    expect(system()).toBeGreaterThan(0);
  });
});

describe("random source", () => {
  it("seeded source repeats exactly for the same seed", () => {
    const a = createSeededRandomSource(42);
    const b = createSeededRandomSource(42);
    const valuesA = [a.next(), a.nextInt(100), a.nextToken()];
    const valuesB = [b.next(), b.nextInt(100), b.nextToken()];
    expect(valuesA).toEqual(valuesB);
    const c = createSeededRandomSource(43);
    expect(c.next()).not.toBe(valuesA[0]);
  });
});

describe("ordering policy", () => {
  it("normalizes keyed results independent of input order", () => {
    const shuffled = [
      { id: "c", value: 3 },
      { id: "a", value: 1 },
      { id: "b", value: 2 },
    ];
    const other = [
      { id: "b", value: 2 },
      { id: "c", value: 3 },
      { id: "a", value: 1 },
    ];
    expect(normalizeKeyedResults(shuffled)).toEqual(normalizeKeyedResults(other));
    expect(normalizeKeyedResults(shuffled).map((entry) => entry.id)).toEqual(["a", "b", "c"]);
  });
});

describe("environment manifest", () => {
  it("is deterministic regardless of allowlist/tool order", () => {
    const first = createEnvironmentManifest({
      siralosVersion: "0.0.0",
      nodeVersion: "v26",
      npmVersion: "11",
      platform: "win32",
      arch: "x64",
      osRelease: null,
      godotExecutableFingerprint: null,
      sandboxBackendId: "sandbox",
      sandboxVersion: "1",
      localePolicy: "en-US",
      timezonePolicy: "UTC",
      environmentAllowlist: ["PATH", "HOME"],
      toolIdentities: [
        { name: "node", digest: SHA("n") },
        { name: "git", digest: SHA("g") },
      ],
    });
    const second = createEnvironmentManifest({
      siralosVersion: "0.0.0",
      nodeVersion: "v26",
      npmVersion: "11",
      platform: "win32",
      arch: "x64",
      osRelease: null,
      godotExecutableFingerprint: null,
      sandboxBackendId: "sandbox",
      sandboxVersion: "1",
      localePolicy: "en-US",
      timezonePolicy: "UTC",
      environmentAllowlist: ["HOME", "PATH"],
      toolIdentities: [
        { name: "git", digest: SHA("g") },
        { name: "node", digest: SHA("n") },
      ],
    });
    expect(second.digest).toBe(first.digest);
  });

  it("surfaces only the changed environment dimension in its delta", () => {
    const base = createEnvironmentManifest({
      siralosVersion: "0.0.0",
      nodeVersion: "v26",
      npmVersion: null,
      platform: null,
      arch: null,
      osRelease: null,
      godotExecutableFingerprint: null,
      sandboxBackendId: null,
      sandboxVersion: null,
      localePolicy: "en-US",
      timezonePolicy: "UTC",
      environmentAllowlist: [],
      toolIdentities: [],
    });
    const changed = createEnvironmentManifest({
      siralosVersion: "0.0.0",
      nodeVersion: "v26.1.0",
      npmVersion: null,
      platform: null,
      arch: null,
      osRelease: null,
      godotExecutableFingerprint: null,
      sandboxBackendId: null,
      sandboxVersion: null,
      localePolicy: "en-US",
      timezonePolicy: "UTC",
      environmentAllowlist: [],
      toolIdentities: [],
    });
    const delta = computeEnvironmentDelta(base, changed);
    expect(delta.changed).toEqual(["nodeVersion"]);
    expect(delta.unchangedContent).toBe(false);
    expect(delta.baseDigest).not.toBe(delta.resultDigest);
  });
});

describe("reproducibility manifest", () => {
  function manifest(nodeVersion: string, modelRoute: string | null = "fake") {
    return createReproducibilityManifest({
      taskId: "task-1",
      executionInputDigest: SHA("a"),
      environmentDigest: SHA("e"),
      taskContractDigest: SHA("c"),
      taskPlanDigest: SHA("p"),
      guidanceDigest: SHA("g"),
      toolSurfaceDigest: SHA("t"),
      capabilityDigest: SHA("k"),
      sourceRevisionSet: [
        { path: "player.gd", revision: "rev_" + "a".repeat(32) },
        { path: "scene.tscn", revision: "rev_" + "b".repeat(32) },
      ],
      validationProfile: "validation-offline-1",
      providerInput: {
        providerRoute: modelRoute,
        modelIdentity: "fake-model",
        reasoningMode: null,
        temperature: null,
        topP: null,
        seed: null,
        parameters: [],
      },
      clockPolicy: { mode: "fixed", fixedMs: 1_000 },
      rngPolicy: { mode: "none", seed: null },
    });
    void nodeVersion;
  }

  it("identical authoritative inputs produce the same reproducibility digest", () => {
    const a = manifest("v26");
    const b = manifest("v26");
    expect(b.digest).toBe(a.digest);
  });

  it("changing exactly one dimension changes the digest and the delta identifies it", () => {
    const a = manifest("v26");
    const b = manifest("v26", "different-model-route");
    expect(b.digest).not.toBe(a.digest);
    const delta = computeReproducibilityDelta(a, b);
    expect(delta.changed).toEqual(["providerInput"]);
    expect(delta.unchanged).toContain("taskContract");
    expect(delta.unchanged).toContain("guidance");
    expect(delta.unchanged).toContain("toolSurface");
  });

  it("records behavior-affecting provider configuration deterministically", () => {
    const first = computeProviderInputIdentityDigest({
      providerRoute: "fake",
      modelIdentity: "m",
      reasoningMode: "low",
      temperature: 0.2,
      topP: 0.9,
      seed: 7,
      parameters: [
        { name: "b", value: "2" },
        { name: "a", value: "1" },
      ],
    });
    const second = computeProviderInputIdentityDigest({
      providerRoute: "fake",
      modelIdentity: "m",
      reasoningMode: "low",
      temperature: 0.2,
      topP: 0.9,
      seed: 7,
      parameters: [
        { name: "a", value: "1" },
        { name: "b", value: "2" },
      ],
    });
    expect(first).toBe(second);
  });
});

describe("deterministic validation plan", () => {
  const registry: ValidationPlanInput["validationRegistry"] = [
    { id: "check-only-parse", appliesTo: [".gd"], baseClass: "required" },
    { id: "lsp-diagnostics", appliesTo: [".gd"], baseClass: "required" },
    { id: "scene-reparse", appliesTo: [".tscn", ".tres"], baseClass: "required" },
    { id: "project-tests", appliesTo: ["tests"], baseClass: "recommended" },
    { id: "godot-runtime", appliesTo: [".gd"], baseClass: "unavailable" },
  ];

  const input: ValidationPlanInput = {
    changedSurfaces: ["scripts/player/player.gd"],
    impactRelationships: [{ source: "scripts/player/player.gd", target: "scenes/player.tscn" }],
    acceptanceCriteria: [
      { id: "parses", verificationKind: "deterministic" },
      { id: "review-clean", verificationKind: "review" },
    ],
    validationRegistry: registry,
  };

  it("identical inputs (shuffled) produce the identical required plan", () => {
    const shuffled: ValidationPlanInput = {
      ...input,
      validationRegistry: [...registry].reverse(),
      impactRelationships: [...input.impactRelationships].reverse(),
      changedSurfaces: [...input.changedSurfaces].reverse(),
    };
    const first = deriveValidationPlan(input);
    const second = deriveValidationPlan(shuffled);
    expect(second.digest).toBe(first.digest);
    expect(second.items.map((item) => item.id)).toEqual(first.items.map((item) => item.id));
    // Scene-reparse is required through the verified impact relationship.
    expect(first.items.find((item) => item.id === "scene-reparse")?.class).toBe("required");
    expect(first.items.find((item) => item.id === "check-only-parse")?.class).toBe("required");
  });

  it("marks unavailable checks honestly without removing required ones", () => {
    const plan = deriveValidationPlan(input);
    expect(plan.items.find((item) => item.id === "godot-runtime")?.class).toBe("unavailable");
    expect(plan.items.find((item) => item.id === "check-only-parse")?.class).toBe("required");
  });
});

describe("deterministic acceptance", () => {
  it("identical evidence in different insertion orders produces identical results and digests", () => {
    const evidence = [
      { id: "ev-1", class: "parser_result", digest: SHA("1") },
      { id: "ev-2", class: "lsp_result", digest: SHA("2") },
    ];
    const a = evaluateAcceptance({
      criterionId: "parses",
      requiredEvidenceClasses: ["parser_result", "lsp_result"],
      availableEvidence: evidence,
    });
    const b = evaluateAcceptance({
      criterionId: "parses",
      requiredEvidenceClasses: ["parser_result", "lsp_result"],
      availableEvidence: [evidence[1]!, evidence[0]!],
    });
    expect(a.outcome).toBe("satisfied");
    expect(b.outcome).toBe("satisfied");
    expect(b.digest).toBe(a.digest);
    expect(b.evidenceIdentities).toEqual(a.evidenceIdentities);
  });

  it("missing evidence classes produce not_satisfied", () => {
    const result = evaluateAcceptance({
      criterionId: "parses",
      requiredEvidenceClasses: ["parser_result", "lsp_result"],
      availableEvidence: [{ id: "ev-1", class: "parser_result", digest: SHA("1") }],
    });
    expect(result.outcome).toBe("not_satisfied");
  });
});

describe("typed retry policy", () => {
  it("same failure class produces the same host decision", () => {
    const transient = classifyRetry("transient_provider_transport", 0);
    expect(transient.decision).toBe("retry");
    expect(transient.nextBackoffMs).toBe(100);
    expect(classifyRetry("transient_provider_transport", 0)).toEqual(transient);
    expect(classifyRetry("transient_provider_transport", 3).decision).toBe("no_retry");
  });

  it("stale revisions and denied approvals never auto-retry", () => {
    expect(classifyRetry("stale_source_revision", 0).decision).toBe("no_retry");
    expect(classifyRetry("approval_denied", 0).decision).toBe("no_retry");
    expect(classifyRetry("infrastructure_unavailable", 0).decision).toBe("no_retry");
    expect(classifyRetry("blocking_review_finding", 0).decision).toBe("repair");
    expect(classifyRetry("malformed_tool_representation", 0).decision).toBe("repair");
  });
});

describe("concurrency normalization", () => {
  it("shuffled completion order produces the same consumed result", () => {
    const results = [
      { id: "b", content: "beta" },
      { id: "a", content: "alpha" },
      { id: "c", content: "gamma" },
    ];
    const shuffled = [results[2]!, results[0]!, results[1]!];
    expect(normalizeConcurrentResults(results).map((entry) => entry.id)).toEqual(
      normalizeConcurrentResults(shuffled).map((entry) => entry.id),
    );
  });
});

describe("deterministic repository discovery", () => {
  it("shuffled input order produces the same ordered candidates and digest", () => {
    const unordered = [
      {
        path: "scenes/player.tscn",
        relevance: "candidate" as const,
        evidence: ["naming similarity"],
      },
      {
        path: "scripts/player/player.gd",
        relevance: "verified" as const,
        evidence: ["exact task target"],
      },
      { path: "tests/test_player.gd", relevance: "candidate" as const, evidence: ["test mapping"] },
      {
        path: "scripts/player/health.gd",
        relevance: "candidate" as const,
        evidence: ["naming similarity"],
      },
    ];
    const first = discoverRepository({
      unorderedCandidates: unordered,
      maxCandidates: 3,
      taskTargets: ["scripts/player/player.gd"],
    });
    const second = discoverRepository({
      unorderedCandidates: [...unordered].reverse(),
      maxCandidates: 3,
      taskTargets: ["scripts/player/player.gd"],
    });
    expect(second.digest).toBe(first.digest);
    expect(second.candidates.map((candidate) => candidate.path)).toEqual(
      first.candidates.map((candidate) => candidate.path),
    );
    // Exact task target ranks first; naming similarity alone never becomes verified.
    expect(first.candidates[0]?.path).toBe("scripts/player/player.gd");
    expect(first.candidates[0]?.relevance).toBe("verified");
    const similarityOnly = discoverRepository({
      unorderedCandidates: [
        {
          path: "scripts/player/player.gd",
          relevance: "candidate" as const,
          evidence: ["naming similarity"],
        },
      ],
      maxCandidates: 3,
      taskTargets: [],
    });
    expect(similarityOnly.candidates[0]?.relevance).toBe("candidate");
  });
});

describe("ownership resolution", () => {
  it("same responsibility resolves the same canonical owner", () => {
    expect(resolveOwner("tool projection")).toEqual(
      expect.objectContaining({ owner: "ToolProjector" }),
    );
    expect(resolveOwner("TOOL PROJECTION")).toEqual(resolveOwner("tool projection"));
    expect(resolveOwner("provider tool schema")?.owner).toBe("ToolProjector");
    expect(resolveOwner("unknown responsibility")).toBeNull();
  });

  it("lists owners in stable canonical order", () => {
    const owners = listOwnership();
    const ids = owners.map((entry) => entry.responsibility);
    expect(ids).toEqual([...ids].sort());
    expect(owners.length).toBeGreaterThan(10);
  });
});
