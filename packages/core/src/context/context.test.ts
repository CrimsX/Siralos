import { describe, expect, it } from "vitest";
import { computeArtifactDigest, type ArtifactDigest } from "../identity/artifact-digest.js";
import {
  CONTEXT_CLASSES,
  PHASE_CONTRACTS,
  contextClassesForPhase,
  createPhaseContract,
  validateAuthorityProfile,
} from "./phase-contract.js";
import {
  buildDependencyManifest,
  computeArtifactLineage,
  createArtifactDependencyManifest,
  createWorkflowArtifactIdentity,
  renderArtifactIdentity,
  renderLineage,
} from "./artifacts.js";
import { deriveArtifactStaleness, isPreparedMutationStale } from "./staleness.js";
import {
  computeProvenanceDigest,
  createContextProvenanceRef,
  renderWhyAcceptanceFailed,
  renderWhyBlocked,
  renderWhyStale,
  renderWhyValidationRequired,
  whyValidationRequired,
} from "./provenance.js";
import {
  accumulateCorrectionPattern,
  createSourceProblemCandidate,
  recordCorrectionPattern,
  renderSourceProblemCandidate,
} from "./source-integrity.js";
import { projectPhaseContext, toolSurfaceForPhase } from "./projection.js";
import { deriveValidationPlan, evaluateAcceptance } from "../determinism/decisions.js";

const SHA = (letter: string): string => letter.repeat(64);

function digestOf(artifactType: string, value: string): ArtifactDigest {
  return computeArtifactDigest({ artifactType, schemaVersion: 1, payload: { value } });
}

describe("context classes", () => {
  it("exposes the five formal classes with bounded artifact kinds", () => {
    expect(CONTEXT_CLASSES).toEqual([
      "global",
      "routing",
      "phase_contract",
      "stable_reference",
      "working",
    ]);
    expect(contextClassesForPhase("review")).toContain("working");
    expect(contextClassesForPhase("planning")).toContain("stable_reference");
  });

  it("every phase contract declares bounded context classes (never repository-wide)", () => {
    for (const contract of Object.values(PHASE_CONTRACTS)) {
      expect(contract.contextClasses.length).toBeGreaterThan(0);
      expect(contract.contextClasses.every((entry) => CONTEXT_CLASSES.includes(entry))).toBe(true);
    }
  });
});

describe("PhaseContract", () => {
  it("is deterministic for equivalent configuration", () => {
    const first = PHASE_CONTRACTS.review;
    const second = PHASE_CONTRACTS.review;
    expect(second.digest.value).toBe(first.digest.value);
    expect(first.digest.artifactType).toBe("PhaseContract");
  });

  it("authority is a fixed vocabulary that can never broaden", () => {
    expect(() =>
      createPhaseContract({
        id: "review",
        version: 2,
        phase: "reviewing",
        inputs: [{ artifactType: "Changeset", optional: false, reason: "review" }],
        authority: {
          readOnly: false,
          mutation: "unrestricted" as never,
          approvalGrant: true,
          acceptanceAuthority: false,
          capabilityNarrowing: [],
        },
        process: [],
        outputs: [{ artifactType: "ReviewVerdict", verificationKind: "review" }],
        verification: [{ id: "v", description: "v", evidenceClass: "review_result" }],
        contextClasses: ["working"],
      }),
    ).toThrow(/mutation must be none or prepared_only/);
  });

  it("a read-only contract cannot declare mutation authority", () => {
    expect(() =>
      validateAuthorityProfile("review", {
        readOnly: true,
        mutation: "prepared_only",
        approvalGrant: false,
        acceptanceAuthority: false,
        capabilityNarrowing: [],
      }),
    ).toThrow(/read-only contract cannot declare mutation/);
  });

  it("every major phase has an explicit contract with inputs/authority/outputs/verification", () => {
    const ids = [
      "planning",
      "inspection",
      "preparation",
      "approval",
      "mutation",
      "verification",
      "validation",
      "impact",
      "review",
      "repair",
      "acceptance",
    ];
    for (const id of ids) {
      const contract = PHASE_CONTRACTS[id as keyof typeof PHASE_CONTRACTS];
      expect(contract).toBeDefined();
      expect(contract.inputs.length).toBeGreaterThan(0);
      expect(contract.outputs.length).toBeGreaterThan(0);
      expect(contract.verification.length).toBeGreaterThan(0);
    }
    // Review is read-only; acceptance has acceptance authority; mutation is prepared-only.
    expect(PHASE_CONTRACTS.review.authority.readOnly).toBe(true);
    expect(PHASE_CONTRACTS.acceptance.authority.acceptanceAuthority).toBe(true);
    expect(PHASE_CONTRACTS.mutation.authority.mutation).toBe("prepared_only");
  });
});

