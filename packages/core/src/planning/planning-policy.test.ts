import { describe, expect, it } from "vitest";
import {
  containsProtectedConfigReference,
  createPlanningPolicy,
  type PlanningDecisionInput,
} from "./planning-policy.js";

function input(overrides: Partial<PlanningDecisionInput> = {}): PlanningDecisionInput {
  return {
    request: "Add a health component to the player script",
    explicitPlanRequest: false,
    inspectionOnly: false,
    expectedMutation: true,
    acceptanceCriterionCount: 2,
    protectedConfigInvolved: false,
    spansMultipleSubsystems: false,
    researchRequired: false,
    capabilityUncertainty: false,
    narrowRepair: false,
    knownTouchpoints: 0,
    ...overrides,
  };
}

describe("createPlanningPolicy", () => {
  it("routes a simple narrow task to none (fixture 1)", () => {
    const policy = createPlanningPolicy();
    const decision = policy.decide(
      input({
        request: "Rename one known local variable in one known file",
        narrowRepair: true,
        knownTouchpoints: 1,
      }),
    );
    expect(decision.depth).toBe("none");
    expect(decision.reason).toBe("narrow-repair-known-surface");
  });

  it("routes a moderately complex task to light (fixture 2)", () => {
    const policy = createPlanningPolicy();
    const decision = policy.decide(input({ knownTouchpoints: 3, acceptanceCriterionCount: 2 }));
    expect(decision.depth).toBe("light");
    expect(decision.reason).toBe("bounded-non-trivial");
  });

  it("routes an unknown surface to light (conservative default)", () => {
    const policy = createPlanningPolicy();
    const decision = policy.decide(input({ knownTouchpoints: 0 }));
    expect(decision.depth).toBe("light");
    expect(decision.reason).toBe("unknown-surface-bounded");
  });

  it("routes a clearly complex multi-system task to full (fixture 3)", () => {
    const policy = createPlanningPolicy();
    const decision = policy.decide(input({ spansMultipleSubsystems: true }));
    expect(decision.depth).toBe("full");
    expect(decision.reason).toBe("multi-subsystem");
  });

  it("routes protected-config involvement to full", () => {
    const policy = createPlanningPolicy();
    const decision = policy.decide(input({ protectedConfigInvolved: true }));
    expect(decision.depth).toBe("full");
    expect(decision.reason).toBe("protected-config");
  });

  it("routes research-required and capability-uncertain tasks to full", () => {
    const policy = createPlanningPolicy();
    expect(policy.decide(input({ researchRequired: true })).depth).toBe("full");
    expect(policy.decide(input({ capabilityUncertainty: true })).depth).toBe("full");
  });

  it("routes broad surfaces and many criteria to full", () => {
    const policy = createPlanningPolicy();
    expect(policy.decide(input({ knownTouchpoints: 5 })).depth).toBe("full");
    expect(policy.decide(input({ acceptanceCriterionCount: 4, knownTouchpoints: 2 })).depth).toBe(
      "full",
    );
  });

  it("routes inspection-only and non-mutating tasks to none", () => {
    const policy = createPlanningPolicy();
    expect(policy.decide(input({ inspectionOnly: true })).depth).toBe("none");
    expect(policy.decide(input({ expectedMutation: false })).depth).toBe("none");
  });

  it("honors an explicit plan request regardless of normal routing (fixture 4)", () => {
    const policy = createPlanningPolicy();
    const decision = policy.decide(
      input({ explicitPlanRequest: true, narrowRepair: true, knownTouchpoints: 1 }),
    );
    expect(decision.depth).toBe("full");
    expect(decision.reason).toBe("explicit-plan-request");
    expect(policy.decide(input({ explicitPlanRequest: true, requestedDepth: "light" })).depth).toBe(
      "light",
    );
  });

  it("is deterministic for identical structured inputs (fixture 5)", () => {
    const policy = createPlanningPolicy();
    const probe = input({ spansMultipleSubsystems: true });
    const first = policy.decide(probe);
    for (let index = 0; index < 50; index += 1) {
      const again = policy.decide(probe);
      expect(again.depth).toBe(first.depth);
      expect(again.reason).toBe(first.reason);
    }
  });

  it("routes a mixed script/native surface to full (S3M11)", () => {
    const policy = createPlanningPolicy();
    const decision = policy.decide(
      input({ surface: "mixed", knownTouchpoints: 2, acceptanceCriterionCount: 2 }),
    );
    expect(decision.depth).toBe("full");
    expect(decision.reason).toBe("mixed-surface-relationships");
  });

  it("keeps a script-only surface on the normal depth ladder (S3M11)", () => {
    const policy = createPlanningPolicy();
    const decision = policy.decide(
      input({ surface: "script_only", knownTouchpoints: 3, acceptanceCriterionCount: 2 }),
    );
    expect(decision.depth).toBe("light");
  });

  it("detects protected config references deterministically", () => {
    expect(containsProtectedConfigReference("Update AGENTS.md rules")).toBe(true);
    expect(containsProtectedConfigReference("touch .solaris/config.json")).toBe(true);
    expect(containsProtectedConfigReference("adjust behavioural config")).toBe(true);
    expect(containsProtectedConfigReference("Update the player script")).toBe(false);
    expect(containsProtectedConfigReference("AGENTS")).toBe(false);
  });
});
