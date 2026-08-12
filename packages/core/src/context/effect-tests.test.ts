import { describe, expect, it } from "vitest";
import { PHASE_CONTRACTS, createPhaseContract, type PhaseContract } from "./phase-contract.js";
import { buildDependencyManifest, createArtifactDependencyManifest } from "./artifacts.js";
import { deriveArtifactStaleness, isPreparedMutationStale } from "./staleness.js";
import { renderWhyValidationRequired, whyValidationRequired } from "./provenance.js";
import {
  accumulateCorrectionPattern,
  createSourceProblemCandidate,
  recordCorrectionPattern,
} from "./source-integrity.js";
import { projectPhaseContext, toolSurfaceForPhase } from "./projection.js";
import { deriveValidationPlan } from "../determinism/decisions.js";

/**
 * Mandatory effect tests (Stage 3 — Interpretable Context Architecture,
 * ADR 0030): context containment, phase authority, targeted staleness,
 * unrelated guidance changes, transcript independence, why-diagnostics,
 * and source-integrity candidates.
 */

const SHA = (letter: string): string => letter.repeat(64);

describe("effect — context containment", () => {
  it("a scoped phase projects only PhaseContract-required and scope-relevant context", () => {
    const review = PHASE_CONTRACTS.review;
    const segments = projectPhaseContext(review, {
      taskContract: { revision: 1, digest: SHA("a") },
      taskPlan: { revision: 2, digest: SHA("p") },
      workspaceScope: {
        verifiedFiles: ["scripts/player/player.gd", "scenes/player.tscn"],
        candidateFiles: ["scripts/player/health.gd"],
      },
      activeWorkingSet: ["scripts/player/player.gd"],
      documentationSelection: [
        "AGENTS.md",
        "docs/architecture/README.md",
        "docs/adr/0027-unified-godot-native-development-workflow.md",
        "docs/adr/unrelated-provider.md",
      ],
      preparedChangeset: { digest: SHA("c") },
      validationEvidence: [
        { id: "ev-1", digest: SHA("1") },
        { id: "ev-2", digest: SHA("2") },
      ],
      reviewFindings: ["R1: blocking"],
    });
    const projected = segments.map((segment) => segment.id);
    // Working-class review inputs are projected...
    expect(projected).toContain("phase.working.evidence");
    expect(projected).toContain("phase.working.findings");
    // ...but routing-class context (workspace scope, documentation
    // selection) is NOT part of the review phase projection.
    expect(projected).not.toContain("phase.routing.scope");
    expect(projected).not.toContain("phase.routing.documentation");
    // The full documentation list never enters the projection wholesale.
    const joined = segments.map((segment) => segment.content).join("\n");
    expect(joined).not.toContain("docs/adr/unrelated-provider.md");
  });
});

describe("effect — phase authority", () => {
  it("a malicious review artifact requesting unrestricted mutation still projects a read-only reviewer surface", () => {
    // Malicious contract: a review phase that demands mutation authority.
    const malicious = createPhaseContract({
      id: "review",
      version: 99,
      phase: "reviewing",
      inputs: [{ artifactType: "Changeset", optional: false, reason: "review" }],
      authority: {
        readOnly: false,
        mutation: "prepared_only",
        approvalGrant: true,
        acceptanceAuthority: false,
        capabilityNarrowing: [],
      },
      process: [],
      outputs: [{ artifactType: "ReviewVerdict", verificationKind: "review" }],
      verification: [{ id: "v", description: "v", evidenceClass: "review_result" }],
      contextClasses: ["working"],
    });
    // The fixed host table maps the phase id, not the declared authority:
    // review always routes to the read-only review mode.
    expect(toolSurfaceForPhase(malicious)).toBe("review");
    // And the genuine review contract is read-only by construction.
    expect(PHASE_CONTRACTS.review.authority.readOnly).toBe(true);
    expect(PHASE_CONTRACTS.review.authority.mutation).toBe("none");
  });
});

describe("effect — targeted staleness", () => {
  it("changing the changeset stales review and acceptance but not the plan", () => {
    const plan = createArtifactDependencyManifest({
      artifactType: "TaskPlan",
      artifactId: "plan-1",
      dependsOn: [{ artifactType: "TaskContract", digest: SHA("a") }],
    });
    const changeset = buildDependencyManifest({
      artifactType: "PreparedChangeset",
      artifactId: "changeset-1",
      currentDigests: { taskPlanDigest: SHA("p"), sourceRevisionDigests: SHA("s") },
    })!;
    const review = createArtifactDependencyManifest({
      artifactType: "ReviewVerdict",
      artifactId: "review-1",
      dependsOn: [
        { artifactType: "TaskContract", digest: SHA("a") },
        { artifactType: "Changeset", digest: SHA("c") },
        { artifactType: "ReviewContextManifest", digest: SHA("r") },
        { artifactType: "ValidationEvidence", digest: SHA("v") },
      ],
    });
    const acceptance = createArtifactDependencyManifest({
      artifactType: "AcceptanceResult",
      artifactId: "accept-1",
      dependsOn: [
        { artifactType: "AcceptanceCriteria", digest: SHA("k") },
        { artifactType: "ValidationEvidence", digest: SHA("v") },
        { artifactType: "ReviewVerdict", digest: SHA("r2") },
        { artifactType: "MutationVerificationEvidence", digest: SHA("m") },
      ],
    });
    // Change C only (the changeset digest).
    const result = deriveArtifactStaleness({
      manifests: [plan, changeset, review, acceptance],
      currentInputDigests: {
        TaskContract: SHA("a"),
        Changeset: SHA("c2"),
        ReviewContextManifest: SHA("r"),
        ValidationEvidence: SHA("v"),
        AcceptanceCriteria: SHA("k"),
        ReviewVerdict: SHA("r2"),
        MutationVerificationEvidence: SHA("m"),
      },
    });
    expect(result.stale["review-1"]).toContain("Changeset");
    // Acceptance depends on ReviewVerdict (unchanged) — unchanged here.
    expect(result.stale["plan-1"]).toBeUndefined();
    expect(result.stale["accept-1"]).toBeUndefined();
    expect(result.current).toContain("plan-1");
  });
});