describe("artifact envelope and dependency manifests", () => {
  it("binds artifact identity with digest and produced-under reference", () => {
    const identity = createWorkflowArtifactIdentity({
      artifactType: "TaskPlan",
      schemaVersion: 1,
      revision: 3,
      digest: digestOf("TaskPlan", "content"),
      producedUnder: digestOf("ExecutionInputManifest", "inputs"),
    });
    expect(identity.digest.artifactType).toBe("TaskPlan");
    expect(renderArtifactIdentity(identity)).toContain("TaskPlan v1 rev 3");
    expect(renderArtifactIdentity(identity)).toContain("produced under");
  });

  it("records high-value dependencies deterministically", () => {
    const manifest = buildDependencyManifest({
      artifactType: "ReviewVerdict",
      artifactId: "review-1",
      currentDigests: {
        taskContractDigest: SHA("a"),
        changesetDigest: SHA("b"),
        reviewContextDigest: SHA("c"),
        validationEvidenceDigest: SHA("d"),
      },
    });
    expect(manifest).not.toBeNull();
    // Dependencies are recorded in canonical sorted order.
    expect(manifest!.dependsOn.map((entry) => entry.artifactType)).toEqual([
      "Changeset",
      "ReviewContextManifest",
      "TaskContract",
      "ValidationEvidence",
    ]);
    const again = buildDependencyManifest({
      artifactType: "ReviewVerdict",
      artifactId: "review-1",
      currentDigests: {
        taskContractDigest: SHA("a"),
        changesetDigest: SHA("b"),
        reviewContextDigest: SHA("c"),
        validationEvidenceDigest: SHA("d"),
      },
    });
    expect(again!.digest).toBe(manifest!.digest);
  });

  it("renders bounded lineage", () => {
    const manifests = [
      createArtifactDependencyManifest({
        artifactType: "TaskPlan",
        artifactId: "plan-1",
        dependsOn: [{ artifactType: "TaskContract", digest: SHA("a") }],
      }),
      createArtifactDependencyManifest({
        artifactType: "ReviewVerdict",
        artifactId: "review-1",
        dependsOn: [{ artifactType: "Changeset", digest: SHA("b") }],
      }),
      createArtifactDependencyManifest({
        artifactType: "AcceptanceResult",
        artifactId: "accept-1",
        dependsOn: [{ artifactType: "ReviewVerdict", digest: SHA("c") }],
      }),
    ];
    const lineage = computeArtifactLineage(manifests, "accept-1");
    expect(lineage[0]?.artifactType).toBe("AcceptanceResult");
    expect(renderLineage(lineage)).toContain("\u2190");
  });
});

