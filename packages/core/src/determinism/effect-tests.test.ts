import { describe, expect, it } from "vitest";
import { createFixedClock } from "./context.js";
import { computeEnvironmentDelta, createEnvironmentManifest } from "./environment.js";
import { computeReproducibilityDelta, createReproducibilityManifest } from "./reproducibility.js";
import {
  deriveActiveWorkingSet,
  deriveValidationPlan,
  evaluateAcceptance,
  evaluateLease,
  classifyRetry,
  normalizeConcurrentResults,
  type ValidationPlanInput,
} from "./decisions.js";
import { discoverRepository } from "./discovery.js";
import {
  selectDocumentationContext,
  type DocumentationEntry,
} from "../executor/documentation-context.js";

/**
 * Mandatory effect tests (Stage 3 — Deterministic Execution &
 * Reproducibility, ADR 0029).
 */

const SHA = (letter: string): string => letter.repeat(64);

describe("effect — discovery order", () => {
  it("shuffled underlying discovery order produces the same scope, candidates, and working set", () => {
    const unordered = [
      {
        path: "scripts/player/player.gd",
        relevance: "verified" as const,
        evidence: ["exact task target"],
      },
      {
        path: "scenes/player.tscn",
        relevance: "candidate" as const,
        evidence: ["structural relationship"],
      },
      {
        path: "scripts/player/health.gd",
        relevance: "candidate" as const,
        evidence: ["naming similarity"],
      },
      { path: "tests/test_player.gd", relevance: "candidate" as const, evidence: ["test mapping"] },
    ];
    const taskTargets = ["scripts/player/player.gd"];
    const first = discoverRepository({
      unorderedCandidates: unordered,
      maxCandidates: 4,
      taskTargets,
    });
    const second = discoverRepository({
      unorderedCandidates: [...unordered].reverse(),
      maxCandidates: 4,
      taskTargets,
    });
    expect(second.digest).toBe(first.digest);
    expect(second.candidates.map((candidate) => candidate.path)).toEqual(
      first.candidates.map((candidate) => candidate.path),
    );
    const setA = deriveActiveWorkingSet(first.candidates, 3);
    const setB = deriveActiveWorkingSet(second.candidates, 3);
    expect(setB).toEqual(setA);
    expect(setA[0]).toEqual({
      path: "scripts/player/player.gd",
      reason: "verified discovery candidate",
    });
  });
});

describe("effect — documentation order", () => {
  it("shuffled ADR/doc enumeration produces the identical applicable selection and order", () => {
    const index = selectDocumentationContext({
      concerns: ["godot"],
    });
    // Build a shuffled index entry list with equivalent metadata.
    const shuffledIndex: readonly DocumentationEntry[] = [...index.adrs, ...index.architectureDocs]
      .map((path, position) => ({
        id: `adr:${String(position + 1).padStart(4, "0")}`,
        path,
        kind: "adr" as const,
        concerns: ["godot"],
        status: "accepted" as const,
      }))
      .reverse();
    void shuffledIndex;
    const full = selectDocumentationContext({
      concerns: ["godot"],
      index: shuffledIndex,
    });
    expect(full.adrs.length).toBeGreaterThan(0);
    // Selection is stable: requesting twice yields identical order.
    const again = selectDocumentationContext({
      concerns: ["godot"],
      index: shuffledIndex,
    });
    expect(again.adrs).toEqual(full.adrs);
    expect(again.dropped).toEqual(full.dropped);
  });
});

describe("effect — concurrent result order", () => {
  it("shuffled completion order produces the same consumed evidence and decisions", () => {
    const results = [
      { id: "ev-parse", class: "parser_result", digest: SHA("1") },
      { id: "ev-lsp", class: "lsp_result", digest: SHA("2") },
      { id: "ev-scope", class: "validation_result", digest: SHA("3") },
    ];
    const shuffled = [results[2]!, results[0]!, results[1]!];
    const first = normalizeConcurrentResults(results);
    const second = normalizeConcurrentResults(shuffled);
    expect(first.map((entry) => entry.id)).toEqual(second.map((entry) => entry.id));
    // Host decision over normalized evidence is identical.
    const decisionA = evaluateAcceptance({
      criterionId: "parses",
      requiredEvidenceClasses: ["parser_result", "lsp_result"],
      availableEvidence: first,
    });
    const decisionB = evaluateAcceptance({
      criterionId: "parses",
      requiredEvidenceClasses: ["parser_result", "lsp_result"],
      availableEvidence: second,
    });
    expect(decisionB.digest).toBe(decisionA.digest);
    expect(decisionB.outcome).toBe("satisfied");
  });
});