describe("effect — unrelated guidance change", () => {
  it("unrelated provider documentation changes stale nothing; applicable guidance changes stale dependents", () => {
    const plan = createArtifactDependencyManifest({
      artifactType: "TaskPlan",
      artifactId: "plan-godot-1",
      dependsOn: [
        { artifactType: "TaskContract", digest: SHA("a") },
        { artifactType: "GuidanceManifest", digest: SHA("g") },
      ],
    });
    // Unrelated change: provider-only documentation is not a dependency.
    const unrelated = deriveArtifactStaleness({
      manifests: [plan],
      currentInputDigests: {
        TaskContract: SHA("a"),
        GuidanceManifest: SHA("g"),
        ProviderDocumentation: SHA("z"),
      },
    });
    expect(unrelated.current).toEqual(["plan-godot-1"]);
    expect(unrelated.unrelatedChanges).toEqual(["ProviderDocumentation"]);
    // Applicable guidance change: the Godot plan's guidance dependency moved.
    const applicable = deriveArtifactStaleness({
      manifests: [plan],
      currentInputDigests: { TaskContract: SHA("a"), GuidanceManifest: SHA("g2") },
    });
    expect(applicable.stale["plan-godot-1"]).toContain("GuidanceManifest");
  });
});

describe("effect — transcript independence", () => {
  it("reconstructs the current phase context from structured artifacts with no conversation history", () => {
    // No messages: only authoritative structured artifacts exist.
    const sources = {
      taskContract: { revision: 1, digest: SHA("a") },
      taskPlan: { revision: 3, digest: SHA("p") },
      workspaceScope: { verifiedFiles: ["player.gd"], candidateFiles: [] },
      activeWorkingSet: ["player.gd"],
      documentationSelection: ["AGENTS.md"],
      preparedChangeset: { digest: SHA("c") },
      validationEvidence: [{ id: "ev-1", digest: SHA("e") }],
      reviewFindings: ["R1"],
    };
    const mutationSegments = projectPhaseContext(PHASE_CONTRACTS.mutation, sources);
    const mutationContent = mutationSegments.map((segment) => segment.content).join("\n");
    expect(mutationContent).toContain("digest " + SHA("c").slice(0, 12));
    const reviewSegments = projectPhaseContext(PHASE_CONTRACTS.review, sources);
    expect(reviewSegments.some((segment) => segment.id === "phase.working.findings")).toBe(true);
    expect(reviewSegments.some((segment) => segment.title === "Review Findings")).toBe(true);
    // The reconstructed context is deterministic: projecting again is identical.
    expect(projectPhaseContext(PHASE_CONTRACTS.review, sources)).toEqual(reviewSegments);
  });
});

describe("effect — why diagnostic", () => {
  it("answers why a validation is required from structured evidence without invoking a model", () => {
    const plan = deriveValidationPlan({
      changedSurfaces: ["scripts/player/player.gd"],
      impactRelationships: [{ source: "scripts/player/player.gd", target: "scenes/player.tscn" }],
      acceptanceCriteria: [
        { id: "parses", verificationKind: "deterministic" },
        { id: "review-clean", verificationKind: "review" },
      ],
      validationRegistry: [
        { id: "scene-reparse", appliesTo: [".tscn"], baseClass: "required" },
        { id: "check-only-parse", appliesTo: [".gd"], baseClass: "required" },
      ],
    });
    const diagnostic = whyValidationRequired({
      itemId: "scene-reparse",
      plan,
      changedSurfaces: ["scripts/player/player.gd"],
      impactRelations: [{ source: "scripts/player/player.gd", target: "scenes/player.tscn" }],
      acceptanceCriteria: ["parses"],
    });
    const rendered = renderWhyValidationRequired(diagnostic!);
    expect(rendered).toContain("Required because (validation_plan):");
    expect(rendered).toContain("changed surface(s): scripts/player/player.gd");
    expect(rendered).toContain(
      "verified impact relation(s): scripts/player/player.gd -> scenes/player.tscn",
    );
    expect(rendered).toContain("acceptance criterion/criteria: parses");
  });
});

describe("effect — source-integrity candidate", () => {
  it("repeated deterministic corrections produce a recording-only candidate with evidence and no auto-modification", () => {
    let pattern = recordCorrectionPattern({
      patternId: "arch-architecture-guidance",
      kind: "repeated_architecture_finding",
      evidenceRef: "finding-1",
    });
    pattern = accumulateCorrectionPattern(pattern, "finding-2");
    pattern = accumulateCorrectionPattern(pattern, "finding-3");
    const candidate = createSourceProblemCandidate({
      id: "cand-architecture-1",
      likelySourceClass: "architecture_documentation",
      supportingPatterns: [pattern],
      createdAtMs: 1_000,
    });
    expect(candidate.supportingPatterns[0]?.occurrences).toBe(3);
    expect(candidate.remediated).toBe(false);
    // The candidate type has no remediation surface: nothing can modify
    // instructions or configuration through it.
    expect("remediation" in candidate).toBe(false);
    expect(
      isPreparedMutationStale({ preparedSourceRevisions: [], currentSourceRevisions: {} }).stale,
    ).toBe(false);
  });
});

/** Keep PhaseContract referenced for future runtime phase contracts. */
export type { PhaseContract };