describe("targeted staleness", () => {
  const manifests = [
    createArtifactDependencyManifest({
      artifactType: "TaskPlan",
      artifactId: "plan-1",
      dependsOn: [{ artifactType: "TaskContract", digest: SHA("a") }],
    }),
    createArtifactDependencyManifest({
      artifactType: "ReviewVerdict",
      artifactId: "review-1",
      dependsOn: [{ artifactType: "Changeset", digest: SHA("b") }],
    }),
    createArtifactDependencyManifest({
      artifactType: "AcceptanceResult",
      artifactId: "accept-1",
      dependsOn: [
        { artifactType: "ReviewVerdict", digest: SHA("c") },
        { artifactType: "ValidationEvidence", digest: SHA("d") },
      ],
    }),
  ];

  it("changing one input stales only its explicit dependents", () => {
    const result = deriveArtifactStaleness({
      manifests,
      currentInputDigests: {
        TaskContract: SHA("a"),
        Changeset: SHA("e"), // changed
        ReviewVerdict: SHA("c"),
        ValidationEvidence: SHA("d"),
      },
    });
    expect(result.stale["review-1"]).toContain("Changeset");
    expect(result.stale["plan-1"]).toBeUndefined();
    expect(result.stale["accept-1"]).toBeUndefined();
    expect(result.current).toEqual(["accept-1", "plan-1"]);
  });

  it("validation evidence changes mark acceptance for reevaluation only", () => {
    const result = deriveArtifactStaleness({
      manifests,
      currentInputDigests: {
        TaskContract: SHA("a"),
        Changeset: SHA("b"),
        ReviewVerdict: SHA("c"),
        ValidationEvidence: SHA("f"),
      },
    });
    expect(result.stale["accept-1"]).toContain("ValidationEvidence");
    expect(result.stale["review-1"]).toBeUndefined();
    expect(result.stale["plan-1"]).toBeUndefined();
  });

  it("unrelated input changes never stale considered artifacts", () => {
    const result = deriveArtifactStaleness({
      manifests,
      currentInputDigests: {
        TaskContract: SHA("a"),
        Changeset: SHA("b"),
        ReviewVerdict: SHA("c"),
        ValidationEvidence: SHA("d"),
        ProviderDocumentation: SHA("z"), // unrelated
      },
    });
    expect(result.unrelatedChanges).toEqual(["ProviderDocumentation"]);
    expect(result.current).toEqual(["accept-1", "plan-1", "review-1"]);
  });

  it("a prepared mutation stales when any bound source revision changes", () => {
    const stale = isPreparedMutationStale({
      preparedSourceRevisions: [{ path: "player.gd", revision: "rev_" + "a".repeat(32) }],
      currentSourceRevisions: { "player.gd": "rev_" + "b".repeat(32) },
    });
    expect(stale.stale).toBe(true);
    expect(stale.stalePaths).toEqual(["player.gd"]);
    const current = isPreparedMutationStale({
      preparedSourceRevisions: [{ path: "player.gd", revision: "rev_" + "a".repeat(32) }],
      currentSourceRevisions: { "player.gd": "rev_" + "a".repeat(32) },
    });
    expect(current.stale).toBe(false);
  });
});

