import { describe, expect, it } from "vitest";
import {
  DEFAULT_EXECUTION_CONTRACT,
  DEFAULT_EXECUTION_CONTRACT_V1,
  EXECUTION_CONTRACT_LIMITS,
  computeExecutionContractDigest,
  createExecutionContract,
  executionContractRef,
  reviseExecutionContract,
  validateExecutionContract,
  type ExecutionRule,
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

  it("default contract: the recovery-aware current default is revision 2, derived from revision 1", () => {
    const v1 = DEFAULT_EXECUTION_CONTRACT_V1;
    const v2 = DEFAULT_EXECUTION_CONTRACT;
    const ruleIds = (rules: readonly ExecutionRule[]): string[] => rules.map((rule) => rule.id);
    // 1. Revision 1 remains revision 1; 2. revision 2 advances to 2.
    expect(v1.revision).toBe(1);
    expect(v2.revision).toBe(2);
    // 3. Both use the same stable contract ID.
    expect(v1.id).toBe("siralos-execution-contract");
    expect(v2.id).toBe(v1.id);
    // 4. Revision 1 does not contain the recovery rule; 5. revision 2 does.
    expect(ruleIds(v1.securityRules)).not.toContain("CORE.SECURITY.RECOVERY_AUTHORITY");
    expect(ruleIds(v2.securityRules)).toContain("CORE.SECURITY.RECOVERY_AUTHORITY");
    // 6. Revising revision 1 never mutates revision 1 in place.
    const v1DigestBefore = computeExecutionContractDigest(v1);
    const probe = reviseExecutionContract(v1, {});
    expect(computeExecutionContractDigest(v1)).toBe(v1DigestBefore);
    expect(probe.revision).toBe(2);
    // 7. Revision 1 and revision 2 digests differ.
    expect(computeExecutionContractDigest(v2)).not.toBe(v1DigestBefore);
    // 8. Contract references distinguish the two revisions.
    expect(executionContractRef(v1)).toEqual({ id: "siralos-execution-contract", revision: 1 });
    expect(executionContractRef(v2)).toEqual({ id: "siralos-execution-contract", revision: 2 });
    // 9. Validation accepts both revisions.
    expect(validateExecutionContract(v1).revision).toBe(1);
    expect(validateExecutionContract(v2).revision).toBe(2);
    // The only material difference is the recovery rule.
    expect(v2.securityRules.length).toBe(v1.securityRules.length + 1);
    expect(v2.gitRules).toEqual(v1.gitRules);
    expect(v2.architectureRules).toEqual(v1.architectureRules);
    expect(v2.testRules).toEqual(v1.testRules);
    expect(v2.reportingRequirements).toEqual(v1.reportingRequirements);
    // Permanent rules exist once and reference enforcement.
    const ids = [...v2.gitRules, ...v2.securityRules, ...v2.architectureRules, ...v2.testRules].map(
      (rule) => rule.id,
    );
    expect(ids).toContain("CORE.GIT.NO_PUSH");
    expect(ids).toContain("CORE.GIT.LOGICAL_COMMITS");
    expect(ids).toContain("CORE.GIT.INSPECT_STAGING");
    expect(ids).toContain("CORE.SECURITY.UNTRUSTED_OUTPUT");
    expect(ids).toContain("CORE.SECURITY.POLICY_AUTHORITATIVE");
    expect(ids).toContain("CORE.SECURITY.NO_BROADENING");
    expect(ids).toContain("CORE.SECURITY.NO_SECRET_LEAK");
    expect(ids).toContain("CORE.SECURITY.RECOVERY_AUTHORITY");
    expect(ids).toContain("CORE.ARCH.BOUNDARIES_AUTHORITATIVE");
    expect(ids).toContain("CORE.ARCH.TASKSTATE_AUTHORITATIVE");
    expect(ids).toContain("CORE.TEST.FINAL_BOUNDARY");
    expect(ids).toContain("CORE.TEST.STANDARD_VALIDATION");
    for (const rule of [
      ...v2.gitRules,
      ...v2.securityRules,
      ...v2.architectureRules,
      ...v2.testRules,
    ]) {
      expect(rule.enforcedBy.length).toBeGreaterThan(0);
      expect(rule.enforcedBy).not.toBe(rule.requirement);
    }
    expect(v2.validationProfile.profileId).toBe(STANDARD_REPO_VALIDATION.profileId);
    // 10. The current default resolves to revision 2.
    expect(validateExecutionContract(v2).revision).toBe(2);
  });

  it("default contract: historical revision 1 is digest-pinned (in-place mutation ratchet)", () => {
    // Revision 1 is frozen history: any edit to its definition changes
    // this digest and fails the gate, forcing a new revision instead.
    expect(computeExecutionContractDigest(DEFAULT_EXECUTION_CONTRACT_V1)).toBe(
      "6cb2335ca5d34c8045623a888eaa40424e5c4136518973055359d2575c263861",
    );
    // The current default is always exactly one revision ahead of V1.
    expect(DEFAULT_EXECUTION_CONTRACT.revision).toBe(DEFAULT_EXECUTION_CONTRACT_V1.revision + 1);
  });

  it("default contract validates at the runtime boundary", () => {
    expect(validateExecutionContract(DEFAULT_EXECUTION_CONTRACT).id).toBe(
      "siralos-execution-contract",
    );
  });
});
