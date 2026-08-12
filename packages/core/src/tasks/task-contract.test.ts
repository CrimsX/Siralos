import { describe, expect, it } from "vitest";
import {
  computeTaskContractDigest,
  createTaskContract,
  reviseTaskContract,
  TASK_CONTRACT_LIMITS,
} from "./task-contract.js";

describe("task contract model", () => {
  it("creates a revision-1 contract with explicit criteria and constraints", () => {
    const contract = createTaskContract({
      id: "task-1",
      request: "Add a health component",
      constraints: [{ id: "scope", kind: "scope", description: "Workspace only." }],
      acceptanceCriteria: [
        { id: "parses", description: "GDScript parses.", verificationKind: "deterministic" },
        { id: "review", description: "Review clean.", verificationKind: "review" },
      ],
      pausePolicy: "on_approval",
    });
    expect(contract.revision).toBe(1);
    expect(contract.request).toBe("Add a health component");
    expect(contract.acceptanceCriteria).toHaveLength(2);
    expect(contract.pausePolicy).toBe("on_approval");
    expect(computeTaskContractDigest(contract)).toMatch(/^[0-9a-f]{64}$/);
  });

  it("rejects empty requests, empty criteria, and duplicate criterion ids", () => {
    expect(() => createTaskContract({ id: "t", request: "  ", acceptanceCriteria: [] })).toThrow(
      /non-empty request/,
    );
    expect(() =>
      createTaskContract({
        id: "t",
        request: "x",
        acceptanceCriteria: [
          { id: "a", description: "a", verificationKind: "deterministic" },
          { id: "a", description: "b", verificationKind: "review" },
        ],
      }),
    ).toThrow(/Duplicate acceptance criterion/);
  });

  it("revisions are immutable: the previous revision is never mutated", () => {
    const original = createTaskContract({
      id: "task-1",
      request: "Original request",
      acceptanceCriteria: [{ id: "a", description: "a", verificationKind: "deterministic" }],
    });
    const revision = reviseTaskContract(original, { id: "task-1", request: "Changed request" });
    expect(revision.revision).toBe(2);
    expect(revision.request).toBe("Changed request");
    // The previous revision object is untouched.
    expect(original.revision).toBe(1);
    expect(original.request).toBe("Original request");
    // Omitted fields carry over.
    expect(revision.acceptanceCriteria).toEqual(original.acceptanceCriteria);
    expect(computeTaskContractDigest(original)).not.toBe(computeTaskContractDigest(revision));
  });

  it("deep-freezes every revision and detaches nested caller input", () => {
    const criteria = [
      { id: "a", description: "criterion", verificationKind: "deterministic" as const },
    ];
    const constraints = [{ id: "scope", description: "workspace", kind: "scope" as const }];
    const contract = createTaskContract({
      id: "task-frozen",
      request: "Request",
      acceptanceCriteria: criteria,
      constraints,
    });

    criteria[0]!.description = "caller mutation";
    constraints[0]!.description = "caller mutation";

    expect(contract.acceptanceCriteria[0]?.description).toBe("criterion");
    expect(contract.constraints[0]?.description).toBe("workspace");
    expect(Object.isFrozen(contract)).toBe(true);
    expect(Object.isFrozen(contract.acceptanceCriteria)).toBe(true);
    expect(Object.isFrozen(contract.acceptanceCriteria[0])).toBe(true);
    expect(() => {
      (contract.acceptanceCriteria as unknown as Array<{ description: string }>)[0]!.description =
        "forged";
    }).toThrow();
  });

  it("rejects an id change across revisions", () => {
    const contract = createTaskContract({
      id: "task-stable-id",
      request: "Request",
      acceptanceCriteria: [
        { id: "a", description: "criterion", verificationKind: "deterministic" },
      ],
    });

    expect(() => reviseTaskContract(contract, { id: "task-different" })).toThrow(
      /must preserve id/,
    );
  });

  it("rejects invalid identities and oversized structured fields", () => {
    expect(() =>
      createTaskContract({
        id: "task with spaces",
        request: "Request",
        acceptanceCriteria: [
          { id: "a", description: "criterion", verificationKind: "deterministic" },
        ],
      }),
    ).toThrow(/Invalid task contract id/);
    expect(() =>
      createTaskContract({
        id: "task-large-request",
        request: "界".repeat(TASK_CONTRACT_LIMITS.maxRequestBytes),
        acceptanceCriteria: [
          { id: "a", description: "criterion", verificationKind: "deterministic" },
        ],
      }),
    ).toThrow(/request exceeds/);
  });

  it("digests are canonical: object key order does not change the digest", () => {
    const contract = createTaskContract({
      id: "task-1",
      request: "Request",
      acceptanceCriteria: [
        { id: "a", description: "a", verificationKind: "deterministic" },
        { id: "b", description: "b", verificationKind: "review" },
      ],
    });
    // Same structure, keys inserted in a different order.
    const reordered = {
      pausePolicy: contract.pausePolicy,
      request: contract.request,
      acceptanceCriteria: contract.acceptanceCriteria,
      revision: contract.revision,
      id: contract.id,
      constraints: contract.constraints,
      digest: contract.digest,
    };
    expect(computeTaskContractDigest(reordered)).toBe(computeTaskContractDigest(contract));
  });
});