describe("provenance and why-diagnostics", () => {
  it("binds important context items to their sources", () => {
    const refs = [
      createContextProvenanceRef({
        item: "Reviewer has no mutation authority",
        kind: "execution_contract",
        id: "SECURITY.REVIEW.READ_ONLY",
      }),
      createContextProvenanceRef({
        item: "run parser behavior tests",
        kind: "acceptance_criterion",
        id: "S3M11.X",
      }),
    ];
    expect(refs[0]?.source.id).toBe("SECURITY.REVIEW.READ_ONLY");
    const a = computeProvenanceDigest(refs);
    const b = computeProvenanceDigest([refs[1]!, refs[0]!]);
    expect(b).toBe(a);
  });

  it("answers why-validation-required from structured plan evidence without a model", () => {
    const plan = deriveValidationPlan({
      changedSurfaces: ["scripts/player/player.gd"],
      impactRelationships: [{ source: "scripts/player/player.gd", target: "scenes/player.tscn" }],
      acceptanceCriteria: [
        { id: "parses", verificationKind: "deterministic" },
        { id: "review-clean", verificationKind: "review" },
      ],
      validationRegistry: [{ id: "scene-reparse", appliesTo: [".tscn"], baseClass: "required" }],
    });
    const diagnostic = whyValidationRequired({
      itemId: "scene-reparse",
      plan,
      changedSurfaces: ["scripts/player/player.gd"],
      impactRelations: [{ source: "scripts/player/player.gd", target: "scenes/player.tscn" }],
      acceptanceCriteria: ["parses"],
    });
    expect(diagnostic).not.toBeNull();
    const rendered = renderWhyValidationRequired(diagnostic!);
    expect(rendered).toContain("changed surface(s): scripts/player/player.gd");
    expect(rendered).toContain("verified impact relation(s)");
    expect(rendered).toContain("acceptance criterion/criteria: parses");
    expect(
      whyValidationRequired({
        itemId: "unknown",
        plan,
        changedSurfaces: [],
        impactRelations: [],
        acceptanceCriteria: [],
      }),
    ).toBeNull();
  });

  it("renders stale, blocked, and acceptance-failure diagnostics deterministically", () => {
    expect(renderWhyStale({ artifactId: "plan-1", reason: "TaskContract changed" })).toContain(
      "TaskContract changed",
    );
    expect(renderWhyBlocked({ reason: "applier unavailable" })).toContain("applier unavailable");
    const acceptance = evaluateAcceptance({
      criterionId: "parses",
      requiredEvidenceClasses: ["parser_result", "lsp_result"],
      availableEvidence: [{ id: "ev-1", class: "parser_result", digest: SHA("1") }],
    });
    const rendered = renderWhyAcceptanceFailed({
      criterionId: "parses",
      missingEvidenceClasses: ["lsp_result"],
      evidenceIdentities: acceptance.evidenceIdentities,
    });
    expect(rendered).toContain("missing evidence class(es): lsp_result");
  });
});

describe("phase projection and tool surface", () => {
  it("maps phases to ToolProjector modes through a fixed host table", () => {
    expect(toolSurfaceForPhase(PHASE_CONTRACTS.planning)).toBe("planning");
    expect(toolSurfaceForPhase(PHASE_CONTRACTS.review)).toBe("review");
    expect(toolSurfaceForPhase(PHASE_CONTRACTS.mutation)).toBe("development");
    expect(toolSurfaceForPhase(PHASE_CONTRACTS.acceptance)).toBe("inspection");
  });

  it("projects only declared context classes", () => {
    const segments = projectPhaseContext(PHASE_CONTRACTS.review, {
      taskContract: { revision: 1, digest: SHA("a") },
      taskPlan: { revision: 1, digest: SHA("p") },
      workspaceScope: { verifiedFiles: ["player.gd"], candidateFiles: [] },
      activeWorkingSet: ["player.gd"],
      documentationSelection: ["AGENTS.md"],
      preparedChangeset: { digest: SHA("c") },
      validationEvidence: [{ id: "ev-1", digest: SHA("e") }],
      reviewFindings: ["R1"],
    });
    const ids = segments.map((segment) => segment.id);
    expect(ids).toContain("phase.contract");
    expect(ids).toContain("phase.working.evidence");
    // Routing-class segments are absent from the review projection
    // (review declares working/stable_reference/phase_contract only).
    expect(ids).not.toContain("phase.routing.documentation");
    expect(ids).not.toContain("phase.routing.scope");
  });
});

describe("source-integrity signals", () => {
  it("records repeated correction patterns and creates recording-only candidates", () => {
    let pattern = recordCorrectionPattern({
      patternId: "arch-7",
      kind: "repeated_architecture_finding",
      evidenceRef: "finding-1",
    });
    pattern = accumulateCorrectionPattern(pattern, "finding-2");
    expect(pattern.occurrences).toBe(2);
    const candidate = createSourceProblemCandidate({
      id: "cand-1",
      likelySourceClass: "architecture_documentation",
      supportingPatterns: [pattern],
      createdAtMs: 1_000,
    });
    expect(candidate.remediated).toBe(false);
    expect(renderSourceProblemCandidate(candidate)).toContain("recorded, not remediated");
    expect(() =>
      createSourceProblemCandidate({
        id: "cand-2",
        likelySourceClass: "instruction",
        supportingPatterns: [],
        createdAtMs: 1,
      }),
    ).toThrow(/supporting evidence patterns/);
  });
});
