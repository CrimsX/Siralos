import { describe, expect, it } from "vitest";
import { computeGDScriptDevelopmentDigest, DEVELOPMENT_LIMITS } from "./development-model.js";

describe("DEVELOPMENT_LIMITS", () => {
  it("keeps the immutable workflow limits at their documented values", () => {
    expect(DEVELOPMENT_LIMITS.maxConcurrentWorkflows).toBe(1);
    expect(DEVELOPMENT_LIMITS.maxFilesPerChangeSet).toBe(16);
    expect(DEVELOPMENT_LIMITS.maxChangeSetDiffBytes).toBe(512 * 1024);
    expect(DEVELOPMENT_LIMITS.maxChangeSetResultBytes).toBe(4 * 1024 * 1024);
    expect(DEVELOPMENT_LIMITS.maxReplacementsPerFile).toBe(32);
    expect(DEVELOPMENT_LIMITS.maxRepairProposals).toBe(3);
    expect(DEVELOPMENT_LIMITS.maxTotalIterations).toBe(4);
    expect(DEVELOPMENT_LIMITS.validationBudgetMs).toBe(2 * 60 * 1000);
    expect(DEVELOPMENT_LIMITS.totalWorkflowBudgetMs).toBe(15 * 60 * 1000);
    expect(DEVELOPMENT_LIMITS.maxPreparedChangeSets).toBe(4);
  });
});

describe("computeGDScriptDevelopmentDigest", () => {
  const parts = {
    request: "Add a health component to the player script",
    projectFingerprint: "a".repeat(64),
    engineFingerprint: "b".repeat(64),
    limits: {
      maxIterations: DEVELOPMENT_LIMITS.maxTotalIterations,
      maxRepairProposals: DEVELOPMENT_LIMITS.maxRepairProposals,
      maxFilesPerChangeSet: DEVELOPMENT_LIMITS.maxFilesPerChangeSet,
    },
    authorizationPolicyVersion: 1,
  };

  it("is deterministic for equal structures", () => {
    expect(computeGDScriptDevelopmentDigest(parts)).toBe(
      computeGDScriptDevelopmentDigest({ ...parts }),
    );
  });

  it("binds the request text", () => {
    expect(computeGDScriptDevelopmentDigest({ ...parts, request: "different request" })).not.toBe(
      computeGDScriptDevelopmentDigest(parts),
    );
  });

  it("binds the project fingerprint", () => {
    expect(
      computeGDScriptDevelopmentDigest({ ...parts, projectFingerprint: "c".repeat(64) }),
    ).not.toBe(computeGDScriptDevelopmentDigest(parts));
  });

  it("binds the engine fingerprint and its absence", () => {
    expect(computeGDScriptDevelopmentDigest({ ...parts, engineFingerprint: null })).not.toBe(
      computeGDScriptDevelopmentDigest(parts),
    );
  });

  it("binds the immutable limits", () => {
    expect(
      computeGDScriptDevelopmentDigest({
        ...parts,
        limits: { ...parts.limits, maxFilesPerChangeSet: 8 },
      }),
    ).not.toBe(computeGDScriptDevelopmentDigest(parts));
  });
});