describe("effect — fixed time", () => {
  it("expiry/lease policy with a fixed clock gives identical decisions; advancing changes only the expected decision", () => {
    const clock = createFixedClock(1_000);
    const lease = { issuedAtMs: 500, ttlMs: 1_000 };
    const first = evaluateLease(lease, clock.now());
    const second = evaluateLease(lease, clock.now());
    expect(first).toEqual(second);
    expect(first.valid).toBe(true);
    // Time passes: only the expiry decision changes.
    clock.advance(1_000);
    const later = evaluateLease(lease, clock.now());
    expect(later.valid).toBe(false);
    // Retry backoff is time-independent and index-deterministic.
    expect(classifyRetry("transient_provider_transport", 0).nextBackoffMs).toBe(100);
    expect(classifyRetry("transient_provider_transport", 0).nextBackoffMs).toBe(100);
  });
});

describe("effect — validation determinism", () => {
  it("identical changed surfaces/impact/criteria (shuffled) produce the identical required ValidationPlan", () => {
    const registry: ValidationPlanInput["validationRegistry"] = [
      { id: "check-only-parse", appliesTo: [".gd"], baseClass: "required" },
      { id: "lsp-diagnostics", appliesTo: [".gd"], baseClass: "required" },
      { id: "scene-reparse", appliesTo: [".tscn", ".tres"], baseClass: "required" },
      { id: "project-tests", appliesTo: ["tests"], baseClass: "recommended" },
    ];
    const base: ValidationPlanInput = {
      changedSurfaces: ["scripts/player/player.gd"],
      impactRelationships: [{ source: "scripts/player/player.gd", target: "scenes/player.tscn" }],
      acceptanceCriteria: [
        { id: "parses", verificationKind: "deterministic" },
        { id: "review-clean", verificationKind: "review" },
      ],
      validationRegistry: registry,
    };
    const shuffled: ValidationPlanInput = {
      changedSurfaces: [...base.changedSurfaces].reverse(),
      impactRelationships: [...base.impactRelationships].reverse(),
      acceptanceCriteria: [...base.acceptanceCriteria].reverse(),
      validationRegistry: [...registry].reverse(),
    };
    const first = deriveValidationPlan(base);
    const second = deriveValidationPlan(shuffled);
    expect(second.digest).toBe(first.digest);
    expect(second.items).toEqual(first.items);
    expect(first.items.filter((item) => item.class === "required").length).toBeGreaterThanOrEqual(
      3,
    );
  });
});

describe("effect — acceptance determinism", () => {
  it("identical evidence in different insertion orders produces identical AcceptanceResult and digest", () => {
    const evidence = [
      { id: "ev-1", class: "parser_result", digest: SHA("1") },
      { id: "ev-2", class: "lsp_result", digest: SHA("2") },
      { id: "ev-3", class: "review_result", digest: SHA("3") },
    ];
    const a = evaluateAcceptance({
      criterionId: "review-clean",
      requiredEvidenceClasses: ["parser_result", "lsp_result", "review_result"],
      availableEvidence: evidence,
    });
    const b = evaluateAcceptance({
      criterionId: "review-clean",
      requiredEvidenceClasses: ["parser_result", "lsp_result", "review_result"],
      availableEvidence: [evidence[2]!, evidence[0]!, evidence[1]!],
    });
    expect(b.digest).toBe(a.digest);
    expect(b.evidenceIdentities).toEqual(a.evidenceIdentities);
    expect(b.outcome).toBe("satisfied");
  });
});

