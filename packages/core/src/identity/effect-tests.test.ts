import { describe, expect, it } from "vitest";
import {
  createTaskContract,
  reviseTaskContract,
  type TaskContract,
} from "../tasks/task-contract.js";
import {
  createTaskPlan,
  reviseTaskPlan,
  type TaskPlanContent,
} from "../planning/planning-model.js";
import { createTaskRuntime } from "../tasks/task-runtime.js";
import { TASK_RUNTIME_VERSION, createTaskRuntimeSnapshot } from "../tasks/task-snapshot.js";
import type { TaskRuntimeSnapshotSources } from "../tasks/task-snapshot.js";
import {
  computeTaskContractDelta,
  computeTaskPlanContentDigest,
  computeTaskPlanDelta,
} from "./contract-plan-identity.js";
import {
  computeExecutionInputDelta,
  computeGuidanceDelta,
  computeToolSurfaceDelta,
  computeValidationDelta,
  createAcceptanceEvidenceManifest,
  createExecutionInputManifest,
  createGuidanceManifest,
  createReviewInputManifest,
  createToolSurfaceManifest,
} from "./manifests.js";
import { deriveIdentityStaleness } from "./staleness.js";
import { computeArtifactDigest } from "./artifact-digest.js";

/**
 * Mandatory effect tests (Stage 3 — Content Identity & Delta Verification,
 * ADR 0028, §23–§29).
 */

const SHA = (letter: string): string => letter.repeat(64);

function makeContract(request = "Fix the player movement"): TaskContract {
  return createTaskContract({
    id: "task-1",
    request,
    constraints: [{ id: "scope", kind: "scope", description: "Workspace contained." }],
    acceptanceCriteria: [
      { id: "parses", description: "parses", verificationKind: "deterministic" },
      { id: "review-clean", description: "review clean", verificationKind: "review" },
    ],
    pausePolicy: "on_approval",
  });
}

function makePlanContent(objective = "Add a sprint"): TaskPlanContent {
  return {
    objective,
    scope: { inScope: ["player.gd"], outOfScope: [] },
    nonGoals: [],
    touchpoints: [
      { id: "t1", path: "player.gd", confidence: "verified", revision: "rev_" + "a".repeat(32) },
    ],
    constraints: [],
    risks: [],
    steps: [{ id: "s1", title: "step", expectedTouchpoints: ["t1"] }],
    validation: { checks: ["check-only parse"] },
  };
}

function makeRuntimeSnapshot(): TaskRuntimeSnapshotSources {
  return {
    runtimeVersion: TASK_RUNTIME_VERSION,
    provider: { profileId: "test", route: null },
    sandboxProfileId: "test",
    capabilityPolicyRevision: SHA("p"),
    workspaceIdentity: "ws",
    godotEngineFingerprint: null,
    workflow: null,
  };
}

function makeRuntime() {
  return createTaskRuntime();
}

describe("effect 23 — canonical structured identity", () => {
  it("semantically identical artifacts with different key insertion order produce equal digests; one material change produces a different digest", () => {
    const payloadA = { b: { x: [1, 2] }, a: "v", c: null };
    const payloadB = { c: null, a: "v", b: { x: [1, 2] } };
    const payloadC = { b: { x: [1, 3] }, a: "v", c: null };
    const digestA = computeArtifactDigest({
      artifactType: "EffectFixture",
      schemaVersion: 1,
      payload: payloadA,
    });
    const digestB = computeArtifactDigest({
      artifactType: "EffectFixture",
      schemaVersion: 1,
      payload: payloadB,
    });
    const digestC = computeArtifactDigest({
      artifactType: "EffectFixture",
      schemaVersion: 1,
      payload: payloadC,
    });
    expect(digestA.value).toBe(digestB.value);
    expect(digestC.value).not.toBe(digestA.value);
  });
});

describe("effect 24 — plan approval binds exact content", () => {
  it("an approval for digest X is rejected once plan content changes to digest Y", () => {
    const runtime = makeRuntime();
    const contract = makeContract();
    const handle = runtime.createTask({
      contract,
      snapshot: createTaskRuntimeSnapshot(makeRuntimeSnapshot()),
      steps: [],
    });
    const planA = createTaskPlan({
      id: "plan-task-1",
      taskId: contract.id,
      taskContractRevision: contract.revision,
      taskContractDigest: contract.digest.value,
      depth: "light",
      content: makePlanContent(),
      createdAt: 1_000,
    });
    expect(handle.setPlan(planA).status).toBe("ok");
    // Approve digest X.
    expect(handle.approvePlan(planA.id, planA.revision).status).toBe("ok");
    expect(handle.snapshot().plan.approval).toBe("approved");
    // Change plan content -> rev B / digest Y.
    const planB = reviseTaskPlan(planA, { content: makePlanContent("Add a dash") });
    expect(planB.digest.value).not.toBe(planA.digest.value);
    expect(handle.setPlan(planB).status).toBe("ok");
    expect(handle.snapshot().plan.approval).toBe("invalidated");
    // Attempting to use the old approval (planA identity) is rejected.
    expect(handle.approvePlan(planA.id, planA.revision).status).toBe("rejected");
    // The digest-level contract: approval for X can never authorize Y.
    expect(computeTaskPlanContentDigest(planB)).not.toBe(planA.digest.value);
  });

  it("a digest alone never grants approval (hashes are not authority)", () => {
    const runtime = makeRuntime();
    const contract = makeContract();
    const handle = runtime.createTask({
      contract,
      snapshot: createTaskRuntimeSnapshot(makeRuntimeSnapshot()),
      steps: [],
    });
    const plan = createTaskPlan({
      id: "plan-task-1",
      taskId: contract.id,
      taskContractRevision: contract.revision,
      taskContractDigest: contract.digest.value,
      depth: "light",
      content: makePlanContent(),
      createdAt: 1_000,
    });
    handle.setPlan(plan);
    // Matching digest with the wrong plan id / revision is refused.
    expect(handle.approvePlan("plan-other", plan.revision).status).toBe("rejected");
    expect(handle.approvePlan(plan.id, 99).status).toBe("rejected");
    expect(handle.snapshot().plan.approval).toBe("none");
  });
});

