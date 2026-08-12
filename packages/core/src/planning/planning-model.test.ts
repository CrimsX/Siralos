import { describe, expect, it } from "vitest";
import { createTaskContract, type TaskContract } from "../tasks/task-contract.js";
import {
  computePlanRevisionDigest,
  createTaskPlan,
  hasMeaningfulAcceptanceCriteria,
  reviseTaskPlan,
  type TaskPlanContent,
} from "./planning-model.js";
import { planTouchpointStaleness, validatePlanCandidate } from "./planning-validation.js";

function makeContract(): TaskContract {
  return createTaskContract({
    id: "task-1",
    request: "Add health regeneration",
    acceptanceCriteria: [
      { id: "parses", description: "Parses cleanly", verificationKind: "deterministic" },
      { id: "tests", description: "Tests pass", verificationKind: "review" },
    ],
  });
}

function makeContent(): TaskPlanContent {
  return {
    objective: "Add health regeneration after 5 seconds without damage.",
    scope: { inScope: ["player health timing"], outOfScope: ["UI work"] },
    nonGoals: ["Health bar animation"],
    touchpoints: [
      {
        id: "t1",
        path: "src/player/player.gd",
        confidence: "verified",
        revision: "rev_".padEnd(36, "a"),
        evidence: "read:src/player/player.gd",
      },
      { id: "t2", path: "tests/player/**", confidence: "candidate" },
    ],
    constraints: [{ id: "c1", description: "Stay within the workspace." }],
    risks: [{ id: "r1", severity: "medium", description: "Damage cooldown interaction." }],
    steps: [
      {
        id: "step-1",
        title: "Update player health timing state",
        expectedTouchpoints: ["t1"],
        verification: ["parses"],
      },
      { id: "step-2", title: "Extend health tests", expectedTouchpoints: ["t2"] },
    ],
    validation: {
      checks: ["check-only parse", "fresh LSP diagnostics"],
      requirements: ["workspace mutation"],
    },
    rollback: { description: "Revert the prepared change set." },
    rationale: "Bounded change in one file plus tests.",
  };
}

