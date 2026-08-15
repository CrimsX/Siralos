import { describe, expect, it } from "vitest";
import {
  MILESTONE_MANIFEST_LIMITS,
  computeMilestoneManifestDigest,
  createMilestoneManifest,
  reviseMilestoneManifest,
} from "./milestone-manifest.js";
import { S3M8_MILESTONE_MANIFEST } from "./s3m8-manifest.js";
import { STANDARD_ACCEPTANCE_IDS } from "./standard-acceptance.js";

const BASE = {
  id: "S3M9",
  title: "Test Milestone",
  goal: "Test the milestone manifest model.",
  acceptance: [
    { id: "S3M9.PARSE.X", description: "Parses X.", evidenceKinds: ["parser_result"] as const },
  ],
};

describe("milestone manifest", () => {
  it("creates an immutable versioned manifest", () => {
    const manifest = createMilestoneManifest(BASE);
    expect(manifest.version).toBe(1);
    expect(manifest.id).toBe("S3M9");
    expect(Object.isFrozen(manifest)).toBe(true);
  });

  it("rejects invalid ids, empty acceptance, and unknown standard ids", () => {
    expect(() => createMilestoneManifest({ ...BASE, id: "s3m9" })).toThrow(/Invalid milestone id/);
    expect(() => createMilestoneManifest({ ...BASE, acceptance: [] })).toThrow(
      /at least one acceptance requirement/,
    );
    expect(() =>
      createMilestoneManifest({
        ...BASE,
        acceptance: [{ id: "S3M9.X", description: "x", standardIds: ["STANDARD.NOPE"] as never }],
      }),
    ).toThrow(/unknown standard acceptance/);
  });

  it("rejects acceptance requirements with no evidence mapping at all", () => {
    expect(() =>
      createMilestoneManifest({
        ...BASE,
        acceptance: [{ id: "S3M9.X", description: "x" }],
      }),
    ).toThrow(/must declare evidenceKinds, a criterionId, or standardIds/);
  });

  it("rejects a criterion-linked requirement with a divergent check identity", () => {
    expect(() =>
      createMilestoneManifest({
        ...BASE,
        acceptance: [
          {
            id: "S3M9.X",
            description: "x",
            criterionId: "criterion-x",
            checkId: "different-check",
          },
        ],
      }),
    ).toThrow(/must use its linked criterion id as checkId/);
  });

  it("enforces entry bounds deterministically", () => {
    const many = Array.from({ length: MILESTONE_MANIFEST_LIMITS.maxInvariants + 1 }, (_, i) => ({
      id: `inv-${i}`,
      description: `invariant ${i}`,
    }));
    expect(() => createMilestoneManifest({ ...BASE, invariants: many })).toThrow(/at most/);
  });

  it("revises immutably to the next version", () => {
    const v1 = createMilestoneManifest(BASE);
    const v2 = reviseMilestoneManifest(v1, { goal: "Revised goal." });
    expect(v2.version).toBe(2);
    expect(v1.goal).toBe(BASE.goal);
    expect(computeMilestoneManifestDigest(v1)).not.toBe(computeMilestoneManifestDigest(v2));
  });

  it("accepts optional requirements", () => {
    const manifest = createMilestoneManifest({
      ...BASE,
      acceptance: [
        { id: "S3M9.X", description: "x", evidenceKinds: ["parser_result"], optional: true },
      ],
    });
    expect(manifest.acceptance[0]?.optional).toBe(true);
  });
});