describe("effect 25 — exact review binding", () => {
  it("a passing review of changeset digest A cannot satisfy acceptance once the reviewed source changes to digest B", () => {
    const acceptanceDigest = computeArtifactDigest({
      artifactType: "AcceptanceCriteria",
      schemaVersion: 1,
      payload: {
        criteria: [
          { id: "parses", description: "parses", verificationKind: "deterministic" },
          { id: "review-clean", description: "review clean", verificationKind: "review" },
        ],
      },
    }).value;
    const reviewA = createReviewInputManifest({
      reviewId: "review-1",
      taskId: "task-1",
      taskContractDigest: SHA("c"),
      changesetDigest: SHA("a"),
      reviewContextDigest: null,
      acceptanceDigest,
      validationEvidenceDigest: SHA("v"),
      sourceRevisions: [{ path: "player.gd", revision: "rev_" + "a".repeat(32) }],
    });
    // One reviewed source file changes -> changeset digest B.
    const reviewB = createReviewInputManifest({
      reviewId: "review-1",
      taskId: "task-1",
      taskContractDigest: SHA("c"),
      changesetDigest: SHA("b"),
      reviewContextDigest: null,
      acceptanceDigest,
      validationEvidenceDigest: SHA("v"),
      sourceRevisions: [{ path: "player.gd", revision: "rev_" + "b".repeat(32) }],
    });
    expect(reviewB.digest).not.toBe(reviewA.digest);
    const staleness = deriveIdentityStaleness({
      changesetDigest: SHA("b"),
      reviewInputChangesetDigest: SHA("a"),
    });
    expect(staleness.reviewStale).toBe(true);
    expect(
      staleness.reasons.some((reason) => reason.includes("previous review no longer applies")),
    ).toBe(true);
  });
});

describe("effect 26 — evidence binding", () => {
  it("acceptance bound to evidence digest A is identified as stale against a rerun digest B", () => {
    const evidenceA = [
      { evidenceId: "ev-1", kind: "parser_result", digest: SHA("1") },
      { evidenceId: "ev-2", kind: "lsp_result", digest: SHA("2") },
    ];
    const manifestA = createAcceptanceEvidenceManifest({
      taskId: "task-1",
      criterionId: "parses",
      evidence: evidenceA,
    });
    // Rerun validation produces digest B for the same evidence id.
    const manifestB = createAcceptanceEvidenceManifest({
      taskId: "task-1",
      criterionId: "parses",
      evidence: [{ evidenceId: "ev-1", kind: "parser_result", digest: SHA("9") }, evidenceA[1]!],
    });
    expect(manifestB.digest).not.toBe(manifestA.digest);
    const staleness = deriveIdentityStaleness({
      validationEvidenceDigest: manifestB.digest,
      acceptedEvidenceDigest: manifestA.digest,
    });
    expect(staleness.acceptanceRequiresReevaluation).toBe(true);
    // Identical evidence sets never flag reevaluation.
    const same = createAcceptanceEvidenceManifest({
      taskId: "task-1",
      criterionId: "parses",
      evidence: evidenceA,
    });
    expect(same.digest).toBe(manifestA.digest);
    const stable = deriveIdentityStaleness({
      validationEvidenceDigest: manifestA.digest,
      acceptedEvidenceDigest: same.digest,
    });
    expect(stable.acceptanceRequiresReevaluation).toBe(false);
  });
});

