import { describe, expect, it } from "vitest";
import { createBlockedDisposition, blockedReasonText } from "./blocked-disposition.js";
import {
  approveUnifiedTarget,
  createUnifiedChangeSet,
  computeTextTargetDigest,
  unifiedChangeSetReadyToApply,
  unifiedPreStateMap,
  type UnifiedTarget,
} from "./unified-change-set.js";
import { classifyDevelopmentSurface } from "./development-surface.js";
import { deriveUnifiedApplyOrder, deriveUnifiedOrderEdges } from "./unified-order.js";
import {
  verifyCrossSurfaceConsistency,
  type CrossSurfaceConsistencyInput,
} from "./cross-surface-consistency.js";

const SHA = (letter: string): string => letter.repeat(64);

function textTarget(path: string): Extract<UnifiedTarget, { readonly kind: "text" }> {
  return {
    kind: "text",
    fileOps: [
      {
        operation: "edit",
        path,
        expectedSha256: SHA("a"),
        replacements: [{ oldText: "old", newText: "new" }],
      },
    ],
  };
}

function nativeTarget(path: string): UnifiedTarget {
  return {
    kind: "native",
    prepared: {
      targetPath: path,
      sourceRevision: "rev_".concat("b".repeat(32)),
      sourceSha256: SHA("c"),
      kind: "scene",
      operations: [
        {
          op: "set_property",
          nodePath: "Player",
          property: "speed",
          value: { kind: "float", value: 120 },
        },
      ],
      expectedSemanticEffect: [],
      preview: { structuralSummary: "set Player.speed", diff: "--- a\n+++ b\n" },
      fingerprint: SHA("d"),
      serializedAfter: "[gd_scene format=3]\n",
      addedLines: 1,
      removedLines: 0,
    },
  };
}

function makeChangeSet(
  overrides: {
    targets?: readonly {
      kind: "text" | "native";
      path: string;
      fingerprint: string;
      preStates: readonly { path: string; sha256: string }[];
      target: UnifiedTarget;
    }[];
    surface?: "script_only" | "native_only" | "mixed" | "none";
  } = {},
): ReturnType<typeof createUnifiedChangeSet> {
  const targets = overrides.targets ?? [
    {
      kind: "text" as const,
      path: "scripts/player.gd",
      fingerprint: SHA("1"),
      preStates: [{ path: "scripts/player.gd", sha256: SHA("a") }],
      target: textTarget("scripts/player.gd"),
    },
    {
      kind: "native" as const,
      path: "scenes/player.tscn",
      fingerprint: SHA("2"),
      preStates: [{ path: "scenes/player.tscn", sha256: SHA("c") }],
      target: nativeTarget("scenes/player.tscn"),
    },
  ];
  return createUnifiedChangeSet({
    id: "unified-1",
    targets,
    surface: overrides.surface ?? "mixed",
    orderRationale: "test order",
    createdAtMs: 1_000,
    ttlMs: 600_000,
  });
}

describe("classifyDevelopmentSurface", () => {
  it("routes script-only from script touchpoints", () => {
    const decision = classifyDevelopmentSurface({
      request: "Add a health component",
      touchpoints: [
        { path: "scripts/player/player.gd", status: "verified" },
        { path: "scripts/player/health.gd", status: "candidate" },
      ],
    });
    expect(decision.kind).toBe("script_only");
  });

  it("routes native-only from scene/resource touchpoints", () => {
    const decision = classifyDevelopmentSurface({
      request: "Tune the player scene",
      touchpoints: [{ path: "scenes/player.tscn", status: "verified" }],
    });
    expect(decision.kind).toBe("native_only");
  });

  it("routes mixed when both surfaces are host-observed", () => {
    const decision = classifyDevelopmentSurface({
      request: "Add a sprint property",
      touchpoints: [
        { path: "scripts/player/player.gd", status: "verified" },
        { path: "scenes/player.tscn", status: "candidate" },
      ],
    });
    expect(decision.kind).toBe("mixed");
  });

  it("routes native when the request references scene terminology even without touchpoints", () => {
    const decision = classifyDevelopmentSurface({
      request: "Add an exported property and configure the scene value",
      touchpoints: [],
    });
    expect(decision.kind).toBe("native_only");
  });

  it("does not route mutation from a request without any evidence", () => {
    const decision = classifyDevelopmentSurface({
      request: "Answer a question",
      touchpoints: [],
    });
    expect(decision.kind).toBe("none");
  });

  it("preserves the verified/candidate distinction in evidence", () => {
    const decision = classifyDevelopmentSurface({
      request: "",
      touchpoints: [
        { path: "scenes/player.tscn", status: "candidate" },
        { path: "scripts/player.gd", status: "verified" },
      ],
    });
    expect(decision.kind).toBe("mixed");
    expect(decision.evidence.join(" ")).toContain("candidate");
    expect(decision.evidence.join(" ")).toContain("verified");
  });
});

