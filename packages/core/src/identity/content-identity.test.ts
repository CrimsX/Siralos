import { describe, expect, it } from "vitest";
import {
  abbreviateDigest,
  computeArtifactDigest,
  computeArtifactDigestHex,
  digestReference,
  sameArtifactDigest,
  validateArtifactDigest,
} from "./artifact-digest.js";
import {
  computeTaskContractArtifactDigest,
  computeTaskContractContentDigest,
  computeTaskContractDelta,
  computeTaskPlanContentDigest,
  computeTaskPlanDelta,
} from "./contract-plan-identity.js";
import {
  canonicalValuesEqual,
  computeItemListDelta,
  computeSectionDelta,
  digestItemList,
} from "./semantic-delta.js";
import { createTaskContract, reviseTaskContract } from "../tasks/task-contract.js";
import {
  createTaskPlan,
  reviseTaskPlan,
  type TaskPlanContent,
} from "../planning/planning-model.js";
import { createPlanningPolicy, type PlanningDecisionInput } from "../planning/planning-policy.js";

const SHA = (letter: string): string => letter.repeat(64);

function makeContract(overrides: { request?: string; criteria?: number } = {}) {
  return createTaskContract({
    id: "task-1",
    request: overrides.request ?? "Fix the player movement",
    constraints: [{ id: "scope", kind: "scope", description: "Contained in workspace." }],
    acceptanceCriteria: Array.from({ length: overrides.criteria ?? 2 }, (_, index) => ({
      id: `c${index + 1}`,
      description: `criterion ${index + 1}`,
      verificationKind: "deterministic" as const,
    })),
    pausePolicy: "on_approval",
  });
}

function makePlanContent(overrides: { objective?: string; steps?: number } = {}): TaskPlanContent {
  return {
    objective: overrides.objective ?? "Add a sprint to the player",
    scope: { inScope: ["player.gd"], outOfScope: [] },
    nonGoals: [],
    touchpoints: [
      {
        id: "t1",
        path: "scripts/player/player.gd",
        confidence: "verified",
        revision: "rev_" + "a".repeat(32),
      },
    ],
    constraints: [],
    risks: [],
    steps: Array.from({ length: overrides.steps ?? 1 }, (_, index) => ({
      id: `s${index + 1}`,
      title: `step ${index + 1}`,
      expectedTouchpoints: ["t1"],
    })),
    validation: { checks: ["check-only parse"] },
    rationale: "narrow change",
  };
}

function makePlan(overrides: { objective?: string } = {}) {
  return createTaskPlan({
    id: "plan-task-1",
    taskId: "task-1",
    taskContractRevision: 1,
    taskContractDigest: SHA("a"),
    depth: "light",
    content: makePlanContent(overrides),
    createdAt: 1_000,
  });
}

describe("artifact digest primitive", () => {
  it("produces identical digests for semantically identical payloads with different key order", () => {
    const first = computeArtifactDigest({
      artifactType: "TestArtifact",
      schemaVersion: 1,
      payload: { a: 1, b: { x: [1, 2], y: "z" }, c: null },
    });
    const second = computeArtifactDigest({
      artifactType: "TestArtifact",
      schemaVersion: 1,
      payload: { c: null, b: { y: "z", x: [1, 2] }, a: 1 },
    });
    expect(first.value).toBe(second.value);
    expect(first.algorithm).toBe("sha256");
    expect(first.artifactType).toBe("TestArtifact");
    expect(first.schemaVersion).toBe(1);
  });

  it("changes the digest when one material field changes", () => {
    const before = computeArtifactDigest({
      artifactType: "TestArtifact",
      schemaVersion: 1,
      payload: { a: 1 },
    });
    const after = computeArtifactDigest({
      artifactType: "TestArtifact",
      schemaVersion: 1,
      payload: { a: 2 },
    });
    expect(after.value).not.toBe(before.value);
  });

  it("domain-separates artifact types and schema versions", () => {
    const payload = { a: 1 };
    const typeA = computeArtifactDigestHex({ artifactType: "TypeA", schemaVersion: 1, payload });
    const typeB = computeArtifactDigestHex({ artifactType: "TypeB", schemaVersion: 1, payload });
    const version2 = computeArtifactDigestHex({ artifactType: "TypeA", schemaVersion: 2, payload });
    expect(typeA).not.toBe(typeB);
    expect(typeA).not.toBe(version2);
  });

  it("validates, references, and abbreviates digests", () => {
    const digest = computeArtifactDigest({
      artifactType: "TestArtifact",
      schemaVersion: 1,
      payload: { value: "x" },
    });
    expect(validateArtifactDigest(digest)).toEqual(digest);
    expect(
      sameArtifactDigest(
        digest,
        computeArtifactDigest({
          artifactType: "TestArtifact",
          schemaVersion: 1,
          payload: { value: "x" },
        }),
      ),
    ).toBe(true);
    expect(digestReference(digest)).toBe(`sha256:${digest.value}`);
    expect(abbreviateDigest(digest, 8)).toBe(digest.value.slice(0, 8));
    expect(() =>
      validateArtifactDigest({
        algorithm: "md5" as never,
        artifactType: "X",
        schemaVersion: 1,
        value: "0".repeat(64),
      }),
    ).toThrow(/algorithm/);
  });

  it("rejects invalid artifact types and schema versions", () => {
    expect(() =>
      computeArtifactDigest({ artifactType: "bad type!", schemaVersion: 1, payload: {} }),
    ).toThrow(/artifact type/);
    expect(() =>
      computeArtifactDigest({ artifactType: "Valid", schemaVersion: 0, payload: {} }),
    ).toThrow(/schema version/);
  });
});

