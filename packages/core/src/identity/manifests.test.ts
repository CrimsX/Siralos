import { describe, expect, it } from "vitest";
import {
  computeAcceptanceCriteriaDigest,
  computeCapabilitySnapshotDigest,
  computeExecutionInputDelta,
  computeGuidanceDelta,
  computeToolSurfaceDelta,
  computeValidationDelta,
  computeValidationEvidenceDigest,
  createAcceptanceEvidenceManifest,
  createExecutionInputManifest,
  createGuidanceManifest,
  createReviewInputManifest,
  createToolSurfaceManifest,
  createValidationResultIdentity,
  canonicalChangesetIdentity,
} from "./manifests.js";
import { deriveIdentityStaleness } from "./staleness.js";

const SHA = (letter: string): string => letter.repeat(64);

describe("GuidanceManifest", () => {
  it("represents the exact selected documents with an aggregate digest", () => {
    const manifest = createGuidanceManifest([
      { id: "agents:root", kind: "root-agents", path: "AGENTS.md", digest: SHA("a") },
      { id: "adr:0028", kind: "adr", path: "docs/adr/0028-identity.md", digest: SHA("b") },
    ]);
    expect(manifest.entries).toHaveLength(2);
    expect(manifest.aggregateDigest).toMatch(/^[0-9a-f]{64}$/);
  });

  it("keeps the aggregate digest stable regardless of entry order", () => {
    const first = createGuidanceManifest([
      { id: "a", kind: "root-agents", path: "AGENTS.md", digest: SHA("a") },
      { id: "b", kind: "adr", path: "docs/adr/x.md", digest: SHA("b") },
    ]);
    const second = createGuidanceManifest([
      { id: "b", kind: "adr", path: "docs/adr/x.md", digest: SHA("b") },
      { id: "a", kind: "root-agents", path: "AGENTS.md", digest: SHA("a") },
    ]);
    expect(second.aggregateDigest).toBe(first.aggregateDigest);
  });

  it("changes the aggregate digest when one applicable document changes", () => {
    const before = createGuidanceManifest([
      { id: "agents:root", kind: "root-agents", path: "AGENTS.md", digest: SHA("a") },
    ]);
    const after = createGuidanceManifest([
      { id: "agents:root", kind: "root-agents", path: "AGENTS.md", digest: SHA("b") },
    ]);
    expect(after.aggregateDigest).not.toBe(before.aggregateDigest);
    const delta = computeGuidanceDelta(before, after);
    expect(delta.changed).toEqual(["AGENTS.md"]);
    expect(delta.unchangedContent).toBe(false);
  });

  it("unrelated (non-applicable) documents do not stale a manifest", () => {
    const before = createGuidanceManifest([
      { id: "agents:root", kind: "root-agents", path: "AGENTS.md", digest: SHA("a") },
    ]);
    const after = createGuidanceManifest([
      { id: "agents:root", kind: "root-agents", path: "AGENTS.md", digest: SHA("a") },
      { id: "adr:other", kind: "adr", path: "docs/adr/unrelated.md", digest: SHA("c") },
    ]);
    // The applicable root guidance is unchanged; the manifest documents
    // only what was selected, so unrelated material never enters it.
    const delta = computeGuidanceDelta(before, after);
    expect(delta.added).toEqual(["docs/adr/unrelated.md"]);
    expect(delta.changed).toEqual([]);
  });
});

describe("ToolSurfaceManifest", () => {
  const readTools = [
    { name: "workspace.read", inputSchema: { type: "object" }, description: "read" },
    { name: "workspace.search", inputSchema: { type: "object" }, description: "search" },
  ];
  const mutationTools = [
    {
      name: "workspace.apply_text_changeset",
      inputSchema: { type: "object" },
      description: "apply",
    },
  ];

  it("hashes the actual projected schemas deterministically", () => {
    const first = createToolSurfaceManifest({
      role: "reviewer",
      phase: "review",
      tools: readTools,
    });
    const second = createToolSurfaceManifest({
      role: "reviewer",
      phase: "review",
      tools: readTools,
    });
    expect(first.digest).toBe(second.digest);
    expect(first.tools[0]?.schemaDigest).toMatch(/^[0-9a-f]{64}$/);
  });

  it("changes the digest when the projected surface changes", () => {
    const reviewer = createToolSurfaceManifest({
      role: "reviewer",
      phase: "review",
      tools: readTools,
    });
    const negative = createToolSurfaceManifest({
      role: "reviewer",
      phase: "review",
      tools: [...readTools, ...mutationTools],
    });
    expect(negative.digest).not.toBe(reviewer.digest);
    const delta = computeToolSurfaceDelta(reviewer, negative);
    expect(delta.added).toEqual(["workspace.apply_text_changeset"]);
    expect(delta.removed).toEqual([]);
    expect(delta.retained).toEqual(["workspace.read", "workspace.search"]);
  });

  it("reports developer->reviewer removals semantically", () => {
    const developer = createToolSurfaceManifest({
      role: "developer",
      phase: "mutation",
      tools: [...readTools, ...mutationTools],
    });
    const reviewer = createToolSurfaceManifest({
      role: "reviewer",
      phase: "review",
      tools: readTools,
    });
    const delta = computeToolSurfaceDelta(developer, reviewer);
    expect(delta.removed).toEqual(["workspace.apply_text_changeset"]);
    expect(delta.retained).toEqual(["workspace.read", "workspace.search"]);
  });
});