describe("createUnifiedChangeSet", () => {
  it("binds every target fingerprint into one combined digest", () => {
    const changeSet = makeChangeSet();
    expect(changeSet.targets).toHaveLength(2);
    expect(changeSet.combinedDigest).toMatch(/^[0-9a-f]{64}$/);
    expect(changeSet.targets[0]?.approval.state).toBe("pending");
    expect(changeSet.targets[0]?.verification).toBeNull();
  });

  it("produces a different combined digest when a target changes", () => {
    const before = makeChangeSet();
    const after = makeChangeSet({
      targets: [
        {
          kind: "text",
          path: "scripts/player.gd",
          fingerprint: SHA("9"),
          preStates: [{ path: "scripts/player.gd", sha256: SHA("a") }],
          target: textTarget("scripts/player.gd"),
        },
        {
          kind: "native",
          path: "scenes/player.tscn",
          fingerprint: SHA("2"),
          preStates: [{ path: "scenes/player.tscn", sha256: SHA("c") }],
          target: nativeTarget("scenes/player.tscn"),
        },
      ],
    });
    expect(after.combinedDigest).not.toBe(before.combinedDigest);
  });

  it("rejects duplicate target paths and overlapping pre-state files", () => {
    expect(() =>
      makeChangeSet({
        targets: [
          {
            kind: "text",
            path: "scripts/player.gd",
            fingerprint: SHA("1"),
            preStates: [{ path: "scripts/player.gd", sha256: SHA("a") }],
            target: textTarget("scripts/player.gd"),
          },
          {
            kind: "native",
            path: "scripts/player.gd",
            fingerprint: SHA("2"),
            preStates: [{ path: "scripts/player.gd", sha256: SHA("c") }],
            target: nativeTarget("scripts/player.gd"),
          },
        ],
      }),
    ).toThrow(/more than once/);
  });

  it("approves a target only under its exact fingerprint", () => {
    const changeSet = makeChangeSet();
    expect(() => approveUnifiedTarget(changeSet, "t-1", SHA("f"))).toThrow(/does not match/);
    const approved = approveUnifiedTarget(changeSet, "t-1", SHA("1"));
    expect(approved.targets[0]?.approval.state).toBe("approved");
    expect(() => approveUnifiedTarget(approved, "t-1", SHA("1"))).toThrow(/already approved/);
  });

  it("reports readiness only when every target is approved and fresh", () => {
    const changeSet = makeChangeSet();
    expect(unifiedChangeSetReadyToApply(changeSet, 2_000).ready).toBe(false);
    const approved = approveUnifiedTarget(changeSet, "t-1", SHA("1"));
    const fully = approveUnifiedTarget(approved, "t-2", SHA("2"));
    expect(unifiedChangeSetReadyToApply(fully, 2_000)).toEqual({ ready: true, reason: null });
    expect(unifiedChangeSetReadyToApply(fully, 2_000 + 600_001).ready).toBe(false);
  });

  it("exposes the combined pre-state map over every target", () => {
    const changeSet = makeChangeSet();
    const map = unifiedPreStateMap(changeSet);
    expect(map.get("scripts/player.gd")).toBe(SHA("a"));
    expect(map.get("scenes/player.tscn")).toBe(SHA("c"));
    expect(map.size).toBe(2);
  });

  it("computes a stable text-target digest", () => {
    const digest = computeTextTargetDigest(textTarget("scripts/player.gd").fileOps);
    expect(digest).toMatch(/^[0-9a-f]{64}$/);
    expect(digest).toBe(computeTextTargetDigest(textTarget("scripts/player.gd").fileOps));
  });
});

describe("deriveUnifiedApplyOrder", () => {
  it("orders a dependent target after the path it references", () => {
    const targets = [
      { targetId: "t-1", path: "scenes/player.tscn", references: ["scripts/player.gd"] },
      { targetId: "t-2", path: "scripts/player.gd", references: [] },
    ];
    const { edges } = deriveUnifiedOrderEdges(targets);
    expect(edges).toEqual([{ before: "t-2", after: "t-1" }]);
    const order = deriveUnifiedApplyOrder(targets, edges);
    expect(order.order).toEqual(["t-2", "t-1"]);
    expect(order.rationale).toContain("dependency");
  });

  it("uses deterministic path order when no dependency exists", () => {
    const targets = [
      { targetId: "t-1", path: "scenes/player.tscn", references: [] },
      { targetId: "t-2", path: "scripts/player.gd", references: [] },
    ];
    const { edges } = deriveUnifiedOrderEdges(targets);
    const order = deriveUnifiedApplyOrder(targets, edges);
    // Deterministic path order (ascending): scenes/... sorts before scripts/...
    expect(order.order).toEqual(["t-1", "t-2"]);
    expect(order.rationale).toContain("path order");
  });

  it("reports unresolved references without failing", () => {
    const targets = [
      { targetId: "t-1", path: "scenes/player.tscn", references: ["scripts/missing.gd"] },
    ];
    const { unresolvedReferences } = deriveUnifiedOrderEdges(targets);
    expect(unresolvedReferences).toEqual([{ targetId: "t-1", path: "scripts/missing.gd" }]);
  });
});