describe("semantic delta helpers", () => {
  it("detects only materially changed sections", () => {
    const base = { request: "a", constraints: [{ id: "x" }], policy: "none" };
    const result = { request: "b", constraints: [{ id: "x" }], policy: "none" };
    const { changed, unchanged } = computeSectionDelta(base, result, [
      "request",
      "constraints",
      "policy",
    ]);
    expect(changed).toEqual(["request"]);
    expect(unchanged).toEqual(["constraints", "policy"]);
  });

  it("detects item additions, removals, changes, and unchanged items by canonical content", () => {
    const base = [
      { id: "a", value: 1 },
      { id: "b", value: 2 },
      { id: "c", value: 3 },
    ];
    const result = [
      { id: "b", value: 2 },
      { id: "c", value: 4 },
      { id: "d", value: 5 },
    ];
    const delta = computeItemListDelta(base, result);
    expect(delta.added).toEqual(["d"]);
    expect(delta.removed).toEqual(["a"]);
    expect(delta.changed).toEqual(["c"]);
    expect(delta.unchanged).toEqual(["b"]);
  });

  it("produces an order-insensitive list digest", () => {
    expect(digestItemList([{ id: "a" }, { id: "b" }])).toBe(
      digestItemList([{ id: "b" }, { id: "a" }]),
    );
    expect(digestItemList([{ id: "a" }, { id: "b" }])).not.toBe(
      digestItemList([{ id: "a" }, { id: "c" }]),
    );
  });

  it("compares canonical values", () => {
    expect(canonicalValuesEqual({ a: 1, b: [2] }, { b: [2], a: 1 })).toBe(true);
    expect(canonicalValuesEqual({ a: 1 }, { a: 2 })).toBe(false);
  });
});

describe("TaskContract identity", () => {
  it("carries a typed content digest alongside its revision", () => {
    const contract = makeContract();
    expect(contract.revision).toBe(1);
    expect(contract.digest.artifactType).toBe("TaskContract");
    expect(contract.digest.value).toMatch(/^[0-9a-f]{64}$/);
    expect(computeTaskContractArtifactDigest(contract).value).toBe(contract.digest.value);
  });

  it("keeps the digest stable across revisions with identical content", () => {
    const original = makeContract();
    const sameContent = reviseTaskContract(original, {
      id: original.id,
      request: original.request,
    });
    expect(sameContent.revision).toBe(2);
    expect(sameContent.digest.value).toBe(original.digest.value);
  });

  it("changes the digest when content changes", () => {
    const original = makeContract();
    const changed = reviseTaskContract(original, {
      id: original.id,
      request: "Fix the camera follow",
    });
    expect(changed.digest.value).not.toBe(original.digest.value);
  });

  it("derives a semantic delta with only the changed sections", () => {
    const original = makeContract();
    const changed = reviseTaskContract(original, {
      id: original.id,
      request: "Fix the camera follow",
      pausePolicy: "none",
    });
    const delta = computeTaskContractDelta(original, changed);
    expect(delta.baseRevision).toBe(1);
    expect(delta.resultRevision).toBe(2);
    expect(delta.baseDigest).toBe(original.digest.value);
    expect(delta.resultDigest).toBe(changed.digest.value);
    expect(delta.changed).toEqual(["request", "pausePolicy"]);
    expect(delta.unchangedContent).toBe(false);
    expect(delta.unchanged).toContain("acceptanceCriteria");
  });

  it("reports unchanged content deltas", () => {
    const original = makeContract();
    const sameContent = reviseTaskContract(original, {
      id: original.id,
      request: original.request,
    });
    const delta = computeTaskContractDelta(original, sameContent);
    expect(delta.unchangedContent).toBe(true);
    expect(delta.changed).toEqual([]);
  });
});