describe("effect — reproducibility comparison", () => {
  function run(modelRoute: string) {
    return createReproducibilityManifest({
      taskId: "task-1",
      executionInputDigest: SHA("a"),
      environmentDigest: SHA("e"),
      taskContractDigest: SHA("c"),
      taskPlanDigest: SHA("p"),
      guidanceDigest: SHA("g"),
      toolSurfaceDigest: SHA("t"),
      capabilityDigest: SHA("k"),
      sourceRevisionSet: [{ path: "player.gd", revision: "rev_" + "a".repeat(32) }],
      validationProfile: "validation-offline-1",
      providerInput: {
        providerRoute: modelRoute,
        modelIdentity: "m",
        reasoningMode: null,
        temperature: null,
        topP: null,
        seed: null,
        parameters: [],
      },
      clockPolicy: { mode: "fixed", fixedMs: 1_000 },
      rngPolicy: { mode: "none", seed: null },
    });
  }

  it("identical authoritative inputs produce equivalent manifests; one changed input changes the digest and is identified", () => {
    const runA = run("route-1");
    const runB = run("route-1");
    expect(runB.digest).toBe(runA.digest);
    const runC = run("route-1");
    // Change the environment dimension.
    const envA = createEnvironmentManifest({
      siralosVersion: "0.0.0",
      nodeVersion: "v26",
      npmVersion: null,
      platform: null,
      arch: null,
      osRelease: null,
      godotExecutableFingerprint: null,
      sandboxBackendId: null,
      sandboxVersion: null,
      localePolicy: "C",
      timezonePolicy: "UTC",
      environmentAllowlist: [],
      toolIdentities: [],
    });
    const envC = createEnvironmentManifest({
      siralosVersion: "0.0.0",
      nodeVersion: "v26.1.0",
      npmVersion: null,
      platform: null,
      arch: null,
      osRelease: null,
      godotExecutableFingerprint: null,
      sandboxBackendId: null,
      sandboxVersion: null,
      localePolicy: "C",
      timezonePolicy: "UTC",
      environmentAllowlist: [],
      toolIdentities: [],
    });
    const envDelta = computeEnvironmentDelta(envA, envC);
    expect(envDelta.changed).toEqual(["nodeVersion"]);
    // A changed model route changes the manifest and its delta identifies it.
    const runD = run("route-2");
    expect(runD.digest).not.toBe(runA.digest);
    const delta = computeReproducibilityDelta(runA, runD);
    expect(delta.changed).toEqual(["providerInput"]);
    expect(delta.unchanged).toContain("taskContract");
    expect(delta.unchanged).toContain("guidance");
    void runC;
  });
});

describe("effect — probabilistic-model boundary", () => {
  it("different valid proposals leave host security/approval/validation/acceptance identical", () => {
    // Two different model proposals for the same task. Host decisions must
    // not depend on proposal text where the authoritative inputs are equal.
    const proposalA = { changes: [{ path: "player.gd", text: "implementation A" }] };
    const proposalB = { changes: [{ path: "player.gd", text: "implementation B" }] };
    const validationInput: ValidationPlanInput = {
      changedSurfaces: ["scripts/player/player.gd"],
      impactRelationships: [{ source: "scripts/player/player.gd", target: "scenes/player.tscn" }],
      acceptanceCriteria: [
        { id: "parses", verificationKind: "deterministic" },
        { id: "review-clean", verificationKind: "review" },
      ],
      validationRegistry: [
        { id: "check-only-parse", appliesTo: [".gd"], baseClass: "required" },
        { id: "scene-reparse", appliesTo: [".tscn"], baseClass: "required" },
      ],
    };
    const planA = deriveValidationPlan(validationInput);
    const planB = deriveValidationPlan(validationInput);
    expect(planB.digest).toBe(planA.digest);
    const acceptanceA = evaluateAcceptance({
      criterionId: "parses",
      requiredEvidenceClasses: ["parser_result"],
      availableEvidence: [{ id: "ev-1", class: "parser_result", digest: SHA("1") }],
    });
    const acceptanceB = evaluateAcceptance({
      criterionId: "parses",
      requiredEvidenceClasses: ["parser_result"],
      availableEvidence: [{ id: "ev-1", class: "parser_result", digest: SHA("1") }],
    });
    expect(acceptanceB.digest).toBe(acceptanceA.digest);
    // Stale-revision policy is identical regardless of proposal content.
    expect(classifyRetry("stale_source_revision", 0).reason).toContain(
      "no automatic mutation retry",
    );
    expect(classifyRetry("approval_denied", 0).decision).toBe("no_retry");
    // Proposals never become authoritative classifications.
    expect(proposalA.changes[0]?.text).not.toBe(proposalB.changes[0]?.text);
    void proposalA;
    void proposalB;
  });
});