describe("verifyCrossSurfaceConsistency", () => {
  function consistencyInput(overrides: Partial<CrossSurfaceConsistencyInput> = {}) {
    const changeSet = makeChangeSet();
    return {
      changeSet,
      documents: new Map(),
      pathExists: () => true,
      scriptTargetPaths: [],
      ...overrides,
    };
  }

  it("reports a concern when a scene script attachment targets a missing script", () => {
    const changeSet = makeChangeSet();
    const documents = new Map([
      [
        "scenes/player.tscn",
        {
          path: "scenes/player.tscn",
          revision: null,
          externalResources: [{ id: "1_abc", type: "Script", path: "res://scripts/missing.gd" }],
          subResources: [],
          nodes: [
            {
              name: "Player",
              type: "Node2D",
              parentPath: ".",
              groups: [],
              properties: [],
              rawAttributes: [],
              script: {
                resource: { id: "1_abc", type: "Script", path: "res://scripts/missing.gd" },
                resolvedPath: "scripts/missing.gd",
              },
            },
          ],
          connections: [],
          editableInstances: [],
        },
      ],
    ]);
    const result = verifyCrossSurfaceConsistency(
      consistencyInput({
        changeSet,
        documents,
        pathExists: (path) => path !== "scripts/missing.gd",
      }),
    );
    expect(result.consistent).toBe(false);
    expect(result.checks.some((check) => check.status === "concern")).toBe(true);
  });

  it("passes when attachments, signals, and references resolve", () => {
    const changeSet = makeChangeSet();
    const documents = new Map([
      [
        "scenes/player.tscn",
        {
          path: "scenes/player.tscn",
          revision: null,
          externalResources: [{ id: "1_abc", type: "Script", path: "res://scripts/player.gd" }],
          subResources: [],
          nodes: [
            {
              name: "Player",
              type: "Node2D",
              parentPath: ".",
              groups: [],
              properties: [],
              rawAttributes: [],
              script: {
                resource: { id: "1_abc", type: "Script", path: "res://scripts/player.gd" },
                resolvedPath: "scripts/player.gd",
              },
            },
            {
              name: "Camera",
              type: "Camera2D",
              parentPath: ".",
              groups: [],
              properties: [],
              rawAttributes: [],
            },
          ],
          connections: [{ signal: "hit", from: "Player", to: "Camera", method: "flash" }],
          editableInstances: [],
        },
      ],
    ]);
    const result = verifyCrossSurfaceConsistency(
      consistencyInput({
        changeSet,
        documents,
        scriptTargetPaths: ["scripts/player.gd"],
      }),
    );
    expect(result.consistent).toBe(true);
  });

  it("discloses the script/scene pair as runtime_evidence_unavailable", () => {
    const changeSet = makeChangeSet();
    const documents = new Map([
      [
        "scenes/player.tscn",
        {
          path: "scenes/player.tscn",
          revision: null,
          externalResources: [{ id: "1_abc", type: "Script", path: "res://scripts/player.gd" }],
          subResources: [],
          nodes: [
            {
              name: "Player",
              type: "Node2D",
              parentPath: ".",
              groups: [],
              properties: [],
              rawAttributes: [],
              script: {
                resource: { id: "1_abc", type: "Script", path: "res://scripts/player.gd" },
                resolvedPath: "scripts/player.gd",
              },
            },
          ],
          connections: [],
          editableInstances: [],
        },
      ],
    ]);
    const result = verifyCrossSurfaceConsistency(
      consistencyInput({
        changeSet,
        documents,
        scriptTargetPaths: ["scripts/player.gd"],
      }),
    );
    expect(
      result.checks.some(
        (check) =>
          check.name === "script-scene-pair" && check.status === "runtime_evidence_unavailable",
      ),
    ).toBe(true);
    // A disclosed runtime-only concern never fails consistency.
    expect(result.consistent).toBe(true);
  });
});

describe("blocked disposition", () => {
  it("builds a typed disposition with preserved changes", () => {
    const disposition = createBlockedDisposition({
      kind: "runtime_verification_required",
      detail: "Acceptance requires a runtime probe that is not available statically.",
      preservedChanges: ["scripts/player.gd"],
    });
    expect(disposition.kind).toBe("runtime_verification_required");
    expect(blockedReasonText(disposition)).toContain("preserved changes: scripts/player.gd");
  });

  it("rejects an empty explanation", () => {
    expect(() => createBlockedDisposition({ kind: "approval_denied", detail: "  " })).toThrow(
      /concrete explanation/,
    );
  });
});