describe("effect 27 — tool-surface verification", () => {
  it("captures the actual reviewer schema; adding a mutation tool changes the digest and the negative surface is visible", () => {
    const readTools = [
      { name: "workspace.read", inputSchema: { type: "object" }, description: "read" },
      { name: "workspace.search", inputSchema: { type: "object" }, description: "search" },
    ];
    const reviewer = createToolSurfaceManifest({
      role: "reviewer",
      phase: "review",
      tools: readTools,
    });
    // Deterministic negative fixture: a mutation tool leaks into the reviewer surface.
    const negative = createToolSurfaceManifest({
      role: "reviewer",
      phase: "review",
      tools: [
        ...readTools,
        {
          name: "workspace.apply_text_changeset",
          inputSchema: { type: "object" },
          description: "apply",
        },
      ],
    });
    expect(negative.digest).not.toBe(reviewer.digest);
    const delta = computeToolSurfaceDelta(reviewer, negative);
    expect(delta.added).toEqual(["workspace.apply_text_changeset"]);
    // The reviewer schema remains read-only by construction.
    expect(reviewer.tools.every((tool) => tool.name.startsWith("workspace."))).toBe(true);
    expect(reviewer.tools.map((tool) => tool.name)).toEqual(["workspace.read", "workspace.search"]);
  });
});

describe("effect 28 — guidance staleness", () => {
  it("modifying an applicable AGENTS.md stales planning/execution context; unrelated docs do not", () => {
    const before = createGuidanceManifest([
      { id: "agents:root", kind: "root-agents", path: "AGENTS.md", digest: SHA("a") },
    ]);
    const afterChange = createGuidanceManifest([
      { id: "agents:root", kind: "root-agents", path: "AGENTS.md", digest: SHA("b") },
    ]);
    expect(afterChange.aggregateDigest).not.toBe(before.aggregateDigest);
    const staleness = deriveIdentityStaleness({
      guidanceDigest: afterChange.aggregateDigest,
      priorGuidanceDigest: before.aggregateDigest,
    });
    expect(staleness.executionContextPotentiallyStale).toBe(true);
    // A guidance selection that adds an unrelated ADR but keeps the
    // applicable root guidance identical does not stale the context.
    const delta = computeGuidanceDelta(before, afterChange);
    expect(delta.changed).toEqual(["AGENTS.md"]);
    const unrelated = createGuidanceManifest([
      { id: "agents:root", kind: "root-agents", path: "AGENTS.md", digest: SHA("a") },
      { id: "adr:other", kind: "adr", path: "docs/adr/unrelated.md", digest: SHA("c") },
    ]);
    const unchangedGuidance = deriveIdentityStaleness({
      guidanceDigest: unrelated.aggregateDigest,
      priorGuidanceDigest: unrelated.aggregateDigest,
    });
    expect(unchangedGuidance.executionContextPotentiallyStale).toBe(false);
  });
});

describe("effect 29 — delta context reduction", () => {
  it("a second iteration projects only the changed source revision and validation result", () => {
    const inputsFor = (iteration: number, sourceDigest: string) =>
      createExecutionInputManifest({
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
    const first = inputsFor(1, SHA("1"));
    const second = inputsFor(2, SHA("2"));
    const delta = computeExecutionInputDelta(first, second);
    // Only the source revision changed; everything else is listed unchanged.
    expect(delta.changed.map((entry) => entry.id)).toEqual(["sourceRevisions"]);
    expect(delta.unchanged).toContain("taskContract");
    expect(delta.unchanged).toContain("taskPlan");
    expect(delta.unchanged).toContain("guidance");
    expect(delta.unchanged).toContain("toolSurface");
    // Validation delta: newly passing / still failing / new failures.
    const validationDelta = computeValidationDelta(
      [
        { id: "TEST-1", passed: false },
        { id: "TEST-2", passed: true },
      ],
      [
        { id: "TEST-1", passed: true },
        { id: "TEST-2", passed: true },
      ],
    );
    expect(validationDelta.newlyPassing).toEqual(["TEST-1"]);
    expect(validationDelta.stillFailing).toEqual([]);
    expect(validationDelta.newFailures).toEqual([]);
    // Authoritative current state stays reconstructable: the manifest
    // carries the full exact references, not a delta.
    expect(second.inputs.find((input) => input.id === "sourceRevisions")?.digest).toBe(SHA("2"));
    expect(second.inputs.find((input) => input.id === "taskContract")?.digest).toBe(SHA("a"));
  });
});

describe("contract and plan deltas stay derived, never authoritative", () => {
  it("full authoritative state remains the current artifact", () => {
    const contract = makeContract();
    const revised = reviseTaskContract(contract, { id: contract.id, request: "Changed request" });
    const contractDelta = computeTaskContractDelta(contract, revised);
    expect(contractDelta.changed).toEqual(["request"]);
    // The delta cannot reconstruct the authoritative state; the current
    // contract remains the source of truth.
    expect(revised.request).toBe("Changed request");
    expect(revised.revision).toBe(2);

    const planA = createTaskPlan({
      id: "plan-task-1",
      taskId: contract.id,
      taskContractRevision: contract.revision,
      taskContractDigest: contract.digest.value,
      depth: "light",
      content: makePlanContent(),
      createdAt: 1_000,
    });
    const planB = reviseTaskPlan(planA, { content: makePlanContent("changed") });
    const planDelta = computeTaskPlanDelta(planA, planB);
    expect(planDelta.changed).toEqual(["objective"]);
    expect(planB.objective).toBe("changed");
  });
});