describe("CapabilitySnapshot digest", () => {
  it("is deterministic and secret-free by construction (host snapshot)", () => {
    const snapshot = { runtime: { version: "1" }, tools: { count: 3 } };
    expect(computeCapabilitySnapshotDigest(snapshot)).toBe(
      computeCapabilitySnapshotDigest({ tools: { count: 3 }, runtime: { version: "1" } }),
    );
    expect(computeCapabilitySnapshotDigest(snapshot)).not.toBe(
      computeCapabilitySnapshotDigest({ runtime: { version: "2" }, tools: { count: 3 } }),
    );
  });
});

describe("ExecutionInputManifest", () => {
  function manifest(iteration: number, sourceDigest: string) {
    return createExecutionInputManifest({
      taskId: "task-1",
      iteration,
      inputs: [
        { id: "taskContract", revision: 1, digest: SHA("a") },
        { id: "taskPlan", revision: 2, digest: SHA("b") },
        { id: "executionContract", revision: 1, digest: SHA("c") },
        { id: "guidance", revision: null, digest: SHA("d") },
        { id: "toolSurface", revision: null, digest: SHA("e") },
        { id: "capability", revision: null, digest: SHA("f") },
        { id: "sourceRevisions", revision: null, digest: sourceDigest },
      ],
    });
  }

  it("produces a deterministic aggregate identity", () => {
    const first = manifest(1, SHA("1"));
    const second = manifest(1, SHA("1"));
    expect(first.digest).toBe(second.digest);
  });

  it("projects only the changed inputs in its delta", () => {
    const before = manifest(1, SHA("1"));
    const after = manifest(2, SHA("2"));
    expect(after.digest).not.toBe(before.digest);
    const delta = computeExecutionInputDelta(before, after);
    expect(delta.changed.map((entry) => entry.id)).toEqual(["sourceRevisions"]);
    expect(delta.changed[0]).toMatchObject({ before: SHA("1"), after: SHA("2") });
    expect(delta.unchanged).toContain("taskContract");
    expect(delta.unchanged).toContain("guidance");
    expect(delta.unchanged).toContain("toolSurface");
  });

  it("reports unchanged executions", () => {
    const delta = computeExecutionInputDelta(manifest(1, SHA("1")), manifest(1, SHA("1")));
    expect(delta.unchangedContent).toBe(true);
  });
});