describe("validatePlanCandidate", () => {
  it("accepts a valid full-plan candidate and returns content", () => {
    const contract = makeContract();
    const candidate = { depth: "full", ...makeContent() };
    const result = validatePlanCandidate(candidate, { contract, depth: "full" });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.content.objective).toContain("health regeneration");
      expect(result.content.touchpoints[0]?.confidence).toBe("verified");
    }
  });

  it("returns an exact detached plan shape without unknown provider fields", () => {
    const contract = makeContract();
    const candidate = {
      ...makeContent(),
      touchpoints: [
        {
          id: "t1",
          path: "src/player/player.gd",
          confidence: "verified",
          revision: "rev_".padEnd(36, "a"),
          evidence: "read:src/player/player.gd",
          untrustedExtra: "must not cross the validation boundary",
        },
      ],
      steps: [
        {
          id: "step-1",
          title: "Update player health timing state",
          expectedTouchpoints: ["t1"],
          verification: ["parses"],
          untrustedExtra: { hidden: true },
        },
      ],
    };

    const result = validatePlanCandidate(candidate, { contract, depth: "full" });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.content.touchpoints[0]).not.toHaveProperty("untrustedExtra");
      expect(result.content.steps[0]).not.toHaveProperty("untrustedExtra");
      candidate.touchpoints[0]!.path = "forged.gd";
      candidate.steps[0]!.expectedTouchpoints[0] = "forged";
      expect(result.content.touchpoints[0]?.path).toBe("src/player/player.gd");
      expect(result.content.steps[0]?.expectedTouchpoints).toEqual(["t1"]);
    }
  });

  it("rejects a depth that does not match the host decision", () => {
    const contract = makeContract();
    const candidate = { depth: "light", ...makeContent() };
    const result = validatePlanCandidate(candidate, { contract, depth: "full" });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.reasons.join(" ")).toContain("host-routed depth");
    }
  });

  it("rejects verified touchpoints without an exact revision handle", () => {
    const contract = makeContract();
    const candidate = {
      ...makeContent(),
      touchpoints: [
        { id: "t1", path: "src/player/player.gd", confidence: "verified", evidence: "read:x" },
      ],
    };
    const result = validatePlanCandidate(candidate, { contract, depth: "full" });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.reasons.join(" ")).toContain("exact inspected workspace revision");
    }
  });

  it("rejects unsupported paths: absolute, escaping, reference namespaces, globs on verified", () => {
    const contract = makeContract();
    for (const path of [
      "/etc/passwd",
      "C:/windows/system32",
      "../outside.gd",
      "@reference/gdunit4/lib.gd",
      "src/player/*.gd",
    ]) {
      const candidate = {
        ...makeContent(),
        touchpoints: [
          {
            id: "t1",
            path,
            confidence: "verified",
            revision: "rev_".padEnd(36, "a"),
          },
        ],
      };
      const result = validatePlanCandidate(candidate, { contract, depth: "full" });
      expect(result.ok).toBe(false);
    }
    // Candidate globs are allowed.
    const glob = {
      ...makeContent(),
      touchpoints: [
        {
          id: "t1",
          path: "src/player/player.gd",
          confidence: "verified",
          revision: "rev_".padEnd(36, "a"),
        },
        { id: "t2", path: "tests/player/**", confidence: "candidate" },
      ],
    };
    expect(validatePlanCandidate(glob, { contract, depth: "full" }).ok).toBe(true);
  });

  it("rejects unknown acceptance-criterion references in steps", () => {
    const contract = makeContract();
    const candidate = {
      ...makeContent(),
      steps: [
        {
          id: "step-1",
          title: "Step one",
          expectedTouchpoints: [],
          verification: ["nonexistent-criterion"],
        },
      ],
    };
    const result = validatePlanCandidate(candidate, { contract, depth: "full" });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.reasons.join(" ")).toContain("nonexistent-criterion");
    }
  });

  it("rejects policy-shaped capability claims (fixtures 24/25/51)", () => {
    const contract = makeContract();
    const candidate = {
      ...makeContent(),
      steps: [
        { id: "step-1", title: "Step one", expectedTouchpoints: [] },
        { id: "step-3", title: "Enable unrestricted network", expectedTouchpoints: [] },
      ],
    };
    const result = validatePlanCandidate(candidate, { contract, depth: "full" });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.reasons.join(" ")).toContain("policy-shaped");
    }
    const sandboxClaim = {
      ...makeContent(),
      objective: "Disable sandbox for implementation.",
    };
    const sandboxResult = validatePlanCandidate(sandboxClaim, { contract, depth: "full" });
    expect(sandboxResult.ok).toBe(false);
  });

  it("rejects secret-shaped content", () => {
    const contract = makeContract();
    const candidate = {
      ...makeContent(),
      objective: "Use the api_key=sk-1234567890abcdef in the config.",
    };
    const result = validatePlanCandidate(candidate, { contract, depth: "full" });
    expect(result.ok).toBe(false);
  });

  it("rejects oversized plans and oversized fields", () => {
    const contract = makeContract();
    const big = {
      ...makeContent(),
      objective: "x".repeat(4096),
    };
    expect(validatePlanCandidate(big, { contract, depth: "full" }).ok).toBe(false);
    const manySteps = {
      ...makeContent(),
      steps: Array.from({ length: 13 }, (_, index) => ({
        id: `step-${index}`,
        title: `Step ${index}`,
        expectedTouchpoints: [],
      })),
    };
    expect(validatePlanCandidate(manySteps, { contract, depth: "full" }).ok).toBe(false);
    // Light plans are bounded tighter.
    const lightSteps = {
      ...makeContent(),
      steps: Array.from({ length: 7 }, (_, index) => ({
        id: `step-${index}`,
        title: `Step ${index}`,
        expectedTouchpoints: [],
      })),
    };
    expect(validatePlanCandidate(lightSteps, { contract, depth: "light" }).ok).toBe(false);
  });
});

