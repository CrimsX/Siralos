import { describe, expect, it } from "vitest";
import {
  DEFAULT_EXECUTION_CONTRACT,
  EXECUTION_CONTRACT_LIMITS,
  computeExecutionContractDigest,
  createExecutionContract,
  reviseExecutionContract,
  validateExecutionContract,
} from "./execution-contract.js";
import { STANDARD_REPO_VALIDATION } from "./validation-profile.js";

const MINIMAL_PROFILE = { profileId: "standard-repo-validation", revision: 1 };

describe("execution contract", () => {
  it("creates the first immutable revision with the default profile", () => {
    const contract = createExecutionContract({
      id: "test-contract",
      validationProfile: MINIMAL_PROFILE,
    });
    expect(contract.revision).toBe(1);
    expect(contract.id).toBe("test-contract");
    expect(contract.validationProfile).toEqual(MINIMAL_PROFILE);
    expect(Object.isFrozen(contract)).toBe(true);
    expect(() => {
      (contract as { revision: number }).revision = 99;
    }).toThrow();
  });

  it("validates rules by group and rejects mismatched kinds", () => {
    expect(() =>
      createExecutionContract({
        id: "test-contract",
        validationProfile: MINIMAL_PROFILE,
        gitRules: [{ id: "CORE.GIT.X", kind: "security", requirement: "x", enforcedBy: "y" }],
      }),
    ).toThrow(/belongs to the git group/);
  });

  it("rejects duplicate rule ids and invalid rule id shapes", () => {
    expect(() =>
      createExecutionContract({
        id: "test-contract",
        validationProfile: MINIMAL_PROFILE,
        gitRules: [
          { id: "CORE.GIT.A", kind: "git", requirement: "a", enforcedBy: "y" },
          { id: "CORE.GIT.A", kind: "git", requirement: "b", enforcedBy: "y" },
        ],
      }),
    ).toThrow(/Duplicate execution rule id/);
    expect(() =>
      createExecutionContract({
        id: "test-contract",
        validationProfile: MINIMAL_PROFILE,
        testRules: [{ id: "lower-case", kind: "test", requirement: "a", enforcedBy: "y" }],
      }),
    ).toThrow(/Invalid execution rule id/);
  });

  it("rejects oversized groups and empty requirements", () => {
    const many = Array.from({ length: EXECUTION_CONTRACT_LIMITS.maxRulesPerGroup + 1 }, (_, i) => ({
      id: `CORE.TEST.R${i}`,
      kind: "test" as const,
      requirement: `rule ${i}`,
      enforcedBy: "enforced",
    }));
    expect(() =>
      createExecutionContract({
        id: "test-contract",
        validationProfile: MINIMAL_PROFILE,
        testRules: many,
      }),
    ).toThrow(/at most/);
    expect(() =>
      createExecutionContract({
        id: "test-contract",
        validationProfile: MINIMAL_PROFILE,
        gitRules: [{ id: "CORE.GIT.X", kind: "git", requirement: "  ", enforcedBy: "y" }],
      }),
    ).toThrow(/non-empty requirement/);
  });

  it("revises immutably and preserves id while advancing revision", () => {
    const v1 = createExecutionContract({
      id: "test-contract",
      validationProfile: MINIMAL_PROFILE,
      gitRules: [
        { id: "CORE.GIT.NO_PUSH", kind: "git", requirement: "no push", enforcedBy: "gate" },
      ],
    });
    const v2 = reviseExecutionContract(v1, {
      gitRules: [
        { id: "CORE.GIT.NO_PUSH", kind: "git", requirement: "no push", enforcedBy: "gate" },
        { id: "CORE.GIT.LOGICAL", kind: "git", requirement: "small commits", enforcedBy: "gate" },
      ],
    });
    expect(v2.revision).toBe(2);
    expect(v2.id).toBe(v1.id);
    expect(v1.gitRules).toHaveLength(1);
    expect(v2.gitRules).toHaveLength(2);
    expect(computeExecutionContractDigest(v1)).not.toBe(computeExecutionContractDigest(v2));
  });

  it("rejects invalid revisions and profile refs", () => {
    expect(() =>
      createExecutionContract({ id: "x", validationProfile: { profileId: "", revision: 1 } }),
    ).toThrow(/profile id/);
    const bad = createExecutionContract({ id: "x", validationProfile: MINIMAL_PROFILE });
    expect(() => validateExecutionContract({ ...bad, revision: 0 })).toThrow(/at least 1/);
    expect(() => validateExecutionContract({ ...bad, id: "no!" })).toThrow(
      /Invalid execution contract id/,
    );
  });

  it("default contract: permanent rules exist once and reference enforcement", () => {
    const contract = DEFAULT_EXECUTION_CONTRACT;
    expect(contract.revision).toBe(1);
    expect(contract.id).toBe("siralos-execution-contract");
    const ids = [
      ...contract.gitRules,
      ...contract.securityRules,
      ...contract.architectureRules,
      ...contract.testRules,
    ].map((rule) => rule.id);
    expect(ids).toContain("CORE.GIT.NO_PUSH");
    expect(ids).toContain("CORE.GIT.LOGICAL_COMMITS");
    expect(ids).toContain("CORE.GIT.INSPECT_STAGING");
    expect(ids).toContain("CORE.SECURITY.UNTRUSTED_OUTPUT");
    expect(ids).toContain("CORE.SECURITY.POLICY_AUTHORITATIVE");
    expect(ids).toContain("CORE.SECURITY.NO_BROADENING");
    expect(ids).toContain("CORE.SECURITY.NO_SECRET_LEAK");
    expect(ids).toContain("CORE.ARCH.BOUNDARIES_AUTHORITATIVE");
    expect(ids).toContain("CORE.ARCH.TASKSTATE_AUTHORITATIVE");
    expect(ids).toContain("CORE.TEST.FINAL_BOUNDARY");
    expect(ids).toContain("CORE.TEST.STANDARD_VALIDATION");
    for (const rule of [
      ...contract.gitRules,
      ...contract.securityRules,
      ...contract.architectureRules,
      ...contract.testRules,
    ]) {
      expect(rule.enforcedBy.length).toBeGreaterThan(0);
      expect(rule.enforcedBy).not.toBe(rule.requirement);
    }
    expect(contract.validationProfile.profileId).toBe(STANDARD_REPO_VALIDATION.profileId);
    expect(validateExecutionContract(contract).revision).toBe(1);
  });

  it("default contract validates at the runtime boundary", () => {
    expect(validateExecutionContract(DEFAULT_EXECUTION_CONTRACT).id).toBe(
      "siralos-execution-contract",
    );
  });
});