describe("TaskPlan identity", () => {
  it("binds the plan to the exact TaskContract content digest", () => {
    const contract = makeContract();
    const plan = createTaskPlan({
      id: "plan-task-1",
      taskId: contract.id,
      taskContractRevision: contract.revision,
      taskContractDigest: computeTaskContractContentDigest(contract),
      depth: "light",
      content: makePlanContent(),
      createdAt: 1_000,
    });
    expect(plan.taskContractDigest).toBe(contract.digest.value);
    expect(plan.digest.artifactType).toBe("TaskPlan");
    expect(computeTaskPlanContentDigest(plan)).toBe(plan.digest.value);
  });

  it("changes the plan digest when plan content changes and reports the semantic delta", () => {
    const first = makePlan();
    const second = reviseTaskPlan(first, { content: makePlanContent({ objective: "Add a dash" }) });
    expect(second.revision).toBe(2);
    expect(second.digest.value).not.toBe(first.digest.value);
    const delta = computeTaskPlanDelta(first, second);
    expect(delta.baseDigest).toBe(first.digest.value);
    expect(delta.resultDigest).toBe(second.digest.value);
    expect(delta.changed).toEqual(["objective"]);
    expect(delta.contractBindingChanged).toBe(false);
  });

  it("reports contract-binding changes in the plan delta", () => {
    const first = makePlan();
    const second = reviseTaskPlan(first, {
      content: makePlanContent(),
      taskContractDigest: SHA("b"),
    });
    const delta = computeTaskPlanDelta(first, second);
    expect(delta.contractBindingChanged).toBe(true);
    expect(delta.unchangedContent).toBe(true);
  });

  it("rejects a plan without an exact TaskContract digest binding", () => {
    expect(() =>
      createTaskPlan({
        id: "plan-task-1",
        taskId: "task-1",
        taskContractRevision: 1,
        taskContractDigest: "not-a-digest",
        depth: "light",
        content: makePlanContent(),
        createdAt: 1_000,
      }),
    ).toThrow(/64-hex TaskContract content digest/);
  });
});

describe("plan approval content binding", () => {
  function taskHandleStub() {
    // The runtime approval gate is covered by task-runtime-planning tests;
    // here we verify the digest-level contract: an approval recorded for
    // digest X can never authorize a plan with digest Y.
    const first = makePlan();
    const second = reviseTaskPlan(first, { content: makePlanContent({ objective: "changed" }) });
    return { first, second };
  }

  it("a content change produces a new plan digest that an old approval cannot satisfy", () => {
    const { first, second } = taskHandleStub();
    expect(second.digest.value).not.toBe(first.digest.value);
    expect(computeTaskPlanContentDigest(second)).not.toBe(first.digest.value);
  });
});

describe("planning policy still routes deterministically with identity inputs", () => {
  it("does not regress on surface routing", () => {
    const policy = createPlanningPolicy();
    const input: PlanningDecisionInput = {
      request: "mixed task",
      explicitPlanRequest: false,
      inspectionOnly: false,
      expectedMutation: true,
      acceptanceCriterionCount: 3,
      protectedConfigInvolved: false,
      spansMultipleSubsystems: false,
      researchRequired: false,
      capabilityUncertainty: false,
      narrowRepair: false,
      knownTouchpoints: 2,
      surface: "mixed",
    };
    expect(policy.decide(input).depth).toBe("full");
  });
});