describe("TaskPlan immutability and revision semantics", () => {
  it("creates an immutable TaskPlan revision 1 with host-owned identity", () => {
    const plan = createTaskPlan({
      id: "plan-task-1",
      taskId: "task-1",
      taskContractDigest: "a".repeat(64),
      taskContractRevision: 1,
      depth: "full",
      content: makeContent(),
      createdAt: 1000,
    });
    expect(plan.revision).toBe(1);
    expect(plan.taskContractRevision).toBe(1);
    expect(() => {
      (plan as { objective: string }).objective = "mutated";
    }).toThrow();
    expect(Object.isFrozen(plan.scope)).toBe(true);
    expect(Object.isFrozen(plan.steps)).toBe(true);
    expect(Object.isFrozen(plan.steps[0]?.expectedTouchpoints)).toBe(true);
    expect(() => {
      (plan.steps as unknown as Array<{ title: string }>)[0]!.title = "forged nested step";
    }).toThrow();
    expect(plan.steps[0]?.title).toBe("Update player health timing state");
  });

  it("creates rev 2 rather than mutating rev 1 (fixture 12)", () => {
    const rev1 = createTaskPlan({
      id: "plan-task-1",
      taskId: "task-1",
      taskContractDigest: "a".repeat(64),
      taskContractRevision: 1,
      depth: "full",
      content: makeContent(),
      createdAt: 1000,
    });
    const rev2 = reviseTaskPlan(rev1, { content: { ...makeContent(), objective: "Changed." } });
    expect(rev2.revision).toBe(2);
    expect(rev2.objective).toBe("Changed.");
    expect(rev1.revision).toBe(1);
    expect(rev1.objective).toContain("health regeneration");
    expect(computePlanRevisionDigest(rev1)).not.toBe(computePlanRevisionDigest(rev2));
  });

  it("records the TaskContract revision (fixture 13)", () => {
    const plan = createTaskPlan({
      id: "plan-task-1",
      taskId: "task-1",
      taskContractDigest: "a".repeat(64),
      taskContractRevision: 3,
      depth: "light",
      content: makeContent(),
      createdAt: 1000,
    });
    expect(plan.taskContractRevision).toBe(3);
  });

  it("rejects non-finite or non-incrementable plan metadata", () => {
    expect(() =>
      createTaskPlan({
        id: "plan-task-1",
        taskId: "task-1",
        taskContractDigest: "a".repeat(64),
      taskContractRevision: Number.NaN,
        depth: "full",
        content: makeContent(),
        createdAt: 1000,
      }),
    ).toThrow(/safe-integer task contract revision/);
    expect(() =>
      createTaskPlan({
        id: "plan-task-1",
        taskId: "task-1",
        taskContractDigest: "a".repeat(64),
      taskContractRevision: 1,
        depth: "full",
        content: makeContent(),
        createdAt: Number.POSITIVE_INFINITY,
      }),
    ).toThrow(/safe-integer createdAt/);

    const plan = structuredClone(
      createTaskPlan({
        id: "plan-task-1",
        taskId: "task-1",
        taskContractDigest: "a".repeat(64),
      taskContractRevision: 1,
        depth: "full",
        content: makeContent(),
        createdAt: 1000,
      }),
    );
    (plan as { revision: number }).revision = Number.MAX_SAFE_INTEGER;
    expect(() => reviseTaskPlan(plan, { content: makeContent() })).toThrow(/incrementable/);
  });

  it("detects meaningful acceptance criteria for full-plan execution", () => {
    const contract = makeContract();
    expect(hasMeaningfulAcceptanceCriteria(contract)).toBe(true);
    const singleUser = createTaskContract({
      id: "task-2",
      request: "Anything",
      acceptanceCriteria: [
        { id: "host-verified", description: "Host verifies", verificationKind: "user" },
      ],
    });
    expect(hasMeaningfulAcceptanceCriteria(singleUser)).toBe(false);
  });

  it("treats a missing current revision as a stale verified touchpoint", () => {
    const plan = createTaskPlan({
      id: "plan-task-1",
      taskId: "task-1",
      taskContractDigest: "a".repeat(64),
      taskContractRevision: 1,
      depth: "full",
      content: makeContent(),
      createdAt: 1000,
    });

    expect(planTouchpointStaleness(plan, () => null)).toContain("src/player/player.gd");
    expect(planTouchpointStaleness(plan, () => "rev_".padEnd(36, "a"))).not.toContain(
      "src/player/player.gd",
    );
  });
});
