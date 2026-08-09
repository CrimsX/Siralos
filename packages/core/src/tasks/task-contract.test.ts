import { describe, expect, it } from "vitest";
import {
  computeTaskContractDigest,
  createTaskContract,
  reviseTaskContract,
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
    };
    expect(computeTaskContractDigest(reordered)).toBe(computeTaskContractDigest(contract));
  });
});