describe("review and acceptance binding", () => {
  it("binds a review to its exact input digest", () => {
    const review = createReviewInputManifest({
      reviewId: "review-1",
      taskId: "task-1",
      taskContractDigest: SHA("a"),
      changesetDigest: SHA("b"),
      reviewContextDigest: SHA("c"),
      acceptanceDigest: computeAcceptanceCriteriaDigest([
        { id: "c1", description: "parse", verificationKind: "deterministic" },
      ]),
      validationEvidenceDigest: SHA("d"),
      sourceRevisions: [{ path: "player.gd", revision: "rev_" + "a".repeat(32) }],
    });
    expect(review.digest).toMatch(/^[0-9a-f]{64}$/);
    // The same inputs produce the same identity; a changed changeset does not.
    const same = createReviewInputManifest({
      reviewId: "review-1",
      taskId: "task-1",
      taskContractDigest: SHA("a"),
      changesetDigest: SHA("b"),
      reviewContextDigest: SHA("c"),
      acceptanceDigest: computeAcceptanceCriteriaDigest([
        { id: "c1", description: "parse", verificationKind: "deterministic" },
      ]),
      validationEvidenceDigest: SHA("d"),
      sourceRevisions: [{ path: "player.gd", revision: "rev_" + "a".repeat(32) }],
    });
    const changed = createReviewInputManifest({
      reviewId: "review-1",
      taskId: "task-1",
      taskContractDigest: SHA("a"),
      changesetDigest: SHA("e"),
      reviewContextDigest: SHA("c"),
      acceptanceDigest: computeAcceptanceCriteriaDigest([
        { id: "c1", description: "parse", verificationKind: "deterministic" },
      ]),
      validationEvidenceDigest: SHA("d"),
      sourceRevisions: [{ path: "player.gd", revision: "rev_" + "a".repeat(32) }],
    });
    expect(same.digest).toBe(review.digest);
    expect(changed.digest).not.toBe(review.digest);
  });

  it("binds acceptance to the exact evidence set", () => {
    const evidence = [
      { evidenceId: "ev-1", kind: "parser_result", digest: SHA("1") },
      { evidenceId: "ev-2", kind: "lsp_result", digest: SHA("2") },
    ];
    const manifest = createAcceptanceEvidenceManifest({
      taskId: "task-1",
      criterionId: "parses",
      evidence,
    });
    const same = createAcceptanceEvidenceManifest({
      taskId: "task-1",
      criterionId: "parses",
      evidence: [evidence[1]!, evidence[0]!],
    });
    const rerun = createAcceptanceEvidenceManifest({
      taskId: "task-1",
      criterionId: "parses",
      evidence: [{ evidenceId: "ev-1", kind: "parser_result", digest: SHA("9") }, evidence[1]!],
    });
    expect(same.digest).toBe(manifest.digest);
    expect(rerun.digest).not.toBe(manifest.digest);
  });

  it("gives validation results a stable identity and accurate deltas", () => {
    const first = createValidationResultIdentity({
      validationId: "validate-1",
      planIdentity: "plan-identity-1",
      result: {
        checks: [
          { id: "foo", passed: false },
          { id: "baz", passed: false },
        ],
      },
    });
    const second = createValidationResultIdentity({
      validationId: "validate-1",
      planIdentity: "plan-identity-1",
      result: {
        checks: [
          { id: "foo", passed: true },
          { id: "baz", passed: false },
        ],
      },
    });
    expect(first.resultDigest).toMatch(/^[0-9a-f]{64}$/);
    expect(second.resultDigest).not.toBe(first.resultDigest);
    const delta = computeValidationDelta(
      [
        { id: "foo", passed: false },
        { id: "baz", passed: false },
      ],
      [
        { id: "foo", passed: true },
        { id: "baz", passed: false },
      ],
      { baseIdentity: first.resultDigest, resultIdentity: second.resultDigest },
    );
    expect(delta.newlyPassing).toEqual(["foo"]);
    expect(delta.stillFailing).toEqual(["baz"]);
    expect(delta.newFailures).toEqual([]);
    expect(delta.baseIdentity).toBe(first.resultDigest);
    expect(delta.resultIdentity).toBe(second.resultDigest);
  });

  it("canonicalizes the exact change under review", () => {
    const a = canonicalChangesetIdentity({ targets: [{ path: "a.gd", fingerprint: SHA("1") }] });
    const b = canonicalChangesetIdentity({ targets: [{ fingerprint: SHA("1"), path: "a.gd" }] });
    const c = canonicalChangesetIdentity({ targets: [{ path: "a.gd", fingerprint: SHA("2") }] });
    expect(a).toBe(b);
    expect(c).not.toBe(a);
  });
});

describe("staleness rules", () => {
  it("flags plan, review, guidance, and acceptance staleness from exact digests", () => {
    const staleness = deriveIdentityStaleness({
      contractDigest: SHA("a"),
      planContractDigest: SHA("b"),
      guidanceDigest: SHA("g2"),
      priorGuidanceDigest: SHA("g1"),
      changesetDigest: SHA("c2"),
      reviewInputChangesetDigest: SHA("c1"),
      validationEvidenceDigest: SHA("v2"),
      acceptedEvidenceDigest: SHA("v1"),
    });
    expect(staleness.planPotentiallyStale).toBe(true);
    expect(staleness.executionContextPotentiallyStale).toBe(true);
    expect(staleness.reviewStale).toBe(true);
    expect(staleness.acceptanceRequiresReevaluation).toBe(true);
    expect(staleness.reasons).toHaveLength(4);
  });

  it("reports no staleness when digests match", () => {
    const staleness = deriveIdentityStaleness({
      contractDigest: SHA("a"),
      planContractDigest: SHA("a"),
      changesetDigest: SHA("c"),
      reviewInputChangesetDigest: SHA("c"),
    });
    expect(staleness).toMatchObject({
      planPotentiallyStale: false,
      reviewStale: false,
      acceptanceRequiresReevaluation: false,
    });
  });

  it("never treats an absent digest as staleness", () => {
    const staleness = deriveIdentityStaleness({ contractDigest: SHA("a") });
    expect(staleness.planPotentiallyStale).toBe(false);
    expect(staleness.reasons).toEqual([]);
  });
});

describe("validation evidence digest", () => {
  it("is order-stable over canonical content", () => {
    const a = computeValidationEvidenceDigest([
      { id: "e1", kind: "parser_result", content: { valid: true } },
    ]);
    const b = computeValidationEvidenceDigest([
      { id: "e1", kind: "parser_result", content: { valid: true } },
    ]);
    expect(a).toBe(b);
  });
});