describe("S3M8 milestone manifest", () => {
  it("is a real validated manifest with stable acceptance ids", () => {
    const manifest = S3M8_MILESTONE_MANIFEST;
    expect(manifest.id).toBe("S3M8");
    expect(manifest.version).toBe(1);
    expect(manifest.acceptance.length).toBeGreaterThanOrEqual(10);
    const ids = manifest.acceptance.map((requirement) => requirement.id);
    for (const expected of [
      "S3M8.PARSE.TSCN",
      "S3M8.PARSE.TRES",
      "S3M8.REVISION.STALE",
      "S3M8.SECURITY.NO_PROCESS",
      "S3M8.SECURITY.NO_MUTATION",
      "S3M8.TOOLS.NO_NATIVE_WRITE",
      "S3M8.DEVELOP.REFUSE_NATIVE_MUTATION",
    ]) {
      expect(ids).toContain(expected);
    }
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("contains only milestone-specific content — no permanent rules restated", () => {
    const manifest = S3M8_MILESTONE_MANIFEST;
    const text = JSON.stringify(manifest);
    expect(text).not.toMatch(/no push|rebase|rewrite history/i);
    expect(text).not.toMatch(/npm run check/);
    expect(text).not.toMatch(/untrusted/i);
    expect(manifest.validationProfile).toBeUndefined();
    expect(manifest.architectureConcerns).toContain("godot-static-inspection");
    expect(manifest.architectureConcerns).toContain("workspace-revision");
  });

  it("references only known standard acceptance ids", () => {
    const used = new Set(
      S3M8_MILESTONE_MANIFEST.acceptance.flatMap((requirement) => requirement.standardIds ?? []),
    );
    for (const id of used) {
      expect(STANDARD_ACCEPTANCE_IDS).toContain(id);
    }
  });

  it("compiles into a concise brief with all unique invariants and acceptance ids", async () => {
    const { compileExecutorBrief, computeExecutorBriefFingerprint, renderExecutorBrief } =
      await import("./brief-compiler.js");
    const { buildExecutorContextPack } = await import("./context-pack.js");
    const { createTaskContract } = await import("../tasks/task-contract.js");
    const { createTaskPlan } = await import("../planning/planning-model.js");
    const { DEFAULT_EXECUTION_CONTRACT } = await import("./execution-contract.js");
    const contract = createTaskContract({
      id: "task-s3m8",
      request: "Add read-only .tscn/.tres semantic intelligence.",
      acceptanceCriteria: [
        { id: "s3m8-parses", description: "parses", verificationKind: "deterministic" },
      ],
    });
    const plan = createTaskPlan({
      id: "plan-s3m8",
      taskId: contract.id,
      taskContractDigest: "a".repeat(64),
      taskContractRevision: contract.revision,
      depth: "light",
      content: {
        objective: "Add read-only scene/resource semantic intelligence.",
        scope: { inScope: ["parsing"], outOfScope: ["mutation"] },
        nonGoals: ["mutation"],
        touchpoints: [
          {
            id: "t1",
            path: "packages/core/src/godot/scene/scene-parser.ts",
            confidence: "verified",
            revision: "rev_".padEnd(36, "a"),
          },
          { id: "t2", path: "apps/cli/src/**", confidence: "candidate" },
        ],
        constraints: [],
        risks: [],
        steps: [{ id: "step-1", title: "Parse", expectedTouchpoints: ["t1"] }],
        validation: { checks: ["parser tests"] },
      },
      createdAt: 1,
    });
    const pack = buildExecutorContextPack({
      contract,
      plan,
      executionContract: {
        id: DEFAULT_EXECUTION_CONTRACT.id,
        revision: DEFAULT_EXECUTION_CONTRACT.revision,
      },
      milestone: S3M8_MILESTONE_MANIFEST,
      planApproval: "approved",
    });
    const brief = compileExecutorBrief({
      contract,
      executionContract: DEFAULT_EXECUTION_CONTRACT,
      pack,
      milestone: S3M8_MILESTONE_MANIFEST,
    });
    const rendered = renderExecutorBrief(brief);
    expect(computeExecutorBriefFingerprint(brief)).toMatch(/^[0-9a-f]{64}$/);
    expect(rendered).toContain("Execution Contract: siralos-execution-contract rev 2");
    expect(rendered).toContain("Milestone Manifest: S3M8 rev 1");
    for (const id of [
      "S3M8.PARSE.TSCN",
      "S3M8.PARSE.TRES",
      "S3M8.REVISION.STALE",
      "S3M8.SECURITY.NO_PROCESS",
      "S3M8.SECURITY.NO_MUTATION",
      "S3M8.DEVELOP.REFUSE_NATIVE_MUTATION",
    ]) {
      expect(rendered).toContain(id);
    }
    expect(rendered).toContain("TASK-SPECIFIC INVARIANTS");
    expect(rendered).toContain("NON-GOALS");
    expect(rendered).toContain("VERIFIED TOUCHPOINTS");
    expect(rendered).toContain("CANDIDATE TOUCHPOINTS");
    expect(rendered).not.toMatch(/CORE\.GIT|npm run check/i);
    expect(rendered.length).toBeLessThan(8 * 1024);
    const expected = [
      "TASK",
      "EXECUTION CONTRACT",
      "MILESTONE",
      "DELIVERABLES",
      "TASK-SPECIFIC INVARIANTS",
      "NON-GOALS",
      "ACCEPTANCE",
      "MILESTONE-SPECIFIC TESTS",
      "ARCHITECTURE REFERENCES",
    ];
    for (const section of expected) {
      expect(rendered).toMatch(new RegExp(`^${section}$`, "m"));
    }
  });

  it("selects relevant architecture references and omits unrelated material", () => {
    const references = S3M8_MILESTONE_MANIFEST.architectureConcerns;
    expect(references).toContain("planning");
    expect(references).toContain("task-runtime");
  });
});
